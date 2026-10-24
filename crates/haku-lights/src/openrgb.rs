/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! A client for the OpenRGB SDK server, following OpenRGB's `Documentation/OpenRGBSDK.md`.
//!
//! Every packet is a 16-byte header (`"ORGB"`, device index, packet id, data size; integers
//! little-endian) and its data. HakuCX speaks protocol versions 0 to 5 (OpenRGB 0.3 to 1.0rc1)
//! and asks for the lower of its own and the server's version. It reads each controller's LED
//! count, switches the controller to its direct ("custom") mode, sends one color per LED, and
//! on [`Client::restore`] puts back the mode the controller had before.

use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::Rgb;

pub const DEFAULT_PORT: u16 = 6742;
/// The highest protocol version HakuCX understands.
pub const CLIENT_PROTOCOL: u32 = 5;
const MAGIC: &[u8; 4] = b"ORGB";
/// Refuse absurd packets from a misbehaving server.
const MAX_PACKET: u32 = 16 * 1024 * 1024;

pub mod packet {
    pub const REQUEST_CONTROLLER_COUNT: u32 = 0;
    pub const REQUEST_CONTROLLER_DATA: u32 = 1;
    pub const REQUEST_PROTOCOL_VERSION: u32 = 40;
    pub const SET_CLIENT_NAME: u32 = 50;
    pub const DEVICE_LIST_UPDATED: u32 = 100;
    pub const UPDATE_LEDS: u32 = 1050;
    pub const SET_CUSTOM_MODE: u32 = 1100;
    pub const UPDATE_MODE: u32 = 1101;
}

/// One RGB controller on the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Controller {
    /// The index HakuCX addresses it by (protocol 0 to 5).
    pub index: u32,
    pub name: String,
    pub leds: usize,
    /// The mode the controller was in before HakuCX took over, and that mode's description
    /// exactly as the server sent it (so it can be sent back unchanged).
    active_mode: i32,
    active_mode_data: Option<Vec<u8>>,
}

pub fn header(device: u32, id: u32, size: u32) -> [u8; 16] {
    let mut h = [0u8; 16];
    h[..4].copy_from_slice(MAGIC);
    h[4..8].copy_from_slice(&device.to_le_bytes());
    h[8..12].copy_from_slice(&id.to_le_bytes());
    h[12..16].copy_from_slice(&size.to_le_bytes());
    h
}

/// The data of an UpdateLEDs packet: its size (counting itself), the color count and one
/// `RGBColor` per LED (red, green, blue, 0).
pub fn update_leds_data(colors: &[Rgb]) -> Vec<u8> {
    let n = colors.len().min(u16::MAX as usize);
    let size = (4 + 2 + 4 * n) as u32;
    let mut d = Vec::with_capacity(size as usize);
    d.extend_from_slice(&size.to_le_bytes());
    d.extend_from_slice(&(n as u16).to_le_bytes());
    for c in &colors[..n] {
        d.extend_from_slice(&[c.r, c.g, c.b, 0]);
    }
    d
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

fn short() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "controller data ends early")
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> io::Result<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or_else(short)?;
        let slice = self.data.get(self.at..end).ok_or_else(short)?;
        self.at = end;
        Ok(slice)
    }
    fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> io::Result<i32> {
        Ok(i32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    /// A length-prefixed, NUL-terminated string.
    fn string(&mut self) -> io::Result<String> {
        let len = self.u16()? as usize;
        let raw = self.bytes(len)?;
        let raw = raw.split(|b| *b == 0).next().unwrap_or_default();
        Ok(String::from_utf8_lossy(raw).into_owned())
    }
    fn skip(&mut self, n: usize) -> io::Result<()> {
        self.bytes(n).map(|_| ())
    }
}

/// Parse the controller data block (the reply to REQUEST_CONTROLLER_DATA), as far as the LED
/// list. Fields are present or absent by `protocol`, per the "Device Data", "Mode Data", "Zone
/// Data", "Segment Data" and "LED Data" tables.
pub fn parse_controller(index: u32, data: &[u8], protocol: u32) -> io::Result<Controller> {
    let mut r = Reader { data, at: 0 };
    let _data_size = r.u32()?;
    let _device_type = r.i32()?;
    let name = r.string()?;
    if protocol >= 1 {
        let _vendor = r.string()?;
    }
    for _ in 0..4 {
        // description, version, serial, location
        let _ = r.string()?;
    }
    let num_modes = r.u16()?;
    let active_mode = r.i32()?;
    let mut active_mode_data = None;
    for mode in 0..num_modes as i32 {
        let start = r.at;
        let _mode_name = r.string()?;
        // value, flags, speed_min, speed_max
        r.skip(16)?;
        if protocol >= 3 {
            // brightness_min, brightness_max
            r.skip(8)?;
        }
        // colors_min, colors_max, speed
        r.skip(12)?;
        if protocol >= 3 {
            // brightness
            r.skip(4)?;
        }
        // direction, color_mode
        r.skip(8)?;
        let colors = r.u16()? as usize;
        r.skip(4 * colors)?;
        if mode == active_mode {
            active_mode_data = Some(data[start..r.at].to_vec());
        }
    }
    let num_zones = r.u16()?;
    for _ in 0..num_zones {
        let _zone_name = r.string()?;
        // type, leds_min, leds_max, leds_count
        r.skip(16)?;
        let matrix_len = r.u16()? as usize;
        r.skip(matrix_len)?;
        if protocol >= 4 {
            let segments = r.u16()?;
            for _ in 0..segments {
                let _segment_name = r.string()?;
                // type, start_idx, leds_count
                r.skip(12)?;
            }
        }
        if protocol >= 5 {
            let _zone_flags = r.u32()?;
        }
    }
    let leds = r.u16()? as usize;
    for _ in 0..leds {
        let _led_name = r.string()?;
        // led_value (removed only in protocol 6)
        r.skip(4)?;
    }
    Ok(Controller {
        index,
        name,
        leds,
        active_mode,
        active_mode_data,
    })
}

pub struct Client {
    stream: TcpStream,
    protocol: u32,
    pub controllers: Vec<Controller>,
    /// The server said its device list changed; [`Client::refresh`] reloads it.
    list_changed: bool,
}

impl Client {
    /// Connect to `host:port`, introduce HakuCX, agree on a protocol version and take over
    /// every controller.
    pub fn connect(address: &str, timeout: Duration) -> io::Result<Self> {
        let addr = address
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such host"))?;
        let stream = TcpStream::connect_timeout(&addr, timeout)?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        let mut client = Self {
            stream,
            protocol: 0,
            controllers: Vec::new(),
            list_changed: false,
        };
        client.send(0, packet::SET_CLIENT_NAME, b"HakuCX\0")?;
        client.send(
            0,
            packet::REQUEST_PROTOCOL_VERSION,
            &CLIENT_PROTOCOL.to_le_bytes(),
        )?;
        // A protocol 0 server does not answer this; give it a moment, then assume 0.
        client
            .stream
            .set_read_timeout(Some(Duration::from_secs(1)))?;
        let server = match client.expect(packet::REQUEST_PROTOCOL_VERSION) {
            Ok(data) if data.len() >= 4 => u32::from_le_bytes(data[..4].try_into().unwrap()),
            Ok(_) => 0,
            Err(e) if is_timeout(&e) => 0,
            Err(e) => return Err(e),
        };
        client.stream.set_read_timeout(Some(timeout))?;
        client.protocol = server.min(CLIENT_PROTOCOL);
        client.load_controllers()?;
        Ok(client)
    }

    pub fn protocol(&self) -> u32 {
        self.protocol
    }

    fn send(&mut self, device: u32, id: u32, data: &[u8]) -> io::Result<()> {
        let mut packet = Vec::with_capacity(16 + data.len());
        packet.extend_from_slice(&header(device, id, data.len() as u32));
        packet.extend_from_slice(data);
        self.stream.write_all(&packet)
    }

    /// Read one packet: (device, id, data).
    fn read_packet(&mut self) -> io::Result<(u32, u32, Vec<u8>)> {
        let mut h = [0u8; 16];
        self.stream.read_exact(&mut h)?;
        if &h[..4] != MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not an OpenRGB server",
            ));
        }
        let device = u32::from_le_bytes(h[4..8].try_into().unwrap());
        let id = u32::from_le_bytes(h[8..12].try_into().unwrap());
        let size = u32::from_le_bytes(h[12..16].try_into().unwrap());
        if size > MAX_PACKET {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "packet too large",
            ));
        }
        let mut data = vec![0u8; size as usize];
        self.stream.read_exact(&mut data)?;
        Ok((device, id, data))
    }

    /// Wait for the reply with packet id `id`, noting device list changes on the way.
    fn expect(&mut self, id: u32) -> io::Result<Vec<u8>> {
        loop {
            let (_, got, data) = self.read_packet()?;
            if got == id {
                return Ok(data);
            }
            if got == packet::DEVICE_LIST_UPDATED {
                self.list_changed = true;
            }
        }
    }

    fn load_controllers(&mut self) -> io::Result<()> {
        self.list_changed = false;
        self.send(0, packet::REQUEST_CONTROLLER_COUNT, &[])?;
        let data = self.expect(packet::REQUEST_CONTROLLER_COUNT)?;
        let count = data
            .get(..4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(short)?;
        let mut controllers = Vec::new();
        for index in 0..count.min(256) {
            let request = if self.protocol >= 1 {
                self.protocol.to_le_bytes().to_vec()
            } else {
                Vec::new()
            };
            self.send(index, packet::REQUEST_CONTROLLER_DATA, &request)?;
            let data = self.expect(packet::REQUEST_CONTROLLER_DATA)?;
            let controller = parse_controller(index, &data, self.protocol)?;
            if controller.leds > 0 {
                self.send(index, packet::SET_CUSTOM_MODE, &[])?;
                controllers.push(controller);
            }
        }
        self.controllers = controllers;
        Ok(())
    }

    /// Handle anything the server sent on its own (device list changes). Returns whether the
    /// controller list was reloaded.
    pub fn refresh(&mut self) -> io::Result<bool> {
        self.stream.set_nonblocking(true)?;
        let mut peek = [0u8; 16];
        let waiting = match self.stream.peek(&mut peek) {
            Ok(0) => {
                self.stream.set_nonblocking(false)?;
                return Err(io::ErrorKind::ConnectionAborted.into());
            }
            Ok(_) => true,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => false,
            Err(e) => {
                self.stream.set_nonblocking(false)?;
                return Err(e);
            }
        };
        self.stream.set_nonblocking(false)?;
        if waiting {
            let (_, id, _) = self.read_packet()?;
            if id == packet::DEVICE_LIST_UPDATED {
                self.list_changed = true;
            }
        }
        if self.list_changed {
            self.load_controllers()?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Set every LED of controller `index` (one color per LED, in the server's LED order).
    pub fn set_leds(&mut self, index: u32, colors: &[Rgb]) -> io::Result<()> {
        self.send(index, packet::UPDATE_LEDS, &update_leds_data(colors))
    }

    /// Give each controller back the mode it had before HakuCX connected.
    pub fn restore(&mut self) -> io::Result<()> {
        for c in self.controllers.clone() {
            let Some(mode) = &c.active_mode_data else {
                continue;
            };
            let size = (4 + 4 + mode.len()) as u32;
            let mut d = Vec::with_capacity(size as usize);
            d.extend_from_slice(&size.to_le_bytes());
            d.extend_from_slice(&c.active_mode.to_le_bytes());
            d.extend_from_slice(mode);
            self.send(c.index, packet::UPDATE_MODE, &d)?;
        }
        Ok(())
    }
}

fn is_timeout(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_layout() {
        assert_eq!(
            header(2, packet::UPDATE_LEDS, 14),
            [
                b'O', b'R', b'G', b'B', 2, 0, 0, 0, 0x1a, 0x04, 0, 0, 14, 0, 0, 0
            ]
        );
    }

    #[test]
    fn colors_are_red_green_blue_zero() {
        let d = update_leds_data(&[Rgb::new(1, 2, 3), Rgb::new(250, 0, 9)]);
        assert_eq!(d, vec![14, 0, 0, 0, 2, 0, 1, 2, 3, 0, 250, 0, 9, 0]);
    }

    #[test]
    fn truncated_data_is_an_error() {
        assert!(parse_controller(0, &[1, 2, 3], 4).is_err());
    }
}

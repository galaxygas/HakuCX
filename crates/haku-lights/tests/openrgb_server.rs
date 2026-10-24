/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The OpenRGB client against a stand-in server that encodes devices field by field from the
//! tables in OpenRGB's `Documentation/OpenRGBSDK.md`, at each protocol version.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::Duration;

use haku_lights::Rgb;
use haku_lights::openrgb::{Client, header, packet};

fn string(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&((s.len() + 1) as u16).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}
fn u32le(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// "Mode Data": name, value, flags, speed min/max, [brightness min/max], colors min/max,
/// speed, [brightness], direction, color mode, colors.
fn mode(name: &str, protocol: u32, colors: &[u32]) -> Vec<u8> {
    let mut m = Vec::new();
    string(&mut m, name);
    for v in [7, 0x20, 1, 4] {
        u32le(&mut m, v);
    }
    if protocol >= 3 {
        u32le(&mut m, 0);
        u32le(&mut m, 100);
    }
    for v in [1, 2, 3] {
        u32le(&mut m, v);
    }
    if protocol >= 3 {
        u32le(&mut m, 80);
    }
    u32le(&mut m, 0);
    u32le(&mut m, 1);
    m.extend_from_slice(&(colors.len() as u16).to_le_bytes());
    for c in colors {
        u32le(&mut m, *c);
    }
    m
}

/// A keyboard: three modes (Direct, Static, Wave with two colors), a matrix zone with one
/// segment, and `leds` LEDs. Returns the controller data and the bytes of the active mode.
fn keyboard(protocol: u32, leds: u16) -> (Vec<u8>, Vec<u8>) {
    let mut d = Vec::new();
    d.extend_from_slice(&5i32.to_le_bytes()); // type: keyboard
    string(&mut d, "Test Keyboard");
    if protocol >= 1 {
        string(&mut d, "HakuCX Labs");
    }
    for s in ["A keyboard", "1.0", "SN1", "HID: /dev/x"] {
        string(&mut d, s);
    }
    let modes = [
        mode("Direct", protocol, &[]),
        mode("Static", protocol, &[0x0000ff]),
        mode("Wave", protocol, &[0x00ff00, 0xff0000]),
    ];
    d.extend_from_slice(&(modes.len() as u16).to_le_bytes());
    d.extend_from_slice(&2i32.to_le_bytes()); // active: Wave
    for m in &modes {
        d.extend_from_slice(m);
    }
    // One zone with a 2 x 3 matrix map.
    d.extend_from_slice(&1u16.to_le_bytes());
    string(&mut d, "Keys");
    for v in [2u32, leds as u32, leds as u32, leds as u32] {
        u32le(&mut d, v);
    }
    let mut matrix = Vec::new();
    u32le(&mut matrix, 2);
    u32le(&mut matrix, 3);
    for i in 0..6 {
        u32le(&mut matrix, i);
    }
    d.extend_from_slice(&(matrix.len() as u16).to_le_bytes());
    d.extend_from_slice(&matrix);
    if protocol >= 4 {
        d.extend_from_slice(&1u16.to_le_bytes());
        string(&mut d, "Left half");
        for v in [0u32, 0, 2] {
            u32le(&mut d, v);
        }
    }
    if protocol >= 5 {
        u32le(&mut d, 0); // zone flags
    }
    d.extend_from_slice(&leds.to_le_bytes());
    for i in 0..leds {
        string(&mut d, &format!("Key {i}"));
        u32le(&mut d, i as u32);
    }
    d.extend_from_slice(&leds.to_le_bytes());
    for _ in 0..leds {
        u32le(&mut d, 0);
    }
    if protocol >= 5 {
        d.extend_from_slice(&0u16.to_le_bytes()); // LED display names
        u32le(&mut d, 0); // flags
    }
    let mut block = Vec::new();
    u32le(&mut block, (d.len() + 4) as u32);
    block.extend_from_slice(&d);
    (block, modes[2].clone())
}

fn read_packet(s: &mut TcpStream) -> Option<(u32, u32, Vec<u8>)> {
    let mut h = [0u8; 16];
    s.read_exact(&mut h).ok()?;
    assert_eq!(&h[..4], b"ORGB");
    let dev = u32::from_le_bytes(h[4..8].try_into().unwrap());
    let id = u32::from_le_bytes(h[8..12].try_into().unwrap());
    let size = u32::from_le_bytes(h[12..16].try_into().unwrap());
    let mut data = vec![0u8; size as usize];
    s.read_exact(&mut data).ok()?;
    Some((dev, id, data))
}

fn reply(s: &mut TcpStream, dev: u32, id: u32, data: &[u8]) {
    s.write_all(&header(dev, id, data.len() as u32)).unwrap();
    s.write_all(data).unwrap();
}

/// Serve one client; every packet the client sends goes to `log`.
fn serve(server_protocol: Option<u32>, log: mpsc::Sender<(u32, u32, Vec<u8>)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut negotiated = 0;
        while let Some((dev, id, data)) = read_packet(&mut s) {
            match id {
                packet::REQUEST_PROTOCOL_VERSION => {
                    // A protocol 0 server ignores this packet.
                    if let Some(p) = server_protocol {
                        let client = u32::from_le_bytes(data[..4].try_into().unwrap());
                        negotiated = p.min(client);
                        reply(&mut s, 0, id, &p.to_le_bytes());
                    }
                }
                packet::REQUEST_CONTROLLER_COUNT => reply(&mut s, 0, id, &1u32.to_le_bytes()),
                packet::REQUEST_CONTROLLER_DATA => {
                    let asked = if data.len() >= 4 {
                        u32::from_le_bytes(data[..4].try_into().unwrap())
                    } else {
                        0
                    };
                    assert_eq!(asked, negotiated);
                    reply(&mut s, dev, id, &keyboard(asked, 4).0);
                }
                _ => {}
            }
            log.send((dev, id, data)).unwrap();
        }
    });
    addr
}

fn run(server_protocol: Option<u32>, expect: u32) {
    let (tx, rx) = mpsc::channel();
    let addr = serve(server_protocol, tx);
    let mut client = Client::connect(&addr, Duration::from_secs(3)).unwrap();
    assert_eq!(client.protocol(), expect);
    assert_eq!(client.controllers.len(), 1);
    assert_eq!(client.controllers[0].name, "Test Keyboard");
    assert_eq!(client.controllers[0].leds, 4);
    let colors = [
        Rgb::new(1, 2, 3),
        Rgb::new(4, 5, 6),
        Rgb::new(7, 8, 9),
        Rgb::new(10, 11, 12),
    ];
    client.set_leds(0, &colors).unwrap();
    client.restore().unwrap();
    drop(client);

    let packets: Vec<_> = rx.iter().collect();
    let ids: Vec<u32> = packets.iter().map(|p| p.1).collect();
    assert_eq!(
        ids,
        vec![
            packet::SET_CLIENT_NAME,
            packet::REQUEST_PROTOCOL_VERSION,
            packet::REQUEST_CONTROLLER_COUNT,
            packet::REQUEST_CONTROLLER_DATA,
            packet::SET_CUSTOM_MODE,
            packet::UPDATE_LEDS,
            packet::UPDATE_MODE,
        ]
    );
    assert_eq!(packets[0].2, b"HakuCX\0");
    assert!(packets[4].2.is_empty());
    assert_eq!(
        packets[5].2,
        vec![
            22, 0, 0, 0, 4, 0, 1, 2, 3, 0, 4, 5, 6, 0, 7, 8, 9, 0, 10, 11, 12, 0
        ]
    );
    // UpdateMode: size, the mode index the keyboard had (2), that mode exactly as sent.
    let wave = keyboard(expect, 4).1;
    let update = &packets[6].2;
    assert_eq!(
        u32::from_le_bytes(update[..4].try_into().unwrap()) as usize,
        update.len()
    );
    assert_eq!(i32::from_le_bytes(update[4..8].try_into().unwrap()), 2);
    assert_eq!(&update[8..], &wave[..]);
}

#[test]
fn protocol_5_server() {
    run(Some(5), 5);
}

#[test]
fn newer_server_is_spoken_to_at_5() {
    run(Some(6), 5);
}

#[test]
fn protocol_4_server() {
    run(Some(4), 4);
}

#[test]
fn protocol_3_server() {
    run(Some(3), 3);
}

#[test]
fn protocol_0_server_that_does_not_answer_the_version_request() {
    run(None, 0);
}

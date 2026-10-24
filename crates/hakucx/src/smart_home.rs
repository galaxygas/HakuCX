/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Smart home (Settings → Smart home): HakuCX publishes its state to your MQTT broker so home
//! automation (Home Assistant, Node-RED, ...) can react, for example by matching the room
//! lights to the theme or dimming them while a video plays. Retained topics, under your prefix:
//!
//! - `<prefix>/status`: `online`, or `offline` (also sent by the broker if HakuCX disappears)
//! - `<prefix>/theme/accent`: the theme color, `#rrggbb`
//! - `<prefix>/media/playing`: `true` or `false`
//! - `<prefix>/media/title`: what plays (empty when nothing does)
//!
//! Plain MQTT (`mqtt://`) on your network; HakuCX does not connect to TLS brokers yet.

use std::thread::JoinHandle;
use std::time::Duration;

use rumqttc::{Client, Event, LastWill, MqttOptions, Packet, QoS};

/// Where to connect: (host, port, user and password).
pub type Broker = (String, u16, Option<(String, String)>);

/// `mqtt://[user:password@]host[:port]`, or just `host[:port]`.
pub fn parse_broker(url: &str) -> Result<Broker, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("Enter your broker's address, like mqtt://192.168.1.10".into());
    }
    let full = if url.contains("://") {
        url.to_owned()
    } else {
        format!("mqtt://{url}")
    };
    let parsed = url::Url::parse(&full).map_err(|_| format!("{url} is not a broker address"))?;
    match parsed.scheme() {
        "mqtt" | "tcp" => {}
        "mqtts" | "ssl" => {
            return Err("HakuCX does not connect to TLS (mqtts) brokers yet; use the broker's plain MQTT port on your network".into());
        }
        other => return Err(format!("{other}:// is not an MQTT address")),
    }
    let host = parsed
        .host_str()
        .filter(|h| !h.is_empty())
        .ok_or("the address has no host")?
        .trim_matches(['[', ']'])
        .to_owned();
    let login = (!parsed.username().is_empty()).then(|| {
        (
            parsed.username().to_owned(),
            parsed.password().unwrap_or_default().to_owned(),
        )
    });
    Ok((host, parsed.port().unwrap_or(1883), login))
}

/// A topic prefix MQTT accepts for publishing (no wildcards, not empty).
pub fn clean_prefix(prefix: &str) -> String {
    let p = prefix.trim().trim_matches('/');
    if p.is_empty() || p.contains(['+', '#', '\0']) {
        "hakucx".into()
    } else {
        p.to_owned()
    }
}

pub struct SmartHome {
    client: Client,
    prefix: String,
    thread: Option<JoinHandle<()>>,
}

impl SmartHome {
    /// Connect in the background. `report` is called (on the MQTT thread) with `Ok(())` when
    /// the broker accepts the connection and `Err(message)` when it fails (it keeps retrying).
    pub fn connect(
        broker: &Broker,
        prefix: &str,
        report: Box<dyn Fn(Result<(), String>) + Send>,
    ) -> Self {
        let prefix = clean_prefix(prefix);
        let id = format!("hakucx-{}", uuid::Uuid::new_v4().simple());
        let mut options = MqttOptions::new(id, broker.0.clone(), broker.1);
        options.set_keep_alive(Duration::from_secs(30));
        options.set_last_will(LastWill::new(
            format!("{prefix}/status"),
            "offline",
            QoS::AtLeastOnce,
            true,
        ));
        if let Some((user, pass)) = &broker.2 {
            options.set_credentials(user.clone(), pass.clone());
        }
        let (client, mut connection) = Client::new(options, 16);
        let online = client.clone();
        let status_topic = format!("{prefix}/status");
        let thread = std::thread::Builder::new()
            .name("smart-home".into())
            .spawn(move || {
                for event in connection.iter() {
                    match event {
                        Ok(Event::Incoming(Packet::ConnAck(_))) => {
                            let _ = online.try_publish(
                                status_topic.clone(),
                                QoS::AtLeastOnce,
                                true,
                                "online",
                            );
                            report(Ok(()));
                        }
                        Ok(_) => {}
                        Err(rumqttc::ConnectionError::RequestsDone) => break,
                        Err(e) => {
                            report(Err(e.to_string()));
                            // The next iteration reconnects; don't hammer the broker.
                            std::thread::sleep(Duration::from_secs(10));
                        }
                    }
                }
            })
            .ok();
        Self {
            client,
            prefix,
            thread,
        }
    }

    /// Publish a retained value under the prefix (`theme/accent`, `media/playing`, ...).
    pub fn publish(&self, key: &str, value: &str) {
        let _ = self.client.try_publish(
            format!("{}/{key}", self.prefix),
            QoS::AtLeastOnce,
            true,
            value.as_bytes().to_vec(),
        );
    }
}

impl Drop for SmartHome {
    fn drop(&mut self) {
        let _ = self.client.try_publish(
            format!("{}/status", self.prefix),
            QoS::AtLeastOnce,
            true,
            "offline",
        );
        let _ = self.client.try_disconnect();
        // The thread ends when the connection closes; don't wait on a slow broker.
        drop(self.thread.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    #[test]
    fn broker_addresses() {
        assert_eq!(
            parse_broker("192.168.1.10").unwrap(),
            ("192.168.1.10".into(), 1883, None)
        );
        assert_eq!(
            parse_broker("mqtt://ha:secret@homeassistant.local:1884").unwrap(),
            (
                "homeassistant.local".into(),
                1884,
                Some(("ha".into(), "secret".into()))
            )
        );
        assert!(parse_broker("mqtts://broker").unwrap_err().contains("TLS"));
        assert!(parse_broker("http://x").is_err());
        assert!(parse_broker("").is_err());
        assert_eq!(clean_prefix("/home/browser/"), "home/browser");
        assert_eq!(clean_prefix("a/#"), "hakucx");
    }

    fn remaining_length(s: &mut std::net::TcpStream) -> usize {
        let (mut value, mut shift) = (0usize, 0);
        loop {
            let mut b = [0u8; 1];
            s.read_exact(&mut b).unwrap();
            value |= ((b[0] & 0x7f) as usize) << shift;
            if b[0] & 0x80 == 0 {
                return value;
            }
            shift += 7;
        }
    }

    /// Against a stand-in broker that speaks MQTT 3.1.1 (CONNECT / CONNACK / PUBLISH / PUBACK).
    #[test]
    fn publishes_retained_state() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            loop {
                let mut first = [0u8; 1];
                if s.read_exact(&mut first).is_err() {
                    return;
                }
                let len = remaining_length(&mut s);
                let mut body = vec![0u8; len];
                s.read_exact(&mut body).unwrap();
                match first[0] >> 4 {
                    1 => {
                        // CONNECT: the will flag (0x04) and will retain (0x20) must be set.
                        let flags = body[7];
                        tx.send(format!(
                            "connect will={} retain={}",
                            flags & 0x04 != 0,
                            flags & 0x20 != 0
                        ))
                        .unwrap();
                        s.write_all(&[0x20, 0x02, 0x00, 0x00]).unwrap();
                    }
                    3 => {
                        let retain = first[0] & 1 == 1;
                        let qos = (first[0] >> 1) & 3;
                        let tl = u16::from_be_bytes([body[0], body[1]]) as usize;
                        let topic = String::from_utf8(body[2..2 + tl].to_vec()).unwrap();
                        let mut at = 2 + tl;
                        if qos > 0 {
                            let id = [body[at], body[at + 1]];
                            at += 2;
                            s.write_all(&[0x40, 0x02, id[0], id[1]]).unwrap();
                        }
                        let payload = String::from_utf8(body[at..].to_vec()).unwrap();
                        tx.send(format!("{topic}={payload} retain={retain}"))
                            .unwrap();
                    }
                    14 => return,
                    _ => {}
                }
            }
        });
        let (ok_tx, ok_rx) = mpsc::channel();
        let home = SmartHome::connect(
            &("127.0.0.1".into(), port, None),
            "test/hx",
            Box::new(move |r| {
                let _ = ok_tx.send(r.is_ok());
            }),
        );
        assert!(ok_rx.recv_timeout(Duration::from_secs(5)).unwrap());
        home.publish("theme/accent", "#7d46f2");
        home.publish("media/playing", "true");
        drop(home);
        let got: Vec<String> = rx.iter().take(5).collect();
        assert_eq!(got[0], "connect will=true retain=true");
        assert!(got.contains(&"test/hx/status=online retain=true".to_string()));
        assert!(got.contains(&"test/hx/theme/accent=#7d46f2 retain=true".to_string()));
        assert!(got.contains(&"test/hx/media/playing=true retain=true".to_string()));
        assert!(got.contains(&"test/hx/status=offline retain=true".to_string()));
    }
}

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! A client for the Razer Chroma SDK REST server that Razer Synapse runs on this computer
//! (Razer's "Chroma SDK REST Documentation"):
//!
//! - `POST http://localhost:54235/razer/chromasdk` with the application's details starts a
//!   session and answers with the session's own URI.
//! - `PUT <uri>/<device>` sets an effect; colors are numbers in BGR order (`0xBBGGRR`).
//! - `PUT <uri>/heartbeat` keeps the session alive (it ends after 15 s of silence; Razer
//!   recommends once a second).
//! - `DELETE <uri>` ends the session, and Synapse takes the lights back.

use std::time::Duration;

use serde_json::{Value, json};

use crate::Rgb;

pub const SERVER: &str = "http://localhost:54235/razer/chromasdk";
/// `RzErrors.h`: the effect does not fit the device.
const INVALID_PARAMETER: i64 = 87;

/// The device types the REST API addresses, each with its custom-effect grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Device {
    Keyboard,
    Mouse,
    Mousepad,
    Headset,
    Keypad,
    ChromaLink,
}

impl Device {
    pub const ALL: [Device; 6] = [
        Device::Keyboard,
        Device::Mouse,
        Device::Mousepad,
        Device::Headset,
        Device::Keypad,
        Device::ChromaLink,
    ];

    pub fn endpoint(self) -> &'static str {
        match self {
            Device::Keyboard => "keyboard",
            Device::Mouse => "mouse",
            Device::Mousepad => "mousepad",
            Device::Headset => "headset",
            Device::Keypad => "keypad",
            Device::ChromaLink => "chromalink",
        }
    }

    /// (rows, columns) of the device's custom effect.
    pub fn grid(self) -> (usize, usize) {
        match self {
            Device::Keyboard => (6, 22),
            Device::Mouse => (9, 7),
            // CHROMA_CUSTOM2's 20 LEDs. Razer's REST page gives 20 for CHROMA_CUSTOM, but Synapse
            // (3.40) answers 87 (invalid parameter) to that and takes 15 (see `MOUSEPAD_LEGACY`).
            Device::Mousepad => (1, 20),
            Device::Headset => (1, 5),
            Device::Keypad => (4, 5),
            Device::ChromaLink => (1, 5),
        }
    }

    /// The custom effect showing `columns` (one color per column, left to right, repeated
    /// down every row).
    pub fn custom_effect(self, columns: &[Rgb]) -> Value {
        let (rows, cols) = self.grid();
        let row: Vec<u32> = (0..cols)
            .map(|i| bgr(columns.get(i).copied().unwrap_or_default()))
            .collect();
        let param = if rows == 1 {
            json!(row)
        } else {
            json!(vec![row; rows])
        };
        let effect = match self {
            // The mouse's 9 x 7 grid and the mousepad's 20 LEDs are CHROMA_CUSTOM2; everything
            // else is CHROMA_CUSTOM.
            Device::Mouse | Device::Mousepad => "CHROMA_CUSTOM2",
            _ => "CHROMA_CUSTOM",
        };
        json!({ "effect": effect, "param": param })
    }
}

/// The older mousepad effect (15 LEDs), for Synapse versions without CHROMA_CUSTOM2.
const MOUSEPAD_LEGACY: usize = 15;

fn mousepad_legacy_effect(columns: &[Rgb]) -> Value {
    let row: Vec<u32> = (0..MOUSEPAD_LEGACY)
        .map(|i| {
            // Spread the 20 columns over 15 LEDs.
            let src = i * columns.len().max(1) / MOUSEPAD_LEGACY;
            bgr(columns.get(src).copied().unwrap_or_default())
        })
        .collect();
    json!({ "effect": "CHROMA_CUSTOM", "param": row })
}

/// A color as the REST API wants it.
pub fn bgr(c: Rgb) -> u32 {
    ((c.b as u32) << 16) | ((c.g as u32) << 8) | c.r as u32
}

/// The details Synapse shows for the connected application.
pub fn init_request() -> Value {
    json!({
        "title": "HakuCX",
        "description": "GX Lights in the HakuCX browser",
        "author": { "name": "HakuCX", "contact": "https://github.com/galaxygas/HakuCX" },
        "device_supported": Device::ALL.iter().map(|d| d.endpoint()).collect::<Vec<_>>(),
        "category": "application"
    })
}

#[derive(Debug)]
pub enum Error {
    /// Synapse's Chroma SDK server is not running here.
    Unreachable(String),
    /// The server answered with a Razer error code (`RzErrors.h`).
    Code(i64),
    Protocol(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unreachable(e) => write!(f, "Razer Chroma is not available ({e})"),
            Error::Code(c) => write!(f, "Razer Chroma error {c}"),
            Error::Protocol(e) => write!(f, "unexpected Razer Chroma reply: {e}"),
        }
    }
}

fn client() -> Result<reqwest::blocking::Client, Error> {
    reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| Error::Unreachable(e.to_string()))
}

/// The `result` code of a reply (0 is success; replies without one count as success).
fn result_code(reply: &Value) -> i64 {
    reply.get("result").and_then(Value::as_i64).unwrap_or(0)
}

/// The session URI must point back at this computer.
pub fn local_uri(uri: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(uri) else {
        return false;
    };
    url.scheme() == "http" && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
}

pub struct Session {
    client: reqwest::blocking::Client,
    uri: String,
    /// This Synapse refused CHROMA_CUSTOM2 for mousepads.
    mousepad_legacy: std::cell::Cell<bool>,
}

impl Session {
    pub fn start() -> Result<Self, Error> {
        let client = client()?;
        let reply: Value = client
            .post(SERVER)
            .json(&init_request())
            .send()
            .and_then(|r| r.json())
            .map_err(|e| Error::Unreachable(e.to_string()))?;
        let Some(uri) = reply.get("uri").and_then(Value::as_str) else {
            return Err(match result_code(&reply) {
                0 => Error::Protocol(reply.to_string()),
                code => Error::Code(code),
            });
        };
        if !local_uri(uri) {
            return Err(Error::Protocol(format!("session URI {uri} is not local")));
        }
        Ok(Self {
            client,
            uri: uri.trim_end_matches('/').to_owned(),
            mousepad_legacy: std::cell::Cell::new(false),
        })
    }

    fn put(&self, path: &str, body: &Value) -> Result<Value, Error> {
        self.client
            .put(format!("{}/{path}", self.uri))
            .json(body)
            .send()
            .and_then(|r| r.json::<Value>())
            .map_err(|e| Error::Unreachable(e.to_string()))
    }

    /// Show `columns` on `device`.
    pub fn set(&self, device: Device, columns: &[Rgb]) -> Result<(), Error> {
        let legacy = device == Device::Mousepad && self.mousepad_legacy.get();
        let effect = if legacy {
            mousepad_legacy_effect(columns)
        } else {
            device.custom_effect(columns)
        };
        match result_code(&self.put(device.endpoint(), &effect)?) {
            0 => Ok(()),
            INVALID_PARAMETER if device == Device::Mousepad && !legacy => {
                self.mousepad_legacy.set(true);
                self.set(device, columns)
            }
            code => Err(Error::Code(code)),
        }
    }

    pub fn heartbeat(&self) -> Result<(), Error> {
        self.put("heartbeat", &json!({})).map(|_| ())
    }

    /// End the session (Synapse takes the lights back).
    pub fn close(self) {
        let _ = self.client.delete(&self.uri).send();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_are_bgr() {
        assert_eq!(bgr(Rgb::new(255, 0, 0)), 255);
        assert_eq!(bgr(Rgb::new(0, 255, 0)), 65280);
        assert_eq!(bgr(Rgb::new(0, 0, 255)), 16711680);
    }

    #[test]
    fn custom_effects_match_the_documented_shapes() {
        let red = [Rgb::new(255, 0, 0); 22];
        let kb = Device::Keyboard.custom_effect(&red);
        assert_eq!(kb["effect"], "CHROMA_CUSTOM");
        assert_eq!(kb["param"].as_array().unwrap().len(), 6);
        assert_eq!(kb["param"][0].as_array().unwrap().len(), 22);
        assert_eq!(kb["param"][5][21], 255);
        let mouse = Device::Mouse.custom_effect(&red);
        assert_eq!(mouse["effect"], "CHROMA_CUSTOM2");
        assert_eq!(mouse["param"].as_array().unwrap().len(), 9);
        assert_eq!(mouse["param"][0].as_array().unwrap().len(), 7);
        let pad = Device::Mousepad.custom_effect(&red);
        assert_eq!(pad["effect"], "CHROMA_CUSTOM2");
        assert_eq!(pad["param"].as_array().unwrap().len(), 20);
        assert_eq!(pad["param"][0], 255);
        let old = mousepad_legacy_effect(&red);
        assert_eq!(old["effect"], "CHROMA_CUSTOM");
        assert_eq!(old["param"].as_array().unwrap().len(), 15);
        let keypad = Device::Keypad.custom_effect(&red);
        assert_eq!(keypad["param"].as_array().unwrap().len(), 4);
        assert_eq!(keypad["param"][0].as_array().unwrap().len(), 5);
        // Missing columns are off.
        assert_eq!(Device::Headset.custom_effect(&[])["param"][4], 0);
    }

    #[test]
    fn init_names_every_device() {
        let r = init_request();
        assert_eq!(r["category"], "application");
        assert_eq!(r["device_supported"].as_array().unwrap().len(), 6);
    }

    #[test]
    fn session_uri_must_be_local() {
        assert!(local_uri("http://localhost:54236/chromasdk"));
        assert!(local_uri("http://127.0.0.1:1234/chromasdk"));
        assert!(!local_uri("http://example.com/chromasdk"));
        assert!(!local_uri("https://localhost/chromasdk"));
    }
}

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! My Flow: a private chat between HakuCX and your phone for links, notes and files.
//!
//! There is no account and no server. HakuCX serves a small page on the local network; the
//! phone opens it by scanning a QR code, and everything between them is end-to-end encrypted
//! with XChaCha20-Poly1305:
//!
//! - The QR code carries a pairing id and a one-time pairing key in the URL fragment, which
//!   browsers never send over the network.
//! - The phone's first message (`hello`, sealed with the pairing key) is answered with a new,
//!   random device key sealed with the pairing key; the pairing key is then forgotten, so a
//!   photo of the QR code is useless afterwards (Opera's Flow does the same with its token).
//! - Every request after that is sealed with the device key, carries a time stamp (no older
//!   than [`MAX_AGE_MS`]) and a random id that is accepted only once, and every answer is
//!   sealed too. Requests that do not open are refused without saying why.
//!
//! This module is the protocol and the store, without any networking (see
//! `hakucx/src/flow_server.rs` for the HTTP side), so it can be tested directly.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zeroize::Zeroizing;

use crate::vault::Protector;

/// How old a request may be (clocks on phones and PCs drift a little).
pub const MAX_AGE_MS: i64 = 10 * 60 * 1000;
/// How long a QR code stays valid.
pub const PAIRING_TTL_MS: i64 = 10 * 60 * 1000;
/// Largest file the phone and HakuCX exchange.
pub const MAX_FILE: usize = 25 * 1024 * 1024;
/// Entries kept (oldest go first; their files stay on disk).
pub const MAX_ENTRIES: usize = 500;
/// Entries a phone gets when it first opens the page.
pub const HISTORY: usize = 100;

pub type KeyBytes = Zeroizing<[u8; 32]>;

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("the operating system's random number generator failed");
    b
}

fn random_id() -> String {
    URL_SAFE_NO_PAD.encode(random::<12>())
}

/// Seal `plaintext` for the other side: base64url(nonce || ciphertext).
pub fn seal(key: &[u8; 32], aad: &str, plaintext: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(seal_bytes(key, aad, plaintext))
}

pub fn seal_bytes(key: &[u8; 32], aad: &str, plaintext: &[u8]) -> Vec<u8> {
    let nonce = random::<24>();
    let cipher = XChaCha20Poly1305::new(key.into());
    let mut out = nonce.to_vec();
    out.extend(
        cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: aad.as_bytes(),
                },
            )
            .expect("XChaCha20-Poly1305 encryption cannot fail for these sizes"),
    );
    out
}

pub fn open(key: &[u8; 32], aad: &str, sealed: &str) -> Option<Vec<u8>> {
    let raw = URL_SAFE_NO_PAD.decode(sealed.trim()).ok()?;
    if raw.len() < 24 + 16 {
        return None;
    }
    let cipher = XChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(
            XNonce::from_slice(&raw[..24]),
            Payload {
                msg: &raw[24..],
                aad: aad.as_bytes(),
            },
        )
        .ok()
}

/// What sealed data is for, so nothing can be replayed in another place.
pub fn aad(device: &str, purpose: &str) -> String {
    format!("hakucx-flow/1 {device} {purpose}")
}

// ------------------------------------------------------------------ the store

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum FlowKind {
    Text,
    Link,
    File,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FlowFile {
    pub name: String,
    pub size: u64,
    pub mime: String,
    /// Where the file is on this computer.
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FlowEntry {
    pub id: String,
    /// Order of arrival (phones ask for entries after the last one they have).
    pub seq: u64,
    pub at: i64,
    /// `pc` (sent from HakuCX) or the id of the phone that sent it.
    pub from: String,
    pub from_name: String,
    pub kind: FlowKind,
    pub text: String,
    pub file: Option<FlowFile>,
}

/// A paired phone, as the UI shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FlowDevice {
    pub id: String,
    pub name: String,
    pub paired_at: i64,
    pub last_seen: Option<i64>,
}

/// The saved form: keys wrapped by the operating system (DPAPI).
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    entries: Vec<FlowEntry>,
    devices: Vec<SavedDevice>,
    next_seq: u64,
    port: Option<u16>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedDevice {
    #[serde(flatten)]
    device: FlowDevice,
    key: String,
}

struct Device {
    info: FlowDevice,
    key: KeyBytes,
}

struct Pairing {
    key: KeyBytes,
    expires: i64,
}

/// What changed, for the browser to save and show.
#[derive(Debug, PartialEq)]
pub enum Change {
    Entry(FlowEntry),
    Paired(FlowDevice),
    Seen,
}

pub struct Flow {
    entries: VecDeque<FlowEntry>,
    devices: Vec<Device>,
    pairings: HashMap<String, Pairing>,
    next_seq: u64,
    /// Request ids already accepted (replay protection), newest last.
    seen_ids: VecDeque<String>,
    pub port: Option<u16>,
    /// The computer's name, shown on the phone.
    pub pc_name: String,
    /// Where files from phones are saved.
    pub inbox_dir: PathBuf,
    changes: Vec<Change>,
}

#[derive(Debug, thiserror::Error)]
pub enum FlowError {
    #[error("could not read My Flow data: {0}")]
    Io(#[from] std::io::Error),
    #[error("My Flow data is damaged: {0}")]
    Json(#[from] serde_json::Error),
}

impl Flow {
    pub fn new(pc_name: String, inbox_dir: PathBuf) -> Self {
        Self {
            entries: VecDeque::new(),
            devices: Vec::new(),
            pairings: HashMap::new(),
            next_seq: 1,
            seen_ids: VecDeque::new(),
            port: None,
            pc_name,
            inbox_dir,
            changes: Vec::new(),
        }
    }

    /// Load `flow.json`; devices whose keys this Windows user cannot unwrap are dropped (they
    /// pair again).
    pub fn load(
        path: &Path,
        protector: &dyn Protector,
        pc_name: String,
        inbox_dir: PathBuf,
    ) -> Result<Self, FlowError> {
        let mut flow = Self::new(pc_name, inbox_dir);
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(flow),
            Err(e) => return Err(e.into()),
        };
        let saved: Saved = serde_json::from_str(&text)?;
        flow.entries = saved.entries.into();
        flow.next_seq = saved
            .next_seq
            .max(flow.entries.iter().map(|e| e.seq + 1).max().unwrap_or(1));
        flow.port = saved.port;
        for d in saved.devices {
            let Ok(blob) = STANDARD.decode(&d.key) else {
                continue;
            };
            let Ok(raw) = protector.unprotect(&blob) else {
                log::warn!(
                    "dropping My Flow device {}: its key cannot be unwrapped",
                    d.device.name
                );
                continue;
            };
            let Ok(bytes) = <[u8; 32]>::try_from(raw.as_slice()) else {
                continue;
            };
            flow.devices.push(Device {
                info: d.device,
                key: Zeroizing::new(bytes),
            });
        }
        Ok(flow)
    }

    pub fn save(&self, path: &Path, protector: &dyn Protector) -> Result<(), FlowError> {
        let mut devices = Vec::new();
        for d in &self.devices {
            let blob = protector
                .protect(d.key.as_slice())
                .map_err(std::io::Error::other)?;
            devices.push(SavedDevice {
                device: d.info.clone(),
                key: STANDARD.encode(blob),
            });
        }
        let saved = Saved {
            entries: self.entries.iter().cloned().collect(),
            devices,
            next_seq: self.next_seq,
            port: self.port,
        };
        crate::storage::write_bytes_atomic(path, serde_json::to_string(&saved)?.as_bytes())
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(())
    }

    pub fn entries(&self) -> Vec<FlowEntry> {
        self.entries.iter().cloned().collect()
    }

    pub fn devices(&self) -> Vec<FlowDevice> {
        self.devices.iter().map(|d| d.info.clone()).collect()
    }

    pub fn has_devices(&self) -> bool {
        !self.devices.is_empty()
    }

    pub fn pairing_open(&self, now: i64) -> bool {
        self.pairings.values().any(|p| p.expires > now)
    }

    pub fn take_changes(&mut self) -> Vec<Change> {
        std::mem::take(&mut self.changes)
    }

    fn push(&mut self, mut entry: FlowEntry) -> FlowEntry {
        entry.seq = self.next_seq;
        self.next_seq += 1;
        self.entries.push_back(entry.clone());
        while self.entries.len() > MAX_ENTRIES {
            self.entries.pop_front();
        }
        entry
    }

    /// Something typed or sent from HakuCX.
    pub fn add_from_pc(
        &mut self,
        kind: FlowKind,
        text: String,
        file: Option<FlowFile>,
        now: i64,
    ) -> FlowEntry {
        let entry = FlowEntry {
            id: random_id(),
            seq: 0,
            at: now,
            from: "pc".into(),
            from_name: self.pc_name.clone(),
            kind,
            text,
            file,
        };
        self.push(entry)
    }

    pub fn remove_entry(&mut self, id: &str) {
        self.entries.retain(|e| e.id != id);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn forget_device(&mut self, id: &str) {
        self.devices.retain(|d| d.info.id != id);
    }

    /// A new QR code: (pairing id, pairing key as base64url).
    pub fn start_pairing(&mut self, now: i64) -> (String, String) {
        self.pairings.retain(|_, p| p.expires > now);
        let id = random_id();
        let key = Zeroizing::new(random::<32>());
        let encoded = URL_SAFE_NO_PAD.encode(key.as_slice());
        self.pairings.insert(
            id.clone(),
            Pairing {
                key,
                expires: now + PAIRING_TTL_MS,
            },
        );
        (id, encoded)
    }

    pub fn cancel_pairings(&mut self) {
        self.pairings.clear();
    }

    fn device(&self, id: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.info.id == id)
    }

    /// Accept a request id once, and only if it is recent.
    fn fresh(&mut self, id: &str, at: i64, now: i64) -> bool {
        if (now - at).abs() > MAX_AGE_MS || id.is_empty() || id.len() > 64 {
            return false;
        }
        if self.seen_ids.iter().any(|s| s == id) {
            return false;
        }
        self.seen_ids.push_back(id.to_owned());
        if self.seen_ids.len() > 4096 {
            self.seen_ids.pop_front();
        }
        true
    }

    /// Open a phone's sealed request for `purpose` and check it is fresh.
    fn open_request(
        &mut self,
        device: &str,
        purpose: &str,
        sealed: &str,
        now: i64,
    ) -> Option<Request> {
        let key = self.device(device)?.key.clone();
        let plain = open(&key, &aad(device, purpose), sealed)?;
        let request: Request = serde_json::from_slice(&plain).ok()?;
        if !self.fresh(&request.id, request.t, now) {
            return None;
        }
        if let Some(d) = self.devices.iter_mut().find(|d| d.info.id == device) {
            d.info.last_seen = Some(now);
        }
        self.changes.push(Change::Seen);
        Some(request)
    }

    // ------------------------------------------------------------------ the phone's requests

    /// `POST /api/hello`: a phone scanned the QR code. Answers with the device key, sealed
    /// with the pairing key.
    pub fn hello(&mut self, pairing: &str, sealed: &str, now: i64) -> Option<String> {
        let entry = self.pairings.get(pairing)?;
        if entry.expires <= now {
            self.pairings.remove(pairing);
            return None;
        }
        let pairing_key = entry.key.clone();
        let plain = open(&pairing_key, &aad(pairing, "hello"), sealed)?;
        let request: Request = serde_json::from_slice(&plain).ok()?;
        if !self.fresh(&request.id, request.t, now) {
            return None;
        }
        // The QR code works once.
        self.pairings.remove(pairing);
        let name = clean_name(request.name.as_deref().unwrap_or("Phone"));
        let device = FlowDevice {
            id: random_id(),
            name,
            paired_at: now,
            last_seen: Some(now),
        };
        let key = Zeroizing::new(random::<32>());
        let answer = serde_json::json!({
            "device": device.id,
            "key": URL_SAFE_NO_PAD.encode(key.as_slice()),
            "pc": self.pc_name,
        });
        self.devices.push(Device {
            info: device.clone(),
            key,
        });
        self.changes.push(Change::Paired(device));
        Some(seal(
            &pairing_key,
            &aad(pairing, "welcome"),
            answer.to_string().as_bytes(),
        ))
    }

    /// `POST /api/send`: a note, link or file from a phone. Returns the stored entry's seq.
    pub fn receive(&mut self, device: &str, sealed: &str, now: i64) -> Option<u64> {
        let request = self.open_request(device, "send", sealed, now)?;
        let from_name = self.device(device)?.info.name.clone();
        let (kind, text, file) = match request.kind.as_deref() {
            Some("text") => {
                let text = request.text.unwrap_or_default();
                let text: String = text.chars().take(20_000).collect();
                if text.trim().is_empty() {
                    return None;
                }
                let kind = if is_link(&text) {
                    FlowKind::Link
                } else {
                    FlowKind::Text
                };
                (kind, text, None)
            }
            Some("file") => {
                let data = STANDARD.decode(request.data.unwrap_or_default()).ok()?;
                if data.is_empty() || data.len() > MAX_FILE {
                    return None;
                }
                let name = safe_file_name(request.name.as_deref().unwrap_or("file"));
                let path = self.store_file(&name, &data).ok()?;
                let file = FlowFile {
                    name: name.clone(),
                    size: data.len() as u64,
                    mime: request
                        .mime
                        .filter(|m| m.len() < 128 && !m.contains(['\r', '\n']))
                        .unwrap_or_else(|| "application/octet-stream".into()),
                    path: path.display().to_string(),
                };
                (FlowKind::File, name, Some(file))
            }
            _ => return None,
        };
        let entry = self.push(FlowEntry {
            id: random_id(),
            seq: 0,
            at: now,
            from: device.to_owned(),
            from_name,
            kind,
            text,
            file,
        });
        let seq = entry.seq;
        self.changes.push(Change::Entry(entry));
        Some(seq)
    }

    fn store_file(&self, name: &str, data: &[u8]) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.inbox_dir)?;
        let (stem, ext) = match name.rsplit_once('.') {
            Some((s, e)) if !s.is_empty() => (s.to_owned(), format!(".{e}")),
            _ => (name.to_owned(), String::new()),
        };
        for n in 0..1000 {
            let candidate = if n == 0 {
                self.inbox_dir.join(name)
            } else {
                self.inbox_dir.join(format!("{stem} ({n}){ext}"))
            };
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(mut f) => {
                    use std::io::Write as _;
                    f.write_all(data)?;
                    return Ok(candidate);
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
        Err(std::io::Error::other("too many files with this name"))
    }

    /// `GET /api/inbox`: entries after `after` (or the recent history), sealed for the phone.
    pub fn inbox(&mut self, device: &str, sealed: &str, now: i64) -> Option<String> {
        let request = self.open_request(device, "inbox", sealed, now)?;
        let after = request.after.unwrap_or(0);
        let mut list: Vec<serde_json::Value> = self
            .entries
            .iter()
            .filter(|e| e.seq > after)
            .map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "seq": e.seq,
                    "at": e.at,
                    "mine": e.from == device,
                    "from": e.from_name,
                    "kind": e.kind,
                    "text": e.text,
                    "file": e.file.as_ref().map(|f| serde_json::json!({
                        "name": f.name, "size": f.size, "mime": f.mime,
                    })),
                })
            })
            .collect();
        if list.len() > HISTORY {
            list.drain(..list.len() - HISTORY);
        }
        let key = self.device(device)?.key.clone();
        let body = serde_json::json!({ "entries": list, "pc": self.pc_name });
        Some(seal(
            &key,
            &aad(device, "inbox-reply"),
            body.to_string().as_bytes(),
        ))
    }

    /// `GET /api/file`: a file from an entry, sealed (binary: nonce || ciphertext).
    pub fn file(&mut self, device: &str, sealed: &str, now: i64) -> Option<Vec<u8>> {
        let request = self.open_request(device, "file", sealed, now)?;
        let id = request.entry?;
        let entry = self.entries.iter().find(|e| e.id == id)?;
        let path = entry.file.as_ref()?.path.clone();
        let data = std::fs::read(&path).ok()?;
        if data.len() > MAX_FILE {
            return None;
        }
        let key = self.device(device)?.key.clone();
        Some(seal_bytes(&key, &aad(device, &format!("file {id}")), &data))
    }
}

/// The JSON inside a phone's sealed request.
#[derive(Deserialize)]
struct Request {
    /// Random request id (replay protection).
    id: String,
    /// The phone's clock, ms since 1970.
    t: i64,
    name: Option<String>,
    kind: Option<String>,
    text: Option<String>,
    data: Option<String>,
    mime: Option<String>,
    after: Option<u64>,
    entry: Option<String>,
}

pub fn is_link(text: &str) -> bool {
    let t = text.trim();
    !t.contains(char::is_whitespace)
        && url::Url::parse(t).is_ok_and(|u| matches!(u.scheme(), "http" | "https"))
}

fn clean_name(name: &str) -> String {
    let n: String = name.chars().filter(|c| !c.is_control()).take(48).collect();
    let n = n.trim();
    if n.is_empty() {
        "Phone".into()
    } else {
        n.into()
    }
}

/// A file name that is safe to create in the inbox folder.
pub fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let mut clean: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .take(120)
        .collect();
    clean = clean.trim().trim_matches('.').to_owned();
    let upper = clean
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.len() == 4
            && upper.as_bytes()[3].is_ascii_digit());
    if clean.is_empty() || reserved {
        format!("file-{}", clean.len())
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Plain;
    impl Protector for Plain {
        fn protect(&self, secret: &[u8]) -> Result<Vec<u8>, String> {
            Ok(secret.iter().map(|b| b ^ 0x5a).collect())
        }
        fn unprotect(&self, blob: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
            Ok(Zeroizing::new(blob.iter().map(|b| b ^ 0x5a).collect()))
        }
    }

    const NOW: i64 = 1_800_000_000_000;

    fn key_of(b64: &str) -> [u8; 32] {
        URL_SAFE_NO_PAD.decode(b64).unwrap().try_into().unwrap()
    }

    fn request(key: &[u8; 32], device: &str, purpose: &str, body: serde_json::Value) -> String {
        seal(key, &aad(device, purpose), body.to_string().as_bytes())
    }

    /// Pair a phone; returns (device id, device key).
    fn pair(flow: &mut Flow) -> (String, [u8; 32]) {
        let (pairing, key) = flow.start_pairing(NOW);
        let pk = key_of(&key);
        let hello = request(
            &pk,
            &pairing,
            "hello",
            serde_json::json!({"id": "h1", "t": NOW, "name": "Pixel"}),
        );
        let answer = flow.hello(&pairing, &hello, NOW).expect("paired");
        let plain = open(&pk, &aad(&pairing, "welcome"), &answer).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&plain).unwrap();
        assert_eq!(v["pc"], "Desk");
        (
            v["device"].as_str().unwrap().into(),
            key_of(v["key"].as_str().unwrap()),
        )
    }

    fn flow(dir: &Path) -> Flow {
        Flow::new("Desk".into(), dir.join("inbox"))
    }

    /// Sealed by the phone page's `crypto.ts` (@noble/ciphers); `ui/src/flow/crypto.test.ts`
    /// opens one sealed here.
    #[test]
    fn opens_what_the_phone_page_seals() {
        let sealed =
            "8RR723wtYNOwUwPZeziKHYkNvpIs5p92DeZ8hh7GuInVgoGnvEFJEJ4OM4jx-WZSSRtcPIhyBarr_y0";
        let plain = open(&[1u8; 32], "hakucx-flow/1 dev send", sealed).unwrap();
        assert_eq!(plain, br#"{"hello":"from ts"}"#);
    }

    #[test]
    fn seal_and_open() {
        let key = [7u8; 32];
        let s = seal(&key, "a", b"hello");
        assert_eq!(open(&key, "a", &s).unwrap(), b"hello");
        assert!(
            open(&key, "b", &s).is_none(),
            "the purpose is authenticated"
        );
        assert!(open(&[8u8; 32], "a", &s).is_none());
        assert!(open(&key, "a", "short").is_none());
    }

    #[test]
    fn pairing_works_once_and_expires() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = flow(dir.path());
        let (pairing, key) = f.start_pairing(NOW);
        let pk = key_of(&key);
        let hello = |id: &str| {
            request(
                &pk,
                &pairing,
                "hello",
                serde_json::json!({"id": id, "t": NOW, "name": "Pixel\u{7}"}),
            )
        };
        assert!(f.hello(&pairing, &hello("a"), NOW).is_some());
        assert_eq!(f.devices()[0].name, "Pixel");
        assert!(matches!(f.take_changes().as_slice(), [Change::Paired(_)]));
        // A second phone with the same QR code gets nothing.
        assert!(f.hello(&pairing, &hello("b"), NOW).is_none());
        assert_eq!(f.devices().len(), 1);

        let (late, key) = f.start_pairing(NOW);
        let lk = key_of(&key);
        let msg = request(
            &lk,
            &late,
            "hello",
            serde_json::json!({"id": "c", "t": NOW + PAIRING_TTL_MS + 1}),
        );
        assert!(
            f.hello(&late, &msg, NOW + PAIRING_TTL_MS + 1).is_none(),
            "expired"
        );
    }

    #[test]
    fn messages_from_the_phone() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = flow(dir.path());
        let (device, key) = pair(&mut f);
        f.take_changes();
        let send = |id: &str, t: i64, body: serde_json::Value| {
            let mut v = body;
            v["id"] = id.into();
            v["t"] = t.into();
            request(&key, &device, "send", v)
        };
        let seq = f
            .receive(
                &device,
                &send(
                    "m1",
                    NOW,
                    serde_json::json!({"kind": "text", "text": "https://example.com/x"}),
                ),
                NOW,
            )
            .unwrap();
        assert_eq!(f.entries()[0].kind, FlowKind::Link);
        assert_eq!(f.entries()[0].seq, seq);
        assert_eq!(f.entries()[0].from_name, "Pixel");
        // Replayed, stale, or sealed for another purpose: refused.
        let again = send("m1", NOW, serde_json::json!({"kind": "text", "text": "hi"}));
        assert!(f.receive(&device, &again, NOW).is_none());
        let old = send(
            "m2",
            NOW - MAX_AGE_MS - 1,
            serde_json::json!({"kind": "text", "text": "hi"}),
        );
        assert!(f.receive(&device, &old, NOW).is_none());
        let wrong = request(
            &key,
            &device,
            "inbox",
            serde_json::json!({"id": "m3", "t": NOW, "kind": "text", "text": "hi"}),
        );
        assert!(f.receive(&device, &wrong, NOW).is_none());
        assert!(
            f.receive(
                "nobody",
                &send("m4", NOW, serde_json::json!({"kind": "text", "text": "hi"})),
                NOW
            )
            .is_none()
        );

        // A file lands in the inbox folder under a safe name, without overwriting.
        let data = STANDARD.encode(b"PNGDATA");
        for id in ["f1", "f2"] {
            let msg = send(
                id,
                NOW,
                serde_json::json!({"kind": "file", "name": "../evil:pic.png", "mime": "image/png", "data": data}),
            );
            f.receive(&device, &msg, NOW).unwrap();
        }
        let files: Vec<FlowFile> = f.entries().into_iter().filter_map(|e| e.file).collect();
        assert_eq!(files[0].name, "evil_pic.png");
        assert!(files[0].path.ends_with("evil_pic.png"));
        assert!(files[1].path.ends_with("evil_pic (1).png"));
        assert_eq!(std::fs::read(&files[1].path).unwrap(), b"PNGDATA");
        assert_eq!(
            f.take_changes()
                .iter()
                .filter(|c| matches!(c, Change::Entry(_)))
                .count(),
            3
        );
    }

    #[test]
    fn inbox_and_files_for_the_phone() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = flow(dir.path());
        let (device, key) = pair(&mut f);
        let file_path = dir.path().join("doc.txt");
        std::fs::write(&file_path, b"hello file").unwrap();
        let first = f.add_from_pc(FlowKind::Text, "note".into(), None, NOW);
        let doc = f.add_from_pc(
            FlowKind::File,
            "doc.txt".into(),
            Some(FlowFile {
                name: "doc.txt".into(),
                size: 10,
                mime: "text/plain".into(),
                path: file_path.display().to_string(),
            }),
            NOW,
        );
        let ask = request(
            &key,
            &device,
            "inbox",
            serde_json::json!({"id": "i1", "t": NOW, "after": first.seq}),
        );
        let sealed = f.inbox(&device, &ask, NOW).unwrap();
        let v: serde_json::Value =
            serde_json::from_slice(&open(&key, &aad(&device, "inbox-reply"), &sealed).unwrap())
                .unwrap();
        let entries = v["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1, "only entries after the one asked for");
        assert_eq!(entries[0]["kind"], "file");
        assert!(
            entries[0]["file"].get("path").is_none(),
            "paths stay on this computer"
        );

        let ask = request(
            &key,
            &device,
            "file",
            serde_json::json!({"id": "i2", "t": NOW, "entry": doc.id}),
        );
        let bytes = f.file(&device, &ask, NOW).unwrap();
        let cipher = XChaCha20Poly1305::new((&key).into());
        let plain = cipher
            .decrypt(
                XNonce::from_slice(&bytes[..24]),
                Payload {
                    msg: &bytes[24..],
                    aad: aad(&device, &format!("file {}", doc.id)).as_bytes(),
                },
            )
            .unwrap();
        assert_eq!(plain, b"hello file");
    }

    #[test]
    fn saved_keys_are_wrapped_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flow.json");
        let mut f = flow(dir.path());
        let (device, key) = pair(&mut f);
        f.add_from_pc(FlowKind::Text, "hi".into(), None, NOW);
        f.port = Some(47652);
        f.save(&path, &Plain).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains(&URL_SAFE_NO_PAD.encode(key)),
            "keys are never saved in the clear"
        );

        let mut g = Flow::load(&path, &Plain, "Desk".into(), dir.path().join("inbox")).unwrap();
        assert_eq!(g.port, Some(47652));
        assert_eq!(g.entries().len(), 1);
        let next = g.add_from_pc(FlowKind::Text, "again".into(), None, NOW);
        assert_eq!(next.seq, 2, "sequence numbers continue");
        let ask = request(
            &key,
            &device,
            "inbox",
            serde_json::json!({"id": "x", "t": NOW}),
        );
        assert!(
            g.inbox(&device, &ask, NOW).is_some(),
            "the reloaded key still works"
        );
    }

    #[test]
    fn names() {
        assert_eq!(safe_file_name("C:\\x\\a?b.txt"), "a_b.txt");
        assert_eq!(safe_file_name("CON.txt"), "file-7");
        assert_eq!(safe_file_name("..."), "file-0");
        assert!(is_link("https://a.example/b"));
        assert!(!is_link("see https://a.example"));
        assert!(!is_link("javascript:alert(1)"));
    }
}

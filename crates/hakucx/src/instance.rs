/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! One process per profile. When HakuCX is already running a profile and is started again
//! (a link clicked in another program while HakuCX is the default browser, a pinned taskbar
//! icon), the new process hands its addresses to the running one and exits.
//!
//! The running process listens on a loopback port and writes the port and a random token to
//! `instance.json` in the profile folder, which only the same Windows user can read. A
//! request must carry the token, so other users and web pages cannot open tabs.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use log::{info, warn};
use serde::{Deserialize, Serialize};

const FILE: &str = "instance.json";
const MAX_REQUEST: usize = 64 * 1024;

#[derive(Serialize, Deserialize)]
struct InstanceFile {
    port: u16,
    token: String,
    pid: u32,
}

#[derive(Serialize, Deserialize)]
struct Request {
    token: String,
    urls: Vec<String>,
}

/// Addresses another launch asked the running HakuCX to open.
pub type Handler = Box<dyn Fn(Vec<String>) + Send + 'static>;

fn file(profile_dir: &Path) -> PathBuf {
    profile_dir.join(FILE)
}

/// If another process runs this profile, give it `urls` and return true (this process
/// should exit). Otherwise return false.
pub fn forward(profile_dir: &Path, urls: &[String]) -> bool {
    let Ok(text) = std::fs::read_to_string(file(profile_dir)) else {
        return false;
    };
    let Ok(info) = serde_json::from_str::<InstanceFile>(&text) else {
        return false;
    };
    let addr = SocketAddr::from(([127, 0, 0, 1], info.port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(400)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let request = Request {
        token: info.token,
        urls: urls.iter().take(64).cloned().collect(),
    };
    let Ok(mut line) = serde_json::to_string(&request) else {
        return false;
    };
    line.push('\n');
    if stream.write_all(line.as_bytes()).is_err() {
        return false;
    }
    let mut answer = String::new();
    let ok = BufReader::new(stream).read_line(&mut answer).is_ok() && answer.trim() == "ok";
    if ok {
        info!("handed {} address(es) to the running HakuCX", urls.len());
    }
    ok
}

/// Become the profile's running instance: listen for other launches and pass their
/// addresses to `handler` (on a worker thread). The listener stops with the process.
pub fn listen(profile_dir: &Path, handler: Handler) {
    let listener = match TcpListener::bind(("127.0.0.1", 0)) {
        Ok(l) => l,
        Err(e) => return warn!("single instance: {e}"),
    };
    let Ok(port) = listener.local_addr().map(|a| a.port()) else {
        return;
    };
    let token =
        uuid::Uuid::new_v4().simple().to_string() + &uuid::Uuid::new_v4().simple().to_string();
    let record = InstanceFile {
        port,
        token: token.clone(),
        pid: std::process::id(),
    };
    let path = file(profile_dir);
    if let Err(e) = haku_core::storage::write_json_atomic(&path, &record) {
        return warn!("single instance: {e}");
    }
    let spawned = std::thread::Builder::new()
        .name("instance".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
                let mut reader = BufReader::new(stream.take_limit());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                let Ok(request) = serde_json::from_str::<Request>(&line) else {
                    continue;
                };
                // Compare without leaking how much matched through timing.
                let matches = request.token.len() == token.len()
                    && request
                        .token
                        .bytes()
                        .zip(token.bytes())
                        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                        == 0;
                if !matches {
                    continue;
                }
                let mut stream = reader.into_inner().into_inner();
                let _ = stream.write_all(b"ok\n");
                handler(request.urls);
            }
        });
    if let Err(e) = spawned {
        warn!("single instance: {e}");
    }
}

/// Remove the instance file when the running process exits cleanly.
pub fn release(profile_dir: &Path) {
    let path = file(profile_dir);
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(info) = serde_json::from_str::<InstanceFile>(&text)
        && info.pid == std::process::id()
    {
        let _ = std::fs::remove_file(path);
    }
}

trait TakeLimit {
    fn take_limit(self) -> std::io::Take<TcpStream>;
}

impl TakeLimit for TcpStream {
    fn take_limit(self) -> std::io::Take<TcpStream> {
        std::io::Read::take(self, MAX_REQUEST as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn second_launch_hands_over_and_needs_the_token() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!forward(dir.path(), &["https://a.test/".into()]));
        let (tx, rx) = mpsc::channel();
        listen(dir.path(), Box::new(move |urls| tx.send(urls).unwrap()));
        assert!(forward(dir.path(), &["https://a.test/".into()]));
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(3)).unwrap(),
            vec!["https://a.test/".to_owned()]
        );

        // A wrong token is ignored.
        let mut info: InstanceFile =
            serde_json::from_str(&std::fs::read_to_string(file(dir.path())).unwrap()).unwrap();
        info.token = "x".repeat(info.token.len());
        std::fs::write(file(dir.path()), serde_json::to_string(&info).unwrap()).unwrap();
        assert!(!forward(dir.path(), &["https://evil.test/".into()]));
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    }
}

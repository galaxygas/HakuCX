/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The download engine. Servo 0.7 cannot download files, so HakuCX fetches them itself with
//! the page's cookies, user agent and referrer, on one worker thread per download.
//!
//! Files are written to `<name>.part` and renamed when complete; paused downloads resume with
//! an HTTP `Range` request. GX Control's network limiter caps the combined rate.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const RUN: u8 = 0;
const PAUSE: u8 = 1;
const CANCEL: u8 = 2;

#[derive(Debug)]
pub enum DownloadMsg {
    Started {
        id: String,
        total: Option<u64>,
        mime: String,
        file_name: String,
        path: PathBuf,
    },
    Progress {
        id: String,
        received: u64,
        total: Option<u64>,
        bps: f32,
    },
    Finished {
        id: String,
        path: PathBuf,
    },
    Paused {
        id: String,
        received: u64,
    },
    Cancelled {
        id: String,
    },
    Failed {
        id: String,
        reason: String,
    },
}

pub struct DownloadRequest {
    pub id: String,
    pub url: String,
    pub referrer: Option<String>,
    pub cookie_header: Option<String>,
    pub user_agent: String,
    /// Final file path chosen up front (asked or derived); `None` = name from the response.
    pub path: Option<PathBuf>,
    pub directory: PathBuf,
    pub name_hint: Option<String>,
    /// Resume from this many bytes of an existing `.part` file.
    pub resume_from: u64,
}

pub struct DownloadEngine {
    tx: Sender<DownloadMsg>,
    rx: Receiver<DownloadMsg>,
    wake: Arc<dyn Fn() + Send + Sync>,
    controls: Mutex<std::collections::HashMap<String, Arc<AtomicU8>>>,
    /// Combined limit in bytes per second shared by all downloads (0 = unlimited).
    pub limit_bps: Arc<AtomicU64>,
    /// Bytes downloaded in total, for the GX Control network graph.
    pub bytes_total: Arc<AtomicU64>,
    client: reqwest::blocking::Client,
}

impl DownloadEngine {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = channel();
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(20))
            .build()
            .expect("HTTP client");
        Self {
            tx,
            rx,
            wake,
            controls: Mutex::new(Default::default()),
            limit_bps: Arc::new(AtomicU64::new(0)),
            bytes_total: Arc::new(AtomicU64::new(0)),
            client,
        }
    }

    /// Messages produced by worker threads since the last call.
    pub fn drain(&self) -> Vec<DownloadMsg> {
        self.rx.try_iter().collect()
    }

    pub fn start(&self, request: DownloadRequest) {
        let control = Arc::new(AtomicU8::new(RUN));
        self.controls
            .lock()
            .expect("lock")
            .insert(request.id.clone(), control.clone());
        let tx = self.tx.clone();
        let wake = self.wake.clone();
        let client = self.client.clone();
        let limit = self.limit_bps.clone();
        let total_counter = self.bytes_total.clone();
        std::thread::Builder::new()
            .name(format!(
                "download-{}",
                &request.id[..8.min(request.id.len())]
            ))
            .spawn(move || {
                let id = request.id.clone();
                let msg = match run(
                    &client,
                    request,
                    &control,
                    &limit,
                    &total_counter,
                    &tx,
                    &*wake,
                ) {
                    Ok(Outcome::Finished(path)) => DownloadMsg::Finished { id, path },
                    Ok(Outcome::Paused(received)) => DownloadMsg::Paused { id, received },
                    Ok(Outcome::Cancelled) => DownloadMsg::Cancelled { id },
                    Err(reason) => DownloadMsg::Failed { id, reason },
                };
                let _ = tx.send(msg);
                wake();
            })
            .expect("spawn download thread");
    }

    pub fn pause(&self, id: &str) {
        if let Some(c) = self.controls.lock().expect("lock").get(id) {
            c.store(PAUSE, Ordering::SeqCst);
        }
    }

    pub fn cancel(&self, id: &str) {
        if let Some(c) = self.controls.lock().expect("lock").get(id) {
            c.store(CANCEL, Ordering::SeqCst);
        }
    }

    pub fn forget(&self, id: &str) {
        self.controls.lock().expect("lock").remove(id);
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.controls.lock().expect("lock").contains_key(id)
    }
}

enum Outcome {
    Finished(PathBuf),
    Paused(u64),
    Cancelled,
}

pub fn part_path(final_path: &Path) -> PathBuf {
    let mut name = final_path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".part");
    final_path.with_file_name(name)
}

fn run(
    client: &reqwest::blocking::Client,
    req: DownloadRequest,
    control: &AtomicU8,
    limit: &AtomicU64,
    total_counter: &AtomicU64,
    tx: &Sender<DownloadMsg>,
    wake: &(dyn Fn() + Send + Sync),
) -> Result<Outcome, String> {
    let mut builder = client
        .get(&req.url)
        .header(reqwest::header::USER_AGENT, &req.user_agent);
    if let Some(r) = &req.referrer {
        builder = builder.header(reqwest::header::REFERER, r);
    }
    if let Some(c) = &req.cookie_header {
        builder = builder.header(reqwest::header::COOKIE, c);
    }
    if req.resume_from > 0 {
        builder = builder.header(
            reqwest::header::RANGE,
            format!("bytes={}-", req.resume_from),
        );
    }
    let mut response = builder.send().map_err(|e| format!("network error: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("server answered {status}"));
    }
    let resumed = req.resume_from > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT;
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    let length = response.content_length();
    let total = if resumed {
        length.map(|l| l + req.resume_from)
    } else {
        length
    };

    let final_path = match req.path {
        Some(p) => p,
        None => {
            let name = req
                .name_hint
                .filter(|h| !h.trim().is_empty())
                .or_else(|| {
                    response
                        .headers()
                        .get(reqwest::header::CONTENT_DISPOSITION)
                        .and_then(|v| v.to_str().ok())
                        .and_then(filename_from_disposition)
                })
                .or_else(|| filename_from_url(response.url().as_str()))
                .unwrap_or_else(|| "download".into());
            let name = haku_core::records::sanitize_file_name(&name);
            haku_core::records::unique_file_name(&req.directory, &name, |p| {
                p.exists() || part_path(p).exists()
            })
        }
    };
    let file_name = final_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "download".into());
    let part = part_path(&final_path);
    let _ = tx.send(DownloadMsg::Started {
        id: req.id.clone(),
        total,
        mime,
        file_name,
        path: final_path.clone(),
    });
    wake();

    let mut file: File = if resumed {
        OpenOptions::new()
            .append(true)
            .open(&part)
            .map_err(|e| format!("cannot open file: {e}"))?
    } else {
        File::create(&part).map_err(|e| format!("cannot create file: {e}"))?
    };
    let mut received = if resumed { req.resume_from } else { 0 };
    let mut buf = vec![0u8; 64 * 1024];
    let mut last_report = Instant::now();
    let mut window_start = Instant::now();
    let mut window_bytes = 0u64;
    let mut rate_bytes = 0u64;
    let mut rate_start = Instant::now();
    loop {
        match control.load(Ordering::SeqCst) {
            PAUSE => {
                file.flush().ok();
                return Ok(Outcome::Paused(received));
            }
            CANCEL => {
                drop(file);
                let _ = std::fs::remove_file(&part);
                return Ok(Outcome::Cancelled);
            }
            _ => {}
        }
        let n = response
            .read(&mut buf)
            .map_err(|e| format!("network error: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("write failed: {e}"))?;
        received += n as u64;
        total_counter.fetch_add(n as u64, Ordering::Relaxed);
        rate_bytes += n as u64;

        // Token-bucket style limiter: never exceed `limit` bytes per second on average.
        let cap = limit.load(Ordering::Relaxed);
        if cap > 0 {
            window_bytes += n as u64;
            let elapsed = window_start.elapsed().as_secs_f64();
            let allowed = elapsed * cap as f64;
            if window_bytes as f64 > allowed {
                let wait = (window_bytes as f64 - allowed) / cap as f64;
                std::thread::sleep(Duration::from_secs_f64(wait.min(1.0)));
            }
            if elapsed > 2.0 {
                window_start = Instant::now();
                window_bytes = 0;
            }
        }

        if last_report.elapsed() >= Duration::from_millis(250) {
            let bps = rate_bytes as f32 / rate_start.elapsed().as_secs_f32().max(0.001);
            let _ = tx.send(DownloadMsg::Progress {
                id: req.id.clone(),
                received,
                total,
                bps,
            });
            wake();
            last_report = Instant::now();
            if rate_start.elapsed() > Duration::from_secs(2) {
                rate_start = Instant::now();
                rate_bytes = 0;
            }
        }
    }
    file.sync_all().map_err(|e| format!("write failed: {e}"))?;
    drop(file);
    std::fs::rename(&part, &final_path).map_err(|e| format!("could not finish file: {e}"))?;
    Ok(Outcome::Finished(final_path))
}

/// `attachment; filename="a b.zip"; filename*=UTF-8''a%20b.zip` -> `a b.zip` (RFC 6266;
/// `filename*` wins).
pub fn filename_from_disposition(value: &str) -> Option<String> {
    let mut plain = None;
    for part in value.split(';').map(str::trim) {
        let Some((key, val)) = part.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let val = val.trim();
        if key == "filename*" {
            let encoded = val.split_once("''").map(|(_, v)| v).unwrap_or(val);
            if let Ok(decoded) = percent_decode(encoded.trim_matches('"')) {
                return Some(decoded);
            }
        } else if key == "filename" {
            plain = Some(val.trim_matches('"').to_owned());
        }
    }
    plain.filter(|s| !s.is_empty())
}

fn percent_decode(s: &str) -> Result<String, ()> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).map_err(|_| ())?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| ())?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| ())
}

pub fn filename_from_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let last = parsed.path_segments()?.rev().find(|s| !s.is_empty())?;
    percent_decode(last).ok().filter(|s| !s.is_empty())
}

/// Decode a `data:` URL into (mime, bytes).
pub fn decode_data_url(data_url: &str) -> Option<(String, Vec<u8>)> {
    use base64::Engine;
    let rest = data_url.strip_prefix("data:")?;
    let (meta, payload) = rest.split_once(',')?;
    let is_base64 = meta.ends_with(";base64");
    let mime = meta
        .trim_end_matches(";base64")
        .split(';')
        .next()
        .unwrap_or("")
        .to_owned();
    let bytes = if is_base64 {
        base64::engine::general_purpose::STANDARD
            .decode(payload)
            .ok()?
    } else {
        percent_decode(payload).ok()?.into_bytes()
    };
    Some((
        if mime.is_empty() {
            "text/plain".into()
        } else {
            mime
        },
        bytes,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disposition_names() {
        assert_eq!(
            filename_from_disposition("attachment; filename=\"a b.zip\"").as_deref(),
            Some("a b.zip")
        );
        assert_eq!(
            filename_from_disposition(
                "attachment; filename=\"x.zip\"; filename*=UTF-8''%E2%82%AC%20rates.zip"
            )
            .as_deref(),
            Some("€ rates.zip")
        );
        assert_eq!(filename_from_disposition("inline"), None);
    }

    #[test]
    fn url_names() {
        assert_eq!(
            filename_from_url("https://x.test/dl/My%20File.zip?x=1").as_deref(),
            Some("My File.zip")
        );
        assert_eq!(filename_from_url("https://x.test/").as_deref(), None);
    }

    #[test]
    fn data_urls() {
        let (m, b) = decode_data_url("data:text/plain;base64,aGk=").unwrap();
        assert_eq!((m.as_str(), b.as_slice()), ("text/plain", b"hi".as_slice()));
        let (_, b) = decode_data_url("data:,a%20b").unwrap();
        assert_eq!(b, b"a b");
    }

    #[test]
    fn part_paths() {
        assert_eq!(part_path(Path::new("/d/a.zip")), Path::new("/d/a.zip.part"));
    }
}

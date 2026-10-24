/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! My Flow's side on the local network: a small HTTP server for the phone page (`ui/flow.html`)
//! and its four requests. Everything the phone sends and receives is sealed (see
//! `haku_core::flow`), so the server only routes requests and refuses what does not open.
//!
//! It runs only while My Flow is on and a phone is paired or a QR code is showing. Windows asks
//! once whether HakuCX may accept connections from the network; it must be allowed on private
//! networks for phones to connect.

use std::io::Read;
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use haku_core::flow::{Flow, MAX_FILE};
use log::info;
use tiny_http::{Header, Method, Request, Response, Server};

/// Tried first, so a phone's bookmark keeps working; another free port if it is taken.
pub const DEFAULT_PORT: u16 = 47652;
const WORKERS: usize = 4;
/// A sealed 25 MB file in base64, plus JSON.
const MAX_BODY: usize = MAX_FILE / 3 * 4 + 64 * 1024;

const PAGE_CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; \
     img-src 'self' blob: data:; connect-src 'self'; base-uri 'none'; form-action 'none'; \
     frame-ancestors 'none'";

pub struct FlowServer {
    server: Arc<Server>,
    workers: Vec<JoinHandle<()>>,
    pub port: u16,
}

impl FlowServer {
    /// Listen on every interface. `changed` runs (on a worker thread) after a request that
    /// changed the store.
    pub fn start(
        flow: Arc<Mutex<Flow>>,
        port: Option<u16>,
        changed: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, String> {
        let server = [port.unwrap_or(DEFAULT_PORT), 0]
            .into_iter()
            .find_map(|p| Server::http(("0.0.0.0", p)).ok())
            .ok_or_else(|| "could not open a network port for My Flow".to_owned())?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .ok_or("no port")?;
        let server = Arc::new(server);
        let workers = (0..WORKERS)
            .filter_map(|i| {
                let (server, flow, changed) = (server.clone(), flow.clone(), changed.clone());
                std::thread::Builder::new()
                    .name(format!("my-flow-{i}"))
                    .spawn(move || {
                        while let Ok(request) = server.recv() {
                            handle(&flow, request, &*changed);
                        }
                    })
                    .ok()
            })
            .collect();
        info!("My Flow listening on port {port}");
        Ok(Self {
            server,
            workers,
            port,
        })
    }
}

impl Drop for FlowServer {
    fn drop(&mut self) {
        for _ in 0..self.workers.len() {
            self.server.unblock();
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

/// This computer's address on the local network: the one its default route uses. Connecting
/// a UDP socket only picks the route; nothing is sent.
pub fn lan_address() -> Option<std::net::IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified() && !ip.is_loopback()).then_some(ip)
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid header")
}

fn with_common<R: Read>(response: Response<R>) -> Response<R> {
    response
        .with_header(header("Cache-Control", "no-store"))
        .with_header(header("X-Content-Type-Options", "nosniff"))
        .with_header(header("Referrer-Policy", "no-referrer"))
}

fn respond<R: Read>(request: Request, response: Response<R>) {
    let _ = request.respond(with_common(response));
}

fn status(request: Request, code: u16) {
    respond(request, Response::empty(code));
}

fn json(request: Request, body: serde_json::Value) {
    respond(
        request,
        Response::from_string(body.to_string())
            .with_header(header("Content-Type", "application/json")),
    );
}

fn query(url: &str, name: &str) -> Option<String> {
    let q = url.split_once('?')?.1;
    url::form_urlencoded::parse(q.as_bytes())
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

/// Requests from web pages elsewhere (the browser sends their Origin) are refused; the phone
/// page's own requests carry its own origin or none.
fn same_origin(request: &Request) -> bool {
    let get = |name: &'static str| {
        request
            .headers()
            .iter()
            .find(|h| h.field.equiv(name))
            .map(|h| h.value.as_str().to_owned())
    };
    match (get("Origin"), get("Host")) {
        (None, _) => true,
        (Some(origin), Some(host)) => origin == format!("http://{host}"),
        (Some(_), None) => false,
    }
}

fn read_body(request: &mut Request) -> Option<serde_json::Value> {
    if request.body_length().is_some_and(|n| n > MAX_BODY) {
        return None;
    }
    let mut body = Vec::new();
    request
        .as_reader()
        .take(MAX_BODY as u64 + 1)
        .read_to_end(&mut body)
        .ok()?;
    if body.len() > MAX_BODY {
        return None;
    }
    serde_json::from_slice(&body).ok()
}

fn handle(flow: &Mutex<Flow>, mut request: Request, changed: &dyn Fn()) {
    let url = request.url().to_owned();
    let path = url.split('?').next().unwrap_or_default().to_owned();
    let now = haku_core::now_ms();
    if !same_origin(&request) {
        return status(request, 403);
    }
    let field =
        |v: &serde_json::Value, k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_owned);
    match (request.method().clone(), path.as_str()) {
        (Method::Get, "/" | "/index.html") => page(request, "flow.html"),
        (Method::Get, p) if p.starts_with("/assets/") => page(request, &p[1..]),
        (Method::Post, "/api/hello") => {
            let answer = read_body(&mut request).and_then(|b| {
                let (pairing, sealed) = (field(&b, "p")?, field(&b, "s")?);
                flow.lock().ok()?.hello(&pairing, &sealed, now)
            });
            match answer {
                Some(s) => {
                    changed();
                    json(request, serde_json::json!({ "s": s }));
                }
                None => status(request, 403),
            }
        }
        (Method::Post, "/api/send") => {
            let seq = read_body(&mut request).and_then(|b| {
                let (device, sealed) = (field(&b, "d")?, field(&b, "s")?);
                flow.lock().ok()?.receive(&device, &sealed, now)
            });
            match seq {
                Some(seq) => {
                    changed();
                    json(request, serde_json::json!({ "seq": seq }));
                }
                None => status(request, 403),
            }
        }
        (Method::Get, "/api/inbox") => {
            let reply = (|| {
                let (device, sealed) = (query(&url, "d")?, query(&url, "s")?);
                flow.lock().ok()?.inbox(&device, &sealed, now)
            })();
            match reply {
                Some(s) => {
                    changed();
                    json(request, serde_json::json!({ "s": s }));
                }
                None => status(request, 403),
            }
        }
        (Method::Get, "/api/file") => {
            let bytes = (|| {
                let (device, sealed) = (query(&url, "d")?, query(&url, "s")?);
                flow.lock().ok()?.file(&device, &sealed, now)
            })();
            match bytes {
                Some(b) => respond(
                    request,
                    Response::from_data(b)
                        .with_header(header("Content-Type", "application/octet-stream")),
                ),
                None => status(request, 403),
            }
        }
        _ => status(request, 404),
    }
}

/// The phone page and its script and style, from the UI build.
fn page(request: Request, path: &str) {
    if path.contains("..") || path.contains('\\') {
        return status(request, 404);
    }
    let Some(file) = crate::protocol::UI_DIST.get_file(path) else {
        return status(request, 404);
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut response = Response::from_data(file.contents().to_vec())
        .with_header(header("Content-Type", mime.as_ref()));
    if path.ends_with(".html") {
        response = response.with_header(header("Content-Security-Policy", PAGE_CSP));
    }
    respond(request, response);
}

/// The address a phone opens (the QR code), with the pairing in the fragment.
pub fn pairing_url(ip: std::net::IpAddr, port: u16, pairing: &str, key: &str) -> String {
    let host = match ip {
        std::net::IpAddr::V6(v6) => format!("[{v6}]"),
        v4 => v4.to_string(),
    };
    format!("http://{host}:{port}/#p={pairing}.{key}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(
            pairing_url("192.168.1.5".parse().unwrap(), 47652, "abc", "k-_"),
            "http://192.168.1.5:47652/#p=abc.k-_"
        );
        assert_eq!(
            query("/api/inbox?d=x%2By&s=abc", "d").as_deref(),
            Some("x+y")
        );
        assert_eq!(query("/api/inbox", "d"), None);
    }

    /// The server end to end on loopback: the page, pairing and a note from the "phone".
    #[test]
    fn serves_the_page_and_pairs() {
        use base64::Engine as _;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use haku_core::flow::{aad, open, seal};

        let dir = tempfile::tempdir().unwrap();
        let flow = Arc::new(Mutex::new(Flow::new("Desk".into(), dir.path().into())));
        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let h = hits.clone();
        let server = FlowServer::start(
            flow.clone(),
            Some(0),
            Arc::new(move || {
                h.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }),
        )
        .unwrap();
        let base = format!("http://127.0.0.1:{}", server.port);
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .build()
            .unwrap();

        let page = client.get(format!("{base}/")).send().unwrap();
        assert_eq!(page.status(), 200);
        assert!(
            page.headers()["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("default-src 'none'")
        );
        assert!(page.text().unwrap().contains("My Flow"));
        assert_eq!(
            client
                .get(format!("{base}/api/nothing"))
                .send()
                .unwrap()
                .status(),
            404
        );

        let (pairing, key) = flow.lock().unwrap().start_pairing(haku_core::now_ms());
        let pk: [u8; 32] = URL_SAFE_NO_PAD.decode(&key).unwrap().try_into().unwrap();
        let hello = seal(
            &pk,
            &aad(&pairing, "hello"),
            serde_json::json!({"id": "1", "t": haku_core::now_ms(), "name": "Test phone"})
                .to_string()
                .as_bytes(),
        );
        // Another site's page cannot call the API.
        let foreign = client
            .post(format!("{base}/api/hello"))
            .header("Origin", "https://evil.example")
            .json(&serde_json::json!({"p": pairing, "s": hello}))
            .send()
            .unwrap();
        assert_eq!(foreign.status(), 403);
        let r: serde_json::Value = client
            .post(format!("{base}/api/hello"))
            .json(&serde_json::json!({"p": pairing, "s": hello}))
            .send()
            .unwrap()
            .json()
            .unwrap();
        let welcome: serde_json::Value = serde_json::from_slice(
            &open(&pk, &aad(&pairing, "welcome"), r["s"].as_str().unwrap()).unwrap(),
        )
        .unwrap();
        let device = welcome["device"].as_str().unwrap().to_owned();
        let dk: [u8; 32] = URL_SAFE_NO_PAD
            .decode(welcome["key"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();

        let note = seal(&dk, &aad(&device, "send"), serde_json::json!({"id": "2", "t": haku_core::now_ms(), "kind": "text", "text": "hi from the phone"}).to_string().as_bytes());
        let sent = client
            .post(format!("{base}/api/send"))
            .json(&serde_json::json!({"d": device, "s": note}))
            .send()
            .unwrap();
        assert_eq!(sent.status(), 200);
        assert_eq!(flow.lock().unwrap().entries()[0].text, "hi from the phone");
        // The same request again is refused.
        let again = client
            .post(format!("{base}/api/send"))
            .json(&serde_json::json!({"d": device, "s": note}))
            .send()
            .unwrap();
        assert_eq!(again.status(), 403);
        assert!(hits.load(std::sync::atomic::Ordering::SeqCst) >= 2);
        drop(server);
    }
}

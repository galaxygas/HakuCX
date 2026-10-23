/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! `haku://` scheme.
//!
//! - `haku://ui/...` serves the browser UI, compiled into the binary from `ui/dist`.
//! - `haku://wallpaper/<file>` serves the profile's custom wallpapers.
//!
//! Internal pages such as `haku://settings/` are not served here: the UI shell renders them
//! itself (see `browser::tabs`), and web content is never allowed to navigate to `haku://`.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use headers::{ContentType, HeaderMapExt};
use http::{HeaderName, HeaderValue};
use servo::protocol_handler::{
    DoneChannel, FetchContext, NetworkError, ProtocolHandler, Request, ResourceFetchTiming,
    Response, ResponseBody,
};

/// The production UI, built by `build.rs` from `ui/`.
static UI_DIST: include_dir::Dir<'static> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/../../ui/dist");

/// Content-Security-Policy for the UI: its own scripts only, no inline script, and network
/// access only for the public APIs the UI uses directly (images, weather, feeds).
const UI_CSP: &str = "default-src 'self' haku:; script-src 'self' haku:; \
     style-src 'self' haku: 'unsafe-inline'; img-src 'self' haku: data: blob: https:; \
     font-src 'self' haku: data:; connect-src 'self' haku: https:; media-src 'self' haku: blob: data:; \
     frame-src 'none'; object-src 'none'; frame-ancestors 'none'; base-uri 'none'";

pub struct HakuProtocolHandler {
    wallpapers: Option<PathBuf>,
}

impl HakuProtocolHandler {
    pub fn new(wallpapers: Option<PathBuf>) -> Self {
        Self { wallpapers }
    }

    fn lookup(&self, host: &str, path: &str) -> Option<Vec<u8>> {
        match host {
            "ui" => UI_DIST.get_file(path).map(|f| f.contents().to_vec()),
            "wallpaper" => {
                let dir = self.wallpapers.as_ref()?;
                // Only plain file names that we created (custom-<ms>.<ext>).
                if path.contains(['/', '\\']) || !path.starts_with("custom-") {
                    return None;
                }
                std::fs::read(dir.join(path)).ok()
            }
            _ => None,
        }
    }
}

impl ProtocolHandler for HakuProtocolHandler {
    fn is_fetchable(&self) -> bool {
        true
    }

    fn is_secure(&self) -> bool {
        true
    }

    fn load(
        &self,
        request: &mut Request,
        _done_chan: &mut DoneChannel,
        _context: &FetchContext,
    ) -> Pin<Box<dyn Future<Output = Response> + Send>> {
        let url = request.current_url();
        let host = url.host_str().unwrap_or_default().to_owned();
        let mut path = url.path().trim_start_matches('/').to_owned();
        if path.is_empty() || path.ends_with('/') {
            path.push_str("index.html");
        }
        if path
            .split('/')
            .any(|segment| segment == ".." || segment.is_empty())
        {
            return Box::pin(std::future::ready(Response::network_error(
                NetworkError::ResourceLoadError("invalid path".to_owned()),
            )));
        }
        let response = match self.lookup(&host, &path) {
            Some(bytes) => {
                let mut response =
                    Response::new(url.clone(), ResourceFetchTiming::new(request.timing_type()));
                let mime = mime_guess::from_path(&path).first_or_octet_stream();
                response.headers.typed_insert(ContentType::from(mime));
                response.headers.insert(
                    HeaderName::from_static("content-security-policy"),
                    HeaderValue::from_static(UI_CSP),
                );
                response.headers.insert(
                    HeaderName::from_static("x-content-type-options"),
                    HeaderValue::from_static("nosniff"),
                );
                *response.body.lock() = ResponseBody::Done(bytes);
                response
            }
            None => Response::network_error(NetworkError::ResourceLoadError(format!(
                "haku://{host}/{path} not found"
            ))),
        };
        Box::pin(std::future::ready(response))
    }
}

/// The page shown instead of a site on a malware blocklist.
pub fn blocked_page(url: &str) -> String {
    let safe = html_escape::encode_text(url);
    format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Blocked: dangerous site</title>
<style>body{{margin:0;min-height:100vh;display:grid;place-items:center;background:#120f19;color:#eee;font:15px system-ui,sans-serif}}
main{{max-width:560px;padding:32px;border:1px solid #ff4458;border-radius:12px;background:#1a1420}}
h1{{color:#ff4458;margin-top:0}} code{{word-break:break-all;color:#ffb3bd}}</style></head>
<body><main><h1>HakuCX blocked this site</h1>
<p>The address <code>{safe}</code> is on a malware blocklist (URLhaus). Opening it could harm your computer or steal your data.</p>
<p>You can turn this protection off in Settings &rarr; Privacy &amp; security &rarr; Protect me from malicious sites.</p></main></body></html>"#
    )
}

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! `haku://` scheme: serves the browser's own UI assets, which are compiled into the binary.

use std::future::Future;
use std::pin::Pin;

use headers::{ContentType, HeaderMapExt};
use http::{HeaderName, HeaderValue};
use servo::protocol_handler::{
    DoneChannel, FetchContext, NetworkError, ProtocolHandler, Request, ResourceFetchTiming,
    Response, ResponseBody,
};

/// Looks up an embedded asset by its path inside `haku://<host>/<path>`.
pub type AssetLookup = fn(host: &str, path: &str) -> Option<&'static [u8]>;

pub struct HakuProtocolHandler {
    lookup: AssetLookup,
}

impl HakuProtocolHandler {
    pub fn new(lookup: AssetLookup) -> Self {
        Self { lookup }
    }
}

/// Content-Security-Policy for every internal page: no remote scripts, no inline handlers
/// from third parties, no framing by web content.
const INTERNAL_CSP: &str = "default-src 'self' haku:; script-src 'self' haku:; \
     style-src 'self' haku: 'unsafe-inline'; img-src 'self' haku: data: blob: https:; \
     font-src 'self' haku: data:; connect-src 'self' haku: https:; media-src 'self' haku: blob: data:; \
     frame-ancestors 'none'";

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
        if path.split('/').any(|segment| segment == "..") {
            return Box::pin(std::future::ready(Response::network_error(
                NetworkError::ResourceLoadError("invalid path".to_owned()),
            )));
        }

        let response = match (self.lookup)(&host, &path) {
            Some(bytes) => {
                let mut response =
                    Response::new(url.clone(), ResourceFetchTiming::new(request.timing_type()));
                let mime = mime_guess::from_path(&path).first_or_octet_stream();
                response.headers.typed_insert(ContentType::from(mime));
                response.headers.insert(
                    HeaderName::from_static("content-security-policy"),
                    HeaderValue::from_static(INTERNAL_CSP),
                );
                *response.body.lock() = ResponseBody::Done(bytes.to_vec());
                response
            }
            None => Response::network_error(NetworkError::ResourceLoadError(format!(
                "haku://{host}/{path} not found"
            ))),
        };
        Box::pin(std::future::ready(response))
    }
}

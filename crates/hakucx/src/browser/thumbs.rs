/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Tab previews for the tab hover card ("Show tab previews"): a small JPEG of each tab, taken
//! while the tab is on screen (Servo does not paint hidden pages), at most every
//! [`EVERY`]. Thumbnails stay in memory only.

use std::collections::HashMap;
use std::io::Cursor;
use std::time::{Duration, Instant};

use base64::Engine as _;
use haku_core::ipc::Event;
use haku_core::session::TabId;

use super::Browser;

const EVERY: Duration = Duration::from_secs(20);
const WIDTH: u32 = 320;

#[derive(Default)]
pub struct Thumbs {
    /// When each tab was last captured, and the address it showed.
    taken: HashMap<TabId, (Instant, String)>,
}

/// Shrink a screenshot to the preview width and encode it as a JPEG data URL.
pub fn encode(image: &image::RgbaImage) -> Option<String> {
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let height = ((h as u64 * WIDTH as u64) / w as u64).clamp(1, 400) as u32;
    let small = image::imageops::thumbnail(image, WIDTH, height);
    let rgb = image::DynamicImage::ImageRgba8(small).to_rgb8();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(Cursor::new(&mut jpeg), 70)
        .encode_image(&rgb)
        .ok()?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&jpeg)
    ))
}

impl Browser {
    /// Capture the visible tab if its preview is old or shows another page (every second,
    /// with the metrics).
    pub(crate) fn thumb_tick(&self) {
        if !self.settings.borrow().tabs.tab_previews {
            return;
        }
        let Some(tab) = self.active_tab_id() else {
            return;
        };
        let Some(webview) = self.pane_webview(super::tabs::Pane::Main) else {
            return;
        };
        if self.tab_for_webview(&webview).as_ref() != Some(&tab) {
            return;
        }
        let url = webview.url().map(|u| u.to_string()).unwrap_or_default();
        let loading = self.views.borrow().get(&tab).is_some_and(|v| v.loading);
        let due = !loading
            && self
                .thumbs
                .borrow()
                .taken
                .get(&tab)
                .is_none_or(|(at, shown)| at.elapsed() >= EVERY || *shown != url);
        if !due {
            return;
        }
        self.thumbs
            .borrow_mut()
            .taken
            .insert(tab.clone(), (Instant::now(), url));
        let sender = self.background_sender();
        webview.take_screenshot(None, move |result| {
            let Ok(image) = result else { return };
            // Encoding takes a few milliseconds; keep it off the UI thread.
            std::thread::spawn(move || {
                if let Some(data) = encode(&image) {
                    sender.send(Box::new(move |b: &Browser| {
                        b.emit(Event::TabThumb { tab, image: data });
                    }));
                }
            });
        });
    }

    pub(crate) fn thumb_forget(&self, tab: &str) {
        self.thumbs.borrow_mut().taken.remove(tab);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnails_are_small_jpegs() {
        let image = image::RgbaImage::from_pixel(1280, 720, image::Rgba([200, 30, 80, 255]));
        let url = encode(&image).unwrap();
        assert!(url.starts_with("data:image/jpeg;base64,"));
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(url.trim_start_matches("data:image/jpeg;base64,"))
            .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (320, 180));
        assert!(encode(&image::RgbaImage::new(0, 0)).is_none());
    }
}

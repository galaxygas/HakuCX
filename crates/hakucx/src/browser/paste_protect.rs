/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Page clipboard access, and Paste Protect (Settings → Privacy & security).
//!
//! HakuCX handles the clipboard for pages itself (Servo lets the embedder do it). When you copy
//! something that looks like payment details or a crypto address from a page, HakuCX remembers
//! it for two minutes; if what you then paste into a page is different (another program or a
//! site replaced it, a known way to redirect payments), it warns before you send it.

use std::time::{Duration, Instant};

use haku_core::ipc::Event;
use log::warn;
use servo::{ClipboardDelegate, StringRequest, WebView};

use super::Browser;

const WATCH: Duration = Duration::from_secs(120);

/// Does the text look like an IBAN, a payment card number or a crypto address?
pub fn sensitive(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() || t.len() > 64 {
        return false;
    }
    let compact: String = t
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    iban(&compact) || card(&compact) || crypto(&compact)
}

fn iban(s: &str) -> bool {
    let s = s.to_ascii_uppercase();
    if !(15..=34).contains(&s.len())
        || !s[..2].chars().all(|c| c.is_ascii_uppercase())
        || !s[2..4].chars().all(|c| c.is_ascii_digit())
        || !s.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return false;
    }
    // ISO 13616: move the first four characters to the end, letters become 10..35, mod 97 = 1.
    let rearranged = format!("{}{}", &s[4..], &s[..4]);
    let mut rem: u32 = 0;
    for c in rearranged.chars() {
        let digits = if c.is_ascii_digit() {
            c.to_string()
        } else {
            (c as u32 - 'A' as u32 + 10).to_string()
        };
        for d in digits.chars() {
            rem = (rem * 10 + d.to_digit(10).unwrap_or(0)) % 97;
        }
    }
    rem == 1
}

fn card(s: &str) -> bool {
    if !(13..=19).contains(&s.len()) || !s.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    // Luhn.
    let sum: u32 = s
        .chars()
        .rev()
        .filter_map(|c| c.to_digit(10))
        .enumerate()
        .map(|(i, d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

fn crypto(s: &str) -> bool {
    let base58 = |c: char| c.is_ascii_alphanumeric() && !matches!(c, '0' | 'O' | 'I' | 'l');
    let eth = s.len() == 42 && s.starts_with("0x") && s[2..].chars().all(|c| c.is_ascii_hexdigit());
    let btc_segwit = (s.starts_with("bc1") && (42..=62).contains(&s.len()))
        && s.chars().all(|c| c.is_ascii_alphanumeric());
    let btc_legacy = (s.starts_with('1') || s.starts_with('3'))
        && (26..=35).contains(&s.len())
        && s.chars().all(base58);
    eth || btc_segwit || btc_legacy
}

/// A sensitive value copied from a page, watched for a while.
#[derive(Default)]
pub struct PasteGuard {
    copied: Option<(String, Instant)>,
}

fn mask(text: &str) -> String {
    let t: Vec<char> = text.trim().chars().collect();
    if t.len() <= 8 {
        return "it".into();
    }
    let head: String = t[..4].iter().collect();
    let tail: String = t[t.len() - 4..].iter().collect();
    format!("{head}…{tail}")
}

fn system_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

impl ClipboardDelegate for Browser {
    fn clear(&self, _webview: WebView) {
        if let Ok(mut c) = arboard::Clipboard::new() {
            let _ = c.clear();
        }
    }

    fn get_text(&self, _webview: WebView, request: StringRequest) {
        let Some(text) = system_text() else {
            request.failure("the clipboard is empty or holds no text".into());
            return;
        };
        let protect = self.settings.borrow().privacy.paste_protect_hijack;
        let copied = self.paste_guard.borrow_mut().copied.take();
        if protect && let Some((value, at)) = copied {
            if at.elapsed() < WATCH && value.trim() != text.trim() {
                warn!("Paste Protect: the clipboard changed after a sensitive copy");
                let shown = mask(&value);
                self.defer(move |b| {
                    b.emit(Event::Toast {
                        text: format!(
                            "Paste Protect: the clipboard changed since you copied {shown}. Check what you pasted before you send it."
                        ),
                        kind: "error".into(),
                    })
                });
            } else if at.elapsed() < WATCH {
                // Still the same: keep watching until it runs out.
                self.paste_guard.borrow_mut().copied = Some((value, at));
            }
        }
        request.success(text);
    }

    fn set_text(&self, _webview: WebView, new_contents: String) {
        if self.settings.borrow().privacy.paste_protect_hijack && sensitive(&new_contents) {
            self.paste_guard.borrow_mut().copied = Some((new_contents.clone(), Instant::now()));
        }
        match arboard::Clipboard::new().and_then(|mut c| c.set_text(new_contents)) {
            Ok(()) => {}
            Err(e) => warn!("clipboard: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_values() {
        assert!(sensitive("GB82 WEST 1234 5698 7654 32"), "a valid IBAN");
        assert!(
            !sensitive("GB82 WEST 1234 5698 7654 33"),
            "bad IBAN checksum"
        );
        assert!(sensitive("4111 1111 1111 1111"), "a Luhn-valid card number");
        assert!(!sensitive("4111 1111 1111 1112"));
        assert!(sensitive("0x52908400098527886E0F7030069857D2E4169EE7"));
        assert!(sensitive("bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq"));
        assert!(sensitive("1BoatSLRHtKNngkdXEeobR76b53LETtpyT"));
        assert!(!sensitive("hello world"));
        assert!(!sensitive("12345"));
        assert_eq!(mask("GB82WEST12345698765432"), "GB82…5432");
        assert_eq!(mask("short"), "it");
    }
}

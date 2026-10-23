/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Extension localization: `_locales/<lang>/messages.json`, `chrome.i18n.getMessage` and
//! `__MSG_name__` substitution in the manifest and CSS.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

/// Messages for one extension, already merged: the best UI locale over the default locale.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct Messages {
    /// Lower-cased message name -> (message, placeholders: lower-cased name -> content).
    entries: BTreeMap<String, (String, BTreeMap<String, String>)>,
    pub locale: String,
}

impl Messages {
    /// Load for `ui_locale` (e.g. `en-US`), falling back to its language (`en`) and then to the
    /// extension's `default_locale`.
    pub fn load(root: &Path, default_locale: Option<&str>, ui_locale: &str) -> Messages {
        let mut candidates = vec![ui_locale.replace('-', "_")];
        if let Some((lang, _)) = ui_locale.split_once(['-', '_']) {
            candidates.push(lang.to_owned());
        }
        let mut messages = Messages::default();
        // Lowest priority first, so better matches overwrite.
        let mut order: Vec<String> = default_locale
            .map(|d| vec![d.to_owned()])
            .unwrap_or_default();
        order.extend(candidates.into_iter().rev());
        for locale in order {
            let path = root.join("_locales").join(&locale).join("messages.json");
            if let Ok(text) = std::fs::read_to_string(&path) {
                let cleaned =
                    super::manifest::strip_json_comments(text.trim_start_matches('\u{feff}'));
                if let Ok(v) = serde_json::from_str::<Value>(&cleaned) {
                    messages.merge(&v);
                    messages.locale = locale;
                }
            }
        }
        messages
    }

    pub fn from_json(v: &Value, locale: &str) -> Messages {
        let mut m = Messages {
            locale: locale.to_owned(),
            ..Default::default()
        };
        m.merge(v);
        m
    }

    fn merge(&mut self, v: &Value) {
        let Some(map) = v.as_object() else { return };
        for (name, entry) in map {
            let Some(message) = entry.get("message").and_then(Value::as_str) else {
                continue;
            };
            let placeholders = entry
                .get("placeholders")
                .and_then(Value::as_object)
                .map(|p| {
                    p.iter()
                        .filter_map(|(k, v)| {
                            Some((k.to_lowercase(), v.get("content")?.as_str()?.to_owned()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            self.entries
                .insert(name.to_lowercase(), (message.to_owned(), placeholders));
        }
    }

    /// `chrome.i18n.getMessage(name, substitutions)`. Unknown names give an empty string.
    pub fn get(&self, name: &str, substitutions: &[String], extension_id: &str) -> String {
        let lower = name.to_lowercase();
        if let Some(predefined) = self.predefined(&lower, extension_id) {
            return predefined;
        }
        let Some((message, placeholders)) = self.entries.get(&lower) else {
            return String::new();
        };
        // Placeholders first ($name$ -> content, which may contain $1..$9), then $1..$9.
        let mut out = String::new();
        let mut rest = message.as_str();
        while let Some(start) = rest.find('$') {
            out.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            if let Some(end) = after.find('$') {
                let key = after[..end].to_lowercase();
                if let Some(content) = placeholders.get(&key) {
                    out.push_str(content);
                    rest = &after[end + 1..];
                    continue;
                }
            }
            out.push('$');
            rest = after;
        }
        out.push_str(rest);
        substitute_numbers(&out, substitutions)
    }

    fn predefined(&self, name: &str, extension_id: &str) -> Option<String> {
        let rtl = ["ar", "he", "fa", "ur"]
            .iter()
            .any(|l| self.locale.starts_with(l));
        Some(match name {
            "@@extension_id" => extension_id.to_owned(),
            "@@ui_locale" => {
                if self.locale.is_empty() {
                    "en".into()
                } else {
                    self.locale.clone()
                }
            }
            "@@bidi_dir" => {
                if rtl {
                    "rtl".into()
                } else {
                    "ltr".into()
                }
            }
            "@@bidi_reversed_dir" => {
                if rtl {
                    "ltr".into()
                } else {
                    "rtl".into()
                }
            }
            "@@bidi_start_edge" => {
                if rtl {
                    "right".into()
                } else {
                    "left".into()
                }
            }
            "@@bidi_end_edge" => {
                if rtl {
                    "left".into()
                } else {
                    "right".into()
                }
            }
            _ => return None,
        })
    }

    /// Replace `__MSG_name__` in a string (manifest values, CSS).
    pub fn localize(&self, text: &str, extension_id: &str) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(start) = rest.find("__MSG_") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 6..];
            match after.find("__") {
                Some(end) => {
                    out.push_str(&self.get(&after[..end], &[], extension_id));
                    rest = &after[end + 2..];
                }
                None => {
                    out.push_str(&rest[start..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out
    }

    /// Every message name -> message, for the in-page `getMessage` (names lower-cased).
    pub fn to_json(&self) -> Value {
        Value::Object(
            self.entries
                .iter()
                .map(|(k, (msg, ph))| {
                    (
                        k.clone(),
                        serde_json::json!({ "message": msg, "placeholders": ph }),
                    )
                })
                .collect(),
        )
    }
}

fn substitute_numbers(text: &str, subs: &[String]) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            match chars.peek() {
                Some('$') => {
                    chars.next();
                    out.push('$');
                }
                Some(d @ '1'..='9') => {
                    let i = *d as usize - '1' as usize;
                    chars.next();
                    if let Some(s) = subs.get(i) {
                        out.push_str(s);
                    }
                }
                _ => out.push('$'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Localize every string in a JSON value (used on the manifest for display and getManifest).
pub fn localize_value(v: &Value, messages: &Messages, extension_id: &str) -> Value {
    match v {
        Value::String(s) if s.contains("__MSG_") => {
            Value::String(messages.localize(s, extension_id))
        }
        Value::Array(a) => Value::Array(
            a.iter()
                .map(|x| localize_value(x, messages, extension_id))
                .collect(),
        ),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| (k.clone(), localize_value(x, messages, extension_id)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn messages() -> Messages {
        Messages::from_json(
            &serde_json::json!({
                "appName": { "message": "Dark Mode" },
                "greet": { "message": "Hello $USER$, you have $1 new $$ items", "placeholders": { "user": { "content": "$2" } } }
            }),
            "en",
        )
    }

    #[test]
    fn get_message_with_placeholders_and_substitutions() {
        let m = messages();
        assert_eq!(m.get("APPNAME", &[], "x"), "Dark Mode");
        assert_eq!(
            m.get("greet", &["3".into(), "Ada".into()], "x"),
            "Hello Ada, you have 3 new $ items"
        );
        assert_eq!(m.get("missing", &[], "x"), "");
        assert_eq!(m.get("@@extension_id", &[], "abc"), "abc");
        assert_eq!(m.get("@@bidi_dir", &[], "abc"), "ltr");
    }

    #[test]
    fn localize_strings() {
        let m = messages();
        assert_eq!(
            m.localize("__MSG_appName__ (beta)", "x"),
            "Dark Mode (beta)"
        );
        assert_eq!(m.localize("no markers", "x"), "no markers");
        assert_eq!(m.localize("__MSG_unterminated", "x"), "__MSG_unterminated");
    }

    #[test]
    fn load_prefers_ui_locale() {
        let dir = tempfile::tempdir().unwrap();
        for (loc, text) in [("en", "Hello"), ("de", "Hallo"), ("pt_BR", "Olá")] {
            let p = dir.path().join("_locales").join(loc);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(
                p.join("messages.json"),
                format!(r#"{{"hi": {{"message": "{text}"}}, "only_{loc}": {{"message": "x"}}}}"#),
            )
            .unwrap();
        }
        let m = Messages::load(dir.path(), Some("en"), "pt-BR");
        assert_eq!(m.get("hi", &[], ""), "Olá");
        assert_eq!(m.get("only_en", &[], ""), "x", "default locale fills gaps");
        let m = Messages::load(dir.path(), Some("en"), "de-AT");
        assert_eq!(m.get("hi", &[], ""), "Hallo");
    }
}

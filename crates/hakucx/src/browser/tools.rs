/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! GX Cleaner, plain HTTP fetches for the UI's feeds, page text, and the AI assistant.

use std::path::Path;
use std::time::Duration;

use haku_core::ipc::{AiMessage, CleanerStats, Event};
use haku_core::records::DownloadState;
use haku_core::settings::{AiProvider, Cleaner};
use log::warn;

use super::Browser;

const KEYRING_SERVICE: &str = "HakuCX";
const KEYRING_USER: &str = "ai-provider-key";
const MAX_HTTP_BYTES: usize = 5 * 1024 * 1024;

/// The AI provider key from the Windows Credential Manager (never stored in settings.json).
pub fn ai_key() -> Option<String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .ok()?
        .get_password()
        .ok()
        .filter(|k| !k.is_empty())
}

fn dir_size(path: &Path, budget: &mut u32) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            total += dir_size(&entry.path(), budget);
        } else {
            total += meta.len();
        }
    }
    total
}

impl Browser {
    pub(crate) fn cleaner_stats(&self) {
        let mut budget = 200_000;
        let profile_bytes = self
            .paths
            .as_ref()
            .map(|p| dir_size(&p.root, &mut budget))
            .unwrap_or(0);
        let downloads = self.downloads.borrow();
        let download_bytes = downloads
            .items
            .iter()
            .filter(|d| d.state == DownloadState::Completed)
            .filter_map(|d| std::fs::metadata(&d.path).ok().map(|m| m.len()))
            .sum();
        let stats = CleanerStats {
            profile_bytes,
            history_visits: self.history.visit_count().unwrap_or(0).max(0) as u64,
            downloads: downloads.items.len() as u32,
            download_bytes,
            sites_with_data: self
                .servo
                .site_data_manager()
                .site_data(servo::StorageType::all())
                .len() as u32,
            cache_entries: self.servo.network_manager().cache_entries().len() as u32,
            open_tabs: self.session.borrow().tabs.len() as u32,
        };
        drop(downloads);
        self.emit(Event::CleanerStats { stats });
    }

    pub(crate) fn run_cleaner(&self, cleaner: &Cleaner) {
        let now = haku_core::now_ms();
        let cutoff = |age: haku_core::settings::CleanerAge| now - age.seconds() as i64 * 1000;
        let mut done = Vec::new();
        if cleaner.cache.enabled {
            self.servo.network_manager().clear_cache();
            done.push("cache");
        }
        if cleaner.cookies.enabled {
            self.servo.site_data_manager().clear_cookies(None);
            done.push("cookies");
        }
        if cleaner.history.enabled && !self.private {
            let to = if cleaner.history.age.seconds() == 0 {
                i64::MAX
            } else {
                cutoff(cleaner.history.age)
            };
            if let Err(e) = self.history.delete_range(i64::MIN, to) {
                warn!("cleaner history: {e}");
            }
            done.push("history");
        }
        if cleaner.tabs.enabled {
            let threshold = cutoff(cleaner.tabs.age);
            let victims: Vec<String> = {
                let session = self.session.borrow();
                let active: Vec<&String> = session.active_tabs.values().collect();
                session
                    .tabs
                    .iter()
                    .filter(|t| !t.pinned && !active.contains(&&t.id) && t.last_active < threshold)
                    .map(|t| t.id.clone())
                    .collect()
            };
            if !victims.is_empty() {
                let mut session = self.session.borrow_mut();
                for id in &victims {
                    session.close(id);
                }
            }
            self.session_changed();
            done.push("tabs");
        }
        if cleaner.downloads.enabled {
            let threshold = if cleaner.downloads.age.seconds() == 0 {
                i64::MAX
            } else {
                cutoff(cleaner.downloads.age)
            };
            let removed = {
                let mut downloads = self.downloads.borrow_mut();
                let mut removed = downloads.clear_finished(Some(threshold));
                if cleaner.include_unfinished_downloads {
                    let (gone, keep): (Vec<_>, Vec<_>) = downloads.items.drain(..).partition(|d| {
                        d.state == DownloadState::Paused && d.started_at < threshold
                    });
                    downloads.items = keep;
                    for d in &gone {
                        let _ =
                            std::fs::remove_file(crate::downloads::part_path(Path::new(&d.path)));
                    }
                    removed.extend(gone);
                }
                removed
            };
            if cleaner.delete_downloaded_files {
                for d in removed
                    .iter()
                    .filter(|d| d.state == DownloadState::Completed)
                {
                    let _ = std::fs::remove_file(&d.path);
                }
            }
            self.downloads_changed();
            done.push("downloads");
        }
        self.play_sound("tab-slash");
        self.emit(Event::CleanerDone {
            summary: if done.is_empty() {
                "Nothing selected to clean".into()
            } else {
                format!("Cleaned {}", done.join(", "))
            },
        });
        self.cleaner_stats();
    }

    pub(crate) fn http_get(&self, request: u32, url: String) {
        let ok = url::Url::parse(&url)
            .map(|u| matches!(u.scheme(), "https" | "http"))
            .unwrap_or(false);
        if !ok {
            self.emit(Event::HttpResult {
                request,
                status: 0,
                body: String::new(),
                error: Some("only http(s) URLs".into()),
            });
            return;
        }
        let sender = self.background_sender();
        let user_agent = servo::prefs::get().user_agent.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<(u16, String), String> {
                let client = reqwest::blocking::Client::builder()
                    .timeout(Duration::from_secs(20))
                    .user_agent(user_agent)
                    .build()
                    .map_err(|e| e.to_string())?;
                let response = client.get(&url).send().map_err(|e| e.to_string())?;
                let status = response.status().as_u16();
                let bytes = response.bytes().map_err(|e| e.to_string())?;
                if bytes.len() > MAX_HTTP_BYTES {
                    return Err("response too large".into());
                }
                Ok((status, String::from_utf8_lossy(&bytes).into_owned()))
            })();
            sender.send(Box::new(move |b: &Browser| match result {
                Ok((status, body)) => b.emit(Event::HttpResult {
                    request,
                    status,
                    body,
                    error: None,
                }),
                Err(error) => b.emit(Event::HttpResult {
                    request,
                    status: 0,
                    body: String::new(),
                    error: Some(error),
                }),
            }));
        });
    }

    pub(crate) fn page_text(&self, request: u32, tab: Option<String>) {
        let Some(tab) = self.target_tab(tab) else {
            return;
        };
        let (title, url) = self
            .session
            .borrow()
            .tab(&tab)
            .map(|t| (t.title.clone(), t.url.clone()))
            .unwrap_or_default();
        let Some(webview) = self.webview_for_tab(&tab) else {
            self.emit(Event::PageText {
                request,
                title,
                url,
                text: String::new(),
            });
            return;
        };
        let browser = self.rc();
        webview.evaluate_javascript(
            "(document.body ? document.body.innerText : '').slice(0, 40000)",
            move |result| {
                let text = match result {
                    Ok(servo::JSValue::String(s)) => s,
                    _ => String::new(),
                };
                browser.emit(Event::PageText {
                    request,
                    title,
                    url,
                    text,
                });
            },
        );
    }

    pub(crate) fn set_ai_key(&self, key: Option<String>) {
        let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER);
        let result = match (entry, key) {
            (Ok(e), Some(k)) if !k.trim().is_empty() => {
                e.set_password(k.trim()).map_err(|e| e.to_string())
            }
            (Ok(e), _) => match e.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(err) => Err(err.to_string()),
            },
            (Err(e), _) => Err(e.to_string()),
        };
        if let Err(e) = result {
            self.toast(format!("Could not store the key: {e}"), "error");
        }
        let stored = ai_key().is_some();
        self.settings.borrow_mut().ai.key_in_vault = stored;
        self.settings_changed();
        self.emit(Event::AiKey { stored });
    }

    pub(crate) fn ai_chat(&self, request: u32, messages: Vec<AiMessage>) {
        let ai = self.settings.borrow().ai.clone();
        if !ai.enabled || ai.provider == AiProvider::None {
            self.emit(Event::AiReply {
                request,
                text: String::new(),
                error: Some("Choose an AI provider in Settings > AI first.".into()),
            });
            return;
        }
        let key = ai_key();
        let sender = self.background_sender();
        std::thread::spawn(move || {
            let result = call_ai(&ai, key.as_deref(), &messages);
            sender.send(Box::new(move |b: &Browser| match result {
                Ok(text) => b.emit(Event::AiReply {
                    request,
                    text,
                    error: None,
                }),
                Err(error) => b.emit(Event::AiReply {
                    request,
                    text: String::new(),
                    error: Some(error),
                }),
            }));
        });
    }
}

impl Browser {
    pub(crate) fn open_text_file(&self, request: u32, extensions: &[String]) {
        let exts: Vec<&str> = extensions
            .iter()
            .map(String::as_str)
            .filter(|e| e.chars().all(|c| c.is_ascii_alphanumeric()))
            .collect();
        let mut dialog = rfd::FileDialog::new().set_parent(&self.window);
        if !exts.is_empty() {
            dialog = dialog.add_filter("Files", &exts);
        }
        let Some(path) = dialog.pick_file() else {
            return;
        };
        match std::fs::metadata(&path) {
            Ok(m) if m.len() > 8 * 1024 * 1024 => {
                self.toast("That file is too large.", "error");
                return;
            }
            Err(e) => {
                self.toast(format!("Could not read the file: {e}"), "error");
                return;
            }
            _ => {}
        }
        match std::fs::read_to_string(&path) {
            Ok(content) => self.emit(Event::TextFile {
                request,
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                content,
            }),
            Err(e) => self.toast(format!("Could not read the file: {e}"), "error"),
        }
    }

    pub(crate) fn save_text_file(&self, suggested: &str, content: &str) {
        let name = haku_core::records::sanitize_file_name(suggested);
        let Some(path) = rfd::FileDialog::new()
            .set_parent(&self.window)
            .set_file_name(&name)
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, content) {
            Ok(()) => self.toast(format!("Saved {}", path.display()), "info"),
            Err(e) => self.toast(format!("Could not save: {e}"), "error"),
        }
    }

    pub(crate) fn pick_download_dir(&self) {
        let Some(dir) = rfd::FileDialog::new()
            .set_parent(&self.window)
            .pick_folder()
        else {
            return;
        };
        self.settings.borrow_mut().downloads.directory = dir.display().to_string();
        self.settings_changed();
    }
}

fn call_ai(
    ai: &haku_core::settings::Ai,
    key: Option<&str>,
    messages: &[AiMessage],
) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;
    let msgs: Vec<serde_json::Value> = messages
        .iter()
        .filter(|m| m.role == "user" || m.role == "assistant")
        .map(|m| serde_json::json!({ "role": m.role, "content": m.content }))
        .collect();
    let json = |r: reqwest::blocking::Response| -> Result<serde_json::Value, String> {
        let status = r.status();
        let body: serde_json::Value = r.json().map_err(|e| e.to_string())?;
        if !status.is_success() {
            let message = body
                .pointer("/error/message")
                .and_then(|m| m.as_str())
                .unwrap_or("request failed");
            return Err(format!("{status}: {message}"));
        }
        Ok(body)
    };
    match ai.provider {
        AiProvider::Anthropic => {
            let key = key.ok_or("Add your Anthropic API key in Settings > AI.")?;
            let model = if ai.model.is_empty() {
                "claude-sonnet-5-5"
            } else {
                &ai.model
            };
            let body = json(
                client
                    .post("https://api.anthropic.com/v1/messages")
                    .header("x-api-key", key)
                    .header("anthropic-version", "2023-06-01")
                    .json(&serde_json::json!({ "model": model, "max_tokens": 2048, "messages": msgs }))
                    .send()
                    .map_err(|e| e.to_string())?,
            )?;
            Ok(body["content"]
                .as_array()
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(|p| p["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default())
        }
        AiProvider::OpenAiCompatible => {
            let endpoint = if ai.endpoint.is_empty() {
                "https://api.openai.com/v1"
            } else {
                ai.endpoint.trim_end_matches('/')
            };
            let mut request = client.post(format!("{endpoint}/chat/completions"));
            if let Some(key) = key {
                request = request.bearer_auth(key);
            }
            let body = json(
                request
                    .json(&serde_json::json!({ "model": ai.model, "messages": msgs }))
                    .send()
                    .map_err(|e| e.to_string())?,
            )?;
            Ok(body
                .pointer("/choices/0/message/content")
                .and_then(|c| c.as_str())
                .unwrap_or_default()
                .to_owned())
        }
        AiProvider::Ollama => {
            let endpoint = if ai.endpoint.is_empty() {
                "http://127.0.0.1:11434"
            } else {
                ai.endpoint.trim_end_matches('/')
            };
            let body = json(
                client
                    .post(format!("{endpoint}/api/chat"))
                    .json(&serde_json::json!({ "model": ai.model, "messages": msgs, "stream": false }))
                    .send()
                    .map_err(|e| e.to_string())?,
            )?;
            Ok(body
                .pointer("/message/content")
                .and_then(|c| c.as_str())
                .unwrap_or_default()
                .to_owned())
        }
        AiProvider::None => Err("No AI provider selected".into()),
    }
}

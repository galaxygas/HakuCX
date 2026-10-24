/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Things HakuCX does to and for web content: request filtering, page styling (force dark,
//! shaders, site mods, cosmetic filtering), the page message channel, downloads, find in page.

use std::path::PathBuf;

use haku_core::ipc::{DownloadAction, Event};
use haku_core::records::{DownloadRecord, DownloadState};
use haku_core::session::TabId;
use log::{info, warn};
use servo::{WebResourceLoad, WebResourceResponse, WebView};

use super::{Browser, page_scripts};
use crate::downloads::{DownloadMsg, DownloadRequest, decode_data_url};

/// Messages a page may send on the page channel. Everything else is ignored.
#[derive(serde::Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
enum PageMessage {
    Download {
        url: String,
        filename: String,
    },
    DownloadData {
        data_url: String,
        filename: String,
    },
    DownloadError {
        reason: String,
    },
    Media(super::media::MediaReport),
    /// Pop the page's video out (its button, the miniplayer, or auto pop out).
    Popout(super::media::PopoutRequest),
    /// A pop out was asked for, but the page has no video with a web address.
    PopoutUnavailable {
        reason: String,
    },
    /// A login form was submitted (the password manager offers to save it).
    LoginSubmit {
        username: String,
        password: String,
    },
    /// A login, address or card field got focus (autofill offers saved entries).
    FieldFocus {
        kind: String,
        rect: super::vault::CssRect,
    },
    /// Class and id names newly seen on the page, for generic element hiding.
    Cosmetic {
        classes: Vec<String>,
        ids: Vec<String>,
    },
}

impl Browser {
    // ------------------------------------------------------------------ request filtering

    /// Built-in blocking. Returns the load when the browser did not handle it.
    pub(crate) fn filter_request(
        &self,
        webview: WebView,
        load: WebResourceLoad,
    ) -> Option<WebResourceLoad> {
        let Some(tab) = self.tab_for_webview(&webview) else {
            return Some(load);
        };
        let request = load.request();
        let url = request.url.to_string();
        let page = webview
            .url()
            .map(|u| u.to_string())
            .unwrap_or_else(|| url.clone());
        let host = webview
            .url()
            .and_then(|u| {
                u.host_str()
                    .map(|h| h.trim_start_matches("www.").to_owned())
            })
            .unwrap_or_default();
        let (ads, trackers, malware) = {
            let s = self.settings.borrow();
            let p = &s.privacy;
            let excepted = |list: &[String]| {
                list.iter()
                    .any(|d| host == *d || host.ends_with(&format!(".{d}")))
            };
            (
                p.block_ads && !excepted(&p.ad_exceptions),
                p.block_trackers && !excepted(&p.tracker_exceptions),
                p.malicious_site_protection,
            )
        };
        let verdict = self.blocker.check(
            &url,
            &page,
            request.destination,
            request.is_for_main_frame,
            crate::adblock::Enabled {
                ads,
                trackers,
                malware,
            },
        );
        let Some(category) = verdict else {
            return Some(load);
        };
        if request.is_for_main_frame {
            warn!("blocked a known malicious site");
            let response = WebResourceResponse::new(request.url.clone());
            let mut intercepted = load.intercept(response);
            intercepted.send_body_data(crate::protocol::blocked_page(&url).into_bytes());
            intercepted.finish();
            return None;
        }
        let _ = category;
        let response = WebResourceResponse::new(request.url.clone());
        load.intercept(response).cancel();
        let count = {
            let mut views = self.views.borrow_mut();
            let view = views.get_mut(&tab);
            view.map(|v| {
                v.blocked += 1;
                v.blocked
            })
            .unwrap_or(0)
        };
        self.blocked_total.set(self.blocked_total.get() + 1);
        self.emit(Event::Blocked {
            tab,
            count,
            total: self.blocked_total.get(),
        });
        None
    }

    /// Re-apply force dark, the active shader, site mods and cosmetic filters to a tab.
    pub(crate) fn apply_page_style(&self, tab: &TabId) {
        let Some(webview) = self.webview_for_tab(tab) else {
            return;
        };
        let Some(url) = webview.url() else { return };
        let host = url.host_str().unwrap_or_default().to_owned();
        let (force_dark, shader, site_css, cosmetic) = {
            let s = self.settings.borrow();
            let dark_exception = s
                .features
                .force_dark_exceptions
                .iter()
                .any(|d| host == *d || host.ends_with(&format!(".{d}")));
            let shader = s
                .features
                .shaders_enabled
                .then(|| {
                    page_scripts::SHADERS
                        .iter()
                        .find(|x| x.id == s.features.shader)
                })
                .flatten()
                .map(|x| x.filter);
            let site_css = if s.features.web_modding {
                self.site_mods.borrow().css_for_host(&host)
            } else {
                String::new()
            };
            let ads_on = s.privacy.block_ads
                && s.privacy.cosmetic_filtering
                && !s
                    .privacy
                    .ad_exceptions
                    .iter()
                    .any(|d| host == *d || host.ends_with(&format!(".{d}")));
            (
                s.features.force_dark_pages && !dark_exception,
                shader,
                site_css,
                ads_on,
            )
        };
        let mut css = site_css;
        if cosmetic {
            let cosmetic_css = self.blocker.cosmetic_css(url.as_str());
            if !cosmetic_css.is_empty() {
                css.push('\n');
                css.push_str(&cosmetic_css);
            }
        }
        webview.evaluate_javascript(page_scripts::style_script(force_dark, shader, &css), |_| {});
        let muted = self.session.borrow().tab(tab).is_some_and(|t| t.muted);
        if muted {
            webview.evaluate_javascript(page_scripts::MUTE_ALL, |_| {});
        }
    }

    pub(crate) fn restyle_all_pages(&self) {
        let tabs: Vec<TabId> = self.views.borrow().keys().cloned().collect();
        for tab in tabs {
            self.apply_page_style(&tab);
        }
    }

    // ------------------------------------------------------------------ page channel

    pub(crate) fn handle_page_message(&self, tab: &TabId, payload: &str) {
        if payload.len() > 96 * 1024 * 1024 {
            return;
        }
        let Ok(message) = serde_json::from_str::<PageMessage>(payload) else {
            return;
        };
        match message {
            PageMessage::LoginSubmit { username, password } => {
                self.vault_on_submit(tab, username, password);
            }
            PageMessage::FieldFocus { kind, rect } => self.vault_on_focus(tab, &kind, rect),
            PageMessage::Download { url, filename } => {
                let Ok(parsed) = url::Url::parse(&url) else {
                    return;
                };
                if !matches!(parsed.scheme(), "http" | "https") {
                    return;
                }
                let referrer = self
                    .webview_for_tab(tab)
                    .and_then(|w| w.url())
                    .map(|u| u.to_string());
                let _ = self.start_download(
                    url,
                    Some(filename).filter(|f| !f.is_empty()),
                    referrer,
                    Some(tab.clone()),
                );
            }
            PageMessage::DownloadData { data_url, filename } => {
                self.save_data_url(&data_url, &filename)
            }
            PageMessage::DownloadError { reason } => {
                let text = match reason.as_str() {
                    "too-large" => {
                        "That file is too large to save from the page (64 MB limit for generated files)."
                    }
                    _ => "The page's download could not be saved.",
                };
                self.toast(text, "error");
            }
            PageMessage::Cosmetic {
                mut classes,
                mut ids,
            } => {
                // Bounded: a page cannot make HakuCX do unbounded work.
                classes.truncate(4000);
                ids.truncate(4000);
                let Some(webview) = self.webview_for_tab(tab) else {
                    return;
                };
                let Some(url) = webview.url() else { return };
                let host = url.host_str().unwrap_or_default().to_owned();
                let on = {
                    let settings = self.settings.borrow();
                    let p = &settings.privacy;
                    p.block_ads
                        && p.cosmetic_filtering
                        && !p
                            .ad_exceptions
                            .iter()
                            .any(|d| host == *d || host.ends_with(&format!(".{d}")))
                };
                if !on {
                    return;
                }
                let css = self
                    .blocker
                    .generic_cosmetic_css(url.as_str(), &classes, &ids);
                if !css.is_empty() {
                    webview.evaluate_javascript(page_scripts::append_cosmetic_script(&css), |_| {});
                }
            }
            PageMessage::Media(report) => self.tab_media_report(tab, report),
            PageMessage::Popout(request) => self.tab_popout_request(tab, request),
            PageMessage::PopoutUnavailable { reason } => self.popout_unavailable(&reason),
        }
    }

    // ------------------------------------------------------------------ downloads

    pub(crate) fn download_dir(&self) -> PathBuf {
        let configured = self.settings.borrow().downloads.directory.clone();
        if !configured.is_empty() {
            let p = PathBuf::from(configured);
            if p.is_dir() {
                return p;
            }
        }
        dirs::download_dir().unwrap_or_else(std::env::temp_dir)
    }

    fn cookie_header(&self, url: &str) -> Option<String> {
        let parsed = url::Url::parse(url).ok()?;
        let cookies = self
            .servo
            .site_data_manager()
            .cookies_for_url(parsed, servo::CookieSource::HTTP);
        if cookies.is_empty() {
            return None;
        }
        Some(
            cookies
                .iter()
                .map(|c| format!("{}={}", c.name(), c.value()))
                .collect::<Vec<_>>()
                .join("; "),
        )
    }

    /// Download `url` with the page's cookies. Asks where to save it if the setting says so.
    /// Returns the download's id, or `None` if the user cancelled the save dialog.
    pub(crate) fn start_download(
        &self,
        url: String,
        name_hint: Option<String>,
        referrer: Option<String>,
        _tab: Option<TabId>,
    ) -> Option<String> {
        let directory = self.download_dir();
        let path = if self.settings.borrow().downloads.ask_where {
            let suggested = name_hint
                .clone()
                .or_else(|| crate::downloads::filename_from_url(&url))
                .map(|n| haku_core::records::sanitize_file_name(&n))
                .unwrap_or_else(|| "download".into());
            // Cancelling the dialog cancels the download.
            Some(
                rfd::FileDialog::new()
                    .set_parent(&self.window)
                    .set_directory(&directory)
                    .set_file_name(&suggested)
                    .save_file()?,
            )
        } else {
            None
        };
        let record = DownloadRecord::new(&url, name_hint.as_deref().unwrap_or(""), "", "", None);
        let id = record.id.clone();
        let mut record = record;
        record.referrer = referrer.clone();
        self.downloads.borrow_mut().push(record);
        self.downloads_changed();
        info!("download started");
        self.download_engine.start(DownloadRequest {
            id: id.clone(),
            cookie_header: self.cookie_header(&url),
            url,
            referrer,
            user_agent: servo::prefs::get().user_agent.clone(),
            path,
            directory,
            name_hint,
            resume_from: 0,
        });
        if self.settings.borrow().downloads.show_popup {
            self.emit(Event::Open {
                target: "downloads-popup".into(),
            });
        }
        Some(id)
    }

    /// A navigation landed on something that is not a page: download it instead and return the
    /// tab to where it was.
    pub(crate) fn divert_to_download(&self, tab: &TabId, mime: &str) {
        let Some(webview) = self.webview_for_tab(tab) else {
            return;
        };
        let Some(url) = webview.url() else { return };
        if !matches!(url.scheme(), "http" | "https") {
            return;
        }
        info!("treating a {mime} response as a download");
        let can_back = webview.can_go_back();
        if can_back {
            webview.go_back(1);
        } else {
            let has_internal = self
                .views
                .borrow()
                .get(tab)
                .is_some_and(|v| v.internal_before_web.is_some());
            if has_internal {
                self.go_back(tab);
            } else {
                // A tab opened only for this file: close it, like other browsers do.
                self.session.borrow_mut().close(tab);
                self.session_changed();
            }
        }
        let _ = self.start_download(url.to_string(), None, None, Some(tab.clone()));
    }

    fn save_data_url(&self, data_url: &str, filename: &str) {
        let Some((mime, bytes)) = decode_data_url(data_url) else {
            return;
        };
        let dir = self.download_dir();
        let name = if filename.is_empty() {
            "download".to_owned()
        } else {
            haku_core::records::sanitize_file_name(filename)
        };
        let path = haku_core::records::unique_file_name(&dir, &name, |p| p.exists());
        let mut record = DownloadRecord::new(
            "data:",
            &name,
            &path.display().to_string(),
            &mime,
            Some(bytes.len() as u64),
        );
        match std::fs::write(&path, &bytes) {
            Ok(()) => {
                record.received_bytes = bytes.len() as u64;
                record.state = DownloadState::Completed;
                self.lights_signal(haku_lights::effects::Signal::DownloadDone);
            }
            Err(e) => record.state = DownloadState::Failed(e.to_string()),
        }
        record.ended_at = Some(haku_core::now_ms());
        self.downloads.borrow_mut().push(record);
        self.downloads_changed();
    }

    /// Apply worker thread progress to the download list.
    pub(crate) fn poll_downloads(&self) {
        let messages = self.download_engine.drain();
        if messages.is_empty() {
            return;
        }
        let mut list_changed = false;
        for message in messages {
            let mut downloads = self.downloads.borrow_mut();
            match message {
                DownloadMsg::Started {
                    id,
                    total,
                    mime,
                    file_name,
                    path,
                } => {
                    if let Some(d) = downloads.get_mut(&id) {
                        d.total_bytes = total;
                        d.mime = mime;
                        d.file_name = file_name;
                        d.path = path.display().to_string();
                        d.state = DownloadState::InProgress;
                    }
                    list_changed = true;
                }
                DownloadMsg::Progress {
                    id,
                    received,
                    total,
                    bps,
                } => {
                    if let Some(d) = downloads.get_mut(&id) {
                        d.received_bytes = received;
                        d.total_bytes = total.or(d.total_bytes);
                    }
                    drop(downloads);
                    self.emit(Event::DownloadProgress {
                        id,
                        received,
                        total,
                        bps,
                    });
                }
                DownloadMsg::Finished { id, path } => {
                    let mut open_when_done = None;
                    if let Some(d) = downloads.get_mut(&id) {
                        d.state = DownloadState::Completed;
                        d.path = path.display().to_string();
                        d.received_bytes = std::fs::metadata(&path)
                            .map(|m| m.len())
                            .unwrap_or(d.received_bytes);
                        d.total_bytes = Some(d.received_bytes);
                        d.ended_at = Some(haku_core::now_ms());
                        open_when_done = Some(path.clone());
                        self.lights_signal(haku_lights::effects::Signal::DownloadDone);
                    }
                    self.download_engine.forget(&id);
                    drop(downloads);
                    if self.settings.borrow().downloads.open_when_done
                        && let Some(p) = open_when_done
                    {
                        let _ = open::that_detached(p);
                    }
                    self.play_sound("download");
                    list_changed = true;
                }
                DownloadMsg::Paused { id, received } => {
                    if let Some(d) = downloads.get_mut(&id) {
                        d.state = DownloadState::Paused;
                        d.received_bytes = received;
                    }
                    self.download_engine.forget(&id);
                    list_changed = true;
                }
                DownloadMsg::Cancelled { id } => {
                    if let Some(d) = downloads.get_mut(&id) {
                        d.state = DownloadState::Cancelled;
                        d.ended_at = Some(haku_core::now_ms());
                    }
                    self.download_engine.forget(&id);
                    list_changed = true;
                }
                DownloadMsg::Failed { id, reason } => {
                    if let Some(d) = downloads.get_mut(&id) {
                        d.state = DownloadState::Failed(reason);
                        d.ended_at = Some(haku_core::now_ms());
                    }
                    self.download_engine.forget(&id);
                    list_changed = true;
                }
            }
        }
        if list_changed {
            self.downloads_changed();
        }
    }

    pub(crate) fn download_action(&self, id: &str, action: DownloadAction) {
        let Some(record) = self
            .downloads
            .borrow()
            .items
            .iter()
            .find(|d| d.id == id)
            .cloned()
        else {
            return;
        };
        match action {
            DownloadAction::Pause => self.download_engine.pause(id),
            DownloadAction::Cancel => {
                if self.download_engine.is_running(id) {
                    self.download_engine.cancel(id);
                } else {
                    if record.state == DownloadState::Paused {
                        let _ = std::fs::remove_file(crate::downloads::part_path(
                            std::path::Path::new(&record.path),
                        ));
                    }
                    if let Some(d) = self.downloads.borrow_mut().get_mut(id) {
                        d.state = DownloadState::Cancelled;
                    }
                    self.downloads_changed();
                }
            }
            DownloadAction::Resume | DownloadAction::Retry => {
                if self.download_engine.is_running(id) {
                    return;
                }
                let final_path = (!record.path.is_empty()).then(|| PathBuf::from(&record.path));
                let resume_from = match (&action, &final_path) {
                    (DownloadAction::Resume, Some(p)) => {
                        std::fs::metadata(crate::downloads::part_path(p))
                            .map(|m| m.len())
                            .unwrap_or(0)
                    }
                    _ => 0,
                };
                if let Some(d) = self.downloads.borrow_mut().get_mut(id) {
                    d.state = DownloadState::InProgress;
                    d.ended_at = None;
                    if resume_from == 0 {
                        d.received_bytes = 0;
                    }
                }
                self.downloads_changed();
                self.download_engine.start(DownloadRequest {
                    id: id.to_owned(),
                    cookie_header: self.cookie_header(&record.url),
                    url: record.url.clone(),
                    referrer: record.referrer.clone(),
                    user_agent: servo::prefs::get().user_agent.clone(),
                    path: final_path,
                    directory: self.download_dir(),
                    name_hint: None,
                    resume_from,
                });
            }
            DownloadAction::Open => {
                if let Err(e) = open::that_detached(&record.path) {
                    self.toast(format!("Could not open the file: {e}"), "error");
                }
            }
            DownloadAction::ShowInFolder => {
                #[cfg(windows)]
                {
                    let _ = std::process::Command::new("explorer")
                        .arg(format!("/select,{}", record.path))
                        .spawn();
                }
                #[cfg(not(windows))]
                if let Some(parent) = std::path::Path::new(&record.path).parent() {
                    let _ = open::that_detached(parent);
                }
            }
            DownloadAction::Remove => {
                self.downloads
                    .borrow_mut()
                    .items
                    .retain(|d| d.id != id || d.is_active());
                self.downloads_changed();
            }
            DownloadAction::DeleteFile => {
                if record.is_active() {
                    return;
                }
                let _ = std::fs::remove_file(&record.path);
                self.downloads.borrow_mut().items.retain(|d| d.id != id);
                self.downloads_changed();
            }
        }
    }

    // ------------------------------------------------------------------ find and fullscreen

    pub(crate) fn find_in_page(&self, text: &str, forward: bool, match_case: bool) {
        let Some(tab) = self.active_tab_id() else {
            return;
        };
        let Some(webview) = self.webview_for_tab(&tab) else {
            self.emit(Event::FindResult {
                matches: 0,
                active: 0,
            });
            return;
        };
        let browser = self.rc();
        webview.evaluate_javascript(
            page_scripts::find_script(text, forward, match_case),
            move |result| {
                let (matches, active) = match result {
                    Ok(servo::JSValue::String(s)) => {
                        let mut parts = s.split(',').map(|p| p.parse::<u32>().unwrap_or(0));
                        (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
                    }
                    _ => (0, 0),
                };
                browser.emit(Event::FindResult { matches, active });
            },
        );
    }

    pub(crate) fn stop_find(&self) {
        if let Some(webview) = self.active_tab_id().and_then(|t| self.webview_for_tab(&t)) {
            webview.evaluate_javascript(page_scripts::FIND_CLEAR, |_| {});
        }
    }

    pub(crate) fn set_page_fullscreen(&self, on: bool) {
        self.window
            .set_fullscreen(on.then_some(winit::window::Fullscreen::Borderless(None)));
        self.lights_fullscreen(on);
        self.emit(Event::Fullscreen { on });
    }
}

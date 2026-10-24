/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The password manager and form autofill: the vault (`haku_core::vault`) opened with the
//! Windows key, save prompts for submitted logins, suggestions for focused fields, filling
//! after the user picks an entry, Windows Hello before secrets are shown, CSV import/export.
//!
//! Secrets never go to the UI except a password the user asked to see (after verification)
//! and a password the user just typed (in the save prompt). Filling is always a user action.

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use haku_core::ipc::{
    AutofillItem, CardInfo, CardInput, Event, LoginInfo, LoginInput, Rect, VaultStatus,
};
use haku_core::session::TabId;
use haku_core::vault::{
    self, Address, Card, Login, Protector, Submission, Vault, VaultData, VaultError,
};
use log::{info, warn};
use serde_json::json;

use super::Browser;
use crate::os_secrets::{self, Dpapi};

/// How long one Windows Hello verification lets the user see passwords (Chrome uses a minute).
const VERIFIED_FOR: Duration = Duration::from_secs(60);
const FILL_SCRIPT: &str = include_str!("autofill_fill.js");

#[derive(Default)]
pub struct VaultState {
    pub vault: Option<Vault>,
    /// `ready`, `needs-password`, `error` or `unavailable`.
    pub state: &'static str,
    pub message: String,
    pub hello: bool,
    verified_until: Option<Instant>,
    /// When we last read or wrote the file (another window may change it).
    file_time: Option<SystemTime>,
    prompts: HashMap<u32, Prompt>,
    /// The focused field suggestions are showing for: (tab, kind).
    suggesting: Option<(TabId, String)>,
}

struct Prompt {
    tab: TabId,
    origin: String,
    update: Option<String>,
}

/// A rectangle in page CSS pixels, as page scripts report it.
#[derive(Clone, Copy, Debug, serde::Deserialize)]
pub struct CssRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Browser {
    fn vault_path(&self) -> Option<std::path::PathBuf> {
        self.ext_paths.as_ref().map(|p| p.vault())
    }

    fn file_time(path: &std::path::Path) -> Option<SystemTime> {
        std::fs::metadata(path).and_then(|m| m.modified()).ok()
    }

    /// Open the vault at startup with the Windows key.
    pub(crate) fn vault_load(&self) {
        let Some(path) = self.vault_path() else {
            let mut v = self.vault.borrow_mut();
            v.state = "unavailable";
            v.message = "This profile does not keep passwords.".into();
            return;
        };
        let mut v = self.vault.borrow_mut();
        match std::fs::read(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                v.vault = Some(Vault::create());
                v.state = "ready";
            }
            Err(e) => {
                v.state = "error";
                v.message = format!("The password file could not be read: {e}");
            }
            Ok(bytes) => match Vault::open_os(&bytes, &Dpapi) {
                Ok(opened) => {
                    v.vault = Some(opened);
                    v.state = "ready";
                    v.file_time = Self::file_time(&path);
                }
                Err(VaultError::NeedsPassword) => {
                    v.state = "needs-password";
                    v.message = "Windows can no longer unlock your saved passwords (for example after a Windows reinstall). Enter your recovery password to open them.".into();
                }
                Err(e) => {
                    warn!("vault: {e}");
                    v.state = "error";
                    v.message = format!(
                        "Your saved passwords could not be opened: {e}. The file is left untouched."
                    );
                }
            },
        }
        drop(v);
        // Windows Hello availability takes a moment to query: ask off the UI thread.
        let sender = self.background_sender();
        std::thread::spawn(move || {
            let hello = os_secrets::hello_available();
            sender.send(Box::new(move |b: &Browser| {
                b.vault.borrow_mut().hello = hello;
                b.vault_emit();
            }));
        });
    }

    fn vault_writable(&self) -> bool {
        self.paths.is_some() && self.vault.borrow().state == "ready"
    }

    /// Pick up changes another window of this profile saved.
    fn vault_refresh_from_disk(&self) {
        let Some(path) = self.vault_path() else {
            return;
        };
        let time = Self::file_time(&path);
        if time.is_none() || time == self.vault.borrow().file_time {
            return;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        let mut v = self.vault.borrow_mut();
        v.file_time = time;
        let Some(ours) = v.vault.as_mut() else { return };
        if let Ok(disk) = Vault::open_os(&bytes, &Dpapi) {
            ours.data.merge(&disk.data);
        }
    }

    /// Write the vault (merging what another window may have written first), and sync soon.
    pub(crate) fn vault_save(&self) {
        self.vault_save_quiet();
        self.sync_soon();
    }

    /// Write the vault without scheduling a sync (for changes that came from sync).
    pub(crate) fn vault_save_quiet(&self) {
        if !self.vault_writable() {
            self.vault_emit();
            return;
        }
        self.vault_refresh_from_disk();
        let Some(path) = self.paths.as_ref().map(|p| p.vault()) else {
            return;
        };
        let bytes = {
            let v = self.vault.borrow();
            let Some(vault) = v.vault.as_ref() else {
                return;
            };
            vault.to_bytes(Some(&Dpapi as &dyn Protector))
        };
        match bytes {
            Ok(bytes) => {
                if let Err(e) = haku_core::storage::write_bytes_atomic(&path, &bytes) {
                    self.toast(format!("Could not save your passwords: {e}"), "error");
                }
                self.vault.borrow_mut().file_time = Self::file_time(&path);
            }
            Err(e) => self.toast(format!("Could not encrypt your passwords: {e}"), "error"),
        }
        self.vault_emit();
    }

    /// Send the vault's contents (no secrets) to the UI.
    pub(crate) fn vault_emit(&self) {
        let v = self.vault.borrow();
        let status = VaultStatus {
            state: if v.state.is_empty() {
                "unavailable"
            } else {
                v.state
            }
            .to_owned(),
            message: v.message.clone(),
            recovery_password: v.vault.as_ref().is_some_and(Vault::has_recovery_password),
            hello: v.hello,
            writable: self.paths.is_some(),
        };
        let data = v.vault.as_ref().map(|x| &x.data);
        let mut logins: Vec<LoginInfo> = data
            .map(|d| {
                d.logins
                    .iter()
                    .map(|l| LoginInfo {
                        id: l.id.clone(),
                        origin: l.origin.clone(),
                        username: l.username.clone(),
                        note: l.note.clone(),
                        created: l.created,
                        modified: l.modified,
                        last_used: l.last_used,
                        times_used: l.times_used,
                    })
                    .collect()
            })
            .unwrap_or_default();
        logins.sort_by(|a, b| a.origin.cmp(&b.origin).then(a.username.cmp(&b.username)));
        let cards = data
            .map(|d| {
                d.cards
                    .iter()
                    .map(|c| CardInfo {
                        id: c.id.clone(),
                        nickname: c.nickname.clone(),
                        holder: c.holder.clone(),
                        brand: vault::card_brand(&c.number).to_owned(),
                        last4: c
                            .number
                            .chars()
                            .rev()
                            .take(4)
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect(),
                        exp_month: c.exp_month,
                        exp_year: c.exp_year,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let event = Event::Vault {
            status,
            logins,
            never_save: data
                .map(|d| d.never_save.keys().cloned().collect())
                .unwrap_or_default(),
            cards,
            addresses: data.map(|d| d.addresses.clone()).unwrap_or_default(),
        };
        drop(v);
        self.emit(event);
    }

    fn with_data<R>(&self, f: impl FnOnce(&mut VaultData) -> R) -> Option<R> {
        let mut v = self.vault.borrow_mut();
        v.vault.as_mut().map(|vault| f(&mut vault.data))
    }

    // ------------------------------------------------------------------ verification

    fn hwnd(&self) -> isize {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        match self.window.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
            _ => 0,
        }
    }

    /// Run `then(verified)` once the user has proved who they are with Windows Hello (or
    /// right away if they did in the last minute, or if Windows Hello is not set up).
    fn with_verification(&self, reason: &str, then: impl FnOnce(&Browser, bool) + Send + 'static) {
        let (hello, recent) = {
            let v = self.vault.borrow();
            (
                v.hello,
                v.verified_until.is_some_and(|t| Instant::now() < t),
            )
        };
        if !hello || recent {
            then(self, true);
            return;
        }
        let hwnd = self.hwnd();
        let reason = reason.to_owned();
        let sender = self.background_sender();
        std::thread::spawn(move || {
            let verified = match os_secrets::hello_verify(hwnd, &reason) {
                Ok(v) => v,
                Err(e) => {
                    warn!("Windows Hello: {e}");
                    false
                }
            };
            sender.send(Box::new(move |b: &Browser| {
                if verified {
                    b.vault.borrow_mut().verified_until = Some(Instant::now() + VERIFIED_FOR);
                }
                then(b, verified);
            }));
        });
    }

    pub(crate) fn vault_reveal(&self, request: u32, id: String) {
        self.with_verification(
            "HakuCX wants to show a saved password or card number.",
            move |b, ok| {
                let value = ok
                    .then(|| {
                        b.with_data(|d| {
                            d.logins
                                .iter()
                                .find(|l| l.id == id)
                                .map(|l| l.password.clone())
                                .or_else(|| {
                                    d.cards
                                        .iter()
                                        .find(|c| c.id == id)
                                        .map(|c| c.number.clone())
                                })
                        })
                        .flatten()
                    })
                    .flatten();
                b.emit(Event::Revealed { request, value });
            },
        );
    }

    // ------------------------------------------------------------------ editing

    pub(crate) fn vault_unlock(&self, password: &str) {
        let Some(path) = self.vault_path() else {
            return;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            return;
        };
        match Vault::open_password(&bytes, password) {
            Ok(opened) => {
                {
                    let mut v = self.vault.borrow_mut();
                    v.vault = Some(opened);
                    v.state = "ready";
                    v.message.clear();
                }
                // Re-wrap the key for this Windows account.
                self.vault_save();
                self.toast("Your saved passwords are open again.", "info");
            }
            Err(e) => self.toast(e.to_string(), "error"),
        }
    }

    pub(crate) fn vault_set_recovery(&self, password: Option<String>) {
        if !self.vault_writable() {
            return;
        }
        if password.as_deref().is_some_and(|p| p.chars().count() < 8) {
            self.toast(
                "Use at least 8 characters for the recovery password.",
                "error",
            );
            return;
        }
        self.with_verification(
            "HakuCX wants to change the recovery password for your saved passwords.",
            move |b, ok| {
                if !ok {
                    return;
                }
                let result = {
                    let mut v = b.vault.borrow_mut();
                    v.vault
                        .as_mut()
                        .map(|vault| vault.set_recovery_password(password.as_deref()))
                };
                match result {
                    Some(Ok(())) => {
                        b.vault_save();
                        b.toast(
                            if password.is_some() {
                                "Recovery password set."
                            } else {
                                "Recovery password removed."
                            },
                            "info",
                        );
                    }
                    Some(Err(e)) => b.toast(e.to_string(), "error"),
                    None => {}
                }
            },
        );
    }

    pub(crate) fn vault_save_login(&self, input: LoginInput) {
        if !self.vault_writable() {
            return;
        }
        let site = input.site.trim();
        let with_scheme = if site.contains("://") {
            site.to_owned()
        } else {
            format!("https://{site}")
        };
        let Some(origin) = vault::origin_of(&with_scheme) else {
            self.toast(
                "Enter a web address for the site, like example.com.",
                "error",
            );
            return;
        };
        let now = haku_core::now_ms();
        let saved = self.with_data(|d| {
            let existing = input
                .id
                .as_ref()
                .and_then(|id| d.logins.iter().find(|l| &l.id == id).cloned());
            if input.id.is_some() && existing.is_none() {
                return false;
            }
            let password = match (&input.password, &existing) {
                (Some(p), _) => p.clone(),
                (None, Some(e)) => e.password.clone(),
                (None, None) => return false,
            };
            let base = existing.unwrap_or_default();
            d.upsert_login(
                Login {
                    origin,
                    username: input.username.clone(),
                    password,
                    note: input.note.clone(),
                    ..base
                },
                now,
            );
            true
        });
        if saved == Some(true) {
            self.vault_save();
        }
    }

    pub(crate) fn vault_save_card(&self, input: CardInput) {
        if !self.vault_writable() {
            return;
        }
        if let Some(n) = input.number.as_deref()
            && !vault::luhn_valid(n)
        {
            self.toast("That card number doesn't look right.", "error");
            return;
        }
        if !(1..=12).contains(&input.exp_month) || !(2000..=2100).contains(&input.exp_year) {
            self.toast("Enter the card's expiry month and year.", "error");
            return;
        }
        let now = haku_core::now_ms();
        let saved = self.with_data(|d| {
            let existing = input
                .id
                .as_ref()
                .and_then(|id| d.cards.iter().find(|c| &c.id == id).cloned());
            let number = match (&input.number, &existing) {
                (Some(n), _) => n.clone(),
                (None, Some(e)) => e.number.clone(),
                (None, None) => return false,
            };
            d.upsert_card(
                Card {
                    nickname: input.nickname.clone(),
                    holder: input.holder.clone(),
                    number,
                    exp_month: input.exp_month,
                    exp_year: input.exp_year,
                    ..existing.unwrap_or_default()
                },
                now,
            );
            true
        });
        if saved == Some(true) {
            self.vault_save();
        }
    }

    pub(crate) fn vault_save_address(&self, mut address: Address) {
        if !self.vault_writable() {
            return;
        }
        if address.id.is_empty() {
            address.id = Address::default().id;
        }
        let now = haku_core::now_ms();
        self.with_data(|d| d.upsert_address(address, now));
        self.vault_save();
    }

    pub(crate) fn vault_delete(&self, id: &str) {
        if !self.vault_writable() {
            return;
        }
        let now = haku_core::now_ms();
        if self.with_data(|d| d.delete(id, now)) == Some(true) {
            self.vault_save();
        }
    }

    pub(crate) fn vault_never_remove(&self, origin: &str) {
        if !self.vault_writable() {
            return;
        }
        let now = haku_core::now_ms();
        self.with_data(|d| d.set_never_save(origin, false, now));
        self.vault_save();
    }

    pub(crate) fn vault_import(&self) {
        if !self.vault_writable() {
            self.toast(
                "Passwords can only be imported in a normal window.",
                "error",
            );
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_parent(&self.window)
            .add_filter("Password export (CSV)", &["csv"])
            .pick_file()
        else {
            return;
        };
        let text = match std::fs::read(&path) {
            Ok(bytes) if bytes.len() <= 32 * 1024 * 1024 => {
                zeroize::Zeroizing::new(String::from_utf8_lossy(&bytes).into_owned())
            }
            Ok(_) => return self.toast("That file is too large.", "error"),
            Err(e) => return self.toast(format!("Could not read the file: {e}"), "error"),
        };
        let now = haku_core::now_ms();
        let Some(report) = self.with_data(|d| d.import_csv(&text, now)) else {
            return;
        };
        self.vault_save();
        info!("imported passwords: {report:?}");
        self.toast(
            format!(
                "Imported {} new and {} updated passwords{}{}.",
                report.added,
                report.updated,
                if report.unchanged > 0 {
                    format!(", {} already saved", report.unchanged)
                } else {
                    String::new()
                },
                if report.skipped > 0 {
                    format!(
                        ", skipped {} rows without a web address or password",
                        report.skipped
                    )
                } else {
                    String::new()
                }
            ),
            "info",
        );
    }

    pub(crate) fn vault_export(&self) {
        self.with_verification("HakuCX wants to export your saved passwords.", |b, ok| {
            if !ok {
                return;
            }
            let Some(csv) = b.with_data(|d| d.export_csv()) else { return };
            let Some(path) = rfd::FileDialog::new()
                .set_parent(&b.window)
                .set_file_name("HakuCX Passwords.csv")
                .add_filter("CSV", &["csv"])
                .save_file()
            else {
                return;
            };
            match std::fs::write(&path, csv.as_bytes()) {
                Ok(()) => b.toast(format!("Exported to {}. Anyone who can open this file can read your passwords: delete it when you are done.", path.display()), "info"),
                Err(e) => b.toast(format!("Export failed: {e}"), "error"),
            }
        });
    }

    // ------------------------------------------------------------------ pages

    /// The tab's page origin (`https://host`), if it is a web page.
    fn tab_origin(&self, tab: &TabId) -> Option<String> {
        let url = self.webview_for_tab(tab)?.url()?;
        vault::origin_of(url.as_str())
    }

    /// A login form was submitted on a page.
    pub(crate) fn vault_on_submit(&self, tab: &TabId, username: String, password: String) {
        if !self.settings.borrow().autofill.offer_to_save || !self.vault_writable() {
            return;
        }
        if password.is_empty() || password.len() > 1000 || username.len() > 1000 {
            return;
        }
        let Some(origin) = self.tab_origin(tab) else {
            return;
        };
        self.vault_refresh_from_disk();
        let now = haku_core::now_ms();
        let Some(kind) = self.with_data(|d| d.classify(&origin, &username, &password)) else {
            return;
        };
        let update = match kind {
            Submission::Never => return,
            Submission::Known(id) => {
                self.with_data(|d| d.touch_login(&id, now));
                self.vault_save();
                return;
            }
            Submission::Update(id) => Some(id),
            Submission::New => None,
        };
        let prompt = self.next_id();
        self.vault.borrow_mut().prompts.insert(
            prompt,
            Prompt {
                tab: tab.clone(),
                origin: origin.clone(),
                update: update.clone(),
            },
        );
        self.emit(Event::PasswordPrompt {
            prompt,
            tab: tab.clone(),
            origin,
            username,
            password,
            update: update.is_some(),
        });
    }

    pub(crate) fn vault_prompt_answer(
        &self,
        prompt: u32,
        action: &str,
        username: String,
        password: String,
    ) {
        let Some(p) = self.vault.borrow_mut().prompts.remove(&prompt) else {
            return;
        };
        let now = haku_core::now_ms();
        match action {
            "save" if !password.is_empty() => {
                self.with_data(|d| {
                    let base = p
                        .update
                        .as_ref()
                        .and_then(|id| d.logins.iter().find(|l| &l.id == id).cloned());
                    let id = d.upsert_login(
                        Login {
                            origin: p.origin.clone(),
                            username,
                            password,
                            ..base.unwrap_or_default()
                        },
                        now,
                    );
                    d.touch_login(&id, now);
                });
                self.vault_save();
            }
            "never" => {
                self.with_data(|d| d.set_never_save(&p.origin, true, now));
                self.vault_save();
            }
            _ => {}
        }
        let _ = p.tab;
    }

    /// A login, address or card field got focus: offer what is saved for it.
    pub(crate) fn vault_on_focus(&self, tab: &TabId, kind: &str, rect: CssRect) {
        let settings = self.settings.borrow().autofill.clone();
        let enabled = match kind {
            "login" => settings.suggest_passwords,
            "address" => settings.addresses,
            "card" => settings.payment_methods,
            _ => false,
        };
        if !enabled
            || self.vault.borrow().state != "ready"
            || self.active_tab_id().as_ref() != Some(tab)
        {
            return;
        }
        let Some(origin) = self.tab_origin(tab) else {
            return;
        };
        // Cards are only offered on secure pages (https, or this computer), as in Chrome.
        let local = ["http://localhost", "http://127.0.0.1", "http://[::1]"]
            .iter()
            .any(|l| origin == *l || origin.starts_with(&format!("{l}:")));
        if kind == "card" && !origin.starts_with("https://") && !local {
            return;
        }
        self.vault_refresh_from_disk();
        let items: Vec<AutofillItem> = self
            .with_data(|d| match kind {
                "login" => d
                    .logins_for(&origin)
                    .into_iter()
                    .map(|l| AutofillItem {
                        id: l.id.clone(),
                        label: if l.username.is_empty() {
                            "(no username)".into()
                        } else {
                            l.username.clone()
                        },
                        sublabel: url::Url::parse(&l.origin)
                            .ok()
                            .and_then(|u| u.host_str().map(str::to_owned))
                            .unwrap_or_default(),
                    })
                    .collect(),
                "address" => d
                    .addresses
                    .iter()
                    .map(|a| AutofillItem {
                        id: a.id.clone(),
                        label: if a.full_name.is_empty() {
                            a.street.lines().next().unwrap_or("Address").to_owned()
                        } else {
                            a.full_name.clone()
                        },
                        sublabel: [a.street.lines().next().unwrap_or(""), a.city.as_str()]
                            .iter()
                            .filter(|s| !s.is_empty())
                            .copied()
                            .collect::<Vec<_>>()
                            .join(", "),
                    })
                    .collect(),
                _ => d
                    .cards
                    .iter()
                    .map(|c| AutofillItem {
                        id: c.id.clone(),
                        label: format!(
                            "{} •••• {}",
                            if c.nickname.is_empty() {
                                vault::card_brand(&c.number)
                            } else {
                                c.nickname.as_str()
                            },
                            &c.number[c.number.len().saturating_sub(4)..]
                        ),
                        sublabel: format!("{} {:02}/{}", c.holder, c.exp_month, c.exp_year % 100)
                            .trim()
                            .to_owned(),
                    })
                    .collect(),
            })
            .unwrap_or_default();
        if items.is_empty() {
            self.vault_hide_suggestions();
            return;
        }
        let Some(rect) = self.page_css_rect_to_ui(tab, rect) else {
            return;
        };
        self.vault.borrow_mut().suggesting = Some((tab.clone(), kind.to_owned()));
        self.emit(Event::AutofillSuggest {
            tab: tab.clone(),
            rect,
            kind: kind.to_owned(),
            items,
        });
    }

    pub(crate) fn vault_hide_suggestions(&self) {
        if self.vault.borrow_mut().suggesting.take().is_some() {
            self.emit(Event::AutofillHide);
        }
    }

    /// Convert a rectangle a page script measured (CSS pixels in the page's viewport) to UI
    /// coordinates.
    fn page_css_rect_to_ui(&self, tab: &TabId, r: CssRect) -> Option<Rect> {
        if !(r.x.is_finite() && r.y.is_finite() && r.w.is_finite() && r.h.is_finite()) {
            return None;
        }
        let pane = self.views.borrow().get(tab).map(|v| v.pane)?;
        let layout = self.layout.borrow().clone();
        let origin = match pane {
            super::Pane::Split => layout.split.unwrap_or(layout.page),
            _ => layout.page,
        };
        let zoom = self
            .session
            .borrow()
            .tab(tab)
            .map(|t| t.zoom)
            .unwrap_or(1.0);
        let device = self.scale() * zoom;
        let ui = self.ui_scale();
        Some(Rect {
            x: (origin.x + r.x * device) / ui,
            y: (origin.y + r.y * device) / ui,
            w: r.w * device / ui,
            h: r.h * device / ui,
        })
    }

    /// The user picked a suggestion: fill it into the tab's focused form.
    pub(crate) fn vault_pick(&self, tab: TabId, id: String) {
        // From a field suggestion or from the address bar's key button.
        self.vault_hide_suggestions();
        if self.active_tab_id().as_ref() != Some(&tab) {
            return;
        }
        let Some(origin) = self.tab_origin(&tab) else {
            return;
        };
        let kind = self
            .with_data(|d| {
                if d.logins.iter().any(|l| l.id == id) {
                    Some("login")
                } else if d.addresses.iter().any(|a| a.id == id) {
                    Some("address")
                } else if d.cards.iter().any(|c| c.id == id) {
                    Some("card")
                } else {
                    None
                }
            })
            .flatten();
        let Some(kind) = kind.map(str::to_owned) else {
            return;
        };
        let is_card = kind == "card";
        let fill = move |b: &Browser, ok: bool| {
            if !ok {
                return;
            }
            // Look the entry up again now, and only fill a login into a page it belongs to.
            let now = haku_core::now_ms();
            let data = b
                .with_data(|d| match kind.as_str() {
                    "login" => {
                        let l = d
                            .logins_for(&origin)
                            .into_iter()
                            .find(|l| l.id == id)?
                            .clone();
                        d.touch_login(&l.id, now);
                        Some(json!({ "username": l.username, "password": l.password }))
                    }
                    "address" => d.addresses.iter().find(|a| a.id == id).map(address_json),
                    "card" => d.cards.iter().find(|c| c.id == id).map(|c| {
                        json!({
                            "holder": c.holder,
                            "number": c.number,
                            "exp": format!("{:02}/{:02}", c.exp_month, c.exp_year % 100),
                            "exp_month": c.exp_month,
                            "exp_year": c.exp_year,
                        })
                    }),
                    _ => None,
                })
                .flatten();
            let Some(data) = data else { return };
            let Some(webview) = b.webview_for_tab(&tab) else {
                return;
            };
            // The page must still be the one the suggestion was for.
            if webview
                .url()
                .and_then(|u| vault::origin_of(u.as_str()))
                .as_deref()
                != Some(origin.as_str())
            {
                return;
            }
            let script = FILL_SCRIPT
                .replace(
                    "__KIND__",
                    &serde_json::to_string(&kind).unwrap_or_default(),
                )
                .replace(
                    "__DATA__",
                    &zeroize::Zeroizing::new(serde_json::to_string(&data).unwrap_or_default()),
                );
            webview.evaluate_javascript(script, |_| {});
            if kind == "login" {
                b.vault_save();
            }
        };
        if is_card && self.settings.borrow().autofill.verify_cards {
            self.with_verification("HakuCX wants to fill in a saved payment card.", fill);
        } else {
            fill(self, true);
        }
    }
}

fn address_json(a: &Address) -> serde_json::Value {
    let mut street = a.street.lines();
    let line1 = street.next().unwrap_or_default();
    let line2 = street.collect::<Vec<_>>().join(", ");
    let mut names = a.full_name.split_whitespace();
    let given = names.next().unwrap_or_default();
    let family = names.collect::<Vec<_>>().join(" ");
    json!({
        "full_name": a.full_name,
        "given_name": given,
        "family_name": family,
        "organization": a.organization,
        "street": a.street,
        "line1": line1,
        "line2": line2,
        "city": a.city,
        "region": a.region,
        "postal_code": a.postal_code,
        "country": a.country,
        "phone": a.phone,
        "email": a.email,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_script_is_valid_after_substitution() {
        let script = FILL_SCRIPT
            .replace("__KIND__", "\"login\"")
            .replace("__DATA__", "{\"username\":\"a\",\"password\":\"b\"}");
        assert!(!script.contains("__KIND__") && !script.contains("__DATA__"));
        assert!(script.trim_end().ends_with("})()"));
    }

    #[test]
    fn address_names_split() {
        let a = Address {
            full_name: "Ada King Lovelace".into(),
            street: "1 Main St\nFlat 2".into(),
            ..Address::default()
        };
        let j = address_json(&a);
        assert_eq!(j["given_name"], "Ada");
        assert_eq!(j["family_name"], "King Lovelace");
        assert_eq!(j["line1"], "1 Main St");
        assert_eq!(j["line2"], "Flat 2");
    }
}

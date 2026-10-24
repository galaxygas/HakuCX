/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! My Flow in the browser: the store (`haku_core::flow`), the local-network server
//! (`flow_server.rs`), and the panel's commands.
//!
//! Only the profile's main window runs it (private and extra windows are separate processes
//! and would fight over the store and the port).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use haku_core::flow::{Change, Flow, FlowFile, FlowKind, PAIRING_TTL_MS};
use haku_core::ipc::{Event, FlowPairing, FlowStatus};
use haku_core::session::TabId;
use log::warn;

use super::Browser;
use crate::flow_server::{self, FlowServer};
use crate::os_secrets::Dpapi;

#[derive(Default)]
pub struct FlowState {
    store: Option<Arc<Mutex<Flow>>>,
    server: Option<FlowServer>,
    pairing: Option<FlowPairing>,
    message: String,
    unread: u32,
    /// When the panel last heard that a phone checked in (those updates are throttled).
    last_seen_emit: i64,
}

impl Browser {
    fn flow_allowed(&self) -> bool {
        !self.private && !self.secondary && self.paths.is_some()
    }

    fn flow_enabled(&self) -> bool {
        self.flow_allowed() && self.settings.borrow().features.my_flow
    }

    fn flow_path(&self) -> Option<std::path::PathBuf> {
        self.paths.as_ref().map(|p| p.root.join("flow.json"))
    }

    fn flow_inbox_dir(&self) -> std::path::PathBuf {
        self.download_dir().join("My Flow")
    }

    fn flow_store(&self) -> Option<Arc<Mutex<Flow>>> {
        if let Some(store) = self.flow.borrow().store.clone() {
            return Some(store);
        }
        let path = self.flow_path()?;
        let pc = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "HakuCX".into());
        let flow = match Flow::load(&path, &Dpapi, pc.clone(), self.flow_inbox_dir()) {
            Ok(f) => f,
            Err(e) => {
                warn!("{e}");
                self.flow.borrow_mut().message = e.to_string();
                Flow::new(pc, self.flow_inbox_dir())
            }
        };
        let store = Arc::new(Mutex::new(flow));
        self.flow.borrow_mut().store = Some(store.clone());
        Some(store)
    }

    fn flow_save(&self) {
        let (Some(store), Some(path)) = (self.flow.borrow().store.clone(), self.flow_path()) else {
            return;
        };
        let result = store.lock().map(|f| f.save(&path, &Dpapi));
        if let Ok(Err(e)) = result {
            warn!("saving My Flow: {e}");
        }
    }

    /// Start or stop the server to match the settings, paired phones and an open QR code.
    pub(crate) fn flow_apply(&self) {
        if !self.flow_enabled() {
            let server = self.flow.borrow_mut().server.take();
            drop(server);
            self.flow.borrow_mut().pairing = None;
            if let Some(store) = self.flow.borrow().store.clone()
                && let Ok(mut f) = store.lock()
            {
                f.cancel_pairings();
            }
            self.flow_emit();
            return;
        }
        let Some(store) = self.flow_store() else {
            return;
        };
        let now = haku_core::now_ms();
        let expired = self
            .flow
            .borrow()
            .pairing
            .as_ref()
            .is_some_and(|p| p.expires <= now);
        if expired {
            self.flow.borrow_mut().pairing = None;
        }
        let (needed, port) = match store.lock() {
            Ok(mut f) => {
                f.inbox_dir = self.flow_inbox_dir();
                (f.has_devices() || f.pairing_open(now), f.port)
            }
            Err(_) => return,
        };
        let running = self.flow.borrow().server.is_some();
        if needed && !running {
            let sender = self.background_sender();
            let changed: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                sender.send(Box::new(|b: &Browser| b.flow_changed()));
            });
            match FlowServer::start(store.clone(), port, changed) {
                Ok(server) => {
                    if let Ok(mut f) = store.lock()
                        && f.port != Some(server.port)
                    {
                        f.port = Some(server.port);
                        drop(f);
                        self.flow_save();
                    }
                    let mut state = self.flow.borrow_mut();
                    state.server = Some(server);
                    state.message.clear();
                }
                Err(e) => self.flow.borrow_mut().message = e,
            }
        } else if !needed && running {
            let server = self.flow.borrow_mut().server.take();
            drop(server);
        }
        self.flow_emit();
    }

    pub(crate) fn flow_emit(&self) {
        let status = {
            let state = self.flow.borrow();
            let (entries, devices) = state
                .store
                .as_ref()
                .and_then(|s| s.lock().ok().map(|f| (f.entries(), f.devices())))
                .unwrap_or_default();
            let address = state
                .server
                .as_ref()
                .map(|s| match flow_server::lan_address() {
                    Some(ip) => format!("{ip}:{}", s.port),
                    None => format!("port {}", s.port),
                });
            FlowStatus {
                state: if !self.flow_allowed() {
                    "unavailable".into()
                } else if !self.settings.borrow().features.my_flow {
                    "off".into()
                } else if !state.message.is_empty() {
                    "unavailable".into()
                } else {
                    "ready".into()
                },
                message: if !self.flow_allowed() {
                    if self.private {
                        "My Flow is not available in private windows.".into()
                    } else {
                        "My Flow runs in this profile's first window.".into()
                    }
                } else {
                    state.message.clone()
                },
                entries,
                devices,
                pairing: state.pairing.clone(),
                address,
                unread: state.unread,
            }
        };
        self.emit(Event::PanelBadge {
            id: "my-flow".into(),
            count: status.unread,
        });
        self.emit(Event::Flow { status });
    }

    /// The server changed the store (runs on the UI thread).
    fn flow_changed(&self) {
        let Some(store) = self.flow.borrow().store.clone() else {
            return;
        };
        let changes = store
            .lock()
            .map(|mut f| f.take_changes())
            .unwrap_or_default();
        if changes.is_empty() {
            return;
        }
        let now = haku_core::now_ms();
        let mut important = false;
        for change in changes {
            match change {
                Change::Entry(entry) => {
                    important = true;
                    self.flow.borrow_mut().unread += 1;
                    let what = match entry.kind {
                        FlowKind::File => format!("a file ({})", entry.text),
                        FlowKind::Link => "a link".into(),
                        FlowKind::Text => "a note".into(),
                    };
                    self.emit(Event::Toast {
                        text: format!("My Flow: {} sent {what}", entry.from_name),
                        kind: "info".into(),
                    });
                }
                Change::Paired(device) => {
                    important = true;
                    self.flow.borrow_mut().pairing = None;
                    self.emit(Event::Toast {
                        text: format!("My Flow: {} is connected", device.name),
                        kind: "success".into(),
                    });
                }
                Change::Seen => {}
            }
        }
        if important {
            self.flow_save();
            self.flow_emit();
        } else if now - self.flow.borrow().last_seen_emit > 30_000 {
            self.flow.borrow_mut().last_seen_emit = now;
            self.flow_emit();
        }
    }

    fn flow_after_edit(&self) {
        self.flow_save();
        self.flow_apply();
    }

    fn flow_add(&self, kind: FlowKind, text: String, file: Option<FlowFile>) {
        let Some(store) = self.flow_store() else {
            return;
        };
        if let Ok(mut f) = store.lock() {
            f.add_from_pc(kind, text, file, haku_core::now_ms());
        }
        self.flow_after_edit();
    }

    pub(crate) fn flow_pair(&self) {
        if !self.flow_enabled() {
            return;
        }
        let Some(store) = self.flow_store() else {
            return;
        };
        let now = haku_core::now_ms();
        let Ok((pairing, key)) = store.lock().map(|mut f| f.start_pairing(now)) else {
            return;
        };
        self.flow_apply();
        let port = self.flow.borrow().server.as_ref().map(|s| s.port);
        let (Some(port), Some(ip)) = (port, flow_server::lan_address()) else {
            self.flow.borrow_mut().message = if port.is_none() {
                "My Flow could not open a network port.".into()
            } else {
                "This computer is not connected to a network. Connect it to the same Wi-Fi as your phone.".into()
            };
            self.flow_emit();
            return;
        };
        self.flow.borrow_mut().message.clear();
        self.flow.borrow_mut().pairing = Some(FlowPairing {
            url: flow_server::pairing_url(ip, port, &pairing, &key),
            expires: now + PAIRING_TTL_MS,
        });
        self.flow_emit();
        // Stop listening once the code expires (if no phone is paired).
        self.background_sender().post_after(
            Duration::from_millis(PAIRING_TTL_MS as u64 + 1000),
            |b: &Browser| b.flow_apply(),
        );
    }

    pub(crate) fn flow_cancel_pairing(&self) {
        self.flow.borrow_mut().pairing = None;
        if let Some(store) = self.flow.borrow().store.clone()
            && let Ok(mut f) = store.lock()
        {
            f.cancel_pairings();
        }
        self.flow_apply();
    }

    pub(crate) fn flow_send(&self, text: String) {
        let text: String = text.trim().chars().take(20_000).collect();
        if text.is_empty() || !self.flow_enabled() {
            return;
        }
        let kind = if haku_core::flow::is_link(&text) {
            FlowKind::Link
        } else {
            FlowKind::Text
        };
        self.flow_add(kind, text, None);
    }

    pub(crate) fn flow_send_page(&self, tab: Option<TabId>) {
        let Some(tab) = self.target_tab(tab) else {
            return;
        };
        let url = self
            .session
            .borrow()
            .tab(&tab)
            .map(|t| t.url.clone())
            .unwrap_or_default();
        if haku_core::flow::is_link(&url) {
            self.flow_send(url);
        } else {
            self.emit(Event::Toast {
                text: "Only web pages can be sent to My Flow.".into(),
                kind: "info".into(),
            });
        }
    }

    pub(crate) fn flow_send_file(&self) {
        if !self.flow_enabled() {
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_parent(&self.window)
            .set_title("Send a file to My Flow")
            .pick_file()
        else {
            return;
        };
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size as usize > haku_core::flow::MAX_FILE {
            self.emit(Event::Toast {
                text: "My Flow sends files up to 25 MB.".into(),
                kind: "error".into(),
            });
            return;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".into());
        let mime = mime_guess::from_path(&path)
            .first_or_octet_stream()
            .to_string();
        let file = FlowFile {
            name: name.clone(),
            size,
            mime,
            path: path.display().to_string(),
        };
        self.flow_add(FlowKind::File, name, Some(file));
    }

    fn flow_entry(&self, id: &str) -> Option<haku_core::flow::FlowEntry> {
        let store = self.flow.borrow().store.clone()?;
        let f = store.lock().ok()?;
        f.entries().into_iter().find(|e| e.id == id)
    }

    pub(crate) fn flow_open(&self, id: &str) {
        let Some(entry) = self.flow_entry(id) else {
            return;
        };
        match (entry.kind, entry.file) {
            (FlowKind::Link, _) => self.open_url_in_new_tab(&entry.text),
            (FlowKind::File, Some(file)) => {
                if let Err(e) = open::that_detached(&file.path) {
                    warn!("opening {}: {e}", file.path);
                }
            }
            _ => {}
        }
    }

    pub(crate) fn flow_show_file(&self, id: &str) {
        if let Some(file) = self.flow_entry(id).and_then(|e| e.file)
            && let Some(dir) = std::path::Path::new(&file.path).parent()
        {
            let _ = open::that_detached(dir);
        }
    }

    pub(crate) fn flow_remove(&self, id: &str) {
        if let Some(store) = self.flow.borrow().store.clone()
            && let Ok(mut f) = store.lock()
        {
            f.remove_entry(id);
        }
        self.flow_after_edit();
    }

    pub(crate) fn flow_clear(&self) {
        if let Some(store) = self.flow.borrow().store.clone()
            && let Ok(mut f) = store.lock()
        {
            f.clear();
        }
        self.flow_after_edit();
    }

    pub(crate) fn flow_forget_device(&self, device: &str) {
        if let Some(store) = self.flow.borrow().store.clone()
            && let Ok(mut f) = store.lock()
        {
            f.forget_device(device);
        }
        self.flow_after_edit();
    }

    pub(crate) fn flow_seen(&self) {
        if self.flow.borrow().unread > 0 {
            self.flow.borrow_mut().unread = 0;
            self.flow_emit();
        }
    }

    pub(crate) fn flow_shutdown(&self) {
        let server = self.flow.borrow_mut().server.take();
        drop(server);
        self.flow_save();
    }
}

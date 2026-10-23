/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! WebExtensions: lifecycle, extension contexts, messaging and the browser integration
//! (toolbar buttons, popups, context menus, shortcuts, request rules). The API methods
//! themselves are in `ext_api.rs`; see `crate::extensions` for the overall design.

use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::time::{Duration, Instant};

use haku_core::extensions::dnr::{self, RuleSet};
use haku_core::extensions::manifest::ContentScript;
use haku_core::extensions::registry::{ExtensionList, ExtensionRecord, ExtensionStorage};
use haku_core::extensions::{Extension, ExtensionSource, package};
use haku_core::ipc::{ContextMenuInfo, Event, ExtCommandInfo, ExtMenuItem, ExtensionInfo, Rect};
use haku_core::session::TabId;
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use log::{info, warn};
use serde_json::{Value, json};
use servo::{
    UserContentManager, UserScript, WebResourceLoad, WebResourceResponse, WebView, WebViewBuilder,
    WebViewId,
};

use super::Browser;
use super::tabs::Pane;
use crate::extensions::{self as ext, ContextKind, Running};

const EXT_PREFIX: &str = "\u{1}haku-ext:";
/// How long a request may wait for an extension's blocking `webRequest` listener. Servo
/// fetches one request at a time while one is held, so this is kept short.
const WEB_REQUEST_TIMEOUT: Duration = Duration::from_millis(400);
/// The longest startup page loads wait for extensions with blocking `webRequest` listeners.
const STARTUP_GATE: Duration = Duration::from_secs(5);
/// Unanswered `sendMessage` calls give up after this long.
const REPLY_TIMEOUT: Duration = Duration::from_secs(300);

/// Why an extension is being started (decides `runtime.onInstalled` / `onStartup`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lifecycle {
    Startup,
    Install,
    Enable,
    Reload,
}

/// Where an API call came from, and where its answer goes.
#[derive(Clone)]
pub struct Endpoint {
    pub ext: String,
    pub kind: ContextKind,
    pub webview: WebView,
    /// For content scripts and extension pages in tabs.
    pub tab: Option<TabId>,
    pub frame: i64,
    pub url: String,
}

pub struct PendingReply {
    pub to: Endpoint,
    pub call: u64,
    pub remaining: usize,
    pub deadline: Instant,
}

pub struct HeldRequest {
    pub load: WebResourceLoad,
    pub tab: TabId,
    pub remaining: usize,
    pub deadline: Instant,
    pub main_frame: bool,
}

/// A `permissions.request` waiting for the user's answer.
pub struct PermissionRequest {
    pub ext: String,
    pub permissions: Vec<String>,
    pub origins: Vec<String>,
    pub to: Endpoint,
    pub call: u64,
}

pub struct PendingInstall {
    pub bytes: Vec<u8>,
}

/// All extension state the browser keeps.
#[derive(Default)]
pub struct ExtState {
    pub list: ExtensionList,
    pub running: BTreeMap<String, Running>,
    /// Installed but failed to load: (record, error).
    pub failed: Vec<(ExtensionRecord, String)>,
    /// Extension pages by webview: (extension, kind, webview).
    pub pages: HashMap<WebViewId, (String, ContextKind, WebView)>,
    pub replies: HashMap<u64, PendingReply>,
    /// Port id -> (opener, receiver).
    pub ports: HashMap<String, (Endpoint, Endpoint)>,
    pub held: HashMap<u64, HeldRequest>,
    pub next_id: u64,
    pub tab_ints: HashMap<TabId, i64>,
    pub int_tabs: HashMap<i64, TabId>,
    pub popup: Option<(String, WebView, Rect)>,
    pub installs: HashMap<u32, PendingInstall>,
    /// `permissions.request` prompts waiting for the user, by request id.
    pub permission_requests: HashMap<u32, PermissionRequest>,
    /// Last tab state each extension saw, for tabs.onUpdated/onCreated/onRemoved/onActivated.
    pub tab_snapshot: HashMap<TabId, Value>,
    pub last_active: Option<TabId>,
    /// The last page context menu, for contextMenus.onClicked: (menu id, tab, info).
    pub last_menu: Option<(u32, TabId, ContextMenuInfo)>,
    /// `scripting.executeScript` calls waiting for a function's result.
    pub scripts: HashMap<u64, super::ext_api::PendingScript>,
    /// Content managers of web panels and popup windows, so content scripts reach them too.
    pub extra_managers: Vec<Rc<UserContentManager>>,
    pub info_dirty: bool,
    /// Startup page loads waiting for these extensions' background pages (see
    /// `ext_after_startup`).
    pub startup_waiting: std::collections::HashSet<String>,
    pub startup_deadline: Option<Instant>,
    pub startup_work: Option<super::Deferred>,
    /// Extensions from `--load-extension`, never saved to the list.
    pub session_only: std::collections::HashSet<String>,
    /// The tab that was active when the popup opened; switching tabs closes the popup.
    pub popup_tab: Option<TabId>,
    /// The popup a click outside just closed (see `ext_dismiss_popup`).
    pub popup_dismissed: Option<(String, Instant)>,
    /// Last download state each listening extension saw.
    pub download_snapshot: Option<HashMap<String, Value>>,
    pub last_tick: Option<Instant>,
    pub last_tab_scan: Option<Instant>,
}

impl ExtState {
    pub fn next(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }
}

#[derive(serde::Deserialize)]
struct ExtCall {
    t: String,
    c: u64,
    m: String,
    #[serde(default)]
    a: Vec<Value>,
    #[serde(default)]
    f: Option<i64>,
    #[serde(default)]
    u: Option<String>,
}

impl Browser {
    // ------------------------------------------------------------------ lifecycle

    pub(crate) fn ext_ui_locale(&self) -> String {
        self.settings
            .borrow()
            .languages
            .accept_languages
            .first()
            .cloned()
            .unwrap_or_else(|| "en-US".into())
    }

    /// Load and start every enabled extension (at startup).
    pub(crate) fn load_extensions(&self) {
        let Some(paths) = self.ext_paths.as_ref() else {
            return;
        };
        let list = ExtensionList::load(&paths.extension_list());
        self.ext.borrow_mut().list = list.clone();
        for record in list.extensions {
            if record.enabled && (!self.private || record.allow_private) {
                self.start_extension(record, Lifecycle::Startup);
            }
        }
        self.ext.borrow_mut().info_dirty = true;
    }

    /// Write the extension list (not from private windows, which save nothing).
    pub(crate) fn save_extension_list(&self) {
        let Some(paths) = self.paths.as_ref() else {
            return;
        };
        let mut list = self.ext.borrow().list.clone();
        let session_only = self.ext.borrow().session_only.clone();
        list.extensions.retain(|r| !session_only.contains(&r.id));
        if let Err(e) = list.save(&paths.extension_list()) {
            warn!("saving extensions.json: {e}");
        }
    }

    /// `--load-extension <folder>`: run an unpacked extension for this session only.
    pub(crate) fn ext_load_for_session(&self, dir: &std::path::Path) {
        match haku_core::extensions::load_unpacked(dir, haku_core::now_ms()) {
            Ok((record, _)) => {
                let id = record.id.clone();
                self.stop_extension(&id);
                {
                    let mut st = self.ext.borrow_mut();
                    st.session_only.insert(id.clone());
                    st.list.upsert(record.clone());
                }
                let stored = self.ext.borrow().list.get(&id).cloned().unwrap_or(record);
                self.start_extension(stored, Lifecycle::Startup);
            }
            Err(e) => warn!("--load-extension {}: {e}", dir.display()),
        }
    }

    fn start_extension(&self, record: ExtensionRecord, lifecycle: Lifecycle) {
        let id = record.id.clone();
        let extension = match Extension::load(record.clone(), &self.ext_ui_locale()) {
            Ok(e) => e,
            Err(e) => {
                warn!("extension {id} failed to load: {e}");
                let mut st = self.ext.borrow_mut();
                st.failed.retain(|(r, _)| r.id != id);
                st.failed.push((record, e.to_string()));
                st.info_dirty = true;
                return;
            }
        };
        let storage = self
            .ext_paths
            .as_ref()
            .map(|p| ExtensionStorage::load(&p.extension_storage(), &id))
            .unwrap_or_default();
        let mut running = Running::new(extension, storage);
        // Static rulesets and persisted dynamic rules.
        for ruleset in running.ext.manifest.rulesets.clone() {
            match running
                .ext
                .file(&ruleset.path)
                .and_then(|p| std::fs::read_to_string(p).ok())
            {
                Some(text) => match serde_json::from_str::<Value>(&text) {
                    Ok(v) => {
                        let (rules, errors) = RuleSet::parse_list(&v);
                        if !errors.is_empty() {
                            running.errors.push(format!(
                                "ruleset {}: {} rules could not be used ({})",
                                ruleset.id,
                                errors.len(),
                                errors[0]
                            ));
                        }
                        running
                            .static_rules
                            .insert(ruleset.id.clone(), RuleSet { rules });
                    }
                    Err(e) => running.errors.push(format!("ruleset {}: {e}", ruleset.id)),
                },
                None => running.errors.push(format!(
                    "ruleset {} is missing ({})",
                    ruleset.id, ruleset.path
                )),
            }
        }
        if let Some(paths) = self.ext_paths.as_ref() {
            let dir = paths.extension_storage();
            if let Ok(text) = std::fs::read_to_string(dir.join(format!("{id}.dnr.json")))
                && let Ok(saved) = serde_json::from_str::<SavedRules>(&text)
            {
                running.dynamic_rules = saved.dynamic;
                running.disabled_static = saved.disabled_static;
            }
            if let Ok(text) = std::fs::read_to_string(dir.join(format!("{id}.scripts.json")))
                && let Ok(saved) = serde_json::from_str::<BTreeMap<String, ContentScript>>(&text)
            {
                running.persisted_scripts = saved.keys().cloned().collect();
                running.registered_scripts = saved;
            }
        }
        running.rebuild_rules();
        if let Some(a) = running.ext.manifest.action.clone() {
            running.action.title = a.default_title.clone();
            running.action.popup = a.default_popup.clone();
        }
        let version = running.ext.manifest.version.clone();
        // Chrome's rules: `onInstalled` for a first install, a new version, a reinstall or an
        // unpacked reload (reason "update"); `onStartup` when the browser starts; nothing when
        // an extension is merely enabled.
        let previous = record.last_version.clone();
        let previous_json = if previous.is_empty() {
            Value::Null
        } else {
            Value::String(previous.clone())
        };
        running.lifecycle = if previous.is_empty() {
            Some(("runtime.onInstalled", vec![json!({ "reason": "install" })]))
        } else if previous != version || matches!(lifecycle, Lifecycle::Install | Lifecycle::Reload)
        {
            Some((
                "runtime.onInstalled",
                vec![json!({ "reason": "update", "previousVersion": previous_json })],
            ))
        } else if lifecycle == Lifecycle::Startup {
            Some(("runtime.onStartup", vec![]))
        } else {
            None
        };
        {
            let mut st = self.ext.borrow_mut();
            st.failed.retain(|(r, _)| r.id != id);
            if let Some(r) = st.list.get_mut(&id) {
                r.last_version = version;
            }
            st.running.insert(id.clone(), running);
            st.info_dirty = true;
        }
        self.save_extension_list();
        self.refresh_content_script(&id);
        self.start_background(&id);
        info!("extension {id} started");
    }

    fn stop_extension(&self, id: &str) {
        let running = self.ext.borrow_mut().running.remove(id);
        let Some(running) = running else { return };
        if let Some(script) = running.content_script.as_ref() {
            for manager in self.all_content_managers() {
                manager.remove_script(script.clone());
            }
        }
        let popup_closed = {
            let mut st = self.ext.borrow_mut();
            st.pages.retain(|_, (ext, _, _)| ext != id);
            st.ports.retain(|_, (a, b)| a.ext != id && b.ext != id);
            st.replies.retain(|_, r| r.to.ext != id);
            // Requests held for its webRequest listeners are released by their deadline.
            st.scripts.retain(|_, s| s.to.ext != id);
            st.permission_requests.retain(|_, p| p.ext != id);
            let open = st.popup.as_ref().is_some_and(|(e, _, _)| e == id);
            if open {
                st.popup = None;
            }
            open
        };
        if popup_closed {
            self.emit(Event::ExtPopupClosed);
        }
        // Tabs showing this extension's pages close, as in Chrome.
        let tabs: Vec<TabId> = self
            .session
            .borrow()
            .tabs
            .iter()
            .filter(|t| t.url.starts_with(&ext::base_url(id)))
            .map(|t| t.id.clone())
            .collect();
        let closed = !tabs.is_empty();
        for t in tabs {
            self.session.borrow_mut().close(&t);
        }
        drop(running);
        if closed {
            self.session_changed();
        }
        self.ext.borrow_mut().info_dirty = true;
    }

    /// Run `work` (the startup page loads) once every extension that may block requests with
    /// `webRequest` has loaded its background page, or after [`STARTUP_GATE`]. Chrome likewise
    /// holds network startup for such extensions, so their listeners see the first pages.
    /// Holding the requests themselves is not an option: Servo intercepts one request at a
    /// time, so the background pages could not load either.
    pub(crate) fn ext_after_startup(&self, work: impl FnOnce(&Browser) + 'static) {
        let waiting: std::collections::HashSet<String> = self
            .ext
            .borrow()
            .running
            .values()
            .filter(|r| r.ext.has_permission("webRequest") && r.background.is_some())
            .map(|r| r.id().to_owned())
            .collect();
        if waiting.is_empty() {
            work(self);
            return;
        }
        let mut st = self.ext.borrow_mut();
        st.startup_waiting = waiting;
        st.startup_deadline = Some(Instant::now() + STARTUP_GATE);
        st.startup_work = Some(Box::new(work));
    }

    /// Open the startup gate if it is due (all waited-for background pages loaded, or timeout).
    fn ext_release_startup(&self, force: bool) {
        let work = {
            let mut st = self.ext.borrow_mut();
            let due = force
                || st.startup_waiting.is_empty()
                || st.startup_deadline.is_some_and(|d| Instant::now() >= d);
            if !due {
                return;
            }
            st.startup_waiting.clear();
            st.startup_deadline = None;
            st.startup_work.take()
        };
        if let Some(work) = work {
            work(self);
        }
    }

    fn start_background(&self, id: &str) {
        let url = self
            .ext
            .borrow()
            .running
            .get(id)
            .and_then(Running::background_url);
        let Some(url) = url.and_then(|u| url::Url::parse(&u).ok()) else {
            return;
        };
        let webview = WebViewBuilder::new(&self.servo, self.ext_context.clone())
            .url(url)
            .hidpi_scale_factor(euclid::Scale::new(1.0))
            .delegate(self.rc())
            .build();
        webview.hide();
        let mut st = self.ext.borrow_mut();
        st.pages.insert(
            webview.id(),
            (id.to_owned(), ContextKind::Background, webview.clone()),
        );
        if let Some(r) = st.running.get_mut(id) {
            r.background = Some(webview);
        }
    }

    /// Every content manager pages use (tabs, web panels).
    fn all_content_managers(&self) -> Vec<Rc<UserContentManager>> {
        let mut out: Vec<Rc<UserContentManager>> = self
            .views
            .borrow()
            .values()
            .filter_map(|v| v.content_manager.clone())
            .collect();
        out.extend(self.ext.borrow().extra_managers.iter().cloned());
        out
    }

    /// Add the running extensions' content scripts to a new page's content manager.
    pub(crate) fn add_extension_scripts(&self, manager: &Rc<UserContentManager>) {
        for running in self.ext.borrow().running.values() {
            if let Some(script) = running.content_script.as_ref() {
                manager.add_script(script.clone());
            }
        }
    }

    pub(crate) fn remember_content_manager(&self, manager: Rc<UserContentManager>) {
        self.ext.borrow_mut().extra_managers.push(manager);
    }

    /// Regenerate an extension's content script and swap it in every content manager. Pages
    /// pick it up on their next load (Servo runs user scripts when a document starts).
    pub(crate) fn refresh_content_script(&self, id: &str) {
        let locale = self.ext_ui_locale();
        let (old, new) = {
            let mut st = self.ext.borrow_mut();
            let Some(r) = st.running.get_mut(id) else {
                return;
            };
            let new = r
                .content_user_script(&locale, self.private)
                .map(|s| Rc::new(UserScript::new(s, None)));
            (std::mem::replace(&mut r.content_script, new.clone()), new)
        };
        for manager in self.all_content_managers() {
            if let Some(old) = old.as_ref() {
                manager.remove_script(old.clone());
            }
            if let Some(new) = new.as_ref() {
                manager.add_script(new.clone());
            }
        }
    }

    // ------------------------------------------------------------------ install, remove, toggle

    pub(crate) fn ext_install_file(&self) {
        if self.paths.is_none() {
            self.toast(
                "Extensions can't be installed in a private window.",
                "error",
            );
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_parent(&self.window)
            .add_filter("Extensions", &["crx", "xpi", "zip"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read(&path) {
            Ok(bytes) if bytes.len() <= 256 * 1024 * 1024 => self.ext_prompt_install(bytes),
            Ok(_) => self.toast("That extension package is too large.", "error"),
            Err(e) => self.toast(format!("Could not read the file: {e}"), "error"),
        }
    }

    pub(crate) fn ext_prompt_install(&self, bytes: Vec<u8>) {
        let manifest = package::read_manifest(&bytes)
            .map_err(|e| e.to_string())
            .and_then(|t| haku_core::extensions::Manifest::parse(&t).map_err(|e| e.to_string()));
        let manifest = match manifest {
            Ok(m) => m,
            Err(e) => {
                self.toast(format!("This extension can't be installed: {e}"), "error");
                return;
            }
        };
        let request = self.next_id();
        let update = self.ext.borrow().list.extensions.iter().any(|r| {
            r.runtime_id == manifest.gecko_id.clone().unwrap_or_default()
                || manifest
                    .key
                    .as_deref()
                    .and_then(package::id_from_manifest_key)
                    .as_deref()
                    == Some(r.id.as_str())
        });
        // Display strings may be localized; use the raw ones if __MSG_ keys cannot resolve yet.
        let name = if manifest.name.starts_with("__MSG_") {
            "This extension".to_owned()
        } else {
            manifest.name.clone()
        };
        self.emit(Event::ExtInstallPrompt {
            request,
            name,
            version: manifest.version.clone(),
            warnings: manifest.permission_warnings(),
            update,
        });
        self.ext
            .borrow_mut()
            .installs
            .insert(request, PendingInstall { bytes });
    }

    pub(crate) fn ext_install_answer(&self, request: u32, accept: bool) {
        let Some(pending) = self.ext.borrow_mut().installs.remove(&request) else {
            return;
        };
        if !accept {
            return;
        }
        let Some(paths) = self.paths.clone() else {
            return;
        };
        match haku_core::extensions::install_package(
            &pending.bytes,
            &paths.extensions(),
            haku_core::now_ms(),
        ) {
            Ok((record, manifest)) => {
                let id = record.id.clone();
                self.stop_extension(&id);
                self.ext.borrow_mut().list.upsert(record.clone());
                self.save_extension_list();
                let stored = self.ext.borrow().list.get(&id).cloned().unwrap_or(record);
                if stored.enabled {
                    self.start_extension(stored, Lifecycle::Install);
                }
                self.toast(format!("Added {}", manifest.name), "info");
            }
            Err(e) => self.toast(format!("Installing failed: {e}"), "error"),
        }
    }

    pub(crate) fn ext_load_unpacked(&self) {
        if self.paths.is_none() {
            self.toast("Extensions can't be loaded in a private window.", "error");
            return;
        }
        let Some(dir) = rfd::FileDialog::new()
            .set_parent(&self.window)
            .pick_folder()
        else {
            return;
        };
        match haku_core::extensions::load_unpacked(&dir, haku_core::now_ms()) {
            Ok((record, _)) => {
                let id = record.id.clone();
                self.stop_extension(&id);
                self.ext.borrow_mut().list.upsert(record.clone());
                self.save_extension_list();
                let stored = self.ext.borrow().list.get(&id).cloned().unwrap_or(record);
                self.start_extension(stored, Lifecycle::Install);
            }
            Err(e) => self.toast(format!("Could not load the extension: {e}"), "error"),
        }
    }

    pub(crate) fn ext_set_enabled(&self, id: &str, enabled: bool) {
        let record = {
            let mut st = self.ext.borrow_mut();
            let Some(r) = st.list.get_mut(id) else { return };
            r.enabled = enabled;
            r.clone()
        };
        self.save_extension_list();
        if enabled {
            if !self.private || record.allow_private {
                self.start_extension(record, Lifecycle::Enable);
            }
        } else {
            self.stop_extension(id);
        }
        self.ext.borrow_mut().info_dirty = true;
    }

    pub(crate) fn ext_update_record(&self, id: &str, change: impl FnOnce(&mut ExtensionRecord)) {
        {
            let mut st = self.ext.borrow_mut();
            if let Some(r) = st.list.get_mut(id) {
                change(r);
            }
            let updated = st.list.get(id).cloned();
            if let (Some(updated), Some(running)) = (updated, st.running.get_mut(id)) {
                running.ext.record = updated;
            }
            st.info_dirty = true;
        }
        self.save_extension_list();
    }

    pub(crate) fn ext_remove(&self, id: &str) {
        let uninstall_url = self
            .ext
            .borrow()
            .running
            .get(id)
            .and_then(|r| r.uninstall_url.clone());
        self.stop_extension(id);
        let record = self.ext.borrow_mut().list.remove(id);
        self.ext.borrow_mut().failed.retain(|(r, _)| r.id != id);
        self.save_extension_list();
        if let (Some(record), Some(paths)) = (record, self.paths.as_ref()) {
            if record.source == ExtensionSource::Package
                && record.dir.starts_with(paths.extensions())
            {
                let _ = std::fs::remove_dir_all(&record.dir);
            }
            let storage = paths.extension_storage();
            let _ = std::fs::remove_file(ExtensionStorage::path(&storage, id));
            let _ = std::fs::remove_file(storage.join(format!("{id}.dnr.json")));
            let _ = std::fs::remove_file(storage.join(format!("{id}.scripts.json")));
        }
        if let Some(url) =
            uninstall_url.filter(|u| u.starts_with("https://") || u.starts_with("http://"))
        {
            self.open_url_in_new_tab(&url);
        }
        self.ext.borrow_mut().info_dirty = true;
    }

    pub(crate) fn ext_reload(&self, id: &str) {
        let record = self.ext.borrow().list.get(id).cloned();
        self.stop_extension(id);
        if let Some(record) = record.filter(|r| r.enabled) {
            self.start_extension(record, Lifecycle::Reload);
        }
    }

    pub(crate) fn ext_open_options(&self, id: &str) {
        let url = self.ext.borrow().running.get(id).and_then(|r| {
            r.ext
                .manifest
                .options_page
                .as_ref()
                .map(|p| format!("{}{}", r.base(), p.trim_start_matches('/')))
        });
        match url {
            Some(url) => self.open_url_in_new_tab(&url),
            None => self.toast("This extension has no options page.", "info"),
        }
    }

    pub(crate) fn ext_toggle_ruleset(&self, id: &str, ruleset: &str, enabled: bool) {
        self.ext_update_record(id, |r| {
            r.ruleset_overrides.insert(ruleset.to_owned(), enabled);
        });
        if let Some(r) = self.ext.borrow_mut().running.get_mut(id) {
            r.rebuild_rules();
        }
    }

    // ------------------------------------------------------------------ UI information

    /// Send the Extensions list to the UI if anything changed (called from `pump`).
    pub(crate) fn ext_flush_info(&self) {
        if !std::mem::take(&mut self.ext.borrow_mut().info_dirty) {
            return;
        }
        let active = self.active_tab_id();
        let st = self.ext.borrow();
        let mut items: Vec<ExtensionInfo> = Vec::new();
        for record in &st.list.extensions {
            if let Some(r) = st.running.get(&record.id) {
                items.push(self.ext_info(r, active.as_ref()));
            } else {
                let failed = st
                    .failed
                    .iter()
                    .find(|(f, _)| f.id == record.id)
                    .map(|(_, e)| e.clone());
                let loaded = Extension::load(record.clone(), &self.ext_ui_locale()).ok();
                items.push(ExtensionInfo {
                    id: record.id.clone(),
                    runtime_id: record.runtime_id.clone(),
                    name: loaded.as_ref().map(|e| e.name.clone()).unwrap_or_else(|| {
                        record
                            .dir
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    }),
                    version: loaded
                        .as_ref()
                        .map(|e| e.manifest.version.clone())
                        .unwrap_or_default(),
                    description: loaded
                        .as_ref()
                        .map(|e| e.description.clone())
                        .unwrap_or_default(),
                    manifest_version: loaded
                        .as_ref()
                        .map(|e| e.manifest.manifest_version)
                        .unwrap_or(0),
                    icon: loaded.as_ref().and_then(|e| {
                        e.manifest.best_icon(48).map(|p| {
                            format!("{}{}", ext::base_url(&record.id), p.trim_start_matches('/'))
                        })
                    }),
                    enabled: record.enabled,
                    allow_private: record.allow_private,
                    pinned: record.pinned,
                    unpacked: record.source == ExtensionSource::Unpacked,
                    path: record.dir.display().to_string(),
                    homepage: loaded
                        .as_ref()
                        .and_then(|e| e.manifest.homepage_url.clone()),
                    has_options: false,
                    has_popup: false,
                    action_title: String::new(),
                    badge_text: String::new(),
                    badge_color: String::new(),
                    action_icon: None,
                    action_enabled: false,
                    warnings: loaded
                        .as_ref()
                        .map(|e| e.manifest.permission_warnings())
                        .unwrap_or_default(),
                    errors: failed.into_iter().collect(),
                    commands: Vec::new(),
                    rulesets: Vec::new(),
                    unsupported: Vec::new(),
                });
            }
        }
        let developer_mode = st.list.developer_mode;
        drop(st);
        self.emit(Event::Extensions {
            items,
            developer_mode,
        });
    }

    fn ext_info(&self, r: &Running, active: Option<&TabId>) -> ExtensionInfo {
        let m = &r.ext.manifest;
        let base = r.base();
        let action = r.action_for(active);
        let icon_url = |p: &str| {
            if p.starts_with("data:") {
                p.to_owned()
            } else {
                format!("{base}{}", p.trim_start_matches('/'))
            }
        };
        let badge = match (&action.badge_text, r.badge_from_rule_count) {
            (Some(t), _) if !t.is_empty() => t.clone(),
            (_, true) => active
                .and_then(|t| r.matched.get(t))
                .filter(|n| **n > 0)
                .map(|n| n.to_string())
                .unwrap_or_default(),
            _ => String::new(),
        };
        let mut unsupported = Vec::new();
        for p in &m.permissions {
            let missing = match p.as_str() {
                "nativeMessaging" => "Native messaging",
                "proxy" => "Proxy control",
                "debugger" => "The debugger API",
                "tabCapture" | "desktopCapture" => "Screen and tab capture",
                "tts" | "ttsEngine" => "Text to speech",
                "privacy" => "Privacy settings control",
                "sidePanel" => "Side panel",
                "identity" => "Sign-in (identity)",
                "webRequestBlocking" if m.manifest_version == 3 => {
                    "Blocking webRequest in Manifest V3"
                }
                _ => continue,
            };
            unsupported.push(missing.to_owned());
        }
        if m.permissions.iter().any(|p| p == "webNavigation") {
            unsupported.push("Navigation events (webNavigation)".to_owned());
        }
        for (key, what) in [
            (
                "chrome_url_overrides",
                "Replacing the new tab, history or bookmarks page",
            ),
            ("devtools_page", "Developer tools panels"),
            ("omnibox", "Address bar keywords (omnibox)"),
            ("side_panel", "Side panel"),
            ("sidebar_action", "Sidebar (Firefox sidebar_action)"),
            (
                "chrome_settings_overrides",
                "Changing the search engine or home page",
            ),
            ("tts_engine", "Text to speech"),
        ] {
            if m.raw.get(key).is_some() && !unsupported.iter().any(|u| u == what) {
                unsupported.push(what.to_owned());
            }
        }
        ExtensionInfo {
            id: r.id().to_owned(),
            runtime_id: r.ext.record.runtime_id.clone(),
            name: r.ext.name.clone(),
            version: m.version.clone(),
            description: r.ext.description.clone(),
            manifest_version: m.manifest_version,
            icon: m.best_icon(48).map(icon_url),
            enabled: true,
            allow_private: r.ext.record.allow_private,
            pinned: r.ext.record.pinned,
            unpacked: r.ext.record.source == ExtensionSource::Unpacked,
            path: r.ext.record.dir.display().to_string(),
            homepage: m.homepage_url.clone(),
            has_options: m.options_page.is_some(),
            has_popup: action.popup.as_deref().is_some_and(|p| !p.is_empty()),
            action_title: action.title.clone().unwrap_or_else(|| r.ext.name.clone()),
            badge_text: badge,
            badge_color: action
                .badge_color
                .clone()
                .unwrap_or_else(|| "#d93025".into()),
            action_icon: action
                .icon
                .as_deref()
                .map(icon_url)
                .or_else(|| {
                    m.action.as_ref().and_then(|a| {
                        let mut icons = a.default_icon.clone();
                        if icons.is_empty() {
                            icons = m.icons.clone();
                        }
                        icons
                            .iter()
                            .find(|(s, _)| **s >= 32)
                            .or_else(|| icons.iter().next_back())
                            .map(|(_, p)| icon_url(p))
                    })
                })
                .or_else(|| m.best_icon(32).map(icon_url)),
            action_enabled: action.enabled.unwrap_or(true),
            warnings: m.permission_warnings(),
            errors: r.errors.clone(),
            commands: m
                .commands
                .iter()
                .map(|(name, spec)| ExtCommandInfo {
                    name: name.clone(),
                    description: if spec.description.is_empty() && name == "_execute_action" {
                        "Activate the extension".into()
                    } else {
                        r.ext
                            .messages
                            .localize(&spec.description, &r.ext.record.runtime_id)
                    },
                    shortcut: r
                        .ext
                        .record
                        .shortcuts
                        .get(name)
                        .cloned()
                        .or_else(|| spec.suggested_key.clone())
                        .unwrap_or_default(),
                })
                .collect(),
            rulesets: m
                .rulesets
                .iter()
                .map(|rs| {
                    (
                        rs.id.clone(),
                        r.ext
                            .record
                            .ruleset_overrides
                            .get(&rs.id)
                            .copied()
                            .unwrap_or(rs.enabled),
                    )
                })
                .collect(),
            unsupported,
        }
    }

    // ------------------------------------------------------------------ serving extension files

    /// Answer a request for `https://<id>.haku-extension.invalid/...`.
    pub(crate) fn serve_extension(&self, webview: &WebView, load: WebResourceLoad, id: &str) {
        let request = load.request();
        let url = request.url.clone();
        let path = url.path().trim_start_matches('/').to_owned();
        let st = self.ext.borrow();
        let Some(running) = st.running.get(id) else {
            drop(st);
            return not_found(load);
        };
        // Who is asking: the extension itself, the browser UI (icons), or a web page.
        let page_url = webview.url();
        let own = st.pages.get(&webview.id()).is_some_and(|(e, _, _)| e == id)
            || page_url.as_ref().and_then(ext::id_of_url).as_deref() == Some(id);
        let ui = self.is_ui(webview);
        let navigation = request.is_for_main_frame;
        let allowed = own
            || ui
            || navigation
            || running
                .ext
                .is_web_accessible(&path, page_url.as_ref().map(url::Url::as_str));
        if !allowed {
            drop(st);
            return not_found(load);
        }
        let incognito = self.private;
        let locale = self.ext_ui_locale();
        let (body, mime) = if path == "__haku/api.js" {
            if !own {
                drop(st);
                return not_found(load);
            }
            let kind = if url.query().is_some_and(|q| q.contains("ctx=background")) {
                ContextKind::Background
            } else {
                ContextKind::Tab
            };
            (
                running
                    .page_api_script(&locale, incognito, kind)
                    .into_bytes(),
                "text/javascript; charset=utf-8".to_owned(),
            )
        } else if path == "_generated_background_page.html" {
            match running.background_html() {
                Some(html) => (html.into_bytes(), "text/html; charset=utf-8".to_owned()),
                None => {
                    drop(st);
                    return not_found(load);
                }
            }
        } else {
            let file = if path.is_empty() {
                None
            } else {
                running.ext.file(&path)
            };
            match file.and_then(|f| std::fs::read(f).ok()) {
                Some(bytes) => {
                    let mime = ext::mime_for(&path);
                    if mime.starts_with("text/html") {
                        (
                            ext::inject_api(&String::from_utf8_lossy(&bytes)).into_bytes(),
                            mime,
                        )
                    } else {
                        (bytes, mime)
                    }
                }
                None => {
                    drop(st);
                    return not_found(load);
                }
            }
        };
        let mut csp = running.csp();
        // The service-worker shim's `importScripts` must evaluate the fetched scripts
        // synchronously, which needs eval. Only the generated page that hosts the worker
        // script gets it (see SECURITY.md).
        if path == "_generated_background_page.html"
            && matches!(
                running.ext.manifest.background,
                Some(haku_core::extensions::manifest::Background::ServiceWorker { .. })
            )
        {
            csp = csp.replacen("script-src", "script-src 'unsafe-eval'", 1);
        }
        drop(st);
        let mut headers = HeaderMap::new();
        if let Ok(v) = HeaderValue::from_str(&mime) {
            headers.insert(http::header::CONTENT_TYPE, v);
        }
        if mime.starts_with("text/html")
            && let Ok(v) = HeaderValue::from_str(&csp)
        {
            headers.insert(HeaderName::from_static("content-security-policy"), v);
        }
        headers.insert(
            http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
            HeaderValue::from_static("*"),
        );
        headers.insert(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        );
        let response = WebResourceResponse::new(url)
            .status_code(StatusCode::OK)
            .headers(headers);
        let mut intercepted = load.intercept(response);
        intercepted.send_body_data(body);
        intercepted.finish();
    }

    // ------------------------------------------------------------------ messages from extensions

    /// A console line from any webview: extension API calls start with [`EXT_PREFIX`].
    pub(crate) fn is_ext_message(message: &str) -> bool {
        message.starts_with(EXT_PREFIX)
    }

    pub(crate) fn handle_ext_message(&self, webview: WebView, message: &str) {
        let Some(payload) = message.strip_prefix(EXT_PREFIX) else {
            return;
        };
        if payload.len() > 64 * 1024 * 1024 {
            return;
        }
        let Ok(call) = serde_json::from_str::<ExtCall>(payload) else {
            return;
        };
        let ext_id = {
            let st = self.ext.borrow();
            match st.running.values().find(|r| r.token == call.t) {
                Some(r) => r.id().to_owned(),
                None => return,
            }
        };
        // Extension pages must be the extension's own origin; anything else is a content script.
        let page_url = webview.url();
        let own_page =
            page_url.as_ref().and_then(ext::id_of_url).as_deref() == Some(ext_id.as_str());
        let kind = {
            let st = self.ext.borrow();
            match st.pages.get(&webview.id()) {
                Some((e, k, _)) if e == &ext_id => *k,
                _ if own_page => ContextKind::Tab,
                _ => ContextKind::Content,
            }
        };
        let tab = self.tab_for_webview(&webview);
        if kind == ContextKind::Content
            && tab.is_none()
            && self.panel_for_webview(&webview).is_none()
        {
            return;
        }
        if kind == ContextKind::Tab {
            self.ext.borrow_mut().pages.insert(
                webview.id(),
                (ext_id.clone(), ContextKind::Tab, webview.clone()),
            );
        }
        let endpoint = Endpoint {
            ext: ext_id,
            kind,
            webview,
            tab,
            frame: if kind == ContextKind::Content {
                call.f.unwrap_or(0)
            } else {
                0
            },
            url: if kind == ContextKind::Content {
                call.u.unwrap_or_default()
            } else {
                page_url.map(|u| u.to_string()).unwrap_or_default()
            },
        };
        if let Some(result) = self.ext_call(&endpoint, call.c, &call.m, call.a)
            && call.c != 0
        {
            self.ext_reply(&endpoint, call.c, result);
        }
    }

    /// Answer an API call.
    pub(crate) fn ext_reply(&self, to: &Endpoint, call: u64, result: Result<Value, String>) {
        let msg = match result {
            Ok(v) => json!({ "c": call, "r": v }),
            Err(e) => json!({ "c": call, "e": e }),
        };
        self.ext_send(to, msg);
    }

    /// Deliver a message to one context.
    pub(crate) fn ext_send(&self, to: &Endpoint, mut msg: Value) {
        let st = self.ext.borrow();
        let Some(r) = st.running.get(&to.ext) else {
            return;
        };
        let json = |v: &Value| serde_json::to_string(v).unwrap_or_else(|_| "null".into());
        let script = if to.kind == ContextKind::Content {
            msg["fr"] = json!(to.frame);
            format!(
                "window.__hakuRelay && window.__hakuRelay({}, {})",
                json(&json!(r.token)),
                json(&msg)
            )
        } else {
            let d = json(&json!(r.dispatch));
            format!("window[{d}] && window[{d}]({})", json(&msg))
        };
        drop(st);
        let target = if to.kind == ContextKind::Content {
            to.tab
                .as_ref()
                .and_then(|t| self.webview_for_tab(t))
                .unwrap_or_else(|| to.webview.clone())
        } else {
            to.webview.clone()
        };
        target.evaluate_javascript(script, |_| {});
    }

    /// Fire an event in every context of an extension that listens to it.
    pub(crate) fn ext_event(
        &self,
        ext_id: &str,
        name: &str,
        args: Vec<Value>,
        include_content: bool,
    ) {
        for target in self.ext_listeners(ext_id, name, include_content, None) {
            let mut msg = json!({ "ev": name, "a": args });
            if target.kind == ContextKind::Content {
                msg["fr"] = Value::Null;
            }
            self.ext_send_raw(&target, msg);
        }
    }

    /// Like `ext_send` but without forcing a frame (content events go to every frame).
    fn ext_send_raw(&self, to: &Endpoint, msg: Value) {
        let st = self.ext.borrow();
        let Some(r) = st.running.get(&to.ext) else {
            return;
        };
        let json = |v: &Value| serde_json::to_string(v).unwrap_or_else(|_| "null".into());
        let (script, target) = if to.kind == ContextKind::Content {
            let Some(w) = to.tab.as_ref().and_then(|t| self.webview_for_tab(t)) else {
                return;
            };
            (
                format!(
                    "window.__hakuRelay && window.__hakuRelay({}, {})",
                    json(&json!(r.token)),
                    json(&msg)
                ),
                w,
            )
        } else {
            let d = json(&json!(r.dispatch));
            (
                format!("window[{d}] && window[{d}]({})", json(&msg)),
                to.webview.clone(),
            )
        };
        drop(st);
        target.evaluate_javascript(script, |_| {});
    }

    /// Contexts of an extension that listen to `event`: extension pages, and (optionally)
    /// content scripts, either in every tab or in one.
    pub(crate) fn ext_listeners(
        &self,
        ext_id: &str,
        event: &str,
        include_content: bool,
        only_tab: Option<&TabId>,
    ) -> Vec<Endpoint> {
        let st = self.ext.borrow();
        let Some(r) = st.running.get(ext_id) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if only_tab.is_none() {
            for (wid, events) in &r.listening {
                if !events.contains(event) {
                    continue;
                }
                if let Some((_, kind, webview)) = st.pages.get(wid) {
                    out.push(Endpoint {
                        ext: ext_id.to_owned(),
                        kind: *kind,
                        webview: webview.clone(),
                        tab: self.tab_for_webview(webview),
                        frame: 0,
                        url: webview.url().map(|u| u.to_string()).unwrap_or_default(),
                    });
                }
            }
        }
        if include_content {
            for (tab, set) in &r.content_listening {
                if only_tab.is_some_and(|t| t != tab) {
                    continue;
                }
                let frames: Vec<i64> = set
                    .iter()
                    .filter(|(_, e)| e == event)
                    .map(|(f, _)| *f)
                    .collect();
                if frames.is_empty() {
                    continue;
                }
                let Some(webview) = self.webview_for_tab(tab) else {
                    continue;
                };
                for frame in frames {
                    out.push(Endpoint {
                        ext: ext_id.to_owned(),
                        kind: ContextKind::Content,
                        webview: webview.clone(),
                        tab: Some(tab.clone()),
                        frame,
                        url: String::new(),
                    });
                }
            }
        }
        out
    }

    /// Send a request-style event (one that listeners answer) and route the first answer to
    /// `reply_to`. Returns false when nobody listens.
    pub(crate) fn ext_ask(
        &self,
        targets: Vec<Endpoint>,
        event: &str,
        args: Vec<Value>,
        reply_to: &Endpoint,
        call: u64,
    ) -> bool {
        if targets.is_empty() {
            return false;
        }
        let id = self.ext.borrow_mut().next();
        self.ext.borrow_mut().replies.insert(
            id,
            PendingReply {
                to: reply_to.clone(),
                call,
                remaining: targets.len(),
                deadline: Instant::now() + REPLY_TIMEOUT,
            },
        );
        for t in targets {
            self.ext_send(&t, json!({ "ev": event, "a": args, "r": id }));
        }
        true
    }

    /// `runtime.reply`: an answer (or a "no answer") to an `ext_ask`.
    pub(crate) fn ext_handle_reply(&self, id: u64, value: Value, responded: bool, is_error: bool) {
        let done = {
            let mut st = self.ext.borrow_mut();
            let Some(p) = st.replies.get_mut(&id) else {
                return;
            };
            if responded {
                st.replies.remove(&id).map(|p| {
                    (
                        p,
                        if is_error {
                            Err(value.as_str().unwrap_or("Error").to_owned())
                        } else {
                            Ok(value)
                        },
                    )
                })
            } else {
                p.remaining = p.remaining.saturating_sub(1);
                if p.remaining == 0 {
                    st.replies.remove(&id).map(|p| (p, Ok(Value::Null)))
                } else {
                    None
                }
            }
        };
        if let Some((pending, result)) = done
            && pending.call != 0
        {
            self.ext_reply(&pending.to, pending.call, result);
        }
    }

    // ------------------------------------------------------------------ ports

    pub(crate) fn ext_port_connect(
        &self,
        from: &Endpoint,
        port: String,
        target: &Value,
        name: String,
        frame: Option<i64>,
    ) {
        let targets = match target.get("kind").and_then(Value::as_str) {
            Some("tab") => {
                let tab = target
                    .get("tabId")
                    .and_then(Value::as_i64)
                    .and_then(|i| self.ext.borrow().int_tabs.get(&i).cloned());
                match tab {
                    Some(tab) => self
                        .ext_listeners(&from.ext, "runtime.onConnect", true, Some(&tab))
                        .into_iter()
                        .filter(|e| frame.is_none_or(|f| e.frame == f))
                        .collect::<Vec<_>>(),
                    None => Vec::new(),
                }
            }
            _ => {
                let mut list = self.ext_listeners(&from.ext, "runtime.onConnect", false, None);
                list.retain(|e| e.webview.id() != from.webview.id());
                // Prefer the background context, as Chrome effectively does for most extensions.
                list.sort_by_key(|e| e.kind != ContextKind::Background);
                list.truncate(1);
                list
            }
        };
        let Some(to) = targets.into_iter().next() else {
            self.ext_send(from, json!({ "ev": "port.disconnected", "a": [port, "Could not establish connection. Receiving end does not exist."] }));
            return;
        };
        let sender = self.ext_sender_json(from);
        self.ext
            .borrow_mut()
            .ports
            .insert(port.clone(), (from.clone(), to.clone()));
        self.ext_send(&to, json!({ "ev": "port.open", "a": [port, name, sender] }));
    }

    pub(crate) fn ext_port_post(&self, from: &Endpoint, port: &str, message: Value) {
        let other = {
            let st = self.ext.borrow();
            st.ports.get(port).map(|(a, b)| {
                if same_endpoint(a, from) {
                    b.clone()
                } else {
                    a.clone()
                }
            })
        };
        if let Some(other) = other {
            self.ext_send(
                &other,
                json!({ "ev": "port.message", "a": [port, message] }),
            );
        }
    }

    pub(crate) fn ext_port_disconnect(&self, from: &Endpoint, port: &str) {
        let ends = self.ext.borrow_mut().ports.remove(port);
        if let Some((a, b)) = ends {
            let other = if same_endpoint(&a, from) { b } else { a };
            self.ext_send(
                &other,
                json!({ "ev": "port.disconnected", "a": [port, Value::Null] }),
            );
        }
    }

    /// Close ports and pending replies of a context that went away (tab closed or navigated,
    /// popup closed).
    pub(crate) fn ext_context_gone(&self, webview: Option<WebViewId>, tab: Option<&TabId>) {
        let dead = |e: &Endpoint| {
            webview.is_some_and(|w| e.webview.id() == w && e.kind != ContextKind::Content)
                || (e.kind == ContextKind::Content
                    && tab.is_some_and(|t| e.tab.as_ref() == Some(t)))
        };
        let mut notify = Vec::new();
        {
            let mut st = self.ext.borrow_mut();
            let ports: Vec<String> = st
                .ports
                .iter()
                .filter(|(_, (a, b))| dead(a) || dead(b))
                .map(|(k, _)| k.clone())
                .collect();
            for p in ports {
                if let Some((a, b)) = st.ports.remove(&p) {
                    let survivor = if dead(&a) { b } else { a };
                    if !dead(&survivor) {
                        notify.push((survivor, p));
                    }
                }
            }
            if let Some(w) = webview {
                st.pages.remove(&w);
                for r in st.running.values_mut() {
                    r.listening.remove(&w);
                }
            }
            if let Some(t) = tab {
                for r in st.running.values_mut() {
                    r.content_listening.remove(t);
                }
            }
        }
        for (e, p) in notify {
            self.ext_send(
                &e,
                json!({ "ev": "port.disconnected", "a": [p, Value::Null] }),
            );
        }
    }

    // ------------------------------------------------------------------ tabs as the API sees them

    pub(crate) fn ext_tab_int(&self, tab: &TabId) -> i64 {
        let mut st = self.ext.borrow_mut();
        if let Some(i) = st.tab_ints.get(tab) {
            return *i;
        }
        let i = st.tab_ints.len() as i64 + 1;
        st.tab_ints.insert(tab.clone(), i);
        st.int_tabs.insert(i, tab.clone());
        i
    }

    pub(crate) fn ext_tab_of(&self, int: i64) -> Option<TabId> {
        self.ext.borrow().int_tabs.get(&int).cloned()
    }

    /// A `tabs.Tab` for an extension. URL, title and favicon need the "tabs" permission or
    /// host access to the tab (or activeTab), as in Chrome.
    pub(crate) fn ext_tab_json(&self, ext_id: &str, tab: &TabId) -> Value {
        let session = self.session.borrow();
        let Some(t) = session.tab(tab).cloned() else {
            return Value::Null;
        };
        // One API window holds every workspace's tabs, so the index is across all of them.
        let index = session.tabs.iter().position(|x| x.id == t.id).unwrap_or(0);
        let active = session.active_workspace == t.workspace_id
            && session.active_tabs.get(&t.workspace_id) == Some(&t.id);
        drop(session);
        let meta = self.tab_meta(tab);
        let url = meta
            .as_ref()
            .map(|m| m.url.clone())
            .unwrap_or_else(|| t.url.clone());
        let st = self.ext.borrow();
        let visible = st.running.get(ext_id).is_some_and(|r| {
            r.ext.has_permission("tabs")
                || r.ext.can_access_url(&url)
                || r.active_tab_grants.contains(tab)
                || url.starts_with(&r.base())
        });
        drop(st);
        let id = self.ext_tab_int(tab);
        let opener = t.opener.as_ref().map(|o| self.ext_tab_int(o));
        let mut v = json!({
            "id": id,
            "index": index,
            "windowId": 1,
            "active": active,
            "highlighted": active,
            "selected": active,
            "pinned": t.pinned,
            "incognito": self.private,
            "status": if meta.as_ref().is_some_and(|m| m.loading) { "loading" } else { "complete" },
            "audible": meta.as_ref().is_some_and(|m| m.audible),
            "mutedInfo": { "muted": t.muted },
            "discarded": t.snoozed,
            "autoDiscardable": true,
            "groupId": -1,
            "lastAccessed": t.last_active,
        });
        if let Some(o) = opener {
            v["openerTabId"] = json!(o);
        }
        if visible {
            v["url"] = json!(url);
            v["title"] = json!(
                meta.as_ref()
                    .map(|m| m.title.clone())
                    .unwrap_or_else(|| t.title.clone())
            );
            if let Some(f) = meta.as_ref().and_then(|m| m.favicon.clone()) {
                v["favIconUrl"] = json!(f);
            }
        }
        v
    }

    /// `runtime.MessageSender` for messages from `from`.
    pub(crate) fn ext_sender_json(&self, from: &Endpoint) -> Value {
        let runtime_id = self
            .ext
            .borrow()
            .running
            .get(&from.ext)
            .map(|r| r.ext.record.runtime_id.clone())
            .unwrap_or_default();
        let mut v = json!({ "id": runtime_id, "url": from.url, "frameId": from.frame, "documentLifecycle": "active" });
        if let Ok(u) = url::Url::parse(&from.url) {
            v["origin"] = json!(u.origin().ascii_serialization());
        }
        if let Some(tab) = from.tab.as_ref()
            && matches!(from.kind, ContextKind::Content | ContextKind::Tab)
        {
            v["tab"] = self.ext_tab_json(&from.ext, tab);
        }
        v
    }

    /// tabs.onCreated / onUpdated / onRemoved / onActivated, from a snapshot diff (called from
    /// `pump`). Cheap when no extension listens.
    pub(crate) fn ext_tab_events(&self) {
        let interested: Vec<String> = {
            let st = self.ext.borrow();
            st.running
                .values()
                .filter(|r| {
                    r.listening
                        .values()
                        .any(|e| e.iter().any(|n| n.starts_with("tabs.")))
                })
                .map(|r| r.id().to_owned())
                .collect()
        };
        if interested.is_empty() {
            return;
        }
        // At most five scans a second: pump runs after every input event.
        {
            let mut st = self.ext.borrow_mut();
            if st
                .last_tab_scan
                .is_some_and(|t| t.elapsed() < Duration::from_millis(200))
            {
                return;
            }
            st.last_tab_scan = Some(Instant::now());
        }
        let tabs: Vec<TabId> = self
            .session
            .borrow()
            .tabs
            .iter()
            .map(|t| t.id.clone())
            .collect();
        let active = self.active_tab_id();
        let mut current: HashMap<TabId, Value> = HashMap::new();
        for t in &tabs {
            // Snapshot with full details (filtered per extension when sent).
            let meta = self.tab_meta(t);
            let tab = self.session.borrow().tab(t).cloned();
            if let Some(tab) = tab {
                current.insert(
                    t.clone(),
                    json!({
                        "url": meta.as_ref().map(|m| m.url.clone()).unwrap_or(tab.url.clone()),
                        "title": meta.as_ref().map(|m| m.title.clone()).unwrap_or(tab.title.clone()),
                        "status": if meta.as_ref().is_some_and(|m| m.loading) { "loading" } else { "complete" },
                        "pinned": tab.pinned,
                        "audible": meta.as_ref().is_some_and(|m| m.audible),
                        "mutedInfo": { "muted": tab.muted },
                        "favIconUrl": meta.as_ref().and_then(|m| m.favicon.clone()),
                    }),
                );
            }
        }
        let previous = std::mem::replace(&mut self.ext.borrow_mut().tab_snapshot, current.clone());
        let previous_active =
            std::mem::replace(&mut self.ext.borrow_mut().last_active, active.clone());
        for ext_id in &interested {
            for (t, now) in &current {
                match previous.get(t) {
                    None => self.ext_event(
                        ext_id,
                        "tabs.onCreated",
                        vec![self.ext_tab_json(ext_id, t)],
                        false,
                    ),
                    Some(before) if before != now => {
                        let mut change = serde_json::Map::new();
                        for (k, v) in now.as_object().into_iter().flatten() {
                            if before.get(k) != Some(v) {
                                change.insert(k.clone(), v.clone());
                            }
                        }
                        let tab_json = self.ext_tab_json(ext_id, t);
                        if tab_json.get("url").is_none() {
                            change.remove("url");
                            change.remove("title");
                            change.remove("favIconUrl");
                        }
                        if !change.is_empty() {
                            let id = self.ext_tab_int(t);
                            self.ext_event(
                                ext_id,
                                "tabs.onUpdated",
                                vec![json!(id), Value::Object(change), tab_json],
                                false,
                            );
                        }
                    }
                    _ => {}
                }
            }
            for t in previous.keys().filter(|t| !current.contains_key(*t)) {
                let id = self.ext_tab_int(t);
                self.ext_event(
                    ext_id,
                    "tabs.onRemoved",
                    vec![
                        json!(id),
                        json!({ "windowId": 1, "isWindowClosing": false }),
                    ],
                    false,
                );
            }
            if active != previous_active
                && let Some(a) = active.as_ref()
            {
                let id = self.ext_tab_int(a);
                let previous_id = previous_active.as_ref().map(|p| self.ext_tab_int(p));
                self.ext_event(
                    ext_id,
                    "tabs.onActivated",
                    vec![json!({ "tabId": id, "windowId": 1, "previousTabId": previous_id })],
                    false,
                );
            }
        }
        for t in previous.keys().filter(|t| !current.contains_key(*t)) {
            self.ext_context_gone(None, Some(t));
        }
    }

    // ------------------------------------------------------------------ toolbar, popups

    pub(crate) fn ext_action_click(&self, id: &str, anchor: Rect) {
        // Clicking the button of an open popup closes it (the mouse-down already did).
        let dismissed = self.ext.borrow_mut().popup_dismissed.take();
        if dismissed.is_some_and(|(e, at)| e == id && at.elapsed() < Duration::from_millis(500)) {
            return;
        }
        if self
            .ext
            .borrow()
            .popup
            .as_ref()
            .is_some_and(|(e, _, _)| e == id)
        {
            self.ext_close_popup();
            return;
        }
        let active = self.active_tab_id();
        let (popup, enabled, base) = {
            let st = self.ext.borrow();
            let Some(r) = st.running.get(id) else { return };
            let a = r.action_for(active.as_ref());
            (
                a.popup.filter(|p| !p.is_empty()),
                a.enabled.unwrap_or(true),
                r.base(),
            )
        };
        if !enabled {
            return;
        }
        if let Some(t) = active.as_ref()
            && let Some(r) = self.ext.borrow_mut().running.get_mut(id)
            && r.ext.has_permission("activeTab")
        {
            r.active_tab_grants.insert(t.clone());
        }
        match popup {
            Some(p) => {
                self.ext_open_popup(id, &format!("{base}{}", p.trim_start_matches('/')), anchor)
            }
            None => {
                let tab = active
                    .map(|t| self.ext_tab_json(id, &t))
                    .unwrap_or(Value::Null);
                self.ext_event(id, "action.onClicked", vec![tab.clone()], false);
                self.ext_event(id, "browserAction.onClicked", vec![tab], false);
            }
        }
    }

    pub(crate) fn ext_open_popup(&self, id: &str, url: &str, anchor: Rect) {
        self.ext_close_popup();
        let Ok(parsed) = url::Url::parse(url) else {
            return;
        };
        let webview = WebViewBuilder::new(&self.servo, self.pane_context(Pane::Popup))
            .url(parsed)
            .hidpi_scale_factor(euclid::Scale::new(self.scale()))
            .delegate(self.rc())
            .build();
        webview.show();
        {
            let mut st = self.ext.borrow_mut();
            st.pages.insert(
                webview.id(),
                (id.to_owned(), ContextKind::Popup, webview.clone()),
            );
            st.popup = Some((id.to_owned(), webview.clone(), anchor));
            st.popup_tab = self.active_tab_id();
        }
        self.emit(Event::ExtPopup {
            id: id.to_owned(),
            anchor,
            width: 360.0,
            height: 240.0,
        });
        self.window.request_redraw();
    }

    /// Fit the popup to its content (called when its page has loaded, and again shortly after
    /// for pages that render late).
    pub(crate) fn ext_measure_popup(&self) {
        let Some((id, webview, anchor)) = self.ext.borrow().popup.clone() else {
            return;
        };
        let browser = self.rc();
        webview.evaluate_javascript(
            // The content's own size, like Chrome: its preferred (max-content) width, capped,
            // then its height at that width. Viewport-independent, so the popup can shrink.
            "(() => { const d = document.documentElement, prev = d.style.width; \
             d.style.width = 'max-content'; const w = Math.ceil(Math.min(d.getBoundingClientRect().width, 800)); \
             d.style.width = w + 'px'; const h = Math.ceil(d.getBoundingClientRect().height); \
             d.style.width = prev; return [w, h]; })()",
            move |result| {
                if let Ok(servo::JSValue::Array(items)) = result {
                    let num = |v: Option<&servo::JSValue>| match v {
                        Some(servo::JSValue::Number(n)) => *n as f32,
                        _ => 0.0,
                    };
                    let (w, h) = (num(items.first()), num(items.get(1)));
                    if w > 0.0 && h > 0.0 {
                        let id = id.clone();
                        browser.defer(move |b| {
                            if b.ext.borrow().popup.as_ref().is_some_and(|(e, _, _)| *e == id) {
                                b.emit(Event::ExtPopup { id, anchor, width: w.clamp(25.0, 800.0), height: h.clamp(25.0, 600.0) });
                            }
                        });
                    }
                }
            },
        );
    }

    pub(crate) fn ext_close_popup(&self) {
        let popup = self.ext.borrow_mut().popup.take();
        if let Some((_, webview, _)) = popup {
            self.ext_context_gone(Some(webview.id()), None);
            drop(webview);
            self.emit(Event::ExtPopupClosed);
            if self.keyboard_pane.get() == Some(Pane::Popup) {
                self.keyboard_pane.set(None);
            }
            self.window.request_redraw();
        }
    }

    pub(crate) fn ext_popup_webview(&self) -> Option<WebView> {
        self.ext.borrow().popup.as_ref().map(|(_, w, _)| w.clone())
    }

    /// A webview closed itself (`window.close()`): the popup, or an offscreen document.
    pub(crate) fn ext_webview_closed(&self, webview: &WebView) -> bool {
        if self
            .ext_popup_webview()
            .is_some_and(|w| w.id() == webview.id())
        {
            self.ext_close_popup();
            return true;
        }
        false
    }

    /// Load events of extension pages (popup sizing).
    pub(crate) fn ext_page_loaded(&self, webview: &WebView) {
        let background = self
            .ext
            .borrow()
            .pages
            .get(&webview.id())
            .filter(|(_, kind, _)| *kind == ContextKind::Background)
            .map(|(id, _, _)| id.clone());
        if let Some(id) = background {
            // The background page has run and registered its listeners: now fire
            // runtime.onInstalled / onStartup (once per start).
            let lifecycle = self
                .ext
                .borrow_mut()
                .running
                .get_mut(&id)
                .and_then(|r| r.lifecycle.take());
            if let Some((event, args)) = lifecycle {
                self.ext_event(&id, event, args, false);
            }
            if self.ext.borrow_mut().startup_waiting.remove(&id) {
                self.ext_release_startup(false);
            }
        }
        if self
            .ext_popup_webview()
            .is_some_and(|w| w.id() == webview.id())
        {
            self.ext_measure_popup();
            self.background
                .post_after(Duration::from_millis(350), |b: &Browser| {
                    b.ext_measure_popup()
                });
            self.background
                .post_after(Duration::from_millis(1200), |b: &Browser| {
                    b.ext_measure_popup()
                });
        }
    }

    // ------------------------------------------------------------------ context menus

    /// Items for a page context menu (and remember the click for onClicked).
    pub(crate) fn ext_menu_items(&self, info: &ContextMenuInfo) -> Vec<ExtMenuItem> {
        let page_url = self.tab_meta(&info.tab).map(|m| m.url).unwrap_or_default();
        let mut contexts = vec!["all", "page"];
        if info.link_url.is_some() {
            contexts.push("link");
        }
        if info.image_url.is_some() {
            contexts.push("image");
        }
        if info.editable {
            contexts.push("editable");
        }
        if info.selection {
            contexts.push("selection");
        }
        let st = self.ext.borrow();
        let mut out = Vec::new();
        for r in st.running.values() {
            let icon = r
                .ext
                .manifest
                .best_icon(16)
                .map(|p| format!("{}{}", r.base(), p.trim_start_matches('/')));
            for m in &r.menus {
                if !m.visible {
                    continue;
                }
                let ctx_ok = m.contexts.iter().any(|c| contexts.contains(&c.as_str()))
                    || (m.contexts.is_empty()
                        && !info.selection
                        && info.link_url.is_none()
                        && info.image_url.is_none()
                        && !info.editable);
                if !ctx_ok {
                    continue;
                }
                if m.document_patterns
                    .as_ref()
                    .is_some_and(|f| !f.matches(&page_url))
                {
                    continue;
                }
                if let Some(f) = m.target_patterns.as_ref() {
                    let target = info
                        .link_url
                        .as_deref()
                        .or(info.image_url.as_deref())
                        .unwrap_or("");
                    if !target.is_empty() && !f.matches(target) {
                        continue;
                    }
                }
                out.push(ExtMenuItem {
                    ext: r.id().to_owned(),
                    id: m.id.clone(),
                    parent: m.parent.clone(),
                    title: m.title.replace("%s", "…"),
                    kind: m.kind.clone(),
                    checked: m.checked,
                    enabled: m.enabled,
                    icon: if m.parent.is_none() {
                        icon.clone()
                    } else {
                        None
                    },
                });
            }
        }
        drop(st);
        self.ext.borrow_mut().last_menu = Some((info.id, info.tab.clone(), info.clone()));
        out
    }

    pub(crate) fn ext_menu_click(&self, ext_id: &str, item: &str, menu: u32) {
        let Some((menu_id, tab, info)) = self.ext.borrow().last_menu.clone() else {
            return;
        };
        if menu_id != menu {
            return;
        }
        let (was_checked, checked, parent) = {
            let mut st = self.ext.borrow_mut();
            let Some(r) = st.running.get_mut(ext_id) else {
                return;
            };
            let Some(pos) = r.menus.iter().position(|m| m.id == item) else {
                return;
            };
            let kind = r.menus[pos].kind.clone();
            let parent = r.menus[pos].parent.clone();
            let was = r.menus[pos].checked;
            match kind.as_str() {
                "checkbox" => r.menus[pos].checked = !was,
                "radio" => {
                    for m in r
                        .menus
                        .iter_mut()
                        .filter(|m| m.kind == "radio" && m.parent == parent)
                    {
                        m.checked = false;
                    }
                    r.menus[pos].checked = true;
                }
                _ => {}
            }
            let now = r.menus[pos].checked;
            (
                if kind == "normal" { None } else { Some(was) },
                if kind == "normal" { None } else { Some(now) },
                parent,
            )
        };
        let page_url = self.tab_meta(&tab).map(|m| m.url).unwrap_or_default();
        let mut data = json!({
            "menuItemId": item,
            "pageUrl": page_url,
            "frameId": 0,
            "editable": info.editable,
        });
        if let Some(p) = parent {
            data["parentMenuItemId"] = json!(p);
        }
        if let Some(l) = info.link_url.as_ref() {
            data["linkUrl"] = json!(l);
        }
        if let Some(i) = info.image_url.as_ref() {
            data["srcUrl"] = json!(i);
            data["mediaType"] = json!("image");
        }
        if let (Some(w), Some(c)) = (was_checked, checked) {
            data["wasChecked"] = json!(w);
            data["checked"] = json!(c);
        }
        let tab_json = self.ext_tab_json(ext_id, &tab);
        if info.selection
            && let Some(webview) = self.webview_for_tab(&tab)
        {
            let browser = self.rc();
            let ext_id = ext_id.to_owned();
            webview.evaluate_javascript("String(window.getSelection() || '')", move |result| {
                let mut data = data.clone();
                if let Ok(servo::JSValue::String(text)) = result {
                    data["selectionText"] = json!(text);
                }
                let tab_json = tab_json.clone();
                browser.defer(move |b| {
                    b.ext_event(
                        &ext_id,
                        "contextMenus.onClicked",
                        vec![data, tab_json],
                        false,
                    );
                });
            });
            return;
        }
        self.ext_event(
            ext_id,
            "contextMenus.onClicked",
            vec![data, tab_json],
            false,
        );
    }

    // ------------------------------------------------------------------ shortcuts

    /// Extension keyboard shortcuts: `accelerator` is the pressed combination in our notation.
    pub(crate) fn ext_command_for(
        &self,
        matches: impl Fn(&str) -> bool,
    ) -> Option<(String, String)> {
        let st = self.ext.borrow();
        for r in st.running.values() {
            for (name, spec) in &r.ext.manifest.commands {
                let key = r
                    .ext
                    .record
                    .shortcuts
                    .get(name)
                    .cloned()
                    .or_else(|| spec.suggested_key.clone())
                    .unwrap_or_default();
                if !key.is_empty()
                    && matches(&key.replace("Command", "Ctrl").replace("MacCtrl", "Ctrl"))
                {
                    return Some((r.id().to_owned(), name.clone()));
                }
            }
        }
        None
    }

    pub(crate) fn ext_run_command(&self, ext_id: &str, name: &str) {
        if matches!(
            name,
            "_execute_action" | "_execute_browser_action" | "_execute_page_action"
        ) {
            // The popup opens under the toolbar's right edge when we do not know the button.
            let width = self.window.inner_size().width as f32 / self.ui_scale();
            self.ext_action_click(
                ext_id,
                Rect {
                    x: width - 48.0,
                    y: 40.0,
                    w: 32.0,
                    h: 32.0,
                },
            );
            return;
        }
        let tab = self
            .active_tab_id()
            .map(|t| self.ext_tab_json(ext_id, &t))
            .unwrap_or(Value::Null);
        if let Some(t) = self.active_tab_id()
            && let Some(r) = self.ext.borrow_mut().running.get_mut(ext_id)
            && r.ext.has_permission("activeTab")
        {
            r.active_tab_grants.insert(t);
        }
        self.ext_event(ext_id, "commands.onCommand", vec![json!(name), tab], false);
    }

    // ------------------------------------------------------------------ requests

    /// Extension rules and listeners for a request from a tab. Returns the load if the browser
    /// should keep handling it (built-in blocking), `None` if an extension took it.
    pub(crate) fn ext_filter_request(
        &self,
        tab: &TabId,
        webview: &WebView,
        load: WebResourceLoad,
    ) -> Option<WebResourceLoad> {
        let request = load.request();
        let url = request.url.to_string();
        if !(url.starts_with("http://") || url.starts_with("https://") || url.starts_with("ws")) {
            return Some(load);
        }
        let resource_type = resource_type(request.destination, request.is_for_main_frame);
        let initiator = if request.is_for_main_frame {
            None
        } else {
            webview.url().and_then(|u| u.host_str().map(str::to_owned))
        };
        let method = request.method.as_str().to_owned();
        let tab_int = self.ext_tab_int(tab);
        let main_frame = request.is_for_main_frame;

        // declarativeNetRequest: each extension's winning rule, then across extensions block
        // wins over redirect (Chrome's order).
        let mut redirect: Option<(String, String)> = None;
        let mut blocked_by: Option<String> = None;
        {
            let mut st = self.ext.borrow_mut();
            for r in st.running.values_mut() {
                if r.rules.rules.is_empty() {
                    continue;
                }
                if main_frame {
                    r.allow_all.remove(tab);
                    r.matched.remove(tab);
                    r.matched_seen.remove(tab);
                }
                let req = dnr::Request {
                    url: &url,
                    initiator_host: initiator.as_deref(),
                    resource_type,
                    method: &method,
                    tab_id: tab_int,
                };
                let Some(outcome) = r.rules.evaluate(&req, &r.base()) else {
                    continue;
                };
                if let Some(p) = r.allow_all.get(tab)
                    && outcome.priority <= *p
                    && !main_frame
                {
                    continue;
                }
                let can_redirect =
                    r.ext.can_access_url(&url) || r.ext.has_permission("declarativeNetRequest");
                match outcome.action {
                    dnr::Action::Block => {
                        r.count_match(tab, &url, tab_int, outcome.rule_id);
                        blocked_by = Some(r.id().to_owned());
                    }
                    dnr::Action::AllowAllRequests => {
                        r.allow_all.insert(tab.clone(), outcome.priority);
                    }
                    dnr::Action::Redirect(target) if can_redirect && redirect.is_none() => {
                        r.count_match(tab, &url, tab_int, outcome.rule_id);
                        redirect = Some((r.id().to_owned(), target));
                    }
                    dnr::Action::UpgradeScheme
                        if can_redirect && redirect.is_none() && url.starts_with("http://") =>
                    {
                        redirect =
                            Some((r.id().to_owned(), url.replacen("http://", "https://", 1)));
                    }
                    _ => {}
                }
            }
            if blocked_by.is_some() || redirect.is_some() {
                st.info_dirty = true;
            }
        }
        if blocked_by.is_some() {
            self.ext_block(load, main_frame, &url);
            return None;
        }
        if let Some((_, target)) = redirect {
            self.ext_redirect(tab, load, &target, main_frame);
            return None;
        }

        // webRequest.onBeforeRequest listeners (blocking ones may hold the request briefly).
        let mut blocking: Vec<Endpoint> = Vec::new();
        let mut observers: Vec<Endpoint> = Vec::new();
        {
            let st = self.ext.borrow();
            for r in st.running.values() {
                if r.web_request.is_empty() || !r.ext.can_access_url(&url) {
                    continue;
                }
                let relevant: Vec<&(
                    haku_core::extensions::matching::UrlFilter,
                    Vec<String>,
                    bool,
                )> = r
                    .web_request
                    .iter()
                    .filter(|(f, types, _)| {
                        f.matches(&url)
                            && (types.is_empty() || types.iter().any(|t| t == resource_type))
                    })
                    .collect();
                if relevant.is_empty() {
                    continue;
                }
                let Some(bg) = r.background.clone() else {
                    continue;
                };
                let endpoint = Endpoint {
                    ext: r.id().to_owned(),
                    kind: ContextKind::Background,
                    webview: bg,
                    tab: None,
                    frame: 0,
                    url: String::new(),
                };
                if relevant.iter().any(|(_, _, b)| *b) && r.ext.has_permission("webRequestBlocking")
                {
                    blocking.push(endpoint);
                } else {
                    observers.push(endpoint);
                }
            }
        }
        if blocking.is_empty() && observers.is_empty() {
            return Some(load);
        }
        let request_id = self.ext.borrow_mut().next();
        let details = json!({
            "requestId": request_id.to_string(),
            "url": url,
            "method": method,
            // Servo does not say which frame a subresource is for.
            "frameId": 0,
            "parentFrameId": -1,
            "tabId": tab_int,
            "type": resource_type,
            "timeStamp": haku_core::now_ms() as f64,
            "initiator": webview.url().map(|u| u.origin().ascii_serialization()),
            "originUrl": webview.url().map(|u| u.to_string()),
            "documentUrl": webview.url().map(|u| u.to_string()),
        });
        // This runs inside a Servo callback: send the events from `pump`.
        let mut messages: Vec<(Endpoint, Value)> = observers
            .into_iter()
            .map(|o| {
                (
                    o,
                    json!({ "ev": "webRequest.onBeforeRequest", "a": [details], "r": 0 }),
                )
            })
            .collect();
        let held = !blocking.is_empty();
        let count = blocking.len();
        messages.extend(blocking.into_iter().map(|b| {
            (
                b,
                json!({ "ev": "webRequest.onBeforeRequest", "a": [details], "r": request_id }),
            )
        }));
        self.defer(move |b| {
            for (to, msg) in messages {
                b.ext_send(&to, msg);
            }
        });
        if !held {
            return Some(load);
        }
        self.ext.borrow_mut().held.insert(
            request_id,
            HeldRequest {
                load,
                tab: tab.clone(),
                remaining: count,
                deadline: Instant::now() + WEB_REQUEST_TIMEOUT,
                main_frame,
            },
        );
        None
    }

    /// A blocking webRequest listener decided.
    pub(crate) fn ext_web_request_decided(&self, request_id: u64, decision: &Value) {
        let cancel = decision
            .get("cancel")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let redirect = decision
            .get("redirectUrl")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let held = {
            let mut st = self.ext.borrow_mut();
            let Some(h) = st.held.get_mut(&request_id) else {
                return;
            };
            if cancel || redirect.is_some() {
                st.held.remove(&request_id)
            } else {
                h.remaining = h.remaining.saturating_sub(1);
                if h.remaining == 0 {
                    st.held.remove(&request_id)
                } else {
                    return;
                }
            }
        };
        let Some(held) = held else { return };
        let url = held.load.request().url.to_string();
        if cancel {
            self.ext_block(held.load, held.main_frame, &url);
        } else if let Some(target) = redirect {
            self.ext_redirect(&held.tab, held.load, &target, held.main_frame);
        }
        // Otherwise dropping the load lets the request continue normally.
    }

    /// Release held requests whose listeners took too long, and expire unanswered messages.
    pub(crate) fn ext_expire(&self) {
        let now = Instant::now();
        let expired: Vec<u64> = self
            .ext
            .borrow()
            .held
            .iter()
            .filter(|(_, h)| h.deadline <= now)
            .map(|(k, _)| *k)
            .collect();
        for id in expired {
            let held = self.ext.borrow_mut().held.remove(&id);
            drop(held);
        }
        let stale: Vec<u64> = self
            .ext
            .borrow()
            .replies
            .iter()
            .filter(|(_, r)| r.deadline <= now)
            .map(|(k, _)| *k)
            .collect();
        for id in stale {
            let pending = self.ext.borrow_mut().replies.remove(&id);
            if let Some(p) = pending
                && p.call != 0
            {
                self.ext_reply(&p.to, p.call, Ok(Value::Null));
            }
        }
    }

    pub(crate) fn ext_has_held_requests(&self) -> bool {
        !self.ext.borrow().held.is_empty()
    }

    fn ext_block(&self, load: WebResourceLoad, main_frame: bool, url: &str) {
        let request_url = load.request().url.clone();
        if main_frame {
            let html = format!(
                "<!doctype html><meta charset=utf-8><title>Blocked</title><body style=\"font:15px system-ui;background:#16131c;color:#ddd;display:grid;place-items:center;min-height:100vh;margin:0\"><main><h1 style=\"color:#ff4458\">An extension blocked this page</h1><p>{}</p></main>",
                html_escape::encode_text(url)
            );
            let mut headers = HeaderMap::new();
            headers.insert(
                http::header::CONTENT_TYPE,
                HeaderValue::from_static("text/html; charset=utf-8"),
            );
            let mut intercepted =
                load.intercept(WebResourceResponse::new(request_url).headers(headers));
            intercepted.send_body_data(html.into_bytes());
            intercepted.finish();
        } else {
            load.intercept(WebResourceResponse::new(request_url))
                .cancel();
        }
    }

    /// Redirect a request. Servo does not follow redirects from intercepted responses, so a
    /// navigation is cancelled and the tab loads the target; a subresource is answered with
    /// the target's content (extension files, data: URLs) or fetched by HakuCX.
    fn ext_redirect(&self, tab: &TabId, load: WebResourceLoad, target: &str, main_frame: bool) {
        let request_url = load.request().url.clone();
        if main_frame {
            load.intercept(WebResourceResponse::new(request_url))
                .cancel();
            let tab = tab.clone();
            let target = target.to_owned();
            self.defer(move |b| {
                b.navigate_tab(&tab, &target, haku_core::history::Transition::Link)
            });
            return;
        }
        let respond = |load: WebResourceLoad, bytes: Vec<u8>, mime: String| {
            let mut headers = HeaderMap::new();
            if let Ok(v) = HeaderValue::from_str(&mime) {
                headers.insert(http::header::CONTENT_TYPE, v);
            }
            headers.insert(
                http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
                HeaderValue::from_static("*"),
            );
            let mut intercepted = load.intercept(
                WebResourceResponse::new(request_url.clone())
                    .status_code(StatusCode::OK)
                    .headers(headers),
            );
            intercepted.send_body_data(bytes);
            intercepted.finish();
        };
        if let Some(path) = url::Url::parse(target)
            .ok()
            .filter(|u| ext::id_of_url(u).is_some())
        {
            let id = ext::id_of_url(&path).unwrap_or_default();
            let file = self
                .ext
                .borrow()
                .running
                .get(&id)
                .and_then(|r| r.ext.file(path.path()));
            match file.and_then(|f| std::fs::read(f).ok()) {
                Some(bytes) => respond(load, bytes, ext::mime_for(path.path())),
                None => load
                    .intercept(WebResourceResponse::new(request_url.clone()))
                    .cancel(),
            }
            return;
        }
        if target.starts_with("data:") {
            match crate::downloads::decode_data_url(target) {
                Some((mime, bytes)) => respond(load, bytes, mime),
                None => load
                    .intercept(WebResourceResponse::new(request_url.clone()))
                    .cancel(),
            }
            return;
        }
        if target.starts_with("http://") || target.starts_with("https://") {
            // Fetch the target off the UI thread and answer when it arrives.
            let target = target.to_owned();
            let sender = self.background.clone();
            let held_id = self.ext.borrow_mut().next();
            self.ext.borrow_mut().held.insert(
                held_id,
                HeldRequest {
                    load,
                    tab: tab.clone(),
                    remaining: 1,
                    deadline: Instant::now() + Duration::from_secs(20),
                    main_frame: false,
                },
            );
            std::thread::spawn(move || {
                let result = reqwest::blocking::Client::builder()
                    .timeout(Duration::from_secs(15))
                    .build()
                    .and_then(|c| c.get(&target).send())
                    .and_then(|r| {
                        let mime = r
                            .headers()
                            .get(http::header::CONTENT_TYPE)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("application/octet-stream")
                            .to_owned();
                        r.bytes().map(|b| (mime, b.to_vec()))
                    });
                sender.send(Box::new(move |b: &Browser| {
                    let held = b.ext.borrow_mut().held.remove(&held_id);
                    let Some(held) = held else { return };
                    let url = held.load.request().url.clone();
                    match result {
                        Ok((mime, bytes)) => {
                            let mut headers = HeaderMap::new();
                            if let Ok(v) = HeaderValue::from_str(&mime) {
                                headers.insert(http::header::CONTENT_TYPE, v);
                            }
                            headers.insert(
                                http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
                                HeaderValue::from_static("*"),
                            );
                            let mut intercepted = held.load.intercept(
                                WebResourceResponse::new(url)
                                    .status_code(StatusCode::OK)
                                    .headers(headers),
                            );
                            intercepted.send_body_data(bytes);
                            intercepted.finish();
                        }
                        Err(_) => held.load.intercept(WebResourceResponse::new(url)).cancel(),
                    }
                }));
            });
            return;
        }
        load.intercept(WebResourceResponse::new(request_url))
            .cancel();
    }

    // ------------------------------------------------------------------ alarms

    /// Fire due alarms (called once a second).
    pub(crate) fn ext_tick(&self) {
        let now = haku_core::now_ms() as f64;
        let mut due: Vec<(String, Value)> = Vec::new();
        {
            let mut st = self.ext.borrow_mut();
            for r in st.running.values_mut() {
                let id = r.id().to_owned();
                let mut done = Vec::new();
                for (name, alarm) in r.alarms.iter_mut() {
                    if alarm.when_ms <= now {
                        due.push((id.clone(), json!({ "name": name, "scheduledTime": alarm.when_ms, "periodInMinutes": alarm.period_minutes })));
                        match alarm.period_minutes {
                            Some(p) => alarm.when_ms = now + p * 60_000.0,
                            None => done.push(name.clone()),
                        }
                    }
                }
                for n in done {
                    r.alarms.remove(&n);
                }
            }
        }
        for (id, alarm) in due {
            self.ext_event(&id, "alarms.onAlarm", vec![alarm], false);
        }
        self.ext_expire();
    }

    /// Persist an extension's storage.
    pub(crate) fn ext_save_storage(&self, id: &str) {
        let Some(paths) = self.paths.as_ref() else {
            return;
        };
        let st = self.ext.borrow();
        if let Some(r) = st.running.get(id)
            && let Err(e) = r.storage.save(&paths.extension_storage(), id)
        {
            warn!("saving extension storage: {e}");
        }
    }

    pub(crate) fn ext_save_dynamic_rules(&self, id: &str) {
        let Some(paths) = self.paths.as_ref() else {
            return;
        };
        let st = self.ext.borrow();
        if let Some(r) = st.running.get(id) {
            let dir = paths.extension_storage();
            let _ = std::fs::create_dir_all(&dir);
            let saved = SavedRules {
                dynamic: r.dynamic_rules.clone(),
                disabled_static: r.disabled_static.clone(),
            };
            if let Err(e) =
                haku_core::storage::write_json_atomic(&dir.join(format!("{id}.dnr.json")), &saved)
            {
                warn!("saving extension rules: {e}");
            }
        }
    }

    /// Persist `scripting.registerContentScripts` entries that asked to persist.
    pub(crate) fn ext_save_registered_scripts(&self, id: &str) {
        let Some(paths) = self.paths.as_ref() else {
            return;
        };
        let st = self.ext.borrow();
        if let Some(r) = st.running.get(id) {
            let dir = paths.extension_storage();
            let _ = std::fs::create_dir_all(&dir);
            let saved: BTreeMap<&String, &ContentScript> = r
                .registered_scripts
                .iter()
                .filter(|(k, _)| r.persisted_scripts.contains(*k))
                .collect();
            if let Err(e) = haku_core::storage::write_json_atomic(
                &dir.join(format!("{id}.scripts.json")),
                &saved,
            ) {
                warn!("saving registered content scripts: {e}");
            }
        }
    }

    // ------------------------------------------------------------------ browser hooks

    /// May a page at `from` navigate to the extension page `to`? Only the extension itself, or
    /// a page it exposes in `web_accessible_resources` (as in Chrome).
    pub(crate) fn ext_navigation_allowed(&self, from: Option<&url::Url>, to: &url::Url) -> bool {
        let Some(id) = ext::id_of_url(to) else {
            return true;
        };
        if from.and_then(ext::id_of_url).as_deref() == Some(id.as_str()) {
            return true;
        }
        self.ext.borrow().running.get(&id).is_some_and(|r| {
            r.ext
                .is_web_accessible(to.path(), from.map(url::Url::as_str))
        })
    }

    /// Is this webview an extension page (background, popup, offscreen, or a tab showing one)?
    pub(crate) fn ext_is_page(&self, webview: &WebView) -> bool {
        let st = self.ext.borrow();
        st.pages
            .get(&webview.id())
            .is_some_and(|(_, kind, _)| *kind != ContextKind::Tab)
            || st
                .popup
                .as_ref()
                .is_some_and(|(_, w, _)| w.id() == webview.id())
    }

    /// A tab started loading a new document: its content scripts and their ports are gone,
    /// and `activeTab` access ends.
    pub(crate) fn ext_tab_navigated(&self, tab: &TabId) {
        self.ext_context_gone(None, Some(tab));
        let mut st = self.ext.borrow_mut();
        for r in st.running.values_mut() {
            r.active_tab_grants.remove(tab);
        }
    }

    /// An error logged by an extension page, for the Extensions page's error list.
    pub(crate) fn ext_note_error(&self, webview: &WebView, message: &str) {
        let mut st = self.ext.borrow_mut();
        let Some(id) = st
            .pages
            .get(&webview.id())
            .map(|(e, _, _)| e.clone())
            .or_else(|| st.popup.as_ref().map(|(e, _, _)| e.clone()))
        else {
            return;
        };
        if let Some(r) = st.running.get_mut(&id) {
            let line: String = message.chars().take(500).collect();
            if r.errors.last() != Some(&line) {
                r.errors.push(line);
            }
            if r.errors.len() > 50 {
                r.errors.remove(0);
            }
            st.info_dirty = true;
        }
    }

    /// `history.onVisited` for a committed navigation.
    pub(crate) fn ext_history_visited(&self, url: &str) {
        let targets: Vec<String> = {
            let st = self.ext.borrow();
            st.running
                .values()
                .filter(|r| {
                    r.ext.has_permission("history")
                        && r.listening
                            .values()
                            .any(|e| e.contains("history.onVisited"))
                })
                .map(|r| r.id().to_owned())
                .collect()
        };
        if targets.is_empty() {
            return;
        }
        let visits = self.history.visits_for_url(url).unwrap_or_default();
        let Some((row, _, at, _)) = visits.last().copied() else {
            return;
        };
        let item = json!({ "id": row.to_string(), "url": url, "title": "", "lastVisitTime": at, "visitCount": visits.len(), "typedCount": visits.iter().filter(|v| v.3 == haku_core::history::Transition::Typed).count() });
        for id in targets {
            self.ext_event(&id, "history.onVisited", vec![item.clone()], false);
        }
    }

    /// `downloads.onCreated` / `onChanged` / `onErased`, from a snapshot diff.
    pub(crate) fn ext_download_events(&self) {
        let interested: Vec<String> = {
            let st = self.ext.borrow();
            st.running
                .values()
                .filter(|r| {
                    r.ext.has_permission("downloads")
                        && r.listening
                            .values()
                            .any(|e| e.iter().any(|n| n.starts_with("downloads.")))
                })
                .map(|r| r.id().to_owned())
                .collect()
        };
        if interested.is_empty() {
            self.ext.borrow_mut().download_snapshot = None;
            return;
        }
        let items = self.downloads.borrow().items.clone();
        let current: HashMap<String, Value> = items
            .iter()
            .map(|r| (r.id.clone(), self.ext_download_json(r)))
            .collect();
        let Some(previous) = self
            .ext
            .borrow_mut()
            .download_snapshot
            .replace(current.clone())
        else {
            return;
        };
        for ext_id in &interested {
            for (id, now) in &current {
                match previous.get(id) {
                    None => self.ext_event(ext_id, "downloads.onCreated", vec![now.clone()], false),
                    Some(before) if before != now => {
                        let mut delta = serde_json::Map::new();
                        delta.insert("id".into(), now["id"].clone());
                        for key in [
                            "state",
                            "paused",
                            "canResume",
                            "error",
                            "filename",
                            "totalBytes",
                            "fileSize",
                            "exists",
                            "endTime",
                            "mime",
                        ] {
                            if before.get(key) != now.get(key) {
                                let mut change = serde_json::Map::new();
                                if let Some(v) = before.get(key) {
                                    change.insert("previous".into(), v.clone());
                                }
                                if let Some(v) = now.get(key) {
                                    change.insert("current".into(), v.clone());
                                }
                                delta.insert(key.into(), Value::Object(change));
                            }
                        }
                        if delta.len() > 1 {
                            self.ext_event(
                                ext_id,
                                "downloads.onChanged",
                                vec![Value::Object(delta)],
                                false,
                            );
                        }
                    }
                    _ => {}
                }
            }
            for (id, before) in &previous {
                if !current.contains_key(id) {
                    self.ext_event(
                        ext_id,
                        "downloads.onErased",
                        vec![before["id"].clone()],
                        false,
                    );
                }
            }
        }
    }

    /// Size the popup webview to the rect the UI gave it.
    pub(crate) fn ext_resize_popup(&self) {
        let rect = self.layout.borrow().popup;
        if let (Some(rect), Some(webview)) = (rect, self.ext_popup_webview()) {
            webview.resize(winit::dpi::PhysicalSize::new(
                rect.w.max(1.0) as u32,
                rect.h.max(1.0) as u32,
            ));
        }
    }

    /// The popup loses focus when another tab comes forward (an options page it opened, a
    /// shortcut): close it then, as Chrome does.
    pub(crate) fn ext_check_popup_focus(&self) {
        let stale = {
            let st = self.ext.borrow();
            st.popup.is_some() && st.popup_tab != self.active_tab_id()
        };
        if stale {
            self.ext_close_popup();
        }
    }

    /// A click outside the popup closes it. Remember which one, so the click that lands on its
    /// own toolbar button does not reopen it.
    pub(crate) fn ext_dismiss_popup(&self) {
        let id = self
            .ext
            .borrow()
            .popup
            .as_ref()
            .map(|(id, _, _)| id.clone());
        if let Some(id) = id {
            self.ext_close_popup();
            self.ext.borrow_mut().popup_dismissed = Some((id, Instant::now()));
        }
    }

    pub(crate) fn ext_set_developer_mode(&self, on: bool) {
        {
            let mut st = self.ext.borrow_mut();
            st.list.developer_mode = on;
            st.info_dirty = true;
        }
        self.save_extension_list();
    }

    /// Change an extension command's shortcut ("" removes it).
    pub(crate) fn ext_set_shortcut(&self, id: &str, command: &str, accelerator: &str) {
        let accelerator = accelerator.trim();
        if !accelerator.is_empty() && super::input::parse_accelerator(accelerator).is_none() {
            self.toast("That shortcut can't be used.", "error");
            return;
        }
        let known = self
            .ext
            .borrow()
            .running
            .get(id)
            .is_some_and(|r| r.ext.manifest.commands.contains_key(command));
        if !known {
            return;
        }
        let taken = self
            .ext_command_for(|k| k.eq_ignore_ascii_case(accelerator))
            .filter(|(e, c)| !(e == id && c == command));
        if !accelerator.is_empty()
            && let Some((other, _)) = taken
        {
            let name = self
                .ext
                .borrow()
                .running
                .get(&other)
                .map(|r| r.ext.name.clone())
                .unwrap_or_default();
            self.toast(format!("{accelerator} is already used by {name}."), "error");
            return;
        }
        self.ext_update_record(id, |r| {
            r.shortcuts
                .insert(command.to_owned(), accelerator.to_owned());
        });
    }

    /// Once a second: alarms, idle state, expired requests and replies.
    pub(crate) fn ext_tick_if_due(&self) {
        let due = self
            .ext
            .borrow()
            .last_tick
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(1));
        if !due {
            return;
        }
        self.ext.borrow_mut().last_tick = Some(Instant::now());
        self.ext_release_startup(false);
        self.ext_tick();
        // idle.onStateChanged
        let idle = super::ext_api::idle_ms() as f64;
        let changes: Vec<(String, &'static str)> = {
            let mut st = self.ext.borrow_mut();
            let mut out = Vec::new();
            for r in st.running.values_mut() {
                if !r.ext.has_permission("idle")
                    || !r
                        .listening
                        .values()
                        .any(|e| e.contains("idle.onStateChanged"))
                {
                    continue;
                }
                let state = if idle >= r.idle_interval * 1000.0 {
                    "idle"
                } else {
                    "active"
                };
                if state != r.idle_state {
                    r.idle_state = state;
                    out.push((r.id().to_owned(), state));
                }
            }
            out
        };
        for (id, state) in changes {
            self.ext_event(&id, "idle.onStateChanged", vec![json!(state)], false);
        }
    }

    /// Drop every extension webview before Servo shuts down.
    pub(crate) fn ext_shutdown(&self) {
        let mut st = self.ext.borrow_mut();
        st.popup = None;
        st.pages.clear();
        st.held.clear();
        st.ports.clear();
        st.replies.clear();
        st.scripts.clear();
        st.running.clear();
    }
}

/// `<id>.dnr.json`: dynamic rules and static rules turned off with `updateStaticRules`.
#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct SavedRules {
    dynamic: Vec<Value>,
    disabled_static: BTreeMap<String, std::collections::BTreeSet<i64>>,
}

fn same_endpoint(a: &Endpoint, b: &Endpoint) -> bool {
    a.webview.id() == b.webview.id() && a.kind == b.kind && a.frame == b.frame && a.tab == b.tab
}

fn not_found(load: WebResourceLoad) {
    let url = load.request().url.clone();
    let mut intercepted =
        load.intercept(WebResourceResponse::new(url).status_code(StatusCode::NOT_FOUND));
    intercepted.send_body_data(b"Not found".to_vec());
    intercepted.finish();
}

/// `declarativeNetRequest`/`webRequest` resource type for a Servo request destination.
pub fn resource_type(
    destination: content_security_policy::Destination,
    main_frame: bool,
) -> &'static str {
    use content_security_policy::Destination as D;
    if main_frame {
        return "main_frame";
    }
    match destination {
        D::Document | D::IFrame | D::Frame => "sub_frame",
        D::Script
        | D::ServiceWorker
        | D::SharedWorker
        | D::Worker
        | D::AudioWorklet
        | D::PaintWorklet => "script",
        D::Style | D::Xslt => "stylesheet",
        D::Image => "image",
        D::Font => "font",
        D::Audio | D::Video | D::Track => "media",
        D::Object | D::Embed => "object",
        D::Report => "csp_report",
        D::None | D::Json | D::Text => "xmlhttprequest",
        _ => "other",
    }
}

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Folder sync (see `haku_core::sync`): each device writes one encrypted bundle into a shared
//! folder and merges the others'. File access happens on worker threads, because cloud
//! folders (OneDrive "files on demand") can block while a file downloads; merging happens on
//! the UI thread.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use haku_core::bookmarks::ROOT_SPEED_DIAL;
use haku_core::ipc::{Event, SyncDevice, SyncStatus};
use haku_core::sync::{self, Bundle, FolderInfo, SyncedSettings, SyncedTab};
use haku_core::vault::{Key, Protector};
use log::{info, warn};

use super::Browser;
use crate::os_secrets::Dpapi;

/// Sync this long after a local change (so a burst of edits is written once).
const AFTER_CHANGE: Duration = Duration::from_secs(20);
/// And at least this often while running.
const PERIOD: Duration = Duration::from_secs(10 * 60);
/// History shared with other devices.
const HISTORY_DAYS: i64 = 30;
const HISTORY_MAX: usize = 20_000;

/// What this device remembers about sync (not secret; the key is stored separately,
/// protected by Windows).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct SyncFile {
    device_id: String,
    last_sync: Option<i64>,
    /// When the synced settings last changed here or were taken from another device.
    settings_modified: i64,
    /// The synced settings as they were then (to notice local changes).
    settings_json: String,
}

#[derive(Default)]
pub struct SyncState {
    key: Option<Key>,
    file: SyncFile,
    /// `off`, `needs-passphrase`, `idle`, `syncing` or `error`.
    state: &'static str,
    message: String,
    devices: Vec<SyncDevice>,
    running: bool,
    due: Option<Instant>,
    last_run: Option<Instant>,
}

fn device_name() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "This computer".into())
}

fn sync_dir(folder: &str) -> PathBuf {
    Path::new(folder).join(sync::DIR_NAME)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

impl Browser {
    fn sync_paths(&self) -> Option<(PathBuf, PathBuf)> {
        let p = self.paths.as_ref()?;
        Some((p.root.join("sync-state.json"), p.root.join("sync-key.bin")))
    }

    /// Read this device's sync state and key at startup.
    pub(crate) fn sync_load(&self) {
        let Some((state_path, key_path)) = self.sync_paths() else {
            self.sync.borrow_mut().state = "off";
            return;
        };
        let mut file: SyncFile = haku_core::storage::read_json_or_default(&state_path);
        if file.device_id.is_empty() {
            file.device_id = uuid::Uuid::new_v4().simple().to_string();
        }
        let key = std::fs::read(&key_path)
            .ok()
            .and_then(|blob| Dpapi.unprotect(&blob).ok())
            .and_then(|raw| Key::from_bytes(&raw));
        let enabled = self.settings.borrow().sync.folder_sync;
        let mut s = self.sync.borrow_mut();
        s.state = match (enabled, key.is_some()) {
            (false, _) => "off",
            (true, true) => "idle",
            (true, false) => "needs-passphrase",
        };
        s.key = key;
        s.file = file;
        if s.file.settings_json.is_empty() {
            s.file.settings_json = self.synced_settings_json();
        }
        s.due = Some(Instant::now() + Duration::from_secs(5));
        drop(s);
        self.sync_save_state();
    }

    fn sync_save_state(&self) {
        let Some((state_path, _)) = self.sync_paths() else {
            return;
        };
        let file = self.sync.borrow().file.clone();
        if let Err(e) = haku_core::storage::write_json_atomic(&state_path, &file) {
            warn!("saving sync state: {e}");
        }
    }

    fn synced_settings_json(&self) -> String {
        let value = serde_json::to_value(&*self.settings.borrow()).unwrap_or_default();
        sync::settings_subset(&value).to_string()
    }

    pub(crate) fn sync_emit(&self) {
        let s = self.sync.borrow();
        let status = SyncStatus {
            state: if s.state.is_empty() { "off" } else { s.state }.to_owned(),
            message: s.message.clone(),
            last_sync: s.file.last_sync,
            this_device: device_name(),
            devices: s.devices.clone(),
        };
        drop(s);
        self.emit(Event::Sync { status });
    }

    /// Something that syncs changed here: sync soon.
    pub(crate) fn sync_soon(&self) {
        let mut s = self.sync.borrow_mut();
        if s.key.is_some() {
            let at = Instant::now() + AFTER_CHANGE;
            s.due = Some(s.due.map_or(at, |d| d.min(at)));
        }
    }

    /// Settings changed: if a synced section changed, remember when.
    pub(crate) fn sync_settings_changed(&self) {
        let json = self.synced_settings_json();
        let changed = {
            let mut s = self.sync.borrow_mut();
            if s.file.settings_json == json {
                false
            } else {
                s.file.settings_json = json;
                s.file.settings_modified = haku_core::now_ms();
                true
            }
        };
        if changed {
            self.sync_save_state();
            self.sync_soon();
        }
    }

    /// Called from the background poll: run when due.
    pub(crate) fn sync_tick(&self) {
        let due = {
            let s = self.sync.borrow();
            let enabled = self.settings.borrow().sync.folder_sync;
            enabled
                && s.key.is_some()
                && !s.running
                && (s.due.is_some_and(|d| Instant::now() >= d)
                    || s.last_run.is_some_and(|t| t.elapsed() >= PERIOD))
        };
        if due {
            self.sync_now();
        }
    }

    /// Start using `folder` with `passphrase`: join the folder if other devices use it,
    /// otherwise set it up. Argon2id runs on a worker thread.
    pub(crate) fn sync_setup(&self, folder: String, passphrase: String) {
        if self.paths.is_none() {
            self.toast("Sync can't be set up from a private window.", "error");
            return;
        }
        if passphrase.chars().count() < 10 {
            self.toast(
                "Use a passphrase of at least 10 characters. Every device that syncs needs it.",
                "error",
            );
            return;
        }
        if !Path::new(&folder).is_dir() {
            self.toast("That folder does not exist.", "error");
            return;
        }
        {
            let mut s = self.sync.borrow_mut();
            s.state = "syncing";
            s.message = "Checking the passphrase...".into();
        }
        self.sync_emit();
        let sender = self.background_sender();
        let dir = sync_dir(&folder);
        std::thread::spawn(move || {
            let result = (|| -> Result<Key, String> {
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let info_path = dir.join(sync::INFO_FILE);
                match std::fs::read(&info_path) {
                    Ok(bytes) => {
                        let info = FolderInfo::parse(&bytes).map_err(|e| e.to_string())?;
                        info.unlock(&passphrase).map_err(|e| e.to_string())
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        let (info, key) =
                            FolderInfo::create(&passphrase).map_err(|e| e.to_string())?;
                        write_atomic(&info_path, &info.to_bytes()).map_err(|e| e.to_string())?;
                        Ok(key)
                    }
                    Err(e) => Err(e.to_string()),
                }
            })();
            sender.send(Box::new(move |b: &Browser| match result {
                Ok(key) => {
                    let Some((_, key_path)) = b.sync_paths() else {
                        return;
                    };
                    match Dpapi.protect(key.as_bytes()) {
                        Ok(blob) => {
                            if let Err(e) = haku_core::storage::write_bytes_atomic(&key_path, &blob)
                            {
                                b.toast(format!("Could not store the sync key: {e}"), "error");
                                return;
                            }
                        }
                        Err(e) => {
                            b.toast(
                                format!("Windows could not protect the sync key: {e}"),
                                "error",
                            );
                            return;
                        }
                    }
                    {
                        let mut settings = b.settings.borrow_mut();
                        settings.sync.folder_sync = true;
                        settings.sync.folder = folder.clone();
                    }
                    b.settings_changed();
                    {
                        let mut s = b.sync.borrow_mut();
                        s.key = Some(key);
                        s.state = "idle";
                        s.message.clear();
                    }
                    info!("sync set up");
                    b.sync_now();
                }
                Err(e) => {
                    {
                        let mut s = b.sync.borrow_mut();
                        s.state = if s.key.is_some() {
                            "idle"
                        } else {
                            "needs-passphrase"
                        };
                        s.message = e.clone();
                    }
                    b.toast(e, "error");
                    b.sync_emit();
                }
            }));
        });
    }

    /// Stop syncing: forget the key and remove this device's bundle from the folder.
    pub(crate) fn sync_disconnect(&self) {
        let folder = self.settings.borrow().sync.folder.clone();
        let device = self.sync.borrow().file.device_id.clone();
        if let Some((_, key_path)) = self.sync_paths() {
            let _ = std::fs::remove_file(key_path);
        }
        if !folder.is_empty() {
            let path = sync_dir(&folder).join(sync::bundle_name(&device));
            std::thread::spawn(move || {
                let _ = std::fs::remove_file(path);
            });
        }
        {
            let mut s = self.sync.borrow_mut();
            s.key = None;
            s.state = "off";
            s.message.clear();
            s.devices.clear();
        }
        self.settings.borrow_mut().sync.folder_sync = false;
        self.settings_changed();
        self.sync_emit();
    }

    /// Read the other devices' bundles, merge them, and write ours.
    pub(crate) fn sync_now(&self) {
        let folder = self.settings.borrow().sync.folder.clone();
        let enabled = self.settings.borrow().sync.folder_sync;
        if !enabled
            || folder.is_empty()
            || self.sync.borrow().key.is_none()
            || self.sync.borrow().running
        {
            return;
        }
        {
            let mut s = self.sync.borrow_mut();
            s.running = true;
            s.state = "syncing";
            s.due = None;
            s.last_run = Some(Instant::now());
        }
        self.sync_emit();
        let dir = sync_dir(&folder);
        let own = sync::bundle_name(&self.sync.borrow().file.device_id);
        let sender = self.background_sender();
        std::thread::spawn(move || {
            let read = (|| -> Result<(Vec<u8>, Vec<Vec<u8>>), String> {
                let info = std::fs::read(dir.join(sync::INFO_FILE))
                    .map_err(|e| format!("the sync folder can't be read ({e})"))?;
                let mut bundles = Vec::new();
                for entry in std::fs::read_dir(&dir)
                    .map_err(|e| e.to_string())?
                    .flatten()
                {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name == own || !name.ends_with(&format!(".{}", sync::BUNDLE_EXT)) {
                        continue;
                    }
                    if entry.metadata().is_ok_and(|m| m.len() > 256 * 1024 * 1024) {
                        continue;
                    }
                    if let Ok(bytes) = std::fs::read(entry.path()) {
                        bundles.push(bytes);
                    }
                }
                Ok((info, bundles))
            })();
            sender.send(Box::new(move |b: &Browser| b.sync_merge(dir, own, read)));
        });
    }

    fn sync_merge(&self, dir: PathBuf, own: String, read: Result<(Vec<u8>, Vec<Vec<u8>>), String>) {
        let fail = |b: &Browser, message: String| {
            {
                let mut s = b.sync.borrow_mut();
                s.running = false;
                s.state = "error";
                s.message = message;
            }
            b.sync_emit();
        };
        let (info, bundles) = match read {
            Ok(r) => r,
            Err(e) => return fail(self, e),
        };
        let Some(key) = self.sync.borrow().key.clone() else {
            return fail(self, "not set up".into());
        };
        match FolderInfo::parse(&info).and_then(|i| i.verify(&key)) {
            Ok(()) => {}
            Err(sync::SyncError::WrongPassphrase) => {
                // Another device started the folder over with a new passphrase.
                {
                    let mut s = self.sync.borrow_mut();
                    s.key = None;
                    s.running = false;
                    s.state = "needs-passphrase";
                    s.message = "The sync folder now uses a different passphrase. Enter it to keep syncing.".into();
                }
                return self.sync_emit();
            }
            Err(e) => return fail(self, e.to_string()),
        }
        let settings = self.settings.borrow().sync.clone();
        let bookmark_scope = match (settings.bookmarks, settings.speed_dial) {
            (true, _) => Some(None),
            (false, true) => Some(Some(ROOT_SPEED_DIAL)),
            (false, false) => None,
        };
        let mut devices = Vec::new();
        let mut errors = 0;
        let mut bookmarks_changed = false;
        let mut vault_changed = false;
        let mut newest_settings: Option<SyncedSettings> = None;
        let mut imported_visits = 0;
        for bytes in bundles {
            let bundle = match Bundle::open(&bytes, &key) {
                Ok(b) => b,
                Err(e) => {
                    warn!("sync: skipping a bundle: {e}");
                    errors += 1;
                    continue;
                }
            };
            devices.push(SyncDevice {
                id: bundle.device_id.clone(),
                name: bundle.device_name.clone(),
                written_at: bundle.written_at,
                tabs: bundle.tabs.clone().unwrap_or_default(),
            });
            if let (Some(scope), Some(remote)) = (bookmark_scope, bundle.bookmarks.as_ref()) {
                let scope = scope.or(bundle
                    .bookmark_scope
                    .as_deref()
                    .filter(|s| *s == ROOT_SPEED_DIAL));
                bookmarks_changed |= self.bookmarks.borrow_mut().merge(remote, scope);
            }
            if settings.history
                && !self.private
                && let Some(visits) = bundle.history.as_ref()
            {
                match self.history.import_visits(visits) {
                    Ok(n) => imported_visits += n,
                    Err(e) => warn!("sync: history: {e}"),
                }
            }
            if settings.settings
                && let Some(remote) = bundle.settings.clone()
                && remote.modified > newest_settings.as_ref().map_or(0, |s| s.modified)
            {
                newest_settings = Some(remote);
            }
            if settings.passwords
                && let Some(remote) = bundle.vault.as_ref()
            {
                let mut v = self.vault.borrow_mut();
                if v.state == "ready"
                    && let Some(vault) = v.vault.as_mut()
                {
                    vault_changed |= vault.data.merge(remote);
                }
            }
        }
        if bookmarks_changed {
            self.bookmarks_changed();
        }
        if vault_changed {
            self.vault_save_quiet();
        }
        if let Some(remote) = newest_settings
            && remote.modified > self.sync.borrow().file.settings_modified
        {
            let applied = self.settings.borrow_mut().apply_patch(&remote.value);
            match applied {
                Ok(()) => {
                    {
                        let mut s = self.sync.borrow_mut();
                        s.file.settings_modified = remote.modified;
                    }
                    self.settings_changed();
                    // `settings_changed` noticed the new values; keep the remote time.
                    let json = self.synced_settings_json();
                    let mut s = self.sync.borrow_mut();
                    s.file.settings_json = json;
                    s.file.settings_modified = remote.modified;
                }
                Err(e) => warn!("sync: settings: {e}"),
            }
        }
        if imported_visits > 0 {
            info!("sync: {imported_visits} visits from other devices");
        }

        // Our bundle.
        let bundle = self.sync_bundle(&settings);
        let sealed = bundle.seal(&key);
        devices.sort_by_key(|d| std::cmp::Reverse(d.written_at));
        {
            let mut s = self.sync.borrow_mut();
            s.devices = devices;
            if errors > 0 {
                s.message =
                    format!("{errors} device file(s) in the sync folder could not be read.");
            } else {
                s.message.clear();
            }
        }
        let sealed = match sealed {
            Ok(bytes) => bytes,
            Err(e) => return fail(self, e.to_string()),
        };
        let sender = self.background_sender();
        let path = dir.join(own);
        std::thread::spawn(move || {
            let result = write_atomic(&path, &sealed).map_err(|e| e.to_string());
            sender.send(Box::new(move |b: &Browser| {
                {
                    let mut s = b.sync.borrow_mut();
                    s.running = false;
                    match result {
                        Ok(()) => {
                            s.state = "idle";
                            s.file.last_sync = Some(haku_core::now_ms());
                        }
                        Err(e) => {
                            s.state = "error";
                            s.message = format!("Could not write to the sync folder: {e}");
                        }
                    }
                }
                b.sync_save_state();
                b.sync_emit();
            }));
        });
    }

    fn sync_bundle(&self, settings: &haku_core::settings::Sync) -> Bundle {
        let s = self.sync.borrow();
        let bookmarks =
            (settings.bookmarks || settings.speed_dial).then(|| self.bookmarks.borrow().clone());
        let history = (settings.history && !self.private).then(|| {
            let since = haku_core::now_ms() - HISTORY_DAYS * 86_400_000;
            self.history
                .visits_since(since, HISTORY_MAX)
                .unwrap_or_default()
        });
        let synced_settings = settings.settings.then(|| SyncedSettings {
            modified: s.file.settings_modified,
            value: serde_json::from_str(&s.file.settings_json).unwrap_or_default(),
        });
        let tabs = settings.open_tabs.then(|| {
            let session = self.session.borrow();
            session
                .tabs
                .iter()
                .filter(|t| t.url.starts_with("http://") || t.url.starts_with("https://"))
                .map(|t| SyncedTab {
                    title: t.title.clone(),
                    url: t.url.clone(),
                    workspace: session
                        .workspaces
                        .iter()
                        .find(|w| w.id == t.workspace_id)
                        .map(|w| w.name.clone())
                        .unwrap_or_default(),
                })
                .take(500)
                .collect()
        });
        let vault = settings
            .passwords
            .then(|| {
                let v = self.vault.borrow();
                v.vault
                    .as_ref()
                    .filter(|_| v.state == "ready")
                    .map(|x| x.data.clone())
            })
            .flatten();
        Bundle {
            format: sync::FORMAT,
            device_id: s.file.device_id.clone(),
            device_name: device_name(),
            written_at: haku_core::now_ms(),
            bookmark_scope: (!settings.bookmarks && settings.speed_dial)
                .then(|| ROOT_SPEED_DIAL.to_owned()),
            bookmarks,
            history,
            settings: synced_settings,
            tabs,
            vault,
        }
    }
}

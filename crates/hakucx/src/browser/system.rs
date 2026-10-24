/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Windows integration (default browser, start at sign-in), the updater, crash-report notices,
//! and addresses handed over by another launch of HakuCX.

use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use haku_core::ipc::{Event, SystemInfo, UpdateStatus};
use haku_core::update::{self, Release};
use log::{info, warn};

use super::Browser;
use crate::{crash, os_integration as os};

/// The updater's public key (`packaging/update-key.pub`), if this build has one.
const UPDATE_KEY: Option<&str> = option_env!("HAKU_UPDATE_KEY");
const FIRST_CHECK: Duration = Duration::from_secs(60);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_INSTALLER: u64 = 512 * 1024 * 1024;

#[derive(Default)]
pub struct UpdateState {
    state: &'static str,
    version: Option<String>,
    page: Option<String>,
    progress: f32,
    message: String,
    last_check: Option<i64>,
    next_check: Option<Instant>,
    running: bool,
    /// The verified installer, ready to run.
    installer: Option<PathBuf>,
}

impl Browser {
    // ------------------------------------------------------------------ Windows

    pub(crate) fn system_emit(&self) {
        let exe = os::current_exe();
        let info = SystemInfo {
            is_default: os::is_default(),
            registered: exe.as_deref().is_some_and(os::is_registered),
            installed: os::is_installed_copy(),
            portable: os::portable_data_dir().is_some(),
            windows: cfg!(windows),
            crash_reports: std::fs::read_dir(crash::dir(&self.data_root))
                .map(|d| {
                    d.flatten()
                        .filter(|e| e.file_name().to_string_lossy().starts_with("crash-"))
                        .count() as u32
                })
                .unwrap_or(0),
        };
        self.emit(Event::System { info });
    }

    pub(crate) fn make_default_browser(&self) {
        let Some(exe) = os::current_exe() else { return };
        match os::register_browser(&exe) {
            Ok(()) => {
                info!("registered with Windows as a browser");
                os::open_default_apps();
                self.toast(
                    "In Windows Settings, choose HakuCX as your web browser.",
                    "info",
                );
                // Windows records the choice when the user makes it in Settings: look again.
                for secs in [5, 15, 40, 90] {
                    self.background
                        .post_after(Duration::from_secs(secs), |b: &Browser| b.system_emit());
                }
            }
            Err(e) => self.toast(format!("Windows could not register HakuCX: {e}"), "error"),
        }
        self.system_emit();
    }

    pub(crate) fn unregister_browser(&self) {
        if let Err(e) = os::unregister_browser() {
            self.toast(format!("Could not remove the registration: {e}"), "error");
        }
        self.system_emit();
    }

    /// Make Windows' "run at sign-in" entry match the setting (also after the program moved).
    pub(crate) fn apply_run_at_login(&self) {
        if self.private || self.secondary || self.paths.is_none() {
            return;
        }
        let Some(exe) = os::current_exe() else { return };
        let want = self.settings.borrow().startup.run_at_login;
        let current = os::run_at_login_command();
        let ours = format!("\"{}\"", exe.display());
        let result = match (want, current) {
            (true, Some(c)) if c == ours => Ok(()),
            (true, _) => os::set_run_at_login(&exe, true),
            (false, Some(c)) if c == ours => os::set_run_at_login(&exe, false),
            // Another copy of HakuCX owns the entry: leave it alone.
            (false, _) => Ok(()),
        };
        if let Err(e) = result {
            warn!("run at sign-in: {e}");
            self.toast(
                format!("Windows did not accept the start-up setting: {e}"),
                "error",
            );
        }
    }

    pub(crate) fn open_crash_reports(&self) {
        let dir = crash::dir(&self.data_root);
        let _ = std::fs::create_dir_all(&dir);
        let _ = open::that_detached(dir);
    }

    /// Tell the user about crash reports written since the last start (once).
    pub(crate) fn crash_notice(&self) {
        let new = crash::take_new_reports(&self.data_root);
        if new > 0 {
            self.toast(
                "HakuCX closed unexpectedly last time. A crash report was saved on this computer only (Settings → System → Crash reports).",
                "error",
            );
        }
    }

    /// Another launch of HakuCX handed over its addresses: open them here.
    pub(crate) fn open_from_other_launch(&self, urls: Vec<String>) {
        self.window.set_minimized(false);
        self.window.focus_window();
        if urls.is_empty() {
            self.open_url_in_new_tab(haku_core::session::START_URL);
        }
        for url in urls {
            // Files dropped on the program or opened from Explorer arrive as paths.
            let path = std::path::Path::new(&url);
            let target = if path.is_absolute() && path.exists() {
                url::Url::from_file_path(path)
                    .map(|u| u.to_string())
                    .unwrap_or(url)
            } else {
                url
            };
            self.open_url_in_new_tab(&target);
        }
    }

    // ------------------------------------------------------------------ updates

    pub(crate) fn update_emit(&self) {
        let u = self.update.borrow();
        let status = UpdateStatus {
            state: if UPDATE_KEY.is_none() {
                "unavailable"
            } else if u.state.is_empty() {
                "idle"
            } else {
                u.state
            }
            .to_owned(),
            current: env!("CARGO_PKG_VERSION").to_owned(),
            version: u.version.clone(),
            page: u.page.clone(),
            progress: u.progress,
            message: u.message.clone(),
            last_check: u.last_check,
        };
        drop(u);
        self.emit(Event::Update { status });
    }

    /// From the background poll: check once a minute after start, then daily.
    pub(crate) fn update_tick(&self) {
        if UPDATE_KEY.is_none()
            || self.private
            || self.secondary
            || !self.settings.borrow().updates.check_automatically
        {
            return;
        }
        let due = {
            let mut u = self.update.borrow_mut();
            let next = *u
                .next_check
                .get_or_insert_with(|| Instant::now() + FIRST_CHECK);
            Instant::now() >= next && !u.running && u.installer.is_none()
        };
        if due {
            self.update.borrow_mut().next_check = Some(Instant::now() + CHECK_EVERY);
            self.check_for_updates();
        }
    }

    fn update_set(&self, state: &'static str, message: impl Into<String>) {
        {
            let mut u = self.update.borrow_mut();
            u.state = state;
            u.message = message.into();
        }
        self.update_emit();
    }

    pub(crate) fn check_for_updates(&self) {
        let Some(key) = UPDATE_KEY else {
            return self.update_emit();
        };
        if self.update.borrow().running {
            return;
        }
        self.update.borrow_mut().running = true;
        self.update_set("checking", "");
        let installed = os::is_installed_copy();
        let sender = self.background_sender();
        std::thread::spawn(move || {
            let result = fetch_latest();
            sender.send(Box::new(move |b: &Browser| {
                b.update.borrow_mut().last_check = Some(haku_core::now_ms());
                match result {
                    Err(e) => {
                        b.update.borrow_mut().running = false;
                        b.update_set("error", e);
                    }
                    Ok(None) => {
                        b.update.borrow_mut().running = false;
                        b.update_set("up-to-date", "No release has been published yet.");
                    }
                    Ok(Some(release)) if !update::is_newer(&release.version, env!("CARGO_PKG_VERSION")) => {
                        b.update.borrow_mut().running = false;
                        b.update_set("up-to-date", "");
                    }
                    Ok(Some(release)) => {
                        {
                            let mut u = b.update.borrow_mut();
                            u.version = Some(release.version.clone());
                            u.page = Some(release.page.clone());
                        }
                        if installed {
                            b.download_update(release, key);
                        } else {
                            b.update.borrow_mut().running = false;
                            b.update_set("available", "This copy was not installed with the HakuCX installer: download the new version from the release page.");
                        }
                    }
                }
            }));
        });
    }

    fn download_update(&self, release: Release, key: &'static str) {
        self.update_set("downloading", "");
        let sender = self.background_sender();
        std::thread::spawn(move || {
            let progress = sender.clone();
            let result = download_and_verify(&release, key, move |p| {
                progress.send(Box::new(move |b: &Browser| {
                    b.update.borrow_mut().progress = p;
                    b.update_emit();
                }));
            });
            sender.send(Box::new(move |b: &Browser| {
                b.update.borrow_mut().running = false;
                match result {
                    Ok(path) => {
                        info!("update {} downloaded and verified", release.version);
                        b.update.borrow_mut().installer = Some(path);
                        b.update_set("ready", "");
                    }
                    Err(e) => {
                        warn!("update: {e}");
                        b.update_set("error", e);
                    }
                }
            }));
        });
    }

    /// Run the verified installer silently; it waits for HakuCX to close and starts it again.
    pub(crate) fn install_update(&self) {
        let Some(path) = self.update.borrow().installer.clone() else {
            return;
        };
        self.save_if_due(true);
        match std::process::Command::new(&path)
            .args(["/S", "/RELAUNCH"])
            .spawn()
        {
            Ok(_) => self.exit_requested.set(true),
            Err(e) => self.update_set("error", format!("The installer could not start: {e}")),
        }
    }
}

fn client() -> reqwest::Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("HakuCX/", env!("CARGO_PKG_VERSION"), " (updater)"))
        .timeout(Duration::from_secs(60))
        .build()
}

/// The latest published release, or `None` when there is none yet (GitHub answers 404).
fn fetch_latest() -> Result<Option<Release>, String> {
    let response = client()
        .and_then(|c| {
            c.get(update::LATEST_URL)
                .header("Accept", "application/vnd.github+json")
                .send()
        })
        .map_err(|e| format!("Could not reach GitHub: {e}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let text = response
        .error_for_status()
        .and_then(|r| r.text())
        .map_err(|e| format!("Could not reach GitHub: {e}"))?;
    update::parse_latest(&text)
        .map(Some)
        .map_err(|e| e.to_string())
}

fn download_and_verify(
    release: &Release,
    key: &str,
    progress: impl Fn(f32),
) -> Result<PathBuf, String> {
    if release.size > MAX_INSTALLER {
        return Err("The update is unexpectedly large.".into());
    }
    let client = client().map_err(|e| e.to_string())?;
    let signature = client
        .get(&release.signature_url)
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.text())
        .map_err(|e| format!("Could not download the update's signature: {e}"))?;
    let mut response = client
        .get(&release.installer_url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Could not download the update: {e}"))?;
    let total = response.content_length().unwrap_or(release.size).max(1);
    let mut data = Vec::with_capacity(total.min(MAX_INSTALLER) as usize);
    let mut buf = vec![0u8; 256 * 1024];
    let mut last = Instant::now();
    loop {
        let n = response
            .read(&mut buf)
            .map_err(|e| format!("The download stopped: {e}"))?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        if data.len() as u64 > MAX_INSTALLER {
            return Err("The update is unexpectedly large.".into());
        }
        if last.elapsed() > Duration::from_millis(300) {
            last = Instant::now();
            progress(data.len() as f32 / total as f32);
        }
    }
    let name = update::installer_name(&release.version);
    update::verify(&data, &signature, key, &release.version, &name)
        .map_err(|e| format!("The update was not installed: {e}."))?;
    let dir = std::env::temp_dir().join("HakuCX-update");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&name);
    std::fs::write(&path, &data).map_err(|e| e.to_string())?;
    // Check again what is on disk, in case something changed the file in between.
    let written = std::fs::read(&path).map_err(|e| e.to_string())?;
    update::verify(&written, &signature, key, &release.version, &name)
        .map_err(|e| format!("The update was not installed: {e}."))?;
    Ok(path)
}

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Background services and window-level actions: worker-thread results, filter lists,
//! GX Control metrics and limits, sounds, window commands, extra windows.

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use haku_core::ipc::{Event, Metrics, TabMetric, WindowAction};
use log::{info, warn};

use super::Browser;

/// Work a background thread wants done on the UI thread.
pub type BackgroundTask = Box<dyn FnOnce(&Browser) + Send>;

#[derive(Clone)]
pub struct BackgroundSender {
    tx: Sender<BackgroundTask>,
    wake: std::sync::Arc<dyn Fn() + Send + Sync>,
}

impl BackgroundSender {
    pub fn new(tx: Sender<BackgroundTask>, wake: std::sync::Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { tx, wake }
    }

    pub fn send(&self, task: BackgroundTask) {
        if self.tx.send(task).is_ok() {
            (self.wake)();
        }
    }

    /// Run `task` on the UI thread after `delay` (a timer thread; for rare, short waits).
    pub fn post_after(&self, delay: Duration, task: impl FnOnce(&Browser) + Send + 'static) {
        let sender = self.clone();
        let spawned = std::thread::Builder::new()
            .name("timer".into())
            .spawn(move || {
                std::thread::sleep(delay);
                sender.send(Box::new(task));
            });
        if let Err(e) = spawned {
            warn!("timer thread: {e}");
        }
    }
}

pub struct MetricsState {
    system: sysinfo::System,
    pid: sysinfo::Pid,
    last: Instant,
    last_bytes: u64,
}

impl MetricsState {
    pub fn new() -> Self {
        Self {
            system: sysinfo::System::new(),
            pid: sysinfo::get_current_pid().unwrap_or(sysinfo::Pid::from(0)),
            last: Instant::now(),
            last_bytes: 0,
        }
    }
}

impl Browser {
    pub(crate) fn background_sender(&self) -> BackgroundSender {
        self.background.clone()
    }

    /// Run results that worker threads posted.
    pub(crate) fn poll_background(&self) {
        let tasks: Vec<BackgroundTask> = self.background_rx.try_iter().collect();
        for task in tasks {
            task(self);
        }
        let updated = self.blocker.poll();
        if !updated.is_empty() {
            {
                let mut settings = self.settings.borrow_mut();
                for (id, at) in &updated {
                    if let Some(list) = settings
                        .privacy
                        .filter_lists
                        .iter_mut()
                        .find(|l| &l.id == id)
                    {
                        list.last_updated = Some(*at);
                    }
                }
            }
            self.settings_changed();
        }
        self.poll_downloads();
        self.ext_tick_if_due();
        self.sync_tick();
        if self.metrics_due() {
            self.collect_metrics();
        }
    }

    /// Start once the UI is ready: filter lists, limits, music.
    pub(crate) fn start_background_services(&self) {
        if self.services_started.replace(true) {
            return;
        }
        self.reload_filters();
        self.apply_limits();
        self.apply_music();
        // A 1 s heartbeat drives GX Control metrics and download speed readouts.
        let wake = self.wake.clone();
        std::thread::Builder::new()
            .name("heartbeat".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                    wake();
                }
            })
            .ok();
    }

    pub(crate) fn reload_filters(&self) {
        let (lists, custom) = {
            let s = self.settings.borrow();
            let p = &s.privacy;
            // Only lists whose feature is on are loaded, so a switched-off feature never
            // downloads its lists (see docs/NETWORK.md).
            let lists: Vec<_> = p
                .filter_lists
                .iter()
                .filter(|l| match l.category.as_str() {
                    "trackers" => p.block_trackers,
                    "malware" => p.malicious_site_protection,
                    _ => p.block_ads,
                })
                .cloned()
                .collect();
            (
                lists,
                if p.block_ads {
                    p.custom_filters.clone()
                } else {
                    String::new()
                },
            )
        };
        let dir = match &self.paths {
            Some(p) => p.filters(),
            // Private windows still block; they use a temporary filter cache.
            None => std::env::temp_dir().join("hakucx-filters"),
        };
        let _ = std::fs::create_dir_all(&dir);
        self.blocker
            .load(dir, lists, custom, servo::prefs::get().user_agent.clone());
    }

    /// GX Control limits. The network limiter caps HakuCX's own downloads; CPU and RAM limits
    /// are enforced by the Windows job object in `crate::limits`.
    pub(crate) fn apply_limits(&self) {
        let control = self.settings.borrow().control.clone();
        let net = if control.network_limiter {
            control.network_limit_bps
        } else {
            0
        };
        self.download_engine
            .limit_bps
            .store(net, std::sync::atomic::Ordering::Relaxed);
        crate::limits::apply(&control);
    }

    pub(crate) fn apply_music(&self) {
        let (on, track, volume) = {
            let s = self.settings.borrow();
            (
                s.sounds.background_music,
                s.sounds.music_track.clone(),
                s.sounds.music_volume as f32 / 100.0,
            )
        };
        self.audio.borrow_mut().set_music(on, &track, volume);
    }

    /// Play a browser sound if sounds (and that sound) are on.
    pub(crate) fn play_sound(&self, name: &str) {
        let s = self.settings.borrow();
        if !s.sounds.browser_sounds || !s.sounds.enabled.get(name).copied().unwrap_or(true) {
            return;
        }
        let level = s.sounds.levels.get(name).copied().unwrap_or(100) as f32 / 100.0;
        let master = s.sounds.browser_volume as f32 / 100.0;
        self.audio
            .borrow()
            .play(name, &s.sounds.browser_pack, level * master);
    }

    fn metrics_due(&self) -> bool {
        let state = self.metrics.borrow();
        state.last.elapsed() >= Duration::from_millis(950)
    }

    fn collect_metrics(&self) {
        let mut state = self.metrics.borrow_mut();
        let pid = state.pid;
        state.system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            sysinfo::ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory(),
        );
        state.system.refresh_memory();
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1) as f32;
        let (cpu, memory_mb) = state
            .system
            .process(pid)
            .map(|p| (p.cpu_usage() / cores, p.memory() as f32 / 1_048_576.0))
            .unwrap_or((0.0, 0.0));
        let system_memory_mb = state.system.total_memory() as f32 / 1_048_576.0;
        let elapsed = state.last.elapsed().as_secs_f32().max(0.001);
        let bytes = self
            .download_engine
            .bytes_total
            .load(std::sync::atomic::Ordering::Relaxed);
        let download_bps = bytes.saturating_sub(state.last_bytes) as f32 / elapsed;
        state.last_bytes = bytes;
        state.last = Instant::now();
        // Content processes (multi-process mode) are this process's children.
        let children: Vec<u32> = state
            .system
            .processes()
            .values()
            .filter(|p| p.parent() == Some(pid))
            .map(|p| p.pid().as_u32())
            .collect();
        drop(state);
        let control = self.settings.borrow().control.clone();
        if !children.is_empty() {
            crate::limits::adopt_content_processes(&children, &control);
        }
        if control.ram_limiter && memory_mb > control.ram_limit_mb as f32 {
            self.snooze_for_memory();
        }
        // Servo runs pages in this process unless multi-process mode is on, so per-tab numbers
        // are not separable; the list is left empty rather than invented.
        let per_tab: Vec<TabMetric> = Vec::new();
        self.emit(Event::Metrics {
            metrics: Metrics {
                cpu,
                memory_mb,
                system_memory_mb,
                download_bps,
                upload_bps: 0.0,
                per_tab,
                limits_enforced: crate::limits::enforced(),
            },
        });
    }

    /// Soft RAM limit: snooze the least recently used background tab that has a live page.
    fn snooze_for_memory(&self) {
        let candidate = {
            let session = self.session.borrow();
            let views = self.views.borrow();
            let active = session.active_tab_id().cloned();
            session
                .tabs
                .iter()
                .filter(|t| Some(&t.id) != active.as_ref() && !t.pinned && !t.snoozed)
                .filter(|t| {
                    views
                        .get(&t.id)
                        .is_some_and(|v| v.webview.is_some() && !v.audible)
                })
                .min_by_key(|t| t.last_active)
                .map(|t| t.id.clone())
        };
        if let Some(tab) = candidate
            && self.session.borrow_mut().snooze(&tab)
        {
            log::info!("RAM limit reached: snoozed a background tab");
            self.play_sound("limiters");
            self.session_changed();
        }
    }

    pub(crate) fn window_action(&self, action: WindowAction) {
        match action {
            WindowAction::Minimize => self.window.set_minimized(true),
            WindowAction::ToggleMaximize => self.window.set_maximized(!self.window.is_maximized()),
            WindowAction::Close | WindowAction::Quit => self.exit_requested.set(true),
            WindowAction::Drag => {
                if let Err(e) = self.window.drag_window() {
                    warn!("drag_window: {e}");
                }
            }
            WindowAction::ToggleFullscreen => {
                let on = self.window.fullscreen().is_none();
                self.set_page_fullscreen(on);
            }
            WindowAction::NewWindow => self.spawn_window(None, false),
            WindowAction::NewPrivateWindow => self.spawn_window(None, true),
        }
        self.emit(Event::WindowState {
            maximized: self.window.is_maximized(),
            fullscreen: self.window.fullscreen().is_some(),
            focused: self.window.has_focus(),
        });
    }

    /// Open another browser window. Each window is its own process; a private window gets
    /// temporary engine storage and never writes to the profile.
    pub(crate) fn spawn_window(&self, url: Option<String>, private: bool) {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let mut command = std::process::Command::new(exe);
        if private {
            command.arg("--private");
        } else {
            command
                .arg("--profile")
                .arg(&self.profile.id)
                .arg("--secondary-window");
        }
        if let Some(url) = url {
            command.arg("--").arg(url);
        }
        match command.spawn() {
            Ok(_) => info!(
                "opened a new {} window",
                if private { "private" } else { "browser" }
            ),
            Err(e) => self.toast(format!("Could not open a window: {e}"), "error"),
        }
    }
}

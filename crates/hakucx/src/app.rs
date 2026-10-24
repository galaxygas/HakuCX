/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The winit application: creates the window and the engine, routes window events to the
//! browser, and shuts everything down cleanly.

use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use haku_core::ipc::Event;
use haku_core::settings::Settings;
use haku_core::storage::{ProfileInfo, ProfileKind, ProfileList, ProfilePaths};
use log::{error, info};
use servo::protocol_handler::ProtocolRegistry;
use servo::{
    EventLoopWaker, Opts, Preferences, RenderingContext, ServoBuilder, WindowRenderingContext,
};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::{Icon, Window, WindowId};

use crate::browser::{Browser, LaunchOptions};
use crate::protocol::HakuProtocolHandler;

#[derive(Debug)]
pub enum AppEvent {
    Wake,
}

#[derive(Clone)]
pub struct Waker(pub EventLoopProxy<AppEvent>);

impl EventLoopWaker for Waker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }
    fn wake(&self) {
        let _ = self.0.send_event(AppEvent::Wake);
    }
}

/// Everything decided before the engine starts.
pub struct Startup {
    pub options: LaunchOptions,
    pub data_root: PathBuf,
    pub profiles: ProfileList,
    pub profile: ProfileInfo,
    pub paths: Option<ProfilePaths>,
    pub settings: Settings,
    /// Servo stores nothing on disk (private windows, Roguelike profiles).
    pub ephemeral: bool,
    /// Engine storage directory for this process.
    pub engine_dir: Option<PathBuf>,
}

pub struct App {
    waker: Waker,
    startup: Option<Startup>,
    browser: Option<Rc<Browser>>,
    /// The profile folder this process is the running instance of (see `instance.rs`).
    instance_dir: Option<PathBuf>,
}

impl App {
    pub fn new(waker: Waker, startup: Startup, instance_dir: Option<PathBuf>) -> Self {
        Self {
            waker,
            startup: Some(startup),
            browser: None,
            instance_dir,
        }
    }

    fn after_event(&self, event_loop: &ActiveEventLoop) {
        if let Some(browser) = &self.browser {
            browser.poll_background();
            browser.pump();
            // Hand the input and commands of this event to Servo now (like servoshell does),
            // instead of leaving them queued until the next wake-up; then handle whatever
            // that produced.
            browser.servo.spin_event_loop();
            browser.pump();
            if browser.exit_requested() {
                event_loop.exit();
            }
            // While something animates, wake at frame rate so Servo can advance it; otherwise
            // sleep until the next event or wake-up.
            event_loop.set_control_flow(if browser.is_animating() || browser.needs_timer() {
                ControlFlow::WaitUntil(std::time::Instant::now() + FRAME_INTERVAL)
            } else {
                ControlFlow::Wait
            });
        }
    }
}

/// Frame interval while a webview animates (about 120 Hz, Servo's own refresh rate).
const FRAME_INTERVAL: std::time::Duration = std::time::Duration::from_millis(8);
/// HakuCX's window icon (see `haku_brand`).
pub fn app_icon(size: u32) -> Option<Icon> {
    Icon::from_rgba(haku_brand::icon_rgba(size), size, size).ok()
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let Some(startup) = self.startup.take() else {
            return;
        };
        let native_frame = startup.settings.appearance.native_title_bar;
        let title = if startup.options.private {
            "HakuCX (Private)"
        } else {
            "HakuCX"
        };
        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title(title)
                .with_inner_size(LogicalSize::new(1360.0, 860.0))
                .with_min_inner_size(LogicalSize::new(480.0, 360.0))
                .with_decorations(native_frame)
                .with_window_icon(app_icon(64))
                .with_visible(false),
        ) {
            Ok(w) => w,
            Err(e) => {
                error!("could not create the window: {e}");
                event_loop.exit();
                return;
            }
        };
        let display_handle = event_loop.display_handle().expect("display handle");
        let window_handle = window.window_handle().expect("window handle");
        let window_context =
            match WindowRenderingContext::new(display_handle, window_handle, window.inner_size()) {
                Ok(c) => Rc::new(c),
                Err(e) => {
                    error!("could not create a GL context ({e:?}); is the GPU driver installed?");
                    event_loop.exit();
                    return;
                }
            };
        window_context
            .make_current()
            .expect("make GL context current");

        let mut registry = ProtocolRegistry::default();
        let wallpapers = startup.paths.as_ref().map(|p| p.wallpapers());
        registry
            .register("haku", HakuProtocolHandler::new(wallpapers))
            .expect("register haku://");

        let opts = Opts {
            config_dir: startup.engine_dir.clone(),
            temporary_storage: startup.ephemeral,
            multiprocess: startup.settings.system.multiprocess,
            // Developer aid: HAKU_PROFILE=1 prints Servo's time profile (per phase) every 5 s.
            time_profiling: std::env::var_os("HAKU_PROFILE")
                .map(|_| servo::OutputOptions::Stdout(5.0)),
            ..Opts::default()
        };
        // The HTTP disk cache is a SQLite file the engine opens once at startup; an empty path
        // keeps the cache in memory only.
        let disk_cache = match (&startup.engine_dir, startup.settings.system.disk_cache) {
            (Some(dir), true) => dir
                .join("http-cache.sqlite3")
                .to_string_lossy()
                .into_owned(),
            _ => String::new(),
        };
        let preferences = Preferences {
            // Transparent clear so the UI shell can be layered over pages.
            shell_background_color_rgba: [0.0, 0.0, 0.0, 0.0],
            network_http_disk_cache: disk_cache,
            ..Preferences::default()
        };
        let servo = ServoBuilder::default()
            .opts(opts)
            .preferences(preferences)
            .protocol_registry(registry)
            .event_loop_waker(Box::new(self.waker.clone()))
            .build();
        servo.setup_logging();

        let proxy = self.waker.0.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = proxy.send_event(AppEvent::Wake);
        });
        let browser = Browser::new(window, servo, window_context, startup, wake);
        browser.window.set_visible(true);
        if let Some(dir) = self.instance_dir.as_ref() {
            let sender = browser.background_sender();
            crate::instance::listen(
                dir,
                Box::new(move |urls| {
                    sender.send(Box::new(move |b: &Browser| b.open_from_other_launch(urls)));
                }),
            );
        }
        info!("HakuCX {} started", env!("CARGO_PKG_VERSION"));
        self.browser = Some(browser);
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if let StartCause::ResumeTimeReached { .. } = cause
            && let Some(browser) = &self.browser
        {
            browser.servo.spin_event_loop();
            self.after_event(event_loop);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, _event: AppEvent) {
        if let Some(browser) = &self.browser {
            browser.servo.spin_event_loop();
        }
        self.after_event(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(browser) = self.browser.clone() else {
            return;
        };
        browser.servo.spin_event_loop();
        match event {
            WindowEvent::CloseRequested => {
                if browser.should_confirm_quit() {
                    browser.emit(Event::Open {
                        target: "quit".into(),
                    });
                } else {
                    event_loop.exit();
                }
            }
            WindowEvent::RedrawRequested => browser.paint(),
            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    browser.window_resized(size);
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                browser.scale_changed(scale_factor)
            }
            other => browser.handle_window_event(other),
        }
        self.after_event(event_loop);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(browser) = self.browser.take() {
            browser.shutdown();
            // Dropping the last reference to Servo makes it flush cookies and storage.
            drop(browser);
        }
    }
}

/// Decide the profile and engine storage for this process (before Servo starts).
pub fn prepare(options: LaunchOptions) -> Startup {
    let data_root = haku_core::storage::data_root().unwrap_or_else(|e| {
        error!("{e}; using a temporary directory");
        std::env::temp_dir().join("HakuCX")
    });
    let _ = std::fs::create_dir_all(&data_root);
    let mut profiles = ProfileList::load(&data_root);
    let profile_id = options
        .profile
        .clone()
        .or_else(|| profiles.last_used.clone())
        .filter(|id| profiles.get(id).is_some())
        .unwrap_or_else(|| profiles.profiles[0].id.clone());
    let profile = profiles.get(&profile_id).cloned().expect("profile exists");
    if !options.private && !options.secondary {
        profiles.last_used = Some(profile.id.clone());
        if let Err(e) = profiles.save(&data_root) {
            error!("saving profiles.json: {e}");
        }
    }
    let paths = ProfilePaths::new(&data_root, &profile.id)
        .map_err(|e| error!("profile directory unavailable: {e}"))
        .ok();
    let settings = paths
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p.settings()).ok())
        .and_then(|t| Settings::from_json(&t).ok())
        .unwrap_or_default();
    let ephemeral = options.private || profile.kind == ProfileKind::Roguelike;
    let engine_dir = if ephemeral {
        None
    } else if options.secondary {
        // Another window of the same profile: start from a copy of the engine storage so
        // logins carry over without two processes writing the same cookie jar.
        paths.as_ref().map(|p| {
            let copy = std::env::temp_dir().join(format!("hakucx-engine-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&copy);
            if let Ok(entries) = std::fs::read_dir(p.engine()) {
                for entry in entries.flatten() {
                    if entry.path().is_file() {
                        let _ = std::fs::copy(entry.path(), copy.join(entry.file_name()));
                    }
                }
            }
            copy
        })
    } else {
        paths.as_ref().map(|p| p.engine())
    };
    Startup {
        options,
        data_root,
        profiles,
        profile,
        paths,
        settings,
        ephemeral,
        engine_dir,
    }
}

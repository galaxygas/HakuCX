/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The browser: one window, the Servo engine, the UI shell, and every tab's webview.
//!
//! Rules that keep this sane (see AGENTS.md):
//! - Servo delegate methods and evaluation callbacks only record facts and queue work. All
//!   reactions (IPC handling, emitting UI events, creating webviews) happen in [`Browser::pump`],
//!   after `Servo::spin_event_loop` has returned.
//! - `haku-core` owns the state ([`Session`], [`Settings`], bookmarks, history). This module
//!   applies operations to it and then reconciles engine webviews against it.

mod commands;
mod content;
mod controls;
mod delegate;
mod ext_api;
mod extensions;
mod flow;
mod gx_lights;
mod input;
mod media;
mod page_scripts;
mod panels;
pub mod popout;
mod render;
mod services;
mod suggest;
mod sync;
mod system;
mod tabs;
mod tools;
mod vault;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use haku_core::bookmarks::Bookmarks;
use haku_core::history::History;
use haku_core::ipc::{EngineInfo, Event, Rect};
use haku_core::records::{DownloadList, Pinboards, SiteMods, UserThemes};
use haku_core::session::{Session, TabId};
use haku_core::settings::Settings;
use haku_core::storage::{self, ProfileInfo, ProfileKind, ProfileList, ProfilePaths};
use haku_core::themes;
use log::{error, info, warn};
use services::{BackgroundSender, BackgroundTask, MetricsState};
use servo::{RenderingContext, Servo, WebView, WebViewId, WindowRenderingContext};
use winit::dpi::PhysicalPosition;
use winit::keyboard::ModifiersState;
use winit::window::Window;

use crate::adblock::Blocker;
use crate::app::Startup;
use crate::audio::Audio;
use crate::compositor::{Compositor, PixelRect};
use crate::downloads::DownloadEngine;
use crate::texture_context::TextureRenderingContext;

pub use controls::PendingControl;
pub use tabs::{Pane, TabView};

/// Prefix the UI shell uses on `console.debug` lines that carry IPC commands.
pub const IPC_PREFIX: &str = "\u{1}haku-ipc:";
/// Prefix page scripts use for the small, allow-listed page -> core channel.
pub const PAGE_PREFIX: &str = "\u{1}haku-page:";

const SAVE_DEBOUNCE: Duration = Duration::from_secs(2);

/// Options the process was started with.
#[derive(Clone, Debug, Default)]
pub struct LaunchOptions {
    pub urls: Vec<String>,
    pub profile: Option<String>,
    pub private: bool,
    /// An extra window of an already running profile (see `app::prepare`).
    pub secondary: bool,
    /// Unpacked extensions to run for this session only (`--load-extension`).
    pub load_extensions: Vec<std::path::PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerTarget {
    Ui,
    Page(Pane),
    /// The window edge, for resizing a frameless window.
    Edge,
}

/// Layout reported by the UI shell, converted to physical window pixels.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub page: PixelRect,
    pub split: Option<PixelRect>,
    pub panel: Option<PixelRect>,
    /// An open extension popup (drawn and hit-tested above the UI).
    pub popup: Option<PixelRect>,
    pub overlay: bool,
    pub modal: bool,
    pub regions: Vec<PixelRect>,
}

/// Work queued to run after `Servo::spin_event_loop` returns (see the module docs).
pub(crate) type Deferred = Box<dyn FnOnce(&Browser)>;

pub struct Browser {
    this: Weak<Browser>,
    pub window: Window,
    pub servo: Servo,
    window_context: Rc<WindowRenderingContext>,
    compositor: Compositor,
    ui_context: Rc<TextureRenderingContext>,
    /// Page painters: the main pane, the right-hand split pane, sidebar web panels, and
    /// extension popups.
    pane_contexts: [Rc<TextureRenderingContext>; 4],
    /// Painter for hidden extension pages (background pages, offscreen documents).
    ext_context: Rc<TextureRenderingContext>,
    ui: RefCell<Option<WebView>>,

    // ---- profile and state
    /// Where the profile saves data; `None` for private windows and Roguelike profiles.
    paths: Option<ProfilePaths>,
    /// The profile's folders even when nothing is saved (private windows still run the
    /// profile's extensions that are allowed in private).
    ext_paths: Option<ProfilePaths>,
    private: bool,
    profile: ProfileInfo,
    profiles: RefCell<ProfileList>,
    data_root: std::path::PathBuf,
    settings: RefCell<Settings>,
    session: RefCell<Session>,
    bookmarks: RefCell<Bookmarks>,
    history: History,
    downloads: RefCell<DownloadList>,
    pinboards: RefCell<Pinboards>,
    site_mods: RefCell<SiteMods>,
    user_themes: RefCell<UserThemes>,
    system_dark: Cell<bool>,

    // ---- engine mapping
    views: RefCell<HashMap<TabId, TabView>>,
    by_webview: RefCell<HashMap<WebViewId, TabId>>,
    /// Which tab each pane currently shows.
    pane_tabs: RefCell<[Option<TabId>; 2]>,
    /// Sidebar web panels by sidebar item id, and which one is visible.
    web_panels: RefCell<HashMap<String, WebView>>,
    panel_by_webview: RefCell<HashMap<WebViewId, String>>,
    visible_panel: RefCell<Option<String>>,

    // ---- layout and input
    layout: RefCell<Layout>,
    cursor: Cell<PhysicalPosition<f64>>,
    pointer_target: Cell<PointerTarget>,
    /// `None` means the UI shell has keyboard focus.
    keyboard_pane: Cell<Option<Pane>>,
    modifiers: Cell<ModifiersState>,

    // ---- queues (see module docs)
    ui_ready: Cell<bool>,
    inbox: RefCell<Vec<String>>,
    outbox: RefCell<Vec<Event>>,
    deferred: RefCell<Vec<Deferred>>,
    controls: RefCell<HashMap<u32, PendingControl>>,
    next_control_id: Cell<u32>,

    // ---- persistence
    session_dirty: Cell<bool>,
    settings_dirty: Cell<bool>,
    bookmarks_dirty: Cell<bool>,
    downloads_dirty: Cell<bool>,
    last_save: Cell<Instant>,
    blocked_total: Cell<u64>,
    panic_active: Cell<bool>,
    panic_restore: RefCell<Option<Session>>,
    shutting_down: Cell<bool>,
    exit_requested: Cell<bool>,
    /// The UI is recording a key combination (shortcut editor); shortcuts are paused.
    pub(crate) key_capture: Cell<bool>,
    /// Webviews with running animations or transitions. While any is animating, the app must
    /// spin Servo at frame rate: Servo then waits for the embedder's paint loop to advance
    /// animations and to run "update the rendering", which also handles queued input.
    pub(crate) animating: RefCell<std::collections::HashSet<servo::WebViewId>>,
    secondary: bool,
    /// WebExtensions (see `extensions.rs`).
    ext: RefCell<extensions::ExtState>,
    /// The password vault and autofill (see `vault.rs`).
    vault: RefCell<vault::VaultState>,
    /// Folder sync (see `sync.rs`).
    sync: RefCell<sync::SyncState>,
    /// The updater (see `system.rs`).
    update: RefCell<system::UpdateState>,
    /// GX Lights (see `gx_lights.rs`).
    lights: RefCell<gx_lights::LightsState>,
    /// My Flow (see `flow.rs`).
    flow: RefCell<flow::FlowState>,
    /// Media across tabs and the Player (see `media.rs`).
    media: RefCell<media::MediaState>,
    /// The video pop-out window (see `popout.rs`), and one waiting for the app to create it.
    popout: RefCell<Option<popout::Popout>>,
    popout_request: RefCell<Option<popout::PopoutSpec>>,
    popout_counter: Cell<u64>,
    /// Where the last pop out was, for the next one.
    popout_geometry: Cell<
        Option<(
            winit::dpi::PhysicalPosition<i32>,
            winit::dpi::PhysicalSize<u32>,
        )>,
    >,

    // ---- services
    audio: RefCell<Audio>,
    blocker: Blocker,
    download_engine: DownloadEngine,
    background: BackgroundSender,
    background_rx: Receiver<BackgroundTask>,
    wake: Arc<dyn Fn() + Send + Sync>,
    metrics: RefCell<MetricsState>,
    services_started: Cell<bool>,
}

impl Browser {
    pub fn new(
        window: Window,
        servo: Servo,
        window_context: Rc<WindowRenderingContext>,
        startup: Startup,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Rc<Browser> {
        let compositor = unsafe { Compositor::new(&window_context.glow_gl_api()) }
            .expect("compile compositor shaders");
        let size = window.inner_size();
        let ui_context = Rc::new(TextureRenderingContext::new(window_context.clone(), size));
        let pane_contexts = [
            Rc::new(TextureRenderingContext::new(window_context.clone(), size)),
            Rc::new(TextureRenderingContext::new(window_context.clone(), size)),
            Rc::new(TextureRenderingContext::new(window_context.clone(), size)),
            Rc::new(TextureRenderingContext::new(window_context.clone(), size)),
        ];
        // Hidden extension pages never paint; a modest size keeps `window.innerWidth` sane.
        let ext_context = Rc::new(TextureRenderingContext::new(
            window_context.clone(),
            winit::dpi::PhysicalSize::new(800, 600),
        ));
        let Startup {
            options,
            data_root,
            profiles,
            profile,
            paths,
            mut settings,
            ephemeral,
            ..
        } = startup;
        if !settings.control.keep_after_restart {
            settings.control.cpu_limiter = false;
            settings.control.ram_limiter = false;
            settings.control.network_limiter = false;
        }
        let load = |f: fn(&ProfilePaths) -> std::path::PathBuf| paths.as_ref().map(f);

        let mut bookmarks: Bookmarks = load(ProfilePaths::bookmarks)
            .map(|p| storage::read_json_or_default(&p))
            .unwrap_or_default();
        bookmarks.ensure_roots();
        let mut downloads: DownloadList = load(ProfilePaths::downloads)
            .map(|p| storage::read_json_or_default(&p))
            .unwrap_or_default();
        downloads.settle_after_restart();
        let history = match (ephemeral, load(ProfilePaths::history)) {
            (false, Some(path)) => History::open(&path).unwrap_or_else(|e| {
                error!("history database unavailable ({e}); history stays in memory");
                History::in_memory().expect("in-memory sqlite")
            }),
            _ => History::in_memory().expect("in-memory sqlite"),
        };
        let session = if ephemeral || options.secondary || profile.kind == ProfileKind::Roguelike {
            Session::default()
        } else {
            tabs::restore_session(
                load(ProfilePaths::session)
                    .map(|p| storage::read_json_or_default::<Option<Session>>(&p)),
                &settings,
            )
        };
        let (background_tx, background_rx) = std::sync::mpsc::channel();

        let browser = Rc::new_cyclic(|this| Browser {
            this: this.clone(),
            window,
            servo,
            window_context,
            compositor,
            ui_context,
            pane_contexts,
            ext_context,
            ui: RefCell::new(None),
            pinboards: RefCell::new(
                load(ProfilePaths::pinboards)
                    .map(|p| storage::read_json_or_default(&p))
                    .unwrap_or_default(),
            ),
            site_mods: RefCell::new(
                load(ProfilePaths::site_mods)
                    .map(|p| storage::read_json_or_default(&p))
                    .unwrap_or_default(),
            ),
            user_themes: RefCell::new(
                load(ProfilePaths::user_themes)
                    .map(|p| storage::read_json_or_default(&p))
                    .unwrap_or_default(),
            ),
            // Private windows and Roguelike profiles write nothing.
            ext_paths: paths.clone(),
            paths: if ephemeral { None } else { paths.clone() },
            private: options.private,
            profile,
            profiles: RefCell::new(profiles),
            data_root,
            settings: RefCell::new(settings),
            session: RefCell::new(session),
            bookmarks: RefCell::new(bookmarks),
            history,
            downloads: RefCell::new(downloads),
            system_dark: Cell::new(true),
            views: RefCell::new(HashMap::new()),
            by_webview: RefCell::new(HashMap::new()),
            pane_tabs: RefCell::new([None, None]),
            web_panels: RefCell::new(HashMap::new()),
            panel_by_webview: RefCell::new(HashMap::new()),
            visible_panel: RefCell::new(None),
            layout: RefCell::new(Layout::default()),
            cursor: Cell::new(PhysicalPosition::new(0.0, 0.0)),
            pointer_target: Cell::new(PointerTarget::Ui),
            keyboard_pane: Cell::new(None),
            modifiers: Cell::new(ModifiersState::empty()),
            ui_ready: Cell::new(false),
            inbox: RefCell::new(Vec::new()),
            outbox: RefCell::new(Vec::new()),
            deferred: RefCell::new(Vec::new()),
            controls: RefCell::new(HashMap::new()),
            next_control_id: Cell::new(1),
            session_dirty: Cell::new(false),
            settings_dirty: Cell::new(false),
            bookmarks_dirty: Cell::new(false),
            downloads_dirty: Cell::new(false),
            last_save: Cell::new(Instant::now()),
            blocked_total: Cell::new(0),
            panic_active: Cell::new(false),
            panic_restore: RefCell::new(None),
            shutting_down: Cell::new(false),
            exit_requested: Cell::new(false),
            key_capture: Cell::new(false),
            animating: RefCell::default(),
            secondary: options.secondary,
            ext: RefCell::default(),
            vault: RefCell::default(),
            sync: RefCell::default(),
            update: RefCell::default(),
            lights: RefCell::default(),
            media: RefCell::default(),
            flow: RefCell::default(),
            popout: RefCell::new(None),
            popout_request: RefCell::new(None),
            popout_counter: Cell::new(0),
            popout_geometry: Cell::new(None),
            audio: RefCell::new(Audio::new()),
            blocker: Blocker::new(wake.clone()),
            download_engine: DownloadEngine::new(wake.clone()),
            background: BackgroundSender::new(background_tx, wake.clone()),
            background_rx,
            wake,
            metrics: RefCell::new(MetricsState::new()),
            services_started: Cell::new(false),
        });

        if let Some(theme) = browser.window.theme() {
            browser.system_dark.set(theme == winit::window::Theme::Dark);
        }
        browser.apply_engine_settings();
        browser.create_ui_shell();
        browser.vault_load();
        browser.sync_load();
        browser.apply_run_at_login();
        browser.lights_apply();
        browser.flow_apply();
        browser.load_extensions();
        for dir in &options.load_extensions {
            browser.ext_load_for_session(dir);
        }
        let urls = options.urls.clone();
        browser.ext_after_startup(move |b| {
            for url in &urls {
                b.open_url_in_new_tab(url);
            }
            b.reconcile();
        });
        browser
    }

    pub fn exit_requested(&self) -> bool {
        self.exit_requested.get()
    }

    /// Ask before quitting with several tabs open (Opera GX "Warn on quitting").
    pub fn should_confirm_quit(&self) -> bool {
        self.ui_ready.get()
            && self.settings.borrow().tabs.warn_quit
            && self.session.borrow().tabs.len() > 1
    }

    pub fn window_resized(&self, size: winit::dpi::PhysicalSize<u32>) {
        self.window_context.resize(size);
        if let Some(ui) = self.ui_webview() {
            // Resize through the webview (see AGENTS.md).
            ui.resize(size);
        }
        self.emit(Event::WindowState {
            maximized: self.window.is_maximized(),
            fullscreen: self.window.fullscreen().is_some(),
            focused: self.window.has_focus(),
        });
        self.window.request_redraw();
    }

    pub fn scale_changed(&self, scale: f64) {
        let ui_scale = euclid::Scale::new(self.ui_scale());
        let scale = euclid::Scale::new(scale as f32);
        if let Some(ui) = self.ui_webview() {
            ui.set_hidpi_scale_factor(ui_scale);
        }
        for view in self.views.borrow().values() {
            if let Some(w) = &view.webview {
                w.set_hidpi_scale_factor(scale);
            }
        }
    }

    pub(crate) fn rc(&self) -> Rc<Browser> {
        self.this.upgrade().expect("browser outlives its webviews")
    }

    pub(crate) fn scale(&self) -> f32 {
        self.window.scale_factor() as f32
    }

    /// Device pixels per UI CSS pixel: the window's scale times Appearance > UI scale.
    pub(crate) fn ui_scale(&self) -> f32 {
        self.scale() * self.settings.borrow().appearance.ui_scale as f32 / 100.0
    }

    fn create_ui_shell(&self) {
        let url = url::Url::parse("haku://ui/index.html").expect("static url");
        let webview = servo::WebViewBuilder::new(&self.servo, self.ui_context.clone())
            .url(url)
            .hidpi_scale_factor(euclid::Scale::new(self.ui_scale()))
            .delegate(self.rc())
            .build();
        webview.show();
        webview.set_focused(true);
        *self.ui.borrow_mut() = Some(webview);
    }

    pub(crate) fn ui_webview(&self) -> Option<WebView> {
        self.ui.borrow().clone()
    }

    pub fn is_animating(&self) -> bool {
        !self.animating.borrow().is_empty()
    }

    /// Something needs the event loop to wake soon even without input (a request held for an
    /// extension's `webRequest` listener must be released on time).
    pub fn needs_timer(&self) -> bool {
        self.ext_has_held_requests()
    }

    pub(crate) fn is_ui(&self, webview: &WebView) -> bool {
        self.ui
            .borrow()
            .as_ref()
            .is_some_and(|ui| ui.id() == webview.id())
    }

    // ------------------------------------------------------------------ queues

    /// Queue an event for the UI shell.
    pub(crate) fn emit(&self, event: Event) {
        self.outbox.borrow_mut().push(event);
    }

    /// Queue work that must run outside a Servo callback.
    pub(crate) fn defer(&self, f: impl FnOnce(&Browser) + 'static) {
        self.deferred.borrow_mut().push(Box::new(f));
    }

    /// Process queued work. Called after every `spin_event_loop`, never from a Servo callback.
    pub fn pump(&self) {
        for _ in 0..8 {
            let messages = std::mem::take(&mut *self.inbox.borrow_mut());
            let deferred = std::mem::take(&mut *self.deferred.borrow_mut());
            if messages.is_empty() && deferred.is_empty() {
                break;
            }
            for message in messages {
                self.handle_ipc(&message);
            }
            for work in deferred {
                work(self);
            }
        }
        self.ext_tab_events();
        self.ext_check_popup_focus();
        if self.ext_has_held_requests() {
            self.ext_expire();
        }
        self.ext_flush_info();
        self.flush_events();
        self.save_if_due(false);
    }

    fn flush_events(&self) {
        if !self.ui_ready.get() || self.outbox.borrow().is_empty() {
            return;
        }
        let events = std::mem::take(&mut *self.outbox.borrow_mut());
        let Some(ui) = self.ui_webview() else { return };
        let batch = match serde_json::to_string(&events) {
            Ok(json) => json,
            Err(e) => {
                error!("could not encode UI events: {e}");
                return;
            }
        };
        // The JSON is embedded as a JS expression; serde_json output is valid JS except for
        // U+2028/U+2029, which older engines reject inside string literals.
        let batch = batch
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029");
        let script = format!("window.__haku && window.__haku.receive({batch});");
        ui.evaluate_javascript(script, |result| {
            if let Err(error) = result {
                warn!("UI event delivery failed: {error:?}");
            }
        });
    }

    pub(crate) fn next_id(&self) -> u32 {
        let id = self.next_control_id.get();
        self.next_control_id.set(id.wrapping_add(1).max(1));
        id
    }

    pub(crate) fn toast(&self, text: impl Into<String>, kind: &str) {
        self.emit(Event::Toast {
            text: text.into(),
            kind: kind.into(),
        });
    }

    // ------------------------------------------------------------------ state broadcast

    pub(crate) fn engine_info(&self) -> EngineInfo {
        EngineInfo {
            app_version: env!("CARGO_PKG_VERSION").into(),
            servo_version: "0.7.0".into(),
            user_agent: servo::prefs::get().user_agent.clone(),
            os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            private: self.private,
            profile: self.profile.clone(),
            profiles: self.profiles.borrow().profiles.clone(),
            data_dir: self.data_root.display().to_string(),
        }
    }

    pub(crate) fn send_init(&self) {
        let settings = self.settings.borrow().clone();
        let user = self.user_themes.borrow().themes.clone();
        self.emit(Event::Init {
            engine: self.engine_info(),
            session: self.session.borrow().clone(),
            palette: themes::palette_for_settings(
                &settings.appearance,
                &user,
                self.system_dark.get(),
            ),
            themes: themes::all_themes(&user),
            settings: Box::new(settings),
            bookmarks: self.bookmarks.borrow().clone(),
            downloads: self.downloads.borrow().items.clone(),
            pinboards: self.pinboards.borrow().clone(),
            site_mods: self.site_mods.borrow().clone(),
            system_dark: self.system_dark.get(),
        });
        for meta in self.all_tab_meta() {
            self.emit(Event::TabMeta { meta });
        }
        self.ext.borrow_mut().info_dirty = true;
        self.vault_emit();
        self.sync_emit();
        self.system_emit();
        self.update_emit();
        self.lights_emit();
        self.flow_emit();
        self.crash_notice();
    }

    pub(crate) fn session_changed(&self) {
        self.session_dirty.set(true);
        self.emit(Event::Session {
            session: self.session.borrow().clone(),
        });
        self.reconcile();
    }

    pub(crate) fn settings_changed(&self) {
        self.settings_dirty.set(true);
        let settings = self.settings.borrow().clone();
        let user = self.user_themes.borrow().themes.clone();
        self.emit(Event::Settings {
            palette: themes::palette_for_settings(
                &settings.appearance,
                &user,
                self.system_dark.get(),
            ),
            themes: themes::all_themes(&user),
            settings: Box::new(settings),
        });
        self.apply_engine_settings();
        if let Some(ui) = self.ui_webview() {
            ui.set_hidpi_scale_factor(euclid::Scale::new(self.ui_scale()));
        }
        self.sync_settings_changed();
        self.apply_run_at_login();
        self.lights_apply();
        self.popout_settings_changed();
        self.flow_apply();
    }

    pub(crate) fn bookmarks_changed(&self) {
        self.bookmarks_dirty.set(true);
        self.emit(Event::Bookmarks {
            bookmarks: self.bookmarks.borrow().clone(),
        });
        self.sync_soon();
    }

    pub(crate) fn downloads_changed(&self) {
        self.downloads_dirty.set(true);
        self.emit(Event::Downloads {
            items: self.downloads.borrow().items.clone(),
        });
        self.ext_download_events();
    }

    /// Push the settings that live inside the engine (user agent languages, proxy, devtools...).
    fn apply_engine_settings(&self) {
        use haku_core::settings::ProxyMode;
        use servo::PrefValue;
        let s = self.settings.borrow();
        let (http, https) = match s.system.proxy_mode {
            ProxyMode::Manual => (s.system.proxy_url.clone(), s.system.proxy_url.clone()),
            ProxyMode::Direct => (String::new(), String::new()),
            ProxyMode::System => (
                std::env::var("HTTP_PROXY")
                    .or_else(|_| std::env::var("http_proxy"))
                    .unwrap_or_default(),
                std::env::var("HTTPS_PROXY")
                    .or_else(|_| std::env::var("https_proxy"))
                    .unwrap_or_default(),
            ),
        };
        self.servo
            .set_preference("network_http_proxy_uri", PrefValue::Str(http));
        self.servo
            .set_preference("network_https_proxy_uri", PrefValue::Str(https));
        self.servo.set_preference(
            "network_http_no_proxy",
            PrefValue::Str(s.system.proxy_bypass.clone()),
        );
        self.servo
            .set_preference("dom_notification_enabled", PrefValue::Bool(true));
        self.servo
            .set_preference("dom_geolocation_enabled", PrefValue::Bool(true));
        self.servo.set_preference(
            "fonts_default",
            PrefValue::Str(s.appearance.font_standard.clone()),
        );
        self.servo.set_preference(
            "fonts_serif",
            PrefValue::Str(s.appearance.font_serif.clone()),
        );
        self.servo.set_preference(
            "fonts_sans_serif",
            PrefValue::Str(s.appearance.font_sans.clone()),
        );
        self.servo.set_preference(
            "fonts_monospace",
            PrefValue::Str(s.appearance.font_fixed.clone()),
        );
        let font_px = match s.appearance.font_size {
            haku_core::settings::FontSize::VerySmall => 12,
            haku_core::settings::FontSize::Small => 14,
            haku_core::settings::FontSize::Medium => 16,
            haku_core::settings::FontSize::Large => 20,
            haku_core::settings::FontSize::VeryLarge => 24,
        };
        self.servo
            .set_preference("fonts_default_size", PrefValue::Int(font_px));
        if let Some(lang) = s.languages.accept_languages.first() {
            self.servo
                .set_preference("intl_locale_override", PrefValue::Str(lang.clone()));
        }
        self.servo.set_preference(
            "network_enforce_tls_enabled",
            PrefValue::Bool(s.privacy.https_only),
        );
        self.servo
            .set_preference("network_enforce_tls_localhost", PrefValue::Bool(false));
        self.servo.set_preference(
            "js_disable_jit",
            PrefValue::Bool(!s.privacy.javascript_optimization),
        );
        self.servo.set_preference(
            "dom_webrtc_enabled",
            PrefValue::Bool(
                s.system.webrtc_policy != haku_core::settings::WebRtcPolicy::DisableNonProxiedUdp,
            ),
        );
        self.servo
            .set_preference("dom_webgpu_enabled", PrefValue::Bool(s.system.webgpu));
        self.servo.set_preference(
            "gfx_text_antialiasing_enabled",
            PrefValue::Bool(s.system.text_antialiasing),
        );
        self.servo
            .set_preference("dom_permissions_enabled", PrefValue::Bool(true));
        self.servo.set_preference(
            "devtools_server_enabled",
            PrefValue::Bool(s.system.devtools_server),
        );
        self.servo.set_preference(
            "devtools_server_listen_address",
            PrefValue::Str(format!("127.0.0.1:{}", s.system.devtools_port)),
        );
        let theme = match (s.appearance.color_mode, self.system_dark.get()) {
            (haku_core::settings::ColorMode::Dark, _)
            | (haku_core::settings::ColorMode::Auto, true) => servo::Theme::Dark,
            _ => servo::Theme::Light,
        };
        drop(s);
        for view in self.views.borrow().values() {
            if let Some(webview) = &view.webview {
                webview.notify_theme_change(theme);
            }
        }
        if let Some(ui) = self.ui_webview() {
            ui.notify_theme_change(theme);
        }
    }

    // ------------------------------------------------------------------ persistence

    /// Write dirty state to disk, at most every [`SAVE_DEBOUNCE`] unless `force`.
    pub(crate) fn save_if_due(&self, force: bool) {
        let Some(paths) = &self.paths else { return };
        if !force && self.last_save.get().elapsed() < SAVE_DEBOUNCE {
            return;
        }
        let mut wrote = false;
        if self.settings_dirty.replace(false) {
            wrote = true;
            if let Err(e) = storage::write_json_atomic(&paths.settings(), &*self.settings.borrow())
            {
                error!("saving settings: {e}");
            }
        }
        if self.session_dirty.replace(false) && !self.panic_active.get() && !self.secondary {
            wrote = true;
            if let Err(e) = storage::write_json_atomic(&paths.session(), &*self.session.borrow()) {
                error!("saving session: {e}");
            }
        }
        if self.bookmarks_dirty.replace(false) {
            wrote = true;
            if let Err(e) =
                storage::write_json_atomic(&paths.bookmarks(), &*self.bookmarks.borrow())
            {
                error!("saving bookmarks: {e}");
            }
        }
        if self.downloads_dirty.replace(false) {
            wrote = true;
            if let Err(e) =
                storage::write_json_atomic(&paths.downloads(), &*self.downloads.borrow())
            {
                error!("saving downloads: {e}");
            }
        }
        if wrote {
            self.last_save.set(Instant::now());
        }
    }

    pub(crate) fn save_named(&self, which: &str) {
        let Some(paths) = &self.paths else { return };
        let result = match which {
            "pinboards" => {
                storage::write_json_atomic(&paths.pinboards(), &*self.pinboards.borrow())
            }
            "site-mods" => {
                storage::write_json_atomic(&paths.site_mods(), &*self.site_mods.borrow())
            }
            "user-themes" => {
                storage::write_json_atomic(&paths.user_themes(), &*self.user_themes.borrow())
            }
            _ => Ok(()),
        };
        if let Err(e) = result {
            error!("saving {which}: {e}");
        }
    }

    /// Save everything, apply "clear on exit", then drop every webview so the last reference
    /// to Servo can go away. Servo writes cookies and HSTS state only on a clean shutdown.
    pub fn shutdown(&self) {
        if self.shutting_down.replace(true) {
            return;
        }
        info!("shutting down");
        self.lights_shutdown();
        self.flow_shutdown();
        drop(self.popout.borrow_mut().take());
        if self.panic_active.get()
            && let Some(saved) = self.panic_restore.borrow_mut().take()
        {
            *self.session.borrow_mut() = saved;
        }
        self.session_dirty.set(true);
        self.save_if_due(true);
        let clear = self.settings.borrow().privacy.clear_on_exit.clone();
        if clear.history
            && let Err(e) = self.history.clear()
        {
            error!("clearing history on exit: {e}");
        }
        if clear.downloads {
            self.downloads.borrow_mut().clear_finished(None);
            self.downloads_dirty.set(true);
            self.save_if_due(true);
        }
        if clear.cookies {
            self.servo.site_data_manager().clear_cookies(None);
        }
        if clear.cache {
            self.servo.network_manager().clear_cache();
        }
        self.ext_shutdown();
        self.views.borrow_mut().clear();
        self.by_webview.borrow_mut().clear();
        self.web_panels.borrow_mut().clear();
        self.panel_by_webview.borrow_mut().clear();
        self.controls.borrow_mut().clear();
        *self.ui.borrow_mut() = None;
    }
}

/// Convert a UI rect (CSS px) into window pixels.
pub(crate) fn to_pixels(rect: Rect, scale: f32) -> PixelRect {
    PixelRect {
        x: (rect.x * scale).round(),
        y: (rect.y * scale).round(),
        w: (rect.w * scale).round().max(1.0),
        h: (rect.h * scale).round().max(1.0),
    }
}

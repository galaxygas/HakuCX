/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! HakuCX engine spike (M0): one window, a transparent HTML UI shell rendered by Servo,
//! page webviews composited underneath it, IPC between the UI and Rust.

mod compositor;
mod keyutils;
mod protocol;
mod texture_context;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use compositor::{Compositor, PixelRect};
use euclid::Scale;
use log::{info, warn};
use servo::protocol_handler::ProtocolRegistry;
use servo::{
    ConsoleLogLevel, CreateNewWebViewRequest, EventLoopWaker, InputEvent, JSValue, LoadStatus,
    MouseButton as ServoMouseButton, MouseButtonAction, MouseButtonEvent, MouseLeftViewportEvent,
    MouseMoveEvent, Preferences, RenderingContext, Servo, ServoBuilder, UserContentManager,
    UserScript, WebResourceLoad, WebView, WebViewBuilder, WebViewDelegate, WebViewPoint,
    WheelDelta, WheelEvent, WheelMode, WindowRenderingContext,
};
use texture_context::TextureRenderingContext;
use url::Url;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::{Window, WindowId};

/// Prefix the UI shell uses on `console.debug` lines that carry IPC messages to Rust.
const IPC_PREFIX: &str = "\u{1}haku-ipc:";
/// Hosts blocked by the spike's interception test.
const SPIKE_BLOCKED_HOSTS: &[&str] = &["doubleclick.net", "googlesyndication.com"];

/// The production UI, built by `crates/hakucx/build.rs` from `ui/`.
static UI_DIST: include_dir::Dir<'static> = include_dir::include_dir!("$CARGO_MANIFEST_DIR/../../ui/dist");

fn spike_asset(host: &str, path: &str) -> Option<&'static [u8]> {
    if host == "app" {
        return UI_DIST.get_file(path).map(|f| f.contents());
    }
    match (host, path) {
        ("ui", "index.html") => Some(include_bytes!("../spike/index.html")),
        ("ui", "shell.css") => Some(include_bytes!("../spike/shell.css")),
        ("ui", "shell.js") => Some(include_bytes!("../spike/shell.js")),
        ("newtab", "index.html") => Some(include_bytes!("../spike/newtab.html")),
        _ => None,
    }
}

#[derive(Debug)]
enum AppEvent {
    Wake,
}

#[derive(Clone)]
struct Waker(EventLoopProxy<AppEvent>);

impl EventLoopWaker for Waker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }
    fn wake(&self) {
        let _ = self.0.send_event(AppEvent::Wake);
    }
}

struct View {
    webview: WebView,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum PointerTarget {
    Ui,
    Page,
}

struct State {
    this: Weak<State>,
    window: Window,
    servo: Servo,
    window_context: Rc<WindowRenderingContext>,
    compositor: Compositor,
    /// One painter per rendering context: the UI shell has its own, all page tabs share one
    /// (only the active tab is shown in it).
    ui_context: Rc<TextureRenderingContext>,
    page_context: Rc<TextureRenderingContext>,
    ui: RefCell<Option<View>>,
    tabs: RefCell<Vec<View>>,
    active_tab: Cell<usize>,
    /// Where the UI wants the active page drawn (window pixels, top-left origin).
    page_rect: Cell<PixelRect>,
    /// While the UI shows a menu/popover over the page, all input goes to the UI.
    overlay_open: Cell<bool>,
    cursor: Cell<PhysicalPosition<f64>>,
    pointer_target: Cell<PointerTarget>,
    modifiers: Cell<ModifiersState>,
    blocked_requests: Cell<u64>,
    /// The UI shell has run its script and can receive events.
    ui_ready: Cell<bool>,
    /// Servo must never be re-entered from its own callbacks, so IPC messages and UI events
    /// are queued here and handled in [`State::pump`] after `spin_event_loop` returns.
    inbox: RefCell<Vec<String>>,
    outbox: RefCell<Vec<serde_json::Value>>,
}

impl State {
    fn rc(&self) -> Rc<State> {
        self.this.upgrade().expect("state outlives its delegates")
    }

    fn scale(&self) -> f32 {
        self.window.scale_factor() as f32
    }

    fn ui_webview(&self) -> Option<WebView> {
        self.ui.borrow().as_ref().map(|v| v.webview.clone())
    }

    fn active_page(&self) -> Option<WebView> {
        self.tabs
            .borrow()
            .get(self.active_tab.get())
            .map(|v| v.webview.clone())
    }

    fn is_ui(&self, webview: &WebView) -> bool {
        self.ui
            .borrow()
            .as_ref()
            .is_some_and(|ui| ui.webview.id() == webview.id())
    }

    /// Send an event object to the UI shell (`window.__haku.receive(event)`).
    /// Queue an event for the UI shell (`window.__haku.receive(event)`).
    fn emit(&self, event: serde_json::Value) {
        self.outbox.borrow_mut().push(event);
    }

    /// Handle queued IPC messages and deliver queued UI events in one batch. Called after
    /// every `spin_event_loop`, outside any Servo callback.
    fn pump(&self) {
        loop {
            let messages = std::mem::take(&mut *self.inbox.borrow_mut());
            if messages.is_empty() {
                break;
            }
            for message in messages {
                self.handle_ipc(&message);
            }
        }
        if !self.ui_ready.get() || self.outbox.borrow().is_empty() {
            return;
        }
        let events = std::mem::take(&mut *self.outbox.borrow_mut());
        let Some(ui) = self.ui_webview() else { return };
        let batch = serde_json::Value::Array(events);
        let script = format!("for (const e of {batch}) window.__haku.receive(e);");
        ui.evaluate_javascript(script, |result| {
            if let Err(error) = result {
                warn!("UI event delivery failed: {error:?}");
            }
        });
    }

    fn tab_info(&self) -> serde_json::Value {
        let tabs = self.tabs.borrow();
        let list: Vec<_> = tabs
            .iter()
            .map(|t| {
                serde_json::json!({
                    "id": format!("{:?}", t.webview.id()),
                    "url": t.webview.url().map(|u| u.to_string()).unwrap_or_default(),
                    "title": t.webview.page_title().unwrap_or_default(),
                    "loading": t.webview.load_status() != LoadStatus::Complete,
                })
            })
            .collect();
        serde_json::json!({ "type": "tabs", "tabs": list, "active": self.active_tab.get() })
    }

    fn new_page_view(&self, url: Url) {
        let ucm = Rc::new(UserContentManager::new(&self.servo));
        // Spike check (d): user scripts run in pages.
        ucm.add_script(Rc::new(UserScript::new(
            "document.documentElement.dataset.hakuUserScript = '1';".to_owned(),
            None,
        )));
        let webview = WebViewBuilder::new(&self.servo, self.page_context.clone())
            .url(url)
            .hidpi_scale_factor(Scale::new(self.scale()))
            .user_content_manager(ucm)
            .delegate(self.rc())
            .build();
        self.adopt_page(webview);
    }

    fn adopt_page(&self, webview: WebView) {
        if let Some(previous) = self.active_page() {
            previous.hide();
            previous.set_focused(false);
        }
        webview.show();
        webview.set_focused(true);
        let mut tabs = self.tabs.borrow_mut();
        tabs.push(View { webview });
        self.active_tab.set(tabs.len() - 1);
        drop(tabs);
        self.emit(self.tab_info());
    }

    fn activate_tab(&self, index: usize) {
        let tabs = self.tabs.borrow();
        if index >= tabs.len() {
            return;
        }
        if let Some(previous) = tabs.get(self.active_tab.get()) {
            previous.webview.hide();
            previous.webview.set_focused(false);
        }
        tabs[index].webview.show();
        tabs[index].webview.set_focused(true);
        self.active_tab.set(index);
        drop(tabs);
        self.emit(self.tab_info());
        self.window.request_redraw();
    }

    fn close_tab(&self, index: usize) {
        let mut tabs = self.tabs.borrow_mut();
        if index >= tabs.len() || tabs.len() == 1 {
            return;
        }
        tabs.remove(index);
        let next = self.active_tab.get().min(tabs.len() - 1);
        drop(tabs);
        self.active_tab.set(usize::MAX);
        self.activate_tab(next);
    }

    fn set_page_rect(&self, rect: PixelRect) {
        self.page_rect.set(rect);
        let size = PhysicalSize::new(rect.w.max(1.0) as u32, rect.h.max(1.0) as u32);
        // `WebView::resize` resizes the shared rendering context and every webview on it.
        // Never resize the context first: Servo skips the viewport update when the context
        // already has the requested size.
        if let Some(page) = self.active_page() {
            page.resize(size);
        }
        self.window.request_redraw();
    }

    /// Handle one IPC message from the UI shell.
    fn handle_ipc(&self, message: &str) {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(message) else {
            warn!("Malformed IPC message: {message}");
            return;
        };
        let kind = msg["type"].as_str().unwrap_or_default();
        match kind {
            "ready" => {
                self.ui_ready.set(true);
                self.emit(self.tab_info());
                self.emit(
                    serde_json::json!({ "type": "blocked", "count": self.blocked_requests.get() }),
                );
            }
            "page-rect" => {
                let s = self.scale();
                let rect = PixelRect {
                    x: msg["x"].as_f64().unwrap_or(0.0) as f32 * s,
                    y: msg["y"].as_f64().unwrap_or(0.0) as f32 * s,
                    w: msg["w"].as_f64().unwrap_or(1.0) as f32 * s,
                    h: msg["h"].as_f64().unwrap_or(1.0) as f32 * s,
                };
                self.set_page_rect(rect);
            }
            "overlay" => self
                .overlay_open
                .set(msg["open"].as_bool().unwrap_or(false)),
            "navigate" => {
                let input = msg["url"].as_str().unwrap_or_default();
                if let Some(url) = fixup_url(input)
                    && let Some(page) = self.active_page()
                {
                    page.load(url);
                }
            }
            "new-tab" => {
                let url = msg["url"]
                    .as_str()
                    .and_then(fixup_url)
                    .unwrap_or_else(|| Url::parse("haku://newtab/").expect("static url"));
                self.new_page_view(url);
            }
            "activate-tab" => self.activate_tab(msg["index"].as_u64().unwrap_or(0) as usize),
            "close-tab" => self.close_tab(msg["index"].as_u64().unwrap_or(0) as usize),
            "back" => {
                if let Some(page) = self.active_page() {
                    page.go_back(1);
                }
            }
            "forward" => {
                if let Some(page) = self.active_page() {
                    page.go_forward(1);
                }
            }
            "reload" => {
                if let Some(page) = self.active_page() {
                    page.reload();
                }
            }
            "screenshot" => {
                if let Some(page) = self.active_page() {
                    let state = self.rc();
                    page.take_screenshot(None, move |result| match result {
                        Ok(image) => {
                            let path = std::env::temp_dir().join("hakucx-spike-screenshot.png");
                            let ok = image.save(&path).is_ok();
                            state.emit(serde_json::json!({
                                "type": "screenshot", "ok": ok,
                                "path": path.display().to_string(),
                                "w": image.width(), "h": image.height(),
                            }));
                        }
                        Err(error) => state.emit(serde_json::json!({
                            "type": "screenshot", "ok": false, "error": format!("{error:?}")
                        })),
                    });
                }
            }
            "eval-page" => {
                // Spike check (d): read back whether the user script ran in the page.
                if let Some(page) = self.active_page() {
                    // Privilege check: a page forging an IPC line must be rejected.
                    page.evaluate_javascript(
                        format!(
                            "console.debug({:?} + JSON.stringify({{type: 'new-tab'}}))",
                            IPC_PREFIX
                        ),
                        |_| {},
                    );
                    // Interception check: an ad-network request must be cancelled.
                    page.evaluate_javascript(
                        "fetch('https://securepubads.g.doubleclick.net/tag/js/gpt.js').then(() => 'loaded', () => 'failed')",
                        |_| {},
                    );
                    let state = self.rc();
                    page.evaluate_javascript(
                        "document.documentElement.dataset.hakuUserScript || 'missing'",
                        move |result| {
                            let value = match result {
                                Ok(JSValue::String(s)) => s,
                                other => format!("{other:?}"),
                            };
                            state
                                .emit(serde_json::json!({ "type": "eval-result", "value": value }));
                        },
                    );
                }
            }
            other => warn!("Unknown IPC message type: {other}"),
        }
    }

    fn paint(&self) {
        let gl = self.window_context.glow_gl_api();
        if let Err(error) = self.window_context.make_current() {
            warn!("make_current failed: {error:?}");
            return;
        }
        // Let Servo render each visible webview into its own texture.
        let active = self.active_page();
        if let Some(page) = &active {
            page.paint();
        }
        if let Some(ui) = self.ui_webview() {
            ui.paint();
        }

        let size = self.window.inner_size();
        let viewport_h = size.height as f32;
        self.window_context.prepare_for_rendering();
        unsafe {
            self.compositor
                .begin(&gl, size.width, size.height, [0.04, 0.04, 0.05, 1.0]);
            if active.is_some() {
                let rect = self.page_rect.get();
                // Pages without a background paint as white, like every other browser.
                self.compositor
                    .fill(&gl, rect, viewport_h, [1.0, 1.0, 1.0, 1.0]);
                self.compositor
                    .draw_texture(&gl, self.page_context.texture_id(), rect, viewport_h);
            }
            let full = PixelRect {
                x: 0.0,
                y: 0.0,
                w: size.width as f32,
                h: size.height as f32,
            };
            self.compositor
                .draw_texture(&gl, self.ui_context.texture_id(), full, viewport_h);
            self.compositor.end(&gl);
        }
        self.window_context.present();
    }

    fn route_pointer(&self) -> (PointerTarget, Option<WebView>, WebViewPoint) {
        let pos = self.cursor.get();
        let (x, y) = (pos.x as f32, pos.y as f32);
        let rect = self.page_rect.get();
        if !self.overlay_open.get()
            && rect.contains(x, y)
            && let Some(page) = self.active_page()
        {
            let point = servo::DevicePoint::new(x - rect.x, y - rect.y);
            return (PointerTarget::Page, Some(page), point.into());
        }
        (
            PointerTarget::Ui,
            self.ui_webview(),
            servo::DevicePoint::new(x, y).into(),
        )
    }
}

fn fixup_url(input: &str) -> Option<Url> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if let Ok(url) = Url::parse(input)
        && matches!(url.scheme(), "http" | "https" | "haku" | "data" | "file")
    {
        return Some(url);
    }
    if !input.contains(' ') && input.contains('.') {
        return Url::parse(&format!("https://{input}")).ok();
    }
    Url::parse_with_params("https://duckduckgo.com/", &[("q", input)]).ok()
}

impl WebViewDelegate for State {
    fn notify_new_frame_ready(&self, _webview: WebView) {
        self.window.request_redraw();
    }

    fn notify_url_changed(&self, _webview: WebView, _url: Url) {
        self.emit(self.tab_info());
    }

    fn notify_page_title_changed(&self, _webview: WebView, _title: Option<String>) {
        self.emit(self.tab_info());
    }

    fn notify_load_status_changed(&self, _webview: WebView, _status: LoadStatus) {
        self.emit(self.tab_info());
    }

    fn notify_cursor_changed(&self, webview: WebView, cursor: servo::Cursor) {
        // Only the view under the pointer controls the cursor.
        let target = self.pointer_target.get();
        let is_ui = self.is_ui(&webview);
        if (target == PointerTarget::Ui) == is_ui {
            self.window.set_cursor(winit_cursor(cursor));
        }
    }

    fn show_console_message(&self, webview: WebView, level: ConsoleLogLevel, message: String) {
        if let Some(payload) = message.strip_prefix(IPC_PREFIX) {
            // Privilege check (c): only the UI shell webview may speak IPC.
            if self.is_ui(&webview) {
                self.inbox.borrow_mut().push(payload.to_owned());
                self.window.request_redraw();
            } else {
                warn!("Rejected IPC from non-UI webview {:?}", webview.id());
            }
            return;
        }
        info!("[console {level:?}] {message}");
    }

    fn load_web_resource(&self, _webview: WebView, load: WebResourceLoad) {
        let blocked = load.request().url.host_str().is_some_and(|host| {
            SPIKE_BLOCKED_HOSTS
                .iter()
                .any(|b| host == *b || host.ends_with(&format!(".{b}")))
        });
        if blocked {
            self.blocked_requests.set(self.blocked_requests.get() + 1);
            let url = load.request().url.clone();
            load.intercept(servo::WebResourceResponse::new(url))
                .cancel();
            self.emit(
                serde_json::json!({ "type": "blocked", "count": self.blocked_requests.get() }),
            );
        }
    }

    fn request_create_new(&self, _parent: WebView, request: CreateNewWebViewRequest) {
        let webview = request
            .builder(self.page_context.clone())
            .hidpi_scale_factor(Scale::new(self.scale()))
            .delegate(self.rc())
            .build();
        self.adopt_page(webview);
    }
}

fn winit_cursor(cursor: servo::Cursor) -> winit::window::Cursor {
    use servo::Cursor as C;
    use winit::window::CursorIcon as W;
    let icon = match cursor {
        C::Pointer => W::Pointer,
        C::Text | C::VerticalText => W::Text,
        C::Wait => W::Wait,
        C::Progress => W::Progress,
        C::Help => W::Help,
        C::Crosshair => W::Crosshair,
        C::Move | C::AllScroll => W::Move,
        C::NotAllowed | C::NoDrop => W::NotAllowed,
        C::Grab => W::Grab,
        C::Grabbing => W::Grabbing,
        C::EResize | C::WResize | C::EwResize | C::ColResize => W::EwResize,
        C::NResize | C::SResize | C::NsResize | C::RowResize => W::NsResize,
        C::NeResize | C::SwResize | C::NeswResize => W::NeswResize,
        C::NwResize | C::SeResize | C::NwseResize => W::NwseResize,
        C::ContextMenu => W::ContextMenu,
        C::Cell => W::Cell,
        C::Copy => W::Copy,
        C::Alias => W::Alias,
        C::ZoomIn => W::ZoomIn,
        C::ZoomOut => W::ZoomOut,
        _ => W::Default,
    };
    icon.into()
}

enum App {
    Initial(Waker),
    Running(Rc<State>),
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let Self::Initial(waker) = self else { return };
        let window = event_loop
            .create_window(
                Window::default_attributes()
                    .with_title("HakuCX")
                    .with_inner_size(LogicalSize::new(1280.0, 800.0)),
            )
            .expect("create window");
        let display_handle = event_loop.display_handle().expect("display handle");
        let window_handle = window.window_handle().expect("window handle");
        let window_context = Rc::new(
            WindowRenderingContext::new(display_handle, window_handle, window.inner_size())
                .expect("window rendering context"),
        );
        window_context.make_current().expect("make current");

        let mut registry = ProtocolRegistry::default();
        registry
            .register("haku", protocol::HakuProtocolHandler::new(spike_asset))
            .expect("register haku://");

        let prefs = Preferences {
            // Transparent clear so the UI shell can be layered over pages.
            shell_background_color_rgba: [0.0, 0.0, 0.0, 0.0],
            ..Preferences::default()
        };

        let servo = ServoBuilder::default()
            .preferences(prefs)
            .protocol_registry(registry)
            .event_loop_waker(Box::new(waker.clone()))
            .build();
        servo.setup_logging();

        let compositor = unsafe { Compositor::new(&window_context.glow_gl_api()) }
            .expect("compile compositor shaders");

        let size = window.inner_size();
        let ui_context = Rc::new(TextureRenderingContext::new(window_context.clone(), size));
        let page_context = Rc::new(TextureRenderingContext::new(
            window_context.clone(),
            PhysicalSize::new(size.width, size.height.saturating_sub(80).max(1)),
        ));
        let state = Rc::new_cyclic(|this| State {
            this: this.clone(),
            window,
            servo,
            window_context: window_context.clone(),
            compositor,
            ui_context: ui_context.clone(),
            page_context,
            ui: RefCell::new(None),
            tabs: RefCell::new(Vec::new()),
            active_tab: Cell::new(0),
            page_rect: Cell::new(PixelRect {
                x: 0.0,
                y: 80.0,
                w: size.width as f32,
                h: (size.height as f32 - 80.0).max(1.0),
            }),
            overlay_open: Cell::new(false),
            cursor: Cell::new(PhysicalPosition::new(0.0, 0.0)),
            pointer_target: Cell::new(PointerTarget::Ui),
            modifiers: Cell::new(ModifiersState::empty()),
            blocked_requests: Cell::new(0),
            ui_ready: Cell::new(false),
            inbox: RefCell::new(Vec::new()),
            outbox: RefCell::new(Vec::new()),
        });

        let ui = WebViewBuilder::new(&state.servo, ui_context)
            .url(Url::parse("haku://ui/").expect("static url"))
            .hidpi_scale_factor(Scale::new(state.scale()))
            .delegate(state.clone())
            .build();
        ui.show();
        *state.ui.borrow_mut() = Some(View { webview: ui });

        let first = std::env::args()
            .nth(1)
            .and_then(|a| fixup_url(&a))
            .unwrap_or_else(|| Url::parse("haku://newtab/").expect("static url"));
        state.new_page_view(first);

        *self = Self::Running(state);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: AppEvent) {
        if let Self::Running(state) = self {
            state.servo.spin_event_loop();
            state.pump();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Self::Running(state) = self else { return };
        state.servo.spin_event_loop();
        state.pump();
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => state.paint(),
            WindowEvent::Resized(size) => {
                if size.width == 0 || size.height == 0 {
                    return;
                }
                state.window_context.resize(size);
                if let Some(ui) = state.ui_webview() {
                    ui.resize(size);
                }
                state.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let scale = Scale::new(scale_factor as f32);
                if let Some(ui) = state.ui_webview() {
                    ui.set_hidpi_scale_factor(scale);
                }
                for tab in state.tabs.borrow().iter() {
                    tab.webview.set_hidpi_scale_factor(scale);
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => state.modifiers.set(modifiers.state()),
            WindowEvent::CursorMoved { position, .. } => {
                state.cursor.set(position);
                let previous = state.pointer_target.get();
                let (target, webview, point) = state.route_pointer();
                if previous != target {
                    // Tell the view we left that the pointer is gone.
                    let left = if previous == PointerTarget::Page {
                        state.active_page()
                    } else {
                        state.ui_webview()
                    };
                    if let Some(left) = left {
                        left.notify_input_event(InputEvent::MouseLeftViewport(
                            MouseLeftViewportEvent::default(),
                        ));
                    }
                }
                state.pointer_target.set(target);
                if let Some(webview) = webview {
                    webview.notify_input_event(InputEvent::MouseMove(MouseMoveEvent::new(point)));
                }
            }
            WindowEvent::MouseInput {
                state: button_state,
                button,
                ..
            } => {
                let (target, webview, point) = state.route_pointer();
                let Some(webview) = webview else { return };
                if button_state == ElementState::Pressed {
                    // Keyboard focus follows the click.
                    let page_focus = target == PointerTarget::Page;
                    if let Some(page) = state.active_page() {
                        page.set_focused(page_focus);
                    }
                    if let Some(ui) = state.ui_webview() {
                        ui.set_focused(!page_focus);
                    }
                }
                let button = match button {
                    MouseButton::Left => ServoMouseButton::Primary,
                    MouseButton::Right => ServoMouseButton::Secondary,
                    MouseButton::Middle => ServoMouseButton::Auxiliary,
                    MouseButton::Back => ServoMouseButton::Back,
                    MouseButton::Forward => ServoMouseButton::Forward,
                    MouseButton::Other(v) => ServoMouseButton::Other(v),
                };
                let action = match button_state {
                    ElementState::Pressed => MouseButtonAction::Down,
                    ElementState::Released => MouseButtonAction::Up,
                };
                webview.notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(
                    action, button, point,
                )));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (_, webview, point) = state.route_pointer();
                let Some(webview) = webview else { return };
                let (x, y, mode) = match delta {
                    MouseScrollDelta::LineDelta(dx, dy) => {
                        ((dx * 76.0) as f64, (dy * 76.0) as f64, WheelMode::DeltaLine)
                    }
                    MouseScrollDelta::PixelDelta(d) => (d.x, d.y, WheelMode::DeltaPixel),
                };
                webview.notify_input_event(InputEvent::Wheel(WheelEvent::new(
                    WheelDelta { x, y, z: 0.0, mode },
                    point,
                )));
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let keyboard = keyutils::keyboard_event_from_winit(&event, state.modifiers.get());
                // Keyboard goes to whichever view holds focus.
                let page_focused = state.active_page().is_some_and(|p| p.focused());
                let target = if page_focused {
                    state.active_page()
                } else {
                    state.ui_webview()
                };
                if let Some(webview) = target {
                    webview.notify_input_event(InputEvent::Keyboard(keyboard));
                }
            }
            _ => {}
        }
        state.pump();
    }
}

fn main() {
    // Servo installs the process logger (env_logger + forwarding) in `Servo::setup_logging`.
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .expect("create event loop");
    let mut app = App::Initial(Waker(event_loop.create_proxy()));
    event_loop.run_app(&mut app).expect("run event loop");
}

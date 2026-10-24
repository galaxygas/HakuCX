/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Video pop out: a small borderless window that stays on top of other apps and plays a tab's
//! video.
//!
//! Servo draws every webview of a painter into one surface and cannot move a running page to
//! another one, so the tab's own `<video>` element cannot be moved. Instead the pop-out
//! window opens `haku://popout/` with the video's address and position; the tab's copy is
//! paused, and "Back to tab" hands the position back. That works for videos with a web
//! address; videos a page builds from a stream (`blob:`, Media Source, WebRTC) cannot be
//! handed over, and HakuCX says so instead of opening an empty window.

use std::cell::Cell;
use std::rc::Rc;

use haku_core::ipc::Event;
use haku_core::session::TabId;
use log::warn;
use servo::{
    InputEvent, MouseButton as ServoMouseButton, MouseButtonAction, MouseButtonEvent,
    MouseLeftViewportEvent, MouseMoveEvent, RenderingContext, WebView, WebViewBuilder,
    WebViewPoint, WheelDelta, WheelEvent, WheelMode, WindowRenderingContext,
};
use url::Url;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::ModifiersState;
use winit::window::{CursorIcon, ResizeDirection, Window, WindowId};

use super::{Browser, page_scripts};
use crate::keyutils::keyboard_event_from_winit;

/// What to play, from the tab's page.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PopoutSpec {
    #[serde(skip)]
    pub tab: TabId,
    pub src: String,
    pub time: f64,
    pub paused: bool,
    pub volume: f64,
    pub muted: bool,
    pub poster: String,
    pub title: String,
    #[serde(skip)]
    pub width: u32,
    #[serde(skip)]
    pub height: u32,
    /// Opened because its tab was left (auto pop out); going back to the tab closes it.
    #[serde(skip)]
    pub auto: bool,
}

impl PopoutSpec {
    /// The page address: the video and its state travel in the fragment, which is never sent
    /// anywhere. `n` makes every pop out a new document.
    pub fn page_url(&self, n: u64) -> Option<Url> {
        let json = serde_json::to_string(self).ok()?;
        let fragment: String = url::form_urlencoded::byte_serialize(json.as_bytes()).collect();
        Url::parse(&format!("haku://popout/?n={n}#{fragment}")).ok()
    }

    /// Window size for the video's shape: 480 px wide (logical), height by aspect ratio.
    pub fn window_size(&self, scale: f64) -> PhysicalSize<u32> {
        let aspect = if self.width > 0 && self.height > 0 {
            (self.width as f64 / self.height as f64).clamp(0.5, 3.0)
        } else {
            16.0 / 9.0
        };
        let w = 480.0;
        let h = (w / aspect).clamp(160.0, 520.0);
        PhysicalSize::new((w * scale) as u32, (h * scale) as u32)
    }
}

pub struct Popout {
    pub window: Window,
    pub context: Rc<WindowRenderingContext>,
    pub webview: WebView,
    pub spec: PopoutSpec,
    cursor: Cell<PhysicalPosition<f64>>,
    modifiers: Cell<ModifiersState>,
}

/// How close to the border (physical px) a press resizes the window.
const EDGE: f64 = 6.0;
/// Window opacity of a see-through pop out while the pointer is elsewhere.
const SEE_THROUGH_ALPHA: u8 = 150;

impl Popout {
    fn edge(&self, p: PhysicalPosition<f64>) -> Option<ResizeDirection> {
        let size = self.window.inner_size();
        let (w, h) = (size.width as f64, size.height as f64);
        let (l, r, t, b) = (p.x < EDGE, p.x > w - EDGE, p.y < EDGE, p.y > h - EDGE);
        Some(match (l, r, t, b) {
            (true, _, true, _) => ResizeDirection::NorthWest,
            (_, true, true, _) => ResizeDirection::NorthEast,
            (true, _, _, true) => ResizeDirection::SouthWest,
            (_, true, _, true) => ResizeDirection::SouthEast,
            (true, ..) => ResizeDirection::West,
            (_, true, ..) => ResizeDirection::East,
            (_, _, true, _) => ResizeDirection::North,
            (_, _, _, true) => ResizeDirection::South,
            _ => return None,
        })
    }

    fn point(&self) -> WebViewPoint {
        let p = self.cursor.get();
        WebViewPoint::Device(servo::DevicePoint::new(p.x as f32, p.y as f32))
    }
}

impl Browser {
    pub(crate) fn is_popout(&self, webview: &WebView) -> bool {
        self.popout
            .borrow()
            .as_ref()
            .is_some_and(|p| p.webview.id() == webview.id())
    }

    pub(crate) fn is_popout_window(&self, id: WindowId) -> bool {
        self.popout
            .borrow()
            .as_ref()
            .is_some_and(|p| p.window.id() == id)
    }

    /// The tab whose video is in the pop-out window.
    pub(crate) fn popout_source(&self) -> Option<TabId> {
        self.popout.borrow().as_ref().map(|p| p.spec.tab.clone())
    }

    /// A page asked to pop out its video (button, miniplayer or auto pop out).
    pub(crate) fn request_popout(&self, spec: PopoutSpec) {
        if !self.settings.borrow().video.popout {
            return;
        }
        let ok = Url::parse(&spec.src).is_ok_and(|u| matches!(u.scheme(), "http" | "https"));
        if !ok {
            self.popout_unavailable("stream");
            return;
        }
        // A pop out already open shows the new video instead.
        let replace = self.popout.borrow().is_some();
        if replace {
            let old = self.popout.borrow().as_ref().map(|p| p.spec.tab.clone());
            if let Some(old) = old.filter(|t| *t != spec.tab) {
                self.tab_media_script(&old, "closed", &serde_json::json!({}));
            }
            let n = self.popout_counter.get() + 1;
            self.popout_counter.set(n);
            if let Some(popout) = self.popout.borrow_mut().as_mut() {
                if let Some(url) = spec.page_url(n) {
                    popout.webview.load(url);
                }
                popout.window.set_title(&popout_title(&spec.title));
                popout.spec = spec;
                popout.window.focus_window();
            }
            self.popout_changed();
            return;
        }
        *self.popout_request.borrow_mut() = Some(spec);
        // The app creates the window on its next turn (it needs the event loop).
        self.window.request_redraw();
    }

    pub(crate) fn popout_unavailable(&self, reason: &str) {
        let text = if reason == "none" {
            "There is no video on this page to pop out."
        } else {
            "This video can't be popped out: the site builds it from a stream that only this tab can play."
        };
        self.emit(Event::Toast {
            text: text.into(),
            kind: "info".into(),
        });
    }

    pub fn take_popout_request(&self) -> Option<PopoutSpec> {
        self.popout_request.borrow_mut().take()
    }

    /// Where a new pop out goes: where the last one was, else the bottom-right corner of the
    /// screen the browser is on.
    pub fn popout_placement(
        &self,
        spec: &PopoutSpec,
    ) -> (Option<PhysicalPosition<i32>>, PhysicalSize<u32>) {
        if let Some((pos, size)) = self.popout_geometry.get() {
            return (Some(pos), size);
        }
        let scale = self.window.scale_factor();
        let size = spec.window_size(scale);
        let pos = self.window.current_monitor().map(|m| {
            let (mp, ms) = (m.position(), m.size());
            let margin = (32.0 * scale) as i32;
            // Leave room for a taskbar at the bottom.
            let taskbar = (56.0 * scale) as i32;
            PhysicalPosition::new(
                mp.x + ms.width as i32 - size.width as i32 - margin,
                mp.y + ms.height as i32 - size.height as i32 - margin - taskbar,
            )
        });
        (pos, size)
    }

    /// The app created the window and its GL context: start the page in it.
    pub fn open_popout(
        &self,
        window: Window,
        context: Rc<WindowRenderingContext>,
        spec: PopoutSpec,
    ) {
        let n = self.popout_counter.get() + 1;
        self.popout_counter.set(n);
        let Some(url) = spec.page_url(n) else {
            return;
        };
        let webview = WebViewBuilder::new(&self.servo, context.clone())
            .url(url)
            .hidpi_scale_factor(euclid::Scale::new(window.scale_factor() as f32))
            .delegate(self.rc())
            .build();
        webview.show();
        webview.set_focused(true);
        window.set_title(&popout_title(&spec.title));
        let popout = Popout {
            window,
            context,
            webview,
            spec,
            cursor: Cell::new(PhysicalPosition::new(0.0, 0.0)),
            modifiers: Cell::new(ModifiersState::empty()),
        };
        if self.settings.borrow().video.popout_transparent {
            set_window_alpha(&popout.window, SEE_THROUGH_ALPHA);
        }
        *self.popout.borrow_mut() = Some(popout);
        self.popout_changed();
    }

    /// The tab's media state changed sides (popped out or back).
    fn popout_changed(&self) {
        let tabs: Vec<TabId> = self
            .views
            .borrow()
            .iter()
            .filter(|(_, v)| v.media.is_some())
            .map(|(id, _)| id.clone())
            .collect();
        for tab in tabs {
            self.after_tab_update(&tab);
        }
    }

    pub(crate) fn tab_media_script(&self, tab: &TabId, action: &str, arg: &serde_json::Value) {
        if let Some(webview) = self.webview_for_tab(tab) {
            webview.evaluate_javascript(page_scripts::media_action_script(action, arg), |_| {});
        }
    }

    /// Close the pop out. `back`: return the video to its tab (and show the tab); otherwise
    /// it stays paused there at the pop out's position.
    pub(crate) fn close_popout(&self, time: f64, playing: bool, back: bool) {
        let Some(popout) = self.popout.borrow_mut().take() else {
            return;
        };
        if let Ok(pos) = popout.window.outer_position() {
            self.popout_geometry
                .set(Some((pos, popout.window.inner_size())));
        }
        let tab = popout.spec.tab.clone();
        self.animating.borrow_mut().remove(&popout.webview.id());
        drop(popout);
        let arg = serde_json::json!({ "time": time, "play": back && playing });
        self.tab_media_script(&tab, if back { "return" } else { "closed" }, &arg);
        if back {
            if self.session.borrow_mut().activate(&tab) {
                self.session_changed();
            }
            self.window.focus_window();
        }
        self.popout_changed();
    }

    /// Close the pop out at once (panic button): the tab's video stays paused where it was.
    pub(crate) fn dismiss_popout(&self) {
        let Some(popout) = self.popout.borrow_mut().take() else {
            return;
        };
        let tab = popout.spec.tab.clone();
        self.animating.borrow_mut().remove(&popout.webview.id());
        drop(popout);
        self.tab_media_script(&tab, "closed", &serde_json::json!({}));
        self.popout_changed();
    }

    /// Ask the page for the position, then close (the page answers with `popout-close`).
    fn close_popout_from_window(&self) {
        let webview = self.popout.borrow().as_ref().map(|p| p.webview.clone());
        if let Some(webview) = webview {
            webview.evaluate_javascript("window.hakuClose && window.hakuClose()", |_| {});
        }
    }

    /// Messages from the pop-out page.
    pub(crate) fn handle_popout_message(&self, payload: &str) {
        #[derive(serde::Deserialize)]
        #[serde(tag = "type")]
        enum Msg {
            #[serde(rename = "popout-return")]
            Return { time: f64, paused: bool },
            #[serde(rename = "popout-close")]
            Close { time: f64, paused: bool },
            #[serde(rename = "popout-drag")]
            Drag,
        }
        let Ok(msg) = serde_json::from_str::<Msg>(payload) else {
            return;
        };
        match msg {
            Msg::Return { time, paused } => self.close_popout(time, !paused, true),
            Msg::Close { time, paused } => self.close_popout(time, !paused, false),
            Msg::Drag => {
                if let Some(p) = self.popout.borrow().as_ref()
                    && let Err(e) = p.window.drag_window()
                {
                    warn!("drag_window: {e}");
                }
            }
        }
    }

    pub(crate) fn popout_frame_ready(&self) {
        if let Some(p) = self.popout.borrow().as_ref() {
            p.window.request_redraw();
        }
    }

    fn paint_popout(&self) {
        let popout = self.popout.borrow();
        let Some(p) = popout.as_ref() else {
            return;
        };
        if let Err(e) = p.context.make_current() {
            warn!("pop out make_current failed: {e:?}");
            return;
        }
        p.webview.paint();
        p.context.present();
    }

    /// The video settings changed: the pop out may no longer be allowed, and pages update
    /// their pop-out button.
    pub(crate) fn popout_settings_changed(&self) {
        let video = self.settings.borrow().video.clone();
        if !video.popout {
            self.close_popout_from_window();
        }
        if let Some(p) = self.popout.borrow().as_ref() {
            set_window_alpha(
                &p.window,
                if video.popout_transparent {
                    SEE_THROUGH_ALPHA
                } else {
                    255
                },
            );
        }
        let script = page_scripts::media_button_script(video.popout && video.video_pickup);
        for view in self.views.borrow().values() {
            if let Some(webview) = &view.webview {
                webview.evaluate_javascript(&script, |_| {});
            }
        }
    }

    pub fn popout_window_event(&self, event: WindowEvent) {
        match event {
            WindowEvent::RedrawRequested => self.paint_popout(),
            WindowEvent::CloseRequested => self.close_popout_from_window(),
            WindowEvent::Resized(size) => {
                if size.width > 0
                    && size.height > 0
                    && let Some(p) = self.popout.borrow().as_ref()
                {
                    p.context.resize(size);
                    p.webview.resize(size);
                    p.window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(p) = self.popout.borrow().as_ref() {
                    p.webview
                        .set_hidpi_scale_factor(euclid::Scale::new(scale_factor as f32));
                }
            }
            WindowEvent::ModifiersChanged(m) => {
                if let Some(p) = self.popout.borrow().as_ref() {
                    p.modifiers.set(m.state());
                }
            }
            WindowEvent::CursorEntered { .. } => {
                if let Some(p) = self.popout.borrow().as_ref() {
                    set_window_alpha(&p.window, 255);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(p) = self.popout.borrow().as_ref() {
                    p.webview.notify_input_event(InputEvent::MouseLeftViewport(
                        MouseLeftViewportEvent::default(),
                    ));
                    if self.settings.borrow().video.popout_transparent {
                        set_window_alpha(&p.window, SEE_THROUGH_ALPHA);
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let popout = self.popout.borrow();
                let Some(p) = popout.as_ref() else { return };
                p.cursor.set(position);
                let icon = match p.edge(position) {
                    Some(ResizeDirection::East | ResizeDirection::West) => CursorIcon::EwResize,
                    Some(ResizeDirection::North | ResizeDirection::South) => CursorIcon::NsResize,
                    Some(ResizeDirection::NorthEast | ResizeDirection::SouthWest) => {
                        CursorIcon::NeswResize
                    }
                    Some(_) => CursorIcon::NwseResize,
                    None => {
                        p.window.set_cursor(CursorIcon::Default);
                        p.webview
                            .notify_input_event(InputEvent::MouseMove(MouseMoveEvent::new(
                                p.point(),
                            )));
                        return;
                    }
                };
                p.window.set_cursor(icon);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let popout = self.popout.borrow();
                let Some(p) = popout.as_ref() else { return };
                if let Some(dir) = p.edge(p.cursor.get()) {
                    if state == ElementState::Pressed && button == MouseButton::Left {
                        let _ = p.window.drag_resize_window(dir);
                    }
                    return;
                }
                let button = match button {
                    MouseButton::Left => ServoMouseButton::Primary,
                    MouseButton::Right => ServoMouseButton::Secondary,
                    MouseButton::Middle => ServoMouseButton::Auxiliary,
                    MouseButton::Back => ServoMouseButton::Back,
                    MouseButton::Forward => ServoMouseButton::Forward,
                    MouseButton::Other(v) => ServoMouseButton::Other(v),
                };
                let action = match state {
                    ElementState::Pressed => MouseButtonAction::Down,
                    ElementState::Released => MouseButtonAction::Up,
                };
                p.webview
                    .notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(
                        action,
                        button,
                        p.point(),
                    )));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let popout = self.popout.borrow();
                let Some(p) = popout.as_ref() else { return };
                let (x, y, mode) = match delta {
                    MouseScrollDelta::LineDelta(dx, dy) => {
                        ((dx * 76.0) as f64, (dy * 76.0) as f64, WheelMode::DeltaLine)
                    }
                    MouseScrollDelta::PixelDelta(d) => (d.x, d.y, WheelMode::DeltaPixel),
                };
                p.webview
                    .notify_input_event(InputEvent::Wheel(WheelEvent::new(
                        WheelDelta { x, y, z: 0.0, mode },
                        p.point(),
                    )));
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let popout = self.popout.borrow();
                let Some(p) = popout.as_ref() else { return };
                let keyboard = keyboard_event_from_winit(&event, p.modifiers.get());
                p.webview.notify_input_event(InputEvent::Keyboard(keyboard));
            }
            WindowEvent::Moved(_) => {
                if let Some(p) = self.popout.borrow().as_ref()
                    && let Ok(pos) = p.window.outer_position()
                {
                    self.popout_geometry.set(Some((pos, p.window.inner_size())));
                }
            }
            _ => {}
        }
    }
}

fn popout_title(title: &str) -> String {
    if title.is_empty() {
        "HakuCX video".into()
    } else {
        format!("{title} - HakuCX")
    }
}

/// Make the whole window see-through (`alpha` < 255) or opaque.
#[cfg(windows)]
fn set_window_alpha(window: &Window, alpha: u8) {
    use windows::Win32::Foundation::{COLORREF, HWND};
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, LWA_ALPHA, SetLayeredWindowAttributes, SetWindowLongPtrW,
        WS_EX_LAYERED,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(h) = handle.as_raw() else {
        return;
    };
    let hwnd = HWND(h.hwnd.get() as *mut core::ffi::c_void);
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if style & WS_EX_LAYERED.0 as isize == 0 {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | WS_EX_LAYERED.0 as isize);
        }
        if let Err(e) = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA) {
            warn!("SetLayeredWindowAttributes: {e}");
        }
    }
}

#[cfg(not(windows))]
fn set_window_alpha(_window: &Window, _alpha: u8) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> PopoutSpec {
        PopoutSpec {
            tab: "t1".into(),
            src: "https://media.example/v.mp4?a=1&b=2#x".into(),
            time: 12.5,
            paused: false,
            volume: 0.5,
            muted: false,
            poster: String::new(),
            title: "Café \"video\"".into(),
            width: 1920,
            height: 1080,
            auto: false,
        }
    }

    #[test]
    fn page_url_carries_the_spec_in_the_fragment() {
        let url = spec().page_url(3).unwrap();
        assert_eq!(url.scheme(), "haku");
        assert_eq!(url.host_str(), Some("popout"));
        assert_eq!(url.query(), Some("n=3"));
        let fragment = url.fragment().unwrap();
        let json: String = url::form_urlencoded::parse(format!("x={fragment}").as_bytes())
            .next()
            .unwrap()
            .1
            .into_owned();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["src"], "https://media.example/v.mp4?a=1&b=2#x");
        assert_eq!(v["time"], 12.5);
        assert_eq!(v["title"], "Café \"video\"");
        // Browser-side fields stay out of the page.
        assert!(v.get("tab").is_none());
        assert!(v.get("auto").is_none());
    }

    #[test]
    fn window_follows_the_video_shape() {
        let mut s = spec();
        assert_eq!(s.window_size(1.0), PhysicalSize::new(480, 270));
        assert_eq!(s.window_size(2.0), PhysicalSize::new(960, 540));
        s.width = 0;
        assert_eq!(s.window_size(1.0), PhysicalSize::new(480, 270));
        s.width = 1080;
        s.height = 1920;
        // Portrait videos are capped in height.
        assert_eq!(s.window_size(1.0), PhysicalSize::new(480, 520));
    }
}

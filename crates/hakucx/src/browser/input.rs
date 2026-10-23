/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Window input: pointer routing between the UI shell and page panes, frameless-window
//! resizing, IME, and browser keyboard shortcuts (handled here so pages cannot swallow them).

use haku_core::ipc::{Command, Event, WindowAction};
use servo::{
    ImeEvent, InputEvent, Key, KeyState, Modifiers, MouseButton as ServoMouseButton,
    MouseButtonAction, MouseButtonEvent, MouseLeftViewportEvent, MouseMoveEvent, NamedKey, WebView,
    WebViewPoint, WheelDelta, WheelEvent, WheelMode,
};
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::window::{CursorIcon, ResizeDirection};

use super::tabs::Pane;
use super::{Browser, PointerTarget};
use crate::keyutils::keyboard_event_from_winit;

const EDGE: f64 = 6.0;

/// Browser commands that keyboard shortcuts can trigger, with their default accelerators.
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("new-tab", "Ctrl+T"),
    ("close-tab", "Ctrl+W"),
    ("close-tab-alt", "Ctrl+F4"),
    ("reopen-tab", "Ctrl+Shift+T"),
    ("next-tab", "Ctrl+Tab"),
    ("previous-tab", "Ctrl+Shift+Tab"),
    ("next-tab-alt", "Ctrl+PageDown"),
    ("previous-tab-alt", "Ctrl+PageUp"),
    ("focus-address", "Ctrl+L"),
    ("focus-address-alt", "Alt+D"),
    ("focus-address-f6", "F6"),
    ("find", "Ctrl+F"),
    ("find-next", "F3"),
    ("reload", "Ctrl+R"),
    ("reload-f5", "F5"),
    ("hard-reload", "Ctrl+Shift+R"),
    ("hard-reload-f5", "Ctrl+F5"),
    ("back", "Alt+Left"),
    ("forward", "Alt+Right"),
    ("home", "Alt+Home"),
    ("bookmark", "Ctrl+D"),
    ("history", "Ctrl+H"),
    ("downloads", "Ctrl+J"),
    ("bookmarks", "Ctrl+Shift+O"),
    ("settings", "Alt+P"),
    ("extensions", "Ctrl+Shift+E"),
    ("new-window", "Ctrl+N"),
    ("private-window", "Ctrl+Shift+N"),
    ("zoom-in", "Ctrl+="),
    ("zoom-in-plus", "Ctrl+Shift+="),
    ("zoom-out", "Ctrl+-"),
    ("zoom-reset", "Ctrl+0"),
    ("fullscreen", "F11"),
    ("tab-search", "Ctrl+Space"),
    ("command-palette", "Ctrl+Shift+A"),
    ("save-page", "Ctrl+S"),
    ("view-source", "Ctrl+U"),
    ("devtools", "Ctrl+Shift+I"),
    ("clear-data", "Ctrl+Shift+Delete"),
    ("toggle-bookmarks-bar", "Ctrl+Shift+B"),
    ("toggle-sidebar", "Ctrl+Shift+S"),
    ("snapshot", "Ctrl+Shift+5"),
    ("mute-tab", "Ctrl+M"),
    ("easy-setup", "Ctrl+Shift+Y"),
    ("quit", "Ctrl+Shift+Q"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accelerator {
    ctrl: bool,
    shift: bool,
    alt: bool,
    key: String,
}

/// Parse `Ctrl+Shift+T`, `F12`, `Alt+Left`.
pub fn parse_accelerator(text: &str) -> Option<Accelerator> {
    let mut acc = Accelerator {
        ctrl: false,
        shift: false,
        alt: false,
        key: String::new(),
    };
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    // "Ctrl++" means the plus key.
    let (mods, key) = if text.ends_with("++") {
        (&parts[..parts.len().saturating_sub(2)], "+")
    } else {
        (&parts[..parts.len().saturating_sub(1)], *parts.last()?)
    };
    for m in mods {
        match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "cmd" => acc.ctrl = true,
            "shift" => acc.shift = true,
            "alt" | "option" => acc.alt = true,
            _ => return None,
        }
    }
    if key.is_empty() {
        return None;
    }
    acc.key = key.to_ascii_lowercase();
    Some(acc)
}

fn key_name(key: &Key) -> Option<String> {
    Some(match key {
        Key::Character(c) => c.to_ascii_lowercase(),
        Key::Named(n) => match n {
            NamedKey::Tab => "tab".into(),
            NamedKey::PageDown => "pagedown".into(),
            NamedKey::PageUp => "pageup".into(),
            NamedKey::ArrowLeft => "left".into(),
            NamedKey::ArrowRight => "right".into(),
            NamedKey::Home => "home".into(),
            NamedKey::Delete => "delete".into(),
            NamedKey::Escape => "escape".into(),
            NamedKey::Enter => "enter".into(),
            NamedKey::Backspace => "backspace".into(),
            NamedKey::F1 => "f1".into(),
            NamedKey::F2 => "f2".into(),
            NamedKey::F3 => "f3".into(),
            NamedKey::F4 => "f4".into(),
            NamedKey::F5 => "f5".into(),
            NamedKey::F6 => "f6".into(),
            NamedKey::F7 => "f7".into(),
            NamedKey::F8 => "f8".into(),
            NamedKey::F9 => "f9".into(),
            NamedKey::F10 => "f10".into(),
            NamedKey::F11 => "f11".into(),
            NamedKey::F12 => "f12".into(),
            _ => return None,
        },
    })
}

fn matches(acc: &Accelerator, key: &str, mods: Modifiers) -> bool {
    acc.key == key
        && acc.ctrl == mods.contains(Modifiers::CONTROL)
        && acc.alt == mods.contains(Modifiers::ALT)
        && (acc.shift == mods.contains(Modifiers::SHIFT)
            // Shifted characters like "+" already include Shift.
            || (!acc.shift && key.len() == 1 && !key.chars().all(|c| c.is_ascii_alphanumeric())))
}

pub fn winit_cursor(cursor: servo::Cursor) -> winit::window::Cursor {
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

impl Browser {
    fn frameless(&self) -> bool {
        !self.window.is_decorated()
    }

    fn edge_direction(&self, pos: PhysicalPosition<f64>) -> Option<ResizeDirection> {
        if !self.frameless() || self.window.is_maximized() || self.window.fullscreen().is_some() {
            return None;
        }
        let size = self.window.inner_size();
        let (w, h) = (size.width as f64, size.height as f64);
        let (l, r, t, b) = (
            pos.x < EDGE,
            pos.x > w - EDGE,
            pos.y < EDGE,
            pos.y > h - EDGE,
        );
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

    /// Which view the pointer is over, and where in that view's coordinates.
    fn hit_test(&self) -> (PointerTarget, Option<WebView>, WebViewPoint) {
        let pos = self.cursor.get();
        if self.edge_direction(pos).is_some() {
            return (
                PointerTarget::Edge,
                None,
                servo::DevicePoint::new(0.0, 0.0).into(),
            );
        }
        let (x, y) = (pos.x as f32, pos.y as f32);
        let layout = self.layout.borrow().clone();
        if layout.regions.iter().any(|r| r.contains(x, y)) {
            return (
                PointerTarget::Ui,
                self.ui_webview(),
                servo::DevicePoint::new(x, y).into(),
            );
        }
        // A web panel floats above the page, so it receives input even while the page is
        // covered by the (floating) panel overlay.
        if let Some(rect) = layout.panel
            && !layout.modal
            && rect.contains(x, y)
            && let Some(webview) = self.pane_webview(Pane::Panel)
        {
            let point = servo::DevicePoint::new(x - rect.x, y - rect.y);
            return (
                PointerTarget::Page(Pane::Panel),
                Some(webview),
                point.into(),
            );
        }
        if !layout.overlay {
            let panes = [(Pane::Main, Some(layout.page)), (Pane::Split, layout.split)];
            for (pane, rect) in panes {
                let Some(rect) = rect else { continue };
                if rect.contains(x, y)
                    && let Some(webview) = self.pane_webview(pane)
                {
                    let point = servo::DevicePoint::new(x - rect.x, y - rect.y);
                    return (PointerTarget::Page(pane), Some(webview), point.into());
                }
            }
        }
        (
            PointerTarget::Ui,
            self.ui_webview(),
            servo::DevicePoint::new(x, y).into(),
        )
    }

    fn webview_for_target(&self, target: PointerTarget) -> Option<WebView> {
        match target {
            PointerTarget::Ui => self.ui_webview(),
            PointerTarget::Page(pane) => self.pane_webview(pane),
            PointerTarget::Edge => None,
        }
    }

    fn focus(&self, pane: Option<Pane>) {
        self.keyboard_pane.set(pane);
        for p in [Pane::Main, Pane::Split, Pane::Panel] {
            if let Some(w) = self.pane_webview(p) {
                w.set_focused(pane == Some(p));
            }
        }
        if let Some(ui) = self.ui_webview() {
            ui.set_focused(pane.is_none());
        }
    }

    /// Give keyboard focus to the active page (after navigating from the address bar).
    pub(crate) fn focus_page(&self) {
        if self.pane_webview(Pane::Main).is_some() {
            self.focus(Some(Pane::Main));
        }
    }

    pub(crate) fn focus_ui(&self) {
        self.focus(None);
    }

    fn keyboard_target(&self) -> Option<WebView> {
        match self.keyboard_pane.get() {
            Some(pane) => self.pane_webview(pane).or_else(|| self.ui_webview()),
            None => self.ui_webview(),
        }
    }

    pub fn handle_window_event(&self, event: WindowEvent) {
        match event {
            WindowEvent::ModifiersChanged(m) => self.modifiers.set(m.state()),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor.set(position);
                let previous = self.pointer_target.get();
                let (target, webview, point) = self.hit_test();
                if previous != target
                    && let Some(left) = self.webview_for_target(previous)
                {
                    left.notify_input_event(InputEvent::MouseLeftViewport(
                        MouseLeftViewportEvent::default(),
                    ));
                }
                self.pointer_target.set(target);
                if target == PointerTarget::Edge {
                    let icon = match self.edge_direction(position) {
                        Some(ResizeDirection::East | ResizeDirection::West) => CursorIcon::EwResize,
                        Some(ResizeDirection::North | ResizeDirection::South) => {
                            CursorIcon::NsResize
                        }
                        Some(ResizeDirection::NorthEast | ResizeDirection::SouthWest) => {
                            CursorIcon::NeswResize
                        }
                        _ => CursorIcon::NwseResize,
                    };
                    self.window.set_cursor(icon);
                    return;
                }
                if let Some(webview) = webview {
                    webview.notify_input_event(InputEvent::MouseMove(MouseMoveEvent::new(point)));
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(w) = self.webview_for_target(self.pointer_target.get()) {
                    w.notify_input_event(InputEvent::MouseLeftViewport(
                        MouseLeftViewportEvent::default(),
                    ));
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let (target, webview, point) = self.hit_test();
                if target == PointerTarget::Edge {
                    if state == ElementState::Pressed
                        && button == MouseButton::Left
                        && let Some(dir) = self.edge_direction(self.cursor.get())
                    {
                        let _ = self.window.drag_resize_window(dir);
                    }
                    return;
                }
                if state == ElementState::Pressed {
                    match target {
                        PointerTarget::Page(pane) => {
                            self.note_user_input(&webview);
                            self.focus(Some(pane));
                            self.emit(Event::Open {
                                target: "page-clicked".into(),
                            });
                        }
                        _ => self.focus(None),
                    }
                    // Mouse back/forward buttons navigate the page under the pointer.
                    if let PointerTarget::Page(_) = target
                        && matches!(button, MouseButton::Back | MouseButton::Forward)
                    {
                        let command = if button == MouseButton::Back {
                            Command::Back { tab: None }
                        } else {
                            Command::Forward { tab: None }
                        };
                        self.run_command(command);
                        return;
                    }
                }
                let Some(webview) = webview else { return };
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
                webview.notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(
                    action, button, point,
                )));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (_, webview, point) = self.hit_test();
                let Some(webview) = webview else { return };
                let mods = self.modifiers.get();
                let (x, y, mode) = match delta {
                    MouseScrollDelta::LineDelta(dx, dy) => {
                        ((dx * 76.0) as f64, (dy * 76.0) as f64, WheelMode::DeltaLine)
                    }
                    MouseScrollDelta::PixelDelta(d) => (d.x, d.y, WheelMode::DeltaPixel),
                };
                // Ctrl+wheel zooms the page instead of scrolling it.
                if mods.control_key() && !self.is_ui(&webview) {
                    self.run_command(Command::ZoomStep {
                        delta: if y > 0.0 { 1 } else { -1 },
                    });
                    return;
                }
                webview.notify_input_event(InputEvent::Wheel(WheelEvent::new(
                    WheelDelta { x, y, z: 0.0, mode },
                    point,
                )));
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let keyboard = keyboard_event_from_winit(&event, self.modifiers.get());
                if keyboard.event.state == KeyState::Down
                    && self.try_shortcut(
                        &keyboard.event.key,
                        keyboard.event.modifiers,
                        event.repeat,
                    )
                {
                    return;
                }
                if keyboard.event.state == KeyState::Down && !event.repeat {
                    self.keyboard_sound(&keyboard.event.key);
                }
                log::trace!(
                    "key {:?} {:?} -> pane {:?}",
                    keyboard.event.key,
                    keyboard.event.state,
                    self.keyboard_pane.get()
                );
                if let Some(target) = self.keyboard_target() {
                    if keyboard.event.state == KeyState::Down {
                        self.note_user_input(&Some(target.clone()));
                    }
                    target.notify_input_event(InputEvent::Keyboard(keyboard));
                }
            }
            WindowEvent::Ime(ime) => {
                let Some(target) = self.keyboard_target() else {
                    return;
                };
                let event = match ime {
                    Ime::Enabled => ImeEvent::Composition(servo::CompositionEvent {
                        state: servo::CompositionState::Start,
                        data: String::new(),
                    }),
                    Ime::Preedit(text, _) => ImeEvent::Composition(servo::CompositionEvent {
                        state: servo::CompositionState::Update,
                        data: text,
                    }),
                    Ime::Commit(text) => ImeEvent::Composition(servo::CompositionEvent {
                        state: servo::CompositionState::End,
                        data: text,
                    }),
                    Ime::Disabled => ImeEvent::Dismissed,
                };
                target.notify_input_event(InputEvent::Ime(event));
            }
            WindowEvent::Focused(focused) => {
                self.emit(Event::WindowState {
                    maximized: self.window.is_maximized(),
                    fullscreen: self.window.fullscreen().is_some(),
                    focused,
                });
            }
            WindowEvent::ThemeChanged(theme) => {
                self.system_dark.set(theme == winit::window::Theme::Dark);
                self.settings_changed();
            }
            WindowEvent::DroppedFile(path) => {
                if let Ok(url) = url::Url::from_file_path(&path)
                    && let Some(tab) = self.active_tab_id()
                {
                    self.navigate_tab(&tab, url.as_str(), haku_core::history::Transition::Typed);
                }
            }
            _ => {}
        }
    }

    fn note_user_input(&self, webview: &Option<WebView>) {
        if let Some(w) = webview
            && let Some(tab) = self.tab_for_webview(w)
            && let Some(view) = self.views.borrow_mut().get_mut(&tab)
        {
            view.last_input = Some(std::time::Instant::now());
        }
    }

    fn keyboard_sound(&self, key: &Key) {
        let s = self.settings.borrow();
        if !s.sounds.keyboard_sounds {
            return;
        }
        let typing = matches!(key, Key::Character(_))
            || matches!(
                key,
                Key::Named(NamedKey::Enter | NamedKey::Backspace | NamedKey::Tab)
            );
        if typing {
            self.audio.borrow().key(
                &s.sounds.keyboard_pack,
                s.sounds.keyboard_volume as f32 / 100.0,
            );
        }
    }

    /// Run a browser shortcut if this key matches one. Returns true when consumed.
    fn try_shortcut(&self, key: &Key, mods: Modifiers, repeat: bool) -> bool {
        if self.key_capture.get() {
            return false;
        }
        let Some(name) = key_name(key) else {
            return false;
        };
        // The panic key is configurable and has no modifiers by default.
        {
            let s = self.settings.borrow();
            if s.panic.enabled
                && let Some(acc) = parse_accelerator(&s.panic.key)
                && matches(&acc, &name, mods)
            {
                drop(s);
                if !repeat {
                    self.run_command(Command::Panic);
                }
                return true;
            }
        }
        let overrides = self.settings.borrow().shortcuts.overrides.clone();
        let advanced = self.settings.borrow().shortcuts.advanced_shortcuts;
        // Ctrl+1..9 tab selection.
        if mods == Modifiers::CONTROL
            && name.len() == 1
            && name.chars().all(|c| c.is_ascii_digit())
            && name != "0"
        {
            let n: usize = name.parse().unwrap_or(1);
            self.run_command(Command::SelectNthTab { n });
            return true;
        }
        let command = SHORTCUTS.iter().find_map(|(command, default)| {
            let binding = overrides
                .get(*command)
                .map(String::as_str)
                .unwrap_or(default);
            let acc = parse_accelerator(binding)?;
            matches(&acc, &name, mods).then_some(*command)
        });
        let Some(command) = command else { return false };
        // Single-key shortcuts (no modifiers) can be turned off as "advanced".
        if !advanced
            && !mods.intersects(Modifiers::CONTROL | Modifiers::ALT)
            && !name.starts_with('f')
        {
            return false;
        }
        if repeat
            && !matches!(
                command,
                "next-tab"
                    | "previous-tab"
                    | "next-tab-alt"
                    | "previous-tab-alt"
                    | "zoom-in"
                    | "zoom-in-plus"
                    | "zoom-out"
                    | "find-next"
            )
        {
            return true;
        }
        self.run_shortcut(command);
        true
    }

    pub(crate) fn run_shortcut(&self, command: &str) {
        let open = |target: &str| {
            self.emit(Event::Open {
                target: target.into(),
            })
        };
        match command {
            "new-tab" => self.run_command(Command::NewTab {
                url: None,
                background: false,
                relative_to: None,
            }),
            "close-tab" | "close-tab-alt" => {
                if let Some(tab) = self.active_tab_id() {
                    self.run_command(Command::CloseTab { tab });
                }
            }
            "reopen-tab" => self.run_command(Command::ReopenClosed),
            "next-tab" | "next-tab-alt" => self.run_command(Command::CycleTab { forward: true }),
            "previous-tab" | "previous-tab-alt" => {
                self.run_command(Command::CycleTab { forward: false })
            }
            "focus-address" | "focus-address-alt" | "focus-address-f6" => {
                self.focus_ui();
                open("address");
            }
            "find" | "find-next" => {
                self.focus_ui();
                open("find");
            }
            "reload" | "reload-f5" => self.run_command(Command::Reload {
                tab: None,
                bypass_cache: false,
            }),
            "hard-reload" | "hard-reload-f5" => self.run_command(Command::Reload {
                tab: None,
                bypass_cache: true,
            }),
            "back" => self.run_command(Command::Back { tab: None }),
            "forward" => self.run_command(Command::Forward { tab: None }),
            "home" => {
                if let Some(tab) = self.active_tab_id() {
                    self.navigate_tab(
                        &tab,
                        haku_core::session::START_URL,
                        haku_core::history::Transition::Typed,
                    );
                }
            }
            "bookmark" => self.run_command(Command::ToggleBookmark),
            "history" => open("page:haku://history/"),
            "downloads" => open("page:haku://downloads/"),
            "bookmarks" => open("page:haku://bookmarks/"),
            "settings" => open("page:haku://settings/"),
            "extensions" => open("page:haku://extensions/"),
            "new-window" => self.run_command(Command::Window {
                action: WindowAction::NewWindow,
            }),
            "private-window" => self.run_command(Command::Window {
                action: WindowAction::NewPrivateWindow,
            }),
            "zoom-in" | "zoom-in-plus" => self.run_command(Command::ZoomStep { delta: 1 }),
            "zoom-out" => self.run_command(Command::ZoomStep { delta: -1 }),
            "zoom-reset" => self.run_command(Command::ZoomStep { delta: 0 }),
            "fullscreen" => self.run_command(Command::Window {
                action: WindowAction::ToggleFullscreen,
            }),
            "tab-search" => open("tab-search"),
            "command-palette" => open("command-palette"),
            "save-page" => self.run_command(Command::Page {
                action: haku_core::ipc::PageAction::SavePage,
                tab: None,
                data: None,
            }),
            "view-source" => self.run_command(Command::Page {
                action: haku_core::ipc::PageAction::ViewSource,
                tab: None,
                data: None,
            }),
            "devtools" => open("devtools"),
            "clear-data" => open("clear-data"),
            "toggle-bookmarks-bar" => {
                let on = !self.settings.borrow().appearance.show_bookmarks_bar;
                self.run_command(Command::PatchSettings {
                    patch: serde_json::json!({ "appearance": { "showBookmarksBar": on } }),
                });
            }
            "toggle-sidebar" => {
                let on = !self.settings.borrow().sidebar.show;
                self.run_command(Command::PatchSettings {
                    patch: serde_json::json!({ "sidebar": { "show": on } }),
                });
            }
            "snapshot" => self.run_command(Command::Page {
                action: haku_core::ipc::PageAction::Screenshot,
                tab: None,
                data: None,
            }),
            "mute-tab" => {
                if let Some(tab) = self.active_tab_id() {
                    let muted = self.session.borrow().tab(&tab).is_some_and(|t| t.muted);
                    self.run_command(Command::MuteTab { tab, muted: !muted });
                }
            }
            "easy-setup" => open("easy-setup"),
            "quit" => open("quit"),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerators() {
        assert_eq!(
            parse_accelerator("Ctrl+Shift+T"),
            Some(Accelerator {
                ctrl: true,
                shift: true,
                alt: false,
                key: "t".into()
            })
        );
        assert_eq!(parse_accelerator("F12").unwrap().key, "f12");
        assert_eq!(parse_accelerator("Ctrl++").unwrap().key, "+");
        assert!(parse_accelerator("Hyper+X").is_none());
        let acc = parse_accelerator("Ctrl+=").unwrap();
        assert!(matches(&acc, "=", Modifiers::CONTROL));
        assert!(!matches(&acc, "=", Modifiers::CONTROL | Modifiers::ALT));
        let t = parse_accelerator("Ctrl+T").unwrap();
        assert!(!matches(&t, "t", Modifiers::CONTROL | Modifiers::SHIFT));
    }

    #[test]
    fn default_shortcuts_parse_and_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for (name, acc) in SHORTCUTS {
            let a = parse_accelerator(acc).unwrap_or_else(|| panic!("{name}"));
            assert!(
                seen.insert((a.ctrl, a.shift, a.alt, a.key)),
                "duplicate binding for {name}"
            );
        }
    }
}

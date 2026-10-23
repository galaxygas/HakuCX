/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Engine prompts that the UI shell presents: page dialogs, `<select>` pickers, color pickers,
//! file pickers, context menus, permission and HTTP authentication prompts.
//!
//! Each pending prompt gets a numeric id the UI answers with. Dropping a pending prompt
//! (tab closed, browser shutting down) answers it with Servo's default (dismiss / deny).

use haku_core::ipc::{ContextMenuInfo, DialogKind, Event, Rect, SelectOption};
use haku_core::session::TabId;
use haku_core::settings::PermissionSetting;
use servo::{
    AuthenticationRequest, ContextMenu, ContextMenuAction, EmbedderControl, EmbedderControlId,
    FilePicker, PermissionFeature, PermissionRequest, RgbColor, SelectElementOptionOrOptgroup,
    SimpleDialog, WebView,
};

use super::Browser;

pub enum PendingControl {
    Dialog(SimpleDialog, EmbedderControlId),
    Select(servo::SelectElement, EmbedderControlId),
    Color(servo::ColorPicker, EmbedderControlId),
    ContextMenu(ContextMenu, EmbedderControlId, TabId),
    Permission(PermissionRequest, String, String),
    Auth(AuthenticationRequest),
    File(FilePicker, EmbedderControlId),
    BeforeUnload(servo::AllowOrDenyRequest),
    /// "Open this link in another app?" for a tab.
    External(url::Url, TabId),
}

impl PendingControl {
    fn engine_id(&self) -> Option<&EmbedderControlId> {
        match self {
            PendingControl::Dialog(_, id)
            | PendingControl::Select(_, id)
            | PendingControl::Color(_, id)
            | PendingControl::ContextMenu(_, id, _)
            | PendingControl::File(_, id) => Some(id),
            _ => None,
        }
    }
}

fn permission_kind(feature: PermissionFeature) -> &'static str {
    match feature {
        PermissionFeature::Geolocation => "geolocation",
        PermissionFeature::Notifications | PermissionFeature::Push => "notifications",
        PermissionFeature::Midi => "midi",
        PermissionFeature::Camera => "camera",
        PermissionFeature::Microphone => "microphone",
        PermissionFeature::Speaker => "sound",
        PermissionFeature::DeviceInfo => "device-info",
        PermissionFeature::BackgroundSync => "background-sync",
        PermissionFeature::Bluetooth => "bluetooth",
        PermissionFeature::PersistentStorage => "persistent-storage",
        PermissionFeature::ScreenWakeLock(_) => "wake-lock",
        PermissionFeature::Gamepad => "gamepad",
    }
}

fn origin_of(webview: &WebView) -> String {
    webview
        .url()
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_default()
}

impl Browser {
    /// Convert an engine rect (device pixels inside the page) to UI CSS pixels in the window.
    fn page_rect_to_ui(&self, webview: &WebView, rect: servo::DeviceIntRect) -> Rect {
        let scale = self.ui_scale();
        if self.panel_for_webview(webview).is_some() {
            let origin = self.layout.borrow().clone().panel.unwrap_or_default();
            return Rect {
                x: (origin.x + rect.min.x as f32) / scale,
                y: (origin.y + rect.min.y as f32) / scale,
                w: rect.width() as f32 / scale,
                h: rect.height() as f32 / scale,
            };
        }
        let pane_origin = self
            .tab_for_webview(webview)
            .and_then(|tab| {
                let pane = self.views.borrow().get(&tab).map(|v| v.pane)?;
                let layout = self.layout.borrow().clone();
                Some(match pane {
                    super::Pane::Main => layout.page,
                    super::Pane::Split => layout.split.unwrap_or(layout.page),
                    super::Pane::Panel => layout.panel.unwrap_or(layout.page),
                })
            })
            .unwrap_or_default();
        Rect {
            x: (pane_origin.x + rect.min.x as f32) / scale,
            y: (pane_origin.y + rect.min.y as f32) / scale,
            w: rect.width() as f32 / scale,
            h: rect.height() as f32 / scale,
        }
    }

    /// The tab a prompt belongs to, or `panel:<id>` for sidebar web panels.
    pub(crate) fn prompt_target(&self, webview: &WebView) -> Option<TabId> {
        self.tab_for_webview(webview).or_else(|| {
            self.panel_for_webview(webview)
                .map(|p| format!("{}{p}", super::panels::PANEL_TARGET))
        })
    }

    pub(crate) fn show_control(&self, webview: WebView, control: EmbedderControl) {
        let Some(tab) = self.prompt_target(&webview) else {
            // The UI shell's own <select>/<input type=color> use their HTML implementations;
            // dismiss anything else it triggers.
            return;
        };
        let id = self.next_id();
        // `SimpleDialog`'s own id accessor is private; read the id from the wrapper first.
        let engine_id = control.id();
        match control {
            EmbedderControl::SimpleDialog(dialog) => {
                let (kind, default_value) = match &dialog {
                    SimpleDialog::Alert(_) => (DialogKind::Alert, None),
                    SimpleDialog::Confirm(_) => (DialogKind::Confirm, None),
                    SimpleDialog::Prompt(p) => {
                        (DialogKind::Prompt, Some(p.current_value().to_owned()))
                    }
                };
                self.emit(Event::Dialog {
                    id,
                    tab: tab.clone(),
                    kind,
                    message: dialog.message().chars().take(4000).collect(),
                    default_value,
                    origin: origin_of(&webview),
                });
                self.controls
                    .borrow_mut()
                    .insert(id, PendingControl::Dialog(dialog, engine_id));
                // A dialog belongs to its tab: bring that tab forward.
                if !tab.starts_with(super::panels::PANEL_TARGET)
                    && self.active_tab_id().as_ref() != Some(&tab)
                {
                    self.session.borrow_mut().activate(&tab);
                    self.session_changed();
                }
            }
            EmbedderControl::SelectElement(select) => {
                let mut options = Vec::new();
                for entry in select.options() {
                    match entry {
                        SelectElementOptionOrOptgroup::Option(o) => options.push(SelectOption {
                            index: o.id,
                            label: o.label.clone(),
                            disabled: o.is_disabled,
                            group: None,
                        }),
                        SelectElementOptionOrOptgroup::Optgroup {
                            label,
                            options: inner,
                        } => {
                            for o in inner {
                                options.push(SelectOption {
                                    index: o.id,
                                    label: o.label.clone(),
                                    disabled: o.is_disabled,
                                    group: Some(label.clone()),
                                });
                            }
                        }
                    }
                }
                self.emit(Event::Select {
                    id,
                    tab,
                    rect: self.page_rect_to_ui(&webview, select.position()),
                    options,
                    selected: select.selected_options(),
                    multiple: select.allow_select_multiple(),
                });
                self.controls
                    .borrow_mut()
                    .insert(id, PendingControl::Select(select, engine_id));
            }
            EmbedderControl::ColorPicker(picker) => {
                let color = picker
                    .current_color()
                    .map(|c| format!("#{:02x}{:02x}{:02x}", c.red, c.green, c.blue));
                self.emit(Event::ColorPicker {
                    id,
                    tab,
                    rect: self.page_rect_to_ui(&webview, picker.position()),
                    color,
                });
                self.controls
                    .borrow_mut()
                    .insert(id, PendingControl::Color(picker, engine_id));
            }
            EmbedderControl::ContextMenu(menu) => {
                let info = menu.element_info();
                let rect = self.page_rect_to_ui(&webview, menu.position());
                let flags = info.flags;
                self.emit(Event::ContextMenu {
                    menu: ContextMenuInfo {
                        id,
                        tab: tab.clone(),
                        x: rect.x,
                        y: rect.y,
                        link_url: info.link_url.as_ref().map(|u| u.to_string()),
                        image_url: info.image_url.as_ref().map(|u| u.to_string()),
                        editable: flags
                            .contains(servo::ContextMenuElementInformationFlags::EditableText),
                        selection: flags
                            .contains(servo::ContextMenuElementInformationFlags::Selection),
                        can_go_back: webview.can_go_back(),
                        can_go_forward: webview.can_go_forward(),
                    },
                });
                self.controls
                    .borrow_mut()
                    .insert(id, PendingControl::ContextMenu(menu, engine_id, tab));
            }
            EmbedderControl::FilePicker(picker) => {
                // The native file dialog is modal; open it outside the Servo callback.
                self.controls
                    .borrow_mut()
                    .insert(id, PendingControl::File(picker, engine_id));
                self.defer(move |b| b.run_file_picker(id));
            }
            EmbedderControl::InputMethod(ime) => {
                let rect = self.page_rect_to_ui(&webview, ime.position());
                let scale = self.ui_scale() as f64;
                self.window.set_ime_allowed(true);
                self.window.set_ime_cursor_area(
                    winit::dpi::PhysicalPosition::new(rect.x as f64 * scale, rect.y as f64 * scale),
                    winit::dpi::PhysicalSize::new(rect.w as f64 * scale, rect.h as f64 * scale),
                );
            }
        }
    }

    pub(crate) fn hide_control(&self, engine_id: EmbedderControlId) {
        let ours = self
            .controls
            .borrow()
            .iter()
            .find(|(_, c)| c.engine_id() == Some(&engine_id))
            .map(|(id, _)| *id);
        if let Some(id) = ours {
            self.controls.borrow_mut().remove(&id);
            self.emit(Event::HideControl { id });
        } else {
            self.window.set_ime_allowed(false);
        }
    }

    fn run_file_picker(&self, id: u32) {
        let Some(PendingControl::File(mut picker, _)) = self.controls.borrow_mut().remove(&id)
        else {
            return;
        };
        let mut dialog = rfd::FileDialog::new().set_parent(&self.window);
        for pattern in picker.filter_patterns() {
            let ext = pattern.0.trim_start_matches('.').to_owned();
            if !ext.is_empty() && !ext.contains('/') {
                dialog = dialog.add_filter(ext.clone(), &[ext]);
            }
        }
        let paths = if picker.allow_select_multiple() {
            dialog.pick_files().unwrap_or_default()
        } else {
            dialog.pick_file().into_iter().collect()
        };
        if paths.is_empty() {
            picker.dismiss();
        } else {
            picker.select(&paths);
            picker.submit();
        }
    }

    pub(crate) fn request_permission_prompt(&self, webview: WebView, request: PermissionRequest) {
        let origin = origin_of(&webview);
        let kind = permission_kind(request.feature()).to_owned();
        let decision = self.settings.borrow().permissions.effective(&origin, &kind);
        match decision {
            PermissionSetting::Allow => request.allow(),
            PermissionSetting::Block => request.deny(),
            PermissionSetting::Ask => {
                let Some(tab) = self.prompt_target(&webview) else {
                    request.deny();
                    return;
                };
                let id = self.next_id();
                self.emit(Event::Permission {
                    id,
                    tab,
                    origin: origin.clone(),
                    kind: kind.clone(),
                });
                self.controls
                    .borrow_mut()
                    .insert(id, PendingControl::Permission(request, origin, kind));
            }
        }
    }

    pub(crate) fn request_auth_prompt(&self, webview: WebView, request: AuthenticationRequest) {
        let Some(tab) = self.prompt_target(&webview) else {
            return;
        };
        let id = self.next_id();
        self.emit(Event::Auth {
            id,
            tab,
            url: request.url().to_string(),
            proxy: request.for_proxy(),
        });
        self.controls
            .borrow_mut()
            .insert(id, PendingControl::Auth(request));
    }

    pub(crate) fn confirm_external(&self, tab: &TabId, url: url::Url, origin: String) {
        let id = self.next_id();
        self.emit(Event::Dialog {
            id,
            tab: tab.clone(),
            kind: DialogKind::Confirm,
            message: format!(
                "Open this {} link in another app?\n\n{}",
                url.scheme(),
                url.as_str().chars().take(300).collect::<String>()
            ),
            default_value: None,
            origin,
        });
        self.controls
            .borrow_mut()
            .insert(id, PendingControl::External(url, tab.clone()));
    }

    pub(crate) fn request_before_unload(
        &self,
        webview: WebView,
        request: servo::AllowOrDenyRequest,
    ) {
        let Some(tab) = self.tab_for_webview(&webview) else {
            request.allow();
            return;
        };
        let id = self.next_id();
        self.emit(Event::Dialog {
            id,
            tab,
            kind: DialogKind::BeforeUnload,
            message: "This page is asking you to confirm that you want to leave. Changes you made may not be saved.".into(),
            default_value: None,
            origin: origin_of(&webview),
        });
        self.controls
            .borrow_mut()
            .insert(id, PendingControl::BeforeUnload(request));
    }

    // ------------------------------------------------------------------ responses from the UI

    pub(crate) fn answer_dialog(&self, id: u32, confirmed: bool, value: Option<String>) {
        // Take the control out first: answering it may re-enter code that uses `controls`.
        let control = self.controls.borrow_mut().remove(&id);
        match control {
            Some(PendingControl::Dialog(dialog, _)) => match (dialog, confirmed) {
                (SimpleDialog::Prompt(mut p), true) => {
                    if let Some(v) = value {
                        p.set_current_value(&v);
                    }
                    p.confirm();
                }
                (d, true) => d.confirm(),
                (d, false) => d.dismiss(),
            },
            Some(PendingControl::BeforeUnload(request)) => {
                if confirmed {
                    request.allow()
                } else {
                    request.deny()
                }
            }
            Some(PendingControl::External(url, tab)) if confirmed => {
                if let Err(e) = open::that_detached(url.as_str()) {
                    self.toast(format!("No app could open that link: {e}"), "error");
                }
                // A tab that exists only for this link (no page loaded) is closed.
                let blank = self
                    .session
                    .borrow()
                    .tab(&tab)
                    .is_some_and(|t| t.url == "about:blank");
                if blank && self.settings.borrow().tabs.close_external_app_tabs {
                    self.session.borrow_mut().close(&tab);
                    self.session_changed();
                }
            }
            _ => {}
        }
    }

    pub(crate) fn answer_select(&self, id: u32, selected: Vec<usize>) {
        if let Some(PendingControl::Select(mut select, _)) = self.controls.borrow_mut().remove(&id)
        {
            select.select(selected);
            select.submit();
        }
    }

    pub(crate) fn answer_color(&self, id: u32, color: Option<String>) {
        if let Some(PendingControl::Color(mut picker, _)) = self.controls.borrow_mut().remove(&id) {
            let rgb = color
                .as_deref()
                .and_then(haku_core::themes::Rgb::parse)
                .map(|c| RgbColor {
                    red: c.0,
                    green: c.1,
                    blue: c.2,
                });
            picker.select(rgb);
            picker.submit();
        }
    }

    /// The UI closed its context menu without choosing an engine action.
    pub(crate) fn context_menu_closed(&self, id: u32) {
        if let Some(PendingControl::ContextMenu(menu, _, _)) =
            self.controls.borrow_mut().remove(&id)
        {
            menu.dismiss();
        }
    }

    /// Run one of the engine's own context-menu actions for the open menu (if any).
    pub(crate) fn context_menu_engine_action(&self, action: ContextMenuAction) -> bool {
        let id = self
            .controls
            .borrow()
            .iter()
            .find(|(_, c)| matches!(c, PendingControl::ContextMenu(..)))
            .map(|(id, _)| *id);
        let Some(id) = id else { return false };
        if let Some(PendingControl::ContextMenu(menu, _, _)) =
            self.controls.borrow_mut().remove(&id)
        {
            menu.select(action);
            return true;
        }
        false
    }

    pub(crate) fn answer_permission(&self, id: u32, allow: bool, remember: bool) {
        if let Some(PendingControl::Permission(request, origin, kind)) =
            self.controls.borrow_mut().remove(&id)
        {
            if allow {
                request.allow();
            } else {
                request.deny();
            }
            if remember && !self.private {
                self.settings
                    .borrow_mut()
                    .permissions
                    .sites
                    .entry(origin)
                    .or_default()
                    .insert(
                        kind,
                        if allow {
                            PermissionSetting::Allow
                        } else {
                            PermissionSetting::Block
                        },
                    );
                self.settings_changed();
            }
        }
    }

    pub(crate) fn answer_auth(&self, id: u32, username: Option<String>, password: Option<String>) {
        if let Some(PendingControl::Auth(request)) = self.controls.borrow_mut().remove(&id)
            && let (Some(user), Some(pass)) = (username, password)
        {
            request.authenticate(user, pass);
        }
        // Dropping the request without authenticating cancels it.
    }

    /// Dismiss every prompt that belongs to a tab (it was closed or navigated away).
    pub(crate) fn drop_controls_for_tab(&self, tab: &TabId) {
        let ids: Vec<u32> = self
            .controls
            .borrow()
            .iter()
            .filter(|(_, c)| matches!(c, PendingControl::ContextMenu(_, _, t) if t == tab))
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            self.controls.borrow_mut().remove(&id);
            self.emit(Event::HideControl { id });
        }
    }
}

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! `WebViewDelegate`: Servo tells us what happened. These methods record facts and queue work;
//! anything that calls back into Servo is deferred to `Browser::pump`.

use haku_core::ipc::Event;
use haku_core::session::{NewTab, Placement};
use log::{info, warn};
use servo::{
    AllowOrDenyRequest, AuthenticationRequest, ConsoleLogLevel, CreateNewWebViewRequest,
    EmbedderControl, EmbedderControlId, LoadStatus, NavigationRequest, PermissionRequest,
    WebResourceLoad, WebView, WebViewDelegate,
};
use url::Url;

use super::tabs::{Pane, TabView, is_internal};
use super::{Browser, IPC_PREFIX, PAGE_PREFIX, page_scripts};

impl WebViewDelegate for Browser {
    fn notify_new_frame_ready(&self, webview: WebView) {
        log::trace!("frame ready (ui: {})", self.is_ui(&webview));
        self.window.request_redraw();
    }

    fn notify_animating_changed(&self, webview: WebView, animating: bool) {
        log::trace!("animating {animating} (ui: {})", self.is_ui(&webview));
        let mut set = self.animating.borrow_mut();
        if animating {
            set.insert(webview.id());
        } else {
            set.remove(&webview.id());
        }
    }

    fn notify_url_changed(&self, webview: WebView, url: Url) {
        let Some(tab) = self.tab_for_webview(&webview) else {
            return;
        };
        let url_s = url.to_string();
        self.drop_controls_for_tab(&tab);
        if let Some(view) = self.views.borrow_mut().get_mut(&tab) {
            // The page committed a navigation, so whatever internal page was on top is gone.
            view.internal = None;
            view.blocked = 0;
        }
        self.session
            .borrow_mut()
            .update_page(&tab, Some(&url_s), None);
        self.record_history(&tab, &url_s);
        self.session_dirty.set(true);
        self.after_tab_update(&tab);
    }

    fn notify_page_title_changed(&self, webview: WebView, title: Option<String>) {
        if let Some(panel) = self.panel_for_webview(&webview) {
            self.panel_title_changed(&panel, title.as_deref().unwrap_or_default());
            return;
        }
        let Some(tab) = self.tab_for_webview(&webview) else {
            return;
        };
        let title = title.unwrap_or_default();
        let url = webview.url().map(|u| u.to_string()).unwrap_or_default();
        self.session
            .borrow_mut()
            .update_page(&tab, None, Some(&title));
        if !self.private
            && let Err(e) = self.history.update_title(&url, &title)
        {
            warn!("history title: {e}");
        }
        self.session_dirty.set(true);
        self.after_tab_update(&tab);
    }

    fn notify_status_text_changed(&self, webview: WebView, status: Option<String>) {
        let Some(tab) = self.tab_for_webview(&webview) else {
            return;
        };
        if let Some(view) = self.views.borrow_mut().get_mut(&tab) {
            view.status = status;
        }
        self.after_tab_update(&tab);
    }

    fn notify_load_status_changed(&self, webview: WebView, status: LoadStatus) {
        if self.ext_is_page(&webview) {
            if status == LoadStatus::Complete {
                self.defer(move |b| b.ext_page_loaded(&webview));
            }
            return;
        }
        if let Some(panel) = self.panel_for_webview(&webview) {
            self.panel_state(&panel, &webview, status != LoadStatus::Complete);
            return;
        }
        let Some(tab) = self.tab_for_webview(&webview) else {
            return;
        };
        self.set_loading(&tab, status);
        if status == LoadStatus::Started {
            // The old document's content scripts and focused fields are gone.
            let navigated = tab.clone();
            self.defer(move |b| {
                b.ext_tab_navigated(&navigated);
                b.vault_hide_suggestions();
            });
        }
        if status == LoadStatus::HeadParsed || status == LoadStatus::Complete {
            let tab_for_style = tab.clone();
            self.defer(move |b| b.apply_page_style(&tab_for_style));
        }
        if status == LoadStatus::HeadParsed {
            // Servo renders every response as a document; a response that is not a page is a
            // download.
            let browser = self.rc();
            let tab2 = tab.clone();
            let wv = webview.clone();
            self.defer(move |b| {
                wv.evaluate_javascript(page_scripts::CONTENT_TYPE, move |result| {
                    if let Ok(servo::JSValue::String(mime)) = result
                        && !page_scripts::renderable_mime(&mime)
                    {
                        let tab3 = tab2.clone();
                        browser.defer(move |b| b.divert_to_download(&tab3, &mime));
                    }
                });
                let _ = b;
            });
        }
    }

    fn notify_cursor_changed(&self, webview: WebView, cursor: servo::Cursor) {
        let wants = match self.pointer_target.get() {
            super::PointerTarget::Ui => self.is_ui(&webview),
            super::PointerTarget::Page(pane) => self
                .pane_webview(pane)
                .is_some_and(|w| w.id() == webview.id()),
            super::PointerTarget::Edge => false,
        };
        if wants {
            self.window.set_cursor(super::input::winit_cursor(cursor));
        }
    }

    fn notify_favicon_changed(&self, webview: WebView) {
        let Some(tab) = self.tab_for_webview(&webview) else {
            return;
        };
        let data_url = webview
            .favicon()
            .and_then(|image| crate::favicon::to_data_url(&image));
        if let Some(view) = self.views.borrow_mut().get_mut(&tab) {
            view.favicon = data_url;
        }
        self.after_tab_update(&tab);
    }

    fn notify_history_changed(&self, webview: WebView, entries: Vec<Url>, current: usize) {
        let Some(tab) = self.tab_for_webview(&webview) else {
            return;
        };
        let can_back = current > 0;
        let can_forward = current + 1 < entries.len();
        if let Some(view) = self.views.borrow_mut().get_mut(&tab) {
            view.can_go_back = can_back;
            view.can_go_forward = can_forward;
        }
        self.session.borrow_mut().update_history(
            &tab,
            entries.iter().map(|u| u.to_string()).collect(),
            current,
        );
        self.session_dirty.set(true);
        self.after_tab_update(&tab);
    }

    fn notify_closed(&self, webview: WebView) {
        if self.ext_is_page(&webview) {
            self.defer(move |b| {
                b.ext_webview_closed(&webview);
            });
            return;
        }
        // `window.close()` from a page.
        if let Some(tab) = self.tab_for_webview(&webview) {
            self.defer(move |b| {
                b.session.borrow_mut().close(&tab);
                b.session_changed();
            });
        }
    }

    fn notify_crashed(&self, webview: WebView, reason: String, _backtrace: Option<String>) {
        warn!("page crashed: {reason}");
        let Some(tab) = self.tab_for_webview(&webview) else {
            return;
        };
        if let Some(view) = self.views.borrow_mut().get_mut(&tab) {
            view.crashed = Some(reason);
            view.loading = false;
        }
        self.after_tab_update(&tab);
    }

    fn notify_fullscreen_state_changed(&self, _webview: WebView, fullscreen: bool) {
        self.defer(move |b| b.set_page_fullscreen(fullscreen));
    }

    fn request_navigation(&self, webview: WebView, request: NavigationRequest) {
        let url = request.url.clone();
        // Links to other apps (mailto:, steam:, zoommtg:, ...) open outside the browser, after
        // the user confirms, like every major browser.
        if !matches!(
            url.scheme(),
            "http" | "https" | "file" | "data" | "blob" | "about" | "haku" | "view-source"
        ) {
            request.deny();
            if let Some(tab) = self.prompt_target(&webview) {
                let origin = webview
                    .url()
                    .map(|u| u.origin().ascii_serialization())
                    .unwrap_or_default();
                self.defer(move |b| b.confirm_external(&tab, url, origin));
            }
            return;
        }
        // Web content may never navigate to internal pages.
        if is_internal(url.as_str()) {
            warn!("blocked a page navigating to an internal URL");
            request.deny();
            return;
        }
        if !self.ext_navigation_allowed(webview.url().as_ref(), &url) {
            warn!("blocked a page navigating to an extension page it may not open");
            request.deny();
            return;
        }
        if page_scripts::download_extension(&url)
            && let Some(tab) = self
                .tab_for_webview(&webview)
                .or_else(|| self.active_tab_id())
        {
            request.deny();
            let referrer = webview.url().map(|u| u.to_string());
            self.defer(move |b| {
                let _ = b.start_download(url.to_string(), None, referrer, Some(tab));
            });
            return;
        }
        request.allow();
    }

    fn request_create_new(&self, parent: WebView, request: CreateNewWebViewRequest) {
        // window.open() / target=_blank: a new tab next to the opener. Links from a web panel
        // open as ordinary tabs after the active one.
        let parent_tab = match self.tab_for_webview(&parent) {
            Some(t) => t,
            None if self.panel_for_webview(&parent).is_some() || self.ext_is_page(&parent) => {
                match self.active_tab_id() {
                    Some(t) => t,
                    None => return,
                }
            }
            None => return,
        };
        // Pop-up blocker: Servo does not track user activation for the browser, so HakuCX
        // allows a new window only right after a real click or key press in the opener.
        let opener_host = parent
            .url()
            .and_then(|u| {
                u.host_str()
                    .map(|h| h.trim_start_matches("www.").to_owned())
            })
            .unwrap_or_default();
        let (block, allowlisted) = {
            let s = self.settings.borrow();
            (
                s.privacy.block_popups,
                s.privacy
                    .popup_allowlist
                    .iter()
                    .any(|d| opener_host == *d || opener_host.ends_with(&format!(".{d}"))),
            )
        };
        let recent_input = self
            .views
            .borrow()
            .get(&parent_tab)
            .and_then(|v| v.last_input)
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(1500));
        if block
            && !allowlisted
            && !recent_input
            && self.panel_for_webview(&parent).is_none()
            && !self.ext_is_page(&parent)
        {
            log::info!("blocked a pop-up without user activation");
            self.emit(Event::PopupBlocked {
                tab: parent_tab.clone(),
                host: opener_host,
            });
            return;
        }
        let (pane, auto_islands) = (
            self.views
                .borrow()
                .get(&parent_tab)
                .map(|v| v.pane)
                .unwrap_or(Pane::Main),
            self.settings.borrow().tabs.auto_islands,
        );
        let tab_id = self.session.borrow_mut().new_tab(NewTab {
            url: "about:blank".into(),
            background: false,
            workspace: None,
            relative_to: Some(parent_tab),
            placement: Placement::AfterOpenerChildren,
            from_link: auto_islands,
        });
        let content_manager = std::rc::Rc::new(servo::UserContentManager::new(&self.servo));
        page_scripts::install(&content_manager, &self.settings.borrow());
        self.add_extension_scripts(&content_manager);
        let webview = request
            .builder(self.pane_context(pane))
            .hidpi_scale_factor(euclid::Scale::new(self.scale()))
            .user_content_manager(content_manager.clone())
            .delegate(self.rc())
            .build();
        let mut view = TabView::new(pane);
        view.loading = true;
        view.webview = Some(webview.clone());
        self.views.borrow_mut().insert(tab_id.clone(), view);
        self.register_webview(&tab_id, &webview, content_manager);
        self.defer(|b| b.session_changed());
    }

    fn request_permission(&self, webview: WebView, request: PermissionRequest) {
        self.request_permission_prompt(webview, request);
    }

    fn request_authentication(&self, webview: WebView, request: AuthenticationRequest) {
        self.request_auth_prompt(webview, request);
    }

    fn request_unload(&self, webview: WebView, request: AllowOrDenyRequest) {
        self.request_before_unload(webview, request);
    }

    fn show_embedder_control(&self, webview: WebView, control: EmbedderControl) {
        if self.is_ui(&webview) {
            // The UI shell renders its own controls in HTML; let Servo's defaults apply.
            return;
        }
        self.show_control(webview, control);
    }

    fn hide_embedder_control(&self, _webview: WebView, control_id: EmbedderControlId) {
        self.hide_control(control_id);
    }

    fn load_web_resource(&self, webview: WebView, load: WebResourceLoad) {
        if let Some(id) = crate::extensions::id_of_url(&load.request().url) {
            self.serve_extension(&webview, load, &id);
            return;
        }
        let Some(load) = self.filter_request(webview.clone(), load) else {
            return;
        };
        if let Some(tab) = self.tab_for_webview(&webview) {
            // Dropping the load lets the request continue.
            drop(self.ext_filter_request(&tab, &webview, load));
        }
    }

    fn show_notification(&self, webview: WebView, notification: servo::Notification) {
        let origin = webview
            .url()
            .map(|u| u.host_str().unwrap_or_default().to_owned())
            .unwrap_or_default();
        self.emit(Event::Toast {
            text: format!("{origin}: {} {}", notification.title, notification.body)
                .trim()
                .to_owned(),
            kind: "notification".into(),
        });
    }

    fn show_console_message(&self, webview: WebView, level: ConsoleLogLevel, message: String) {
        if let Some(payload) = message.strip_prefix(IPC_PREFIX) {
            if self.is_ui(&webview) {
                self.inbox.borrow_mut().push(payload.to_owned());
                // Make sure the event loop turns over so the message is handled promptly.
                self.window.request_redraw();
            } else {
                warn!(
                    "rejected IPC from a web page ({:?})",
                    webview.url().map(|u| u.origin().ascii_serialization())
                );
            }
            return;
        }
        if Self::is_ext_message(&message) {
            self.defer(move |b| b.handle_ext_message(webview, &message));
            return;
        }
        if matches!(level, ConsoleLogLevel::Error) && self.ext_is_page(&webview) {
            self.defer(move |b| b.ext_note_error(&webview, &message));
            return;
        }
        if let Some(payload) = message.strip_prefix(PAGE_PREFIX) {
            if let Some(tab) = self.tab_for_webview(&webview) {
                let payload = payload.to_owned();
                self.defer(move |b| b.handle_page_message(&tab, &payload));
            }
            return;
        }
        if self.is_ui(&webview) {
            info!("[ui {level:?}] {message}");
        }
    }
}

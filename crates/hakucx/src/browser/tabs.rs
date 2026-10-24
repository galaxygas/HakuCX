/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Tabs <-> webviews.
//!
//! A tab whose current URL is internal (`haku://...`) has no page webview: the UI shell renders
//! that page itself inside the viewport, so internal pages never run in a web content context
//! and never need privileges of their own. A tab showing a web page owns a Servo webview that
//! lives in one of the two pane rendering contexts.
//!
//! Tabs are created lazily: restored and background tabs get a webview only when first shown,
//! which keeps startup fast regardless of how many tabs a session has.

use std::rc::Rc;

use haku_core::history::Transition;
use haku_core::ipc::{Event, Security, TabMeta};
use haku_core::omnibox;
use haku_core::session::{NewTab, START_URL, Session, TabId};
use haku_core::settings::{Settings, StartupMode};
use log::warn;
use servo::{LoadStatus, UserContentManager, WebView, WebViewBuilder};
use url::Url;

use super::{Browser, page_scripts};
use crate::texture_context::TextureRenderingContext;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pane {
    Main = 0,
    Split = 1,
    /// Sidebar web panels (messengers, social, AI).
    Panel = 2,
    /// Extension popups, drawn above the UI shell.
    Popup = 3,
}

impl Pane {
    pub fn index(self) -> usize {
        self as usize
    }
}

/// Engine-side state for one tab.
pub struct TabView {
    pub webview: Option<WebView>,
    /// Which painter the webview belongs to (fixed for the webview's lifetime).
    pub pane: Pane,
    /// Internal page shown on top of (or instead of) the webview.
    pub internal: Option<String>,
    /// The internal page that was showing before this tab's first web page, so Back can return
    /// to it (Start page -> site -> Back).
    pub internal_before_web: Option<String>,
    pub loading: bool,
    pub favicon: Option<String>,
    pub status: Option<String>,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub blocked: u32,
    pub crashed: Option<String>,
    pub audible: bool,
    pub content_manager: Option<Rc<UserContentManager>>,
    /// Pending `Transition` for the next committed navigation (typed URL vs link).
    pub next_transition: Transition,
    /// When the user last clicked or typed in this tab's page (popup blocking).
    pub last_input: Option<std::time::Instant>,
}

impl TabView {
    pub(crate) fn new(pane: Pane) -> Self {
        Self {
            webview: None,
            pane,
            internal: None,
            internal_before_web: None,
            loading: false,
            favicon: None,
            status: None,
            can_go_back: false,
            can_go_forward: false,
            blocked: 0,
            crashed: None,
            audible: false,
            content_manager: None,
            next_transition: Transition::Link,
            last_input: None,
        }
    }
}

pub fn is_internal(url: &str) -> bool {
    url.starts_with("haku:") || url.starts_with("about:")
}

/// Pick the starting session according to the "On startup" settings.
pub fn restore_session(saved: Option<Option<Session>>, settings: &Settings) -> Session {
    let saved = saved.flatten();
    match (settings.startup.mode, saved) {
        (StartupMode::RestoreSession, Some(mut session)) => {
            session.repair();
            for tab in session.tabs.iter_mut() {
                // Every restored tab starts snoozed and loads when first shown.
                tab.snoozed = true;
            }
            if settings.startup.show_start_page_first {
                session.new_tab(NewTab::url(START_URL));
            }
            session
        }
        (StartupMode::SpecificPages, saved) => {
            let mut session = saved
                .map(|mut s| {
                    s.repair();
                    s.tabs.clear();
                    s.active_tabs.clear();
                    s.islands.clear();
                    s.closed.clear();
                    s.split = None;
                    s.mru.clear();
                    s
                })
                .unwrap_or_default();
            let pages: Vec<String> = settings
                .startup
                .pages
                .iter()
                .filter(|p| Url::parse(p).is_ok())
                .cloned()
                .collect();
            if pages.is_empty() {
                session.new_tab(NewTab::url(START_URL));
            }
            for page in pages {
                session.new_tab(NewTab::url(page));
            }
            session.repair();
            session
        }
        (_, saved) => {
            let mut session = saved
                .map(|mut s| {
                    // Keep workspaces, start fresh tabs.
                    s.repair();
                    s.tabs.clear();
                    s.active_tabs.clear();
                    s.islands.clear();
                    s.split = None;
                    s.mru.clear();
                    s
                })
                .unwrap_or_default();
            session.repair();
            session
        }
    }
}

impl Browser {
    // ------------------------------------------------------------------ reconciliation

    /// Make the engine match the session: drop webviews of closed tabs, show the active tab
    /// (creating its webview if needed), hide the rest, and size panes to the layout.
    pub(crate) fn reconcile(&self) {
        // A visible tab is never snoozed (restored tabs start snoozed and wake when shown).
        {
            let mut session = self.session.borrow_mut();
            let visible: Vec<TabId> = match &session.split {
                Some((l, r)) => vec![l.clone(), r.clone()],
                None => session.active_tab_id().cloned().into_iter().collect(),
            };
            for tab in session.tabs.iter_mut() {
                if visible.contains(&tab.id) {
                    tab.snoozed = false;
                }
            }
        }
        let session = self.session.borrow().clone();

        // Drop views of tabs that no longer exist, and of snoozed tabs.
        let gone: Vec<TabId> = self
            .views
            .borrow()
            .keys()
            .filter(|id| session.tab(id).is_none_or(|t| t.snoozed))
            .cloned()
            .collect();
        for id in gone {
            let closed = session.tab(&id).is_none();
            if let Some(view) = self.views.borrow_mut().remove(&id)
                && let Some(webview) = view.webview
            {
                self.by_webview.borrow_mut().remove(&webview.id());
                self.animating.borrow_mut().remove(&webview.id());
            }
            if closed {
                self.emit(Event::TabClosed { tab: id });
            }
        }

        let visible: [Option<TabId>; 2] = match &session.split {
            Some((left, right)) => [Some(left.clone()), Some(right.clone())],
            None => [session.active_tab_id().cloned(), None],
        };

        // Make sure visible tabs have their engine state, in the right pane.
        for (pane, tab) in [Pane::Main, Pane::Split].into_iter().zip(visible.iter()) {
            let Some(tab_id) = tab else { continue };
            let Some(tab) = session.tab(tab_id) else {
                continue;
            };
            let needs_view = !self.views.borrow().contains_key(tab_id);
            if needs_view {
                let mut view = TabView::new(pane);
                if is_internal(&tab.url) {
                    view.internal = Some(tab.url.clone());
                } else {
                    view.webview =
                        Some(self.create_webview(tab_id, pane, Url::parse(&tab.url).ok()));
                }
                self.views.borrow_mut().insert(tab_id.clone(), view);
            } else {
                // Split screen needs the two tabs in different painters; move the right-hand
                // tab's page to the split painter if it lives in the main one.
                let wrong_pane = self
                    .views
                    .borrow()
                    .get(tab_id)
                    .is_some_and(|v| v.webview.is_some() && v.pane != pane);
                let other_pane_tab = visible[1 - pane.index()].as_ref();
                let conflict = wrong_pane
                    && other_pane_tab.is_some_and(|other| {
                        self.views
                            .borrow()
                            .get(other)
                            .is_some_and(|v| v.webview.is_some() && v.pane != pane)
                    });
                if conflict {
                    self.recreate_in_pane(tab_id, pane, &tab.url);
                }
            }
        }

        // Show visible webviews; hide everything else.
        let views = self.views.borrow();
        let mut pane_tabs = [None, None];
        for (id, view) in views.iter() {
            let Some(webview) = &view.webview else {
                continue;
            };
            let shown = visible.iter().flatten().any(|v| v == id) && view.internal.is_none();
            if shown {
                webview.show();
                pane_tabs[view.pane.index()] = Some(id.clone());
            } else {
                webview.hide();
                webview.set_focused(false);
            }
        }
        drop(views);
        *self.pane_tabs.borrow_mut() = pane_tabs;
        self.resize_panes();
        self.apply_mute_state();
        self.window.request_redraw();
    }

    fn recreate_in_pane(&self, tab_id: &TabId, pane: Pane, url: &str) {
        let mut views = self.views.borrow_mut();
        let Some(view) = views.get_mut(tab_id) else {
            return;
        };
        if let Some(old) = view.webview.take() {
            self.by_webview.borrow_mut().remove(&old.id());
            self.animating.borrow_mut().remove(&old.id());
        }
        view.pane = pane;
        drop(views);
        let webview = self.create_webview(tab_id, pane, Url::parse(url).ok());
        if let Some(view) = self.views.borrow_mut().get_mut(tab_id) {
            view.webview = Some(webview);
        }
    }

    pub(crate) fn pane_context(&self, pane: Pane) -> Rc<TextureRenderingContext> {
        self.pane_contexts[pane.index()].clone()
    }

    fn create_webview(&self, tab_id: &TabId, pane: Pane, url: Option<Url>) -> WebView {
        let content_manager = Rc::new(UserContentManager::new(&self.servo));
        page_scripts::install(&content_manager, &self.settings.borrow());
        self.add_extension_scripts(&content_manager);
        let mut builder = WebViewBuilder::new(&self.servo, self.pane_context(pane))
            .hidpi_scale_factor(euclid::Scale::new(self.scale()))
            .user_content_manager(content_manager.clone())
            .delegate(self.rc());
        if let Some(url) = url {
            builder = builder.url(url);
        }
        let webview = builder.build();
        self.register_webview(tab_id, &webview, content_manager);
        webview
    }

    pub(crate) fn register_webview(
        &self,
        tab_id: &TabId,
        webview: &WebView,
        content_manager: Rc<UserContentManager>,
    ) {
        self.by_webview
            .borrow_mut()
            .insert(webview.id(), tab_id.clone());
        if let Some(view) = self.views.borrow_mut().get_mut(tab_id) {
            view.content_manager = Some(content_manager);
        }
        if let Some(tab) = self.session.borrow().tab(tab_id)
            && (tab.zoom - 1.0).abs() > f32::EPSILON
        {
            webview.set_page_zoom(tab.zoom);
        }
    }

    pub(crate) fn tab_for_webview(&self, webview: &WebView) -> Option<TabId> {
        self.by_webview.borrow().get(&webview.id()).cloned()
    }

    pub(crate) fn webview_for_tab(&self, tab: &str) -> Option<WebView> {
        self.views.borrow().get(tab).and_then(|v| v.webview.clone())
    }

    pub(crate) fn active_tab_id(&self) -> Option<TabId> {
        self.session.borrow().active_tab_id().cloned()
    }

    /// The tab a command applies to: the given one, or the active one.
    pub(crate) fn target_tab(&self, tab: Option<TabId>) -> Option<TabId> {
        tab.or_else(|| self.active_tab_id())
    }

    /// The webview showing in a pane right now (none if the pane shows an internal page).
    pub(crate) fn pane_webview(&self, pane: Pane) -> Option<WebView> {
        if pane == Pane::Popup {
            return self.ext_popup_webview();
        }
        if pane == Pane::Panel {
            let id = self.visible_panel.borrow().clone()?;
            return self.web_panels.borrow().get(&id).cloned();
        }
        let tab = self.pane_tabs.borrow()[pane.index()].clone()?;
        let views = self.views.borrow();
        let view = views.get(&tab)?;
        if view.internal.is_some() {
            return None;
        }
        view.webview.clone()
    }

    pub(crate) fn resize_panes(&self) {
        let layout = self.layout.borrow().clone();
        let rects = [Some(layout.page), layout.split];
        for pane in [Pane::Main, Pane::Split] {
            let Some(rect) = rects[pane.index()] else {
                continue;
            };
            if let Some(webview) = self.pane_webview(pane) {
                let size =
                    winit::dpi::PhysicalSize::new(rect.w.max(1.0) as u32, rect.h.max(1.0) as u32);
                // Resize through the webview: Servo skips its viewport update if the context
                // already has the new size.
                webview.resize(size);
            }
        }
    }

    fn apply_mute_state(&self) {
        // Servo 0.7 has no per-webview audio mute API; the session keeps the flag so the UI
        // shows it, and media elements are paused through a page script instead.
        let session = self.session.borrow();
        for (id, view) in self.views.borrow().iter() {
            let (Some(tab), Some(webview)) = (session.tab(id), &view.webview) else {
                continue;
            };
            let script = if tab.muted {
                page_scripts::MUTE_ALL
            } else {
                page_scripts::UNMUTE_ALL
            };
            webview.evaluate_javascript(script, |_| {});
        }
    }

    // ------------------------------------------------------------------ navigation

    /// Navigate a tab to `url`. Internal URLs switch the tab to a UI-rendered page; web URLs
    /// load in the tab's webview (creating it if needed).
    pub(crate) fn navigate_tab(&self, tab_id: &TabId, url: &str, transition: Transition) {
        let Ok(parsed) = Url::parse(url) else {
            warn!("refusing to navigate to unparsable URL");
            return;
        };
        if self.session.borrow().tab(tab_id).is_none() {
            return;
        }
        if !self.views.borrow().contains_key(tab_id) {
            self.views
                .borrow_mut()
                .insert(tab_id.clone(), TabView::new(Pane::Main));
        }
        if is_internal(url) {
            if let Some(view) = self.views.borrow_mut().get_mut(tab_id) {
                view.internal = Some(parsed.to_string());
                view.loading = false;
            }
            self.session.borrow_mut().update_page(
                tab_id,
                Some(parsed.as_str()),
                Some(&internal_title(parsed.as_str())),
            );
            self.after_tab_update(tab_id);
            self.session_changed();
            return;
        }
        let (webview, previous_internal) = {
            let mut views = self.views.borrow_mut();
            let view = views.get_mut(tab_id).expect("inserted above");
            view.next_transition = transition;
            let previous_internal = view.internal.take();
            (view.webview.clone(), previous_internal)
        };
        match webview {
            Some(webview) => webview.load(parsed.clone()),
            None => {
                let pane = self
                    .views
                    .borrow()
                    .get(tab_id)
                    .map(|v| v.pane)
                    .unwrap_or(Pane::Main);
                let webview = self.create_webview(tab_id, pane, Some(parsed.clone()));
                if let Some(view) = self.views.borrow_mut().get_mut(tab_id) {
                    view.webview = Some(webview);
                    view.internal_before_web = previous_internal;
                }
            }
        }
        self.session
            .borrow_mut()
            .update_page(tab_id, Some(parsed.as_str()), None);
        if let Some(view) = self.views.borrow_mut().get_mut(tab_id) {
            view.loading = true;
        }
        self.after_tab_update(tab_id);
        self.session_changed();
    }

    /// Resolve address bar input and navigate.
    pub(crate) fn navigate_input(&self, tab_id: &TabId, input: &str) -> bool {
        let target = {
            let settings = self.settings.borrow();
            omnibox::resolve(input, &settings.search, self.private)
        };
        let Some(target) = target else { return false };
        self.navigate_tab(tab_id, target.url(), Transition::Typed);
        true
    }

    pub(crate) fn open_url_in_new_tab(&self, input: &str) {
        let target = {
            let settings = self.settings.borrow();
            omnibox::resolve(input, &settings.search, self.private)
        };
        let Some(target) = target else { return };
        let id = self.session.borrow_mut().new_tab(NewTab::url(target.url()));
        self.navigate_tab(&id, target.url(), Transition::Typed);
    }

    pub(crate) fn go_back(&self, tab_id: &TabId) {
        let mut views = self.views.borrow_mut();
        let Some(view) = views.get_mut(tab_id) else {
            return;
        };
        if view.internal.is_some() && view.webview.is_some() {
            // An internal page shown over a web page: Back returns to the web page.
            view.internal = None;
            let url = view
                .webview
                .as_ref()
                .and_then(|w| w.url())
                .map(|u| u.to_string());
            drop(views);
            if let Some(url) = url {
                let title = self.webview_for_tab(tab_id).and_then(|w| w.page_title());
                self.session
                    .borrow_mut()
                    .update_page(tab_id, Some(&url), title.as_deref());
            }
            self.session_changed();
            return;
        }
        if let Some(webview) = &view.webview {
            if webview.can_go_back() {
                webview.go_back(1);
                return;
            }
            if let Some(internal) = view.internal_before_web.clone() {
                view.internal = Some(internal.clone());
                drop(views);
                self.session.borrow_mut().update_page(
                    tab_id,
                    Some(&internal),
                    Some(&internal_title(&internal)),
                );
                self.after_tab_update(tab_id);
                self.session_changed();
            }
        }
    }

    pub(crate) fn go_forward(&self, tab_id: &TabId) {
        let views = self.views.borrow();
        let Some(view) = views.get(tab_id) else {
            return;
        };
        if view.internal.is_some() {
            if let Some(webview) = view.webview.clone() {
                drop(views);
                if let Some(v) = self.views.borrow_mut().get_mut(tab_id) {
                    v.internal = None;
                }
                let url = webview.url().map(|u| u.to_string());
                let title = webview.page_title();
                self.session
                    .borrow_mut()
                    .update_page(tab_id, url.as_deref(), title.as_deref());
                self.session_changed();
            }
            return;
        }
        if let Some(webview) = &view.webview
            && webview.can_go_forward()
        {
            webview.go_forward(1);
        }
    }

    // ------------------------------------------------------------------ tab metadata

    pub(crate) fn tab_meta(&self, tab_id: &TabId) -> Option<TabMeta> {
        let session = self.session.borrow();
        let tab = session.tab(tab_id)?;
        let views = self.views.borrow();
        let view = views.get(tab_id);
        let internal = view.is_some_and(|v| v.internal.is_some()) || is_internal(&tab.url);
        let url = tab.url.clone();
        Some(TabMeta {
            tab: tab_id.clone(),
            security: security_of(&url, internal),
            title: if tab.title.is_empty() {
                fallback_title(&url)
            } else {
                tab.title.clone()
            },
            url,
            loading: view.is_some_and(|v| v.loading),
            favicon: view.and_then(|v| v.favicon.clone()),
            can_go_back: view.is_some_and(|v| {
                v.can_go_back
                    || (v.internal.is_some() && v.webview.is_some())
                    || v.internal_before_web.is_some()
            }),
            can_go_forward: view.is_some_and(|v| v.can_go_forward),
            status: view.and_then(|v| v.status.clone()),
            zoom: tab.zoom,
            audible: view.is_some_and(|v| v.audible),
            blocked: view.map(|v| v.blocked).unwrap_or(0),
            crashed: view.and_then(|v| v.crashed.clone()),
        })
    }

    pub(crate) fn all_tab_meta(&self) -> Vec<TabMeta> {
        let ids: Vec<TabId> = self
            .session
            .borrow()
            .tabs
            .iter()
            .map(|t| t.id.clone())
            .collect();
        ids.iter().filter_map(|id| self.tab_meta(id)).collect()
    }

    pub(crate) fn after_tab_update(&self, tab_id: &TabId) {
        if let Some(meta) = self.tab_meta(tab_id) {
            self.emit(Event::TabMeta { meta });
        }
    }

    /// Record a committed navigation in history (not for private windows).
    pub(crate) fn record_history(&self, tab_id: &TabId, url: &str) {
        if self.private {
            return;
        }
        let transition = self
            .views
            .borrow_mut()
            .get_mut(tab_id)
            .map(|v| std::mem::replace(&mut v.next_transition, Transition::Link))
            .unwrap_or(Transition::Link);
        if let Err(e) = self
            .history
            .record_visit(url, "", transition, haku_core::now_ms())
        {
            warn!("history: {e}");
        }
        self.ext_history_visited(url);
    }

    pub(crate) fn set_loading(&self, tab_id: &TabId, status: LoadStatus) {
        if let Some(view) = self.views.borrow_mut().get_mut(tab_id) {
            view.loading = status != LoadStatus::Complete;
        }
        self.after_tab_update(tab_id);
    }
}

fn security_of(url: &str, internal: bool) -> Security {
    if internal {
        return Security::Internal;
    }
    match Url::parse(url).map(|u| u.scheme().to_owned()).as_deref() {
        Ok("https") | Ok("wss") => Security::Secure,
        Ok("http") | Ok("ws") => Security::Insecure,
        Ok("file") | Ok("data") | Ok("blob") => Security::Local,
        _ => Security::None,
    }
}

/// Titles for internal pages (shown in the tab strip and history).
pub fn internal_title(url: &str) -> String {
    let page = Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default();
    match page.as_str() {
        "start" | "" => "Speed Dial",
        "settings" => "Settings",
        "history" => "History",
        "bookmarks" => "Bookmarks",
        "downloads" => "Downloads",
        "extensions" => "Extensions",
        "mods" => "Mods",
        "playground" => "Playground",
        "corner" => "GX Corner",
        "games" => "My Games",
        "cleaner" => "Cleaner",
        "pinboards" => "Pinboards",
        "flow" => "My Flow",
        "about" => "About HakuCX",
        "help" => "Help",
        "tasks" => "Task manager",
        "source" => "Page source",
        "qr" => "QR code",
        _ => "HakuCX",
    }
    .to_owned()
}

fn fallback_title(url: &str) -> String {
    if is_internal(url) {
        return internal_title(url);
    }
    Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.trim_start_matches("www.").to_owned())
        })
        .unwrap_or_else(|| url.to_owned())
}

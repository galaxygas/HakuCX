/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Sidebar web panels: messengers, social sites and AI chats in a page beside the tabs.
//!
//! Each panel has its own webview in the panel painter. Hidden panels keep running (so
//! messengers stay connected and their unread counts update), like in Opera GX.

use std::rc::Rc;

use haku_core::ipc::Event;
use servo::{UserContentManager, WebView, WebViewBuilder};
use url::Url;

use super::tabs::Pane;
use super::{Browser, page_scripts};

/// Prefix for the pseudo tab id used for prompts that come from a web panel.
pub const PANEL_TARGET: &str = "panel:";

impl Browser {
    pub(crate) fn panel_for_webview(&self, webview: &WebView) -> Option<String> {
        self.panel_by_webview.borrow().get(&webview.id()).cloned()
    }

    pub(crate) fn open_web_panel(&self, id: &str, url: &str) {
        let Ok(parsed) = Url::parse(url) else { return };
        if !matches!(parsed.scheme(), "http" | "https") || id.len() > 64 {
            return;
        }
        if !self.web_panels.borrow().contains_key(id) {
            let content_manager = Rc::new(UserContentManager::new(&self.servo));
            page_scripts::install(&content_manager, &self.settings.borrow());
            self.add_extension_scripts(&content_manager);
            self.remember_content_manager(content_manager.clone());
            let webview = WebViewBuilder::new(&self.servo, self.pane_context(Pane::Panel))
                .url(parsed)
                .hidpi_scale_factor(euclid::Scale::new(self.scale()))
                .user_content_manager(content_manager)
                .clipboard_delegate(self.rc())
                .delegate(self.rc())
                .build();
            self.panel_by_webview
                .borrow_mut()
                .insert(webview.id(), id.to_owned());
            self.web_panels.borrow_mut().insert(id.to_owned(), webview);
        }
        for (pid, webview) in self.web_panels.borrow().iter() {
            if pid == id {
                webview.show();
            } else {
                webview.hide();
            }
        }
        *self.visible_panel.borrow_mut() = Some(id.to_owned());
        self.resize_web_panel();
        self.window.request_redraw();
    }

    pub(crate) fn hide_web_panel(&self) {
        if let Some(id) = self.visible_panel.borrow_mut().take()
            && let Some(webview) = self.web_panels.borrow().get(&id)
        {
            webview.hide();
            webview.set_focused(false);
        }
        self.window.request_redraw();
    }

    pub(crate) fn close_web_panel(&self, id: &str) {
        if let Some(webview) = self.web_panels.borrow_mut().remove(id) {
            self.panel_by_webview.borrow_mut().remove(&webview.id());
            self.animating.borrow_mut().remove(&webview.id());
        }
        if self.visible_panel.borrow().as_deref() == Some(id) {
            *self.visible_panel.borrow_mut() = None;
        }
        self.emit(Event::PanelBadge {
            id: id.to_owned(),
            count: 0,
        });
        self.window.request_redraw();
    }

    pub(crate) fn web_panel_navigate(&self, id: &str, action: &str) {
        let Some(webview) = self.web_panels.borrow().get(id).cloned() else {
            return;
        };
        match action {
            "back" if webview.can_go_back() => {
                webview.go_back(1);
            }
            "forward" if webview.can_go_forward() => {
                webview.go_forward(1);
            }
            "reload" => webview.reload(),
            _ => {}
        }
    }

    pub(crate) fn resize_web_panel(&self) {
        let Some(rect) = self.layout.borrow().clone().panel else {
            return;
        };
        if let Some(webview) = self.pane_webview(Pane::Panel) {
            webview.resize(winit::dpi::PhysicalSize::new(
                rect.w.max(1.0) as u32,
                rect.h.max(1.0) as u32,
            ));
        }
    }

    /// Unread count from titles like "(3) Discord" or "Inbox (12)".
    pub(crate) fn panel_title_changed(&self, id: &str, title: &str) {
        self.emit(Event::PanelBadge {
            id: id.to_owned(),
            count: unread_from_title(title),
        });
    }

    pub(crate) fn panel_state(&self, id: &str, webview: &WebView, loading: bool) {
        self.emit(Event::PanelState {
            id: id.to_owned(),
            loading,
            url: webview.url().map(|u| u.to_string()).unwrap_or_default(),
            can_go_back: webview.can_go_back(),
        });
    }
}

pub fn unread_from_title(title: &str) -> u32 {
    let t = title.trim();
    let paren = |s: &str| -> Option<u32> {
        let open = s.find('(')?;
        let close = s[open..].find(')')? + open;
        s[open + 1..close].trim().trim_end_matches('+').parse().ok()
    };
    if t.starts_with('(') {
        return paren(t).unwrap_or(0);
    }
    if t.ends_with(')')
        && let Some(open) = t.rfind('(')
    {
        return paren(&t[open..]).unwrap_or(0);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::unread_from_title;

    #[test]
    fn unread_counts() {
        assert_eq!(unread_from_title("(3) Discord"), 3);
        assert_eq!(unread_from_title("Inbox (12)"), 12);
        assert_eq!(unread_from_title("(99+) X"), 99);
        assert_eq!(unread_from_title("WhatsApp"), 0);
        assert_eq!(unread_from_title("Rust (programming language)"), 0);
    }
}

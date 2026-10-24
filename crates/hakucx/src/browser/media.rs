/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Media across tabs: what each tab plays (for the miniplayer), controlling it, popping a
//! video out (on request, or automatically when its tab is left), and pausing the sidebar
//! Player while a tab plays sound ("Automatically pause playback when other media starts").

use std::time::Duration;

use haku_core::ipc::{MediaAction, MediaInfo};
use haku_core::session::TabId;
use serde_json::json;

use super::Browser;
use super::page_scripts;
use super::popout::PopoutSpec;

/// The sidebar panel the Player uses (music services).
pub const PLAYER_PANEL: &str = "player";

#[derive(Default)]
pub struct MediaState {
    /// The Player panel is playing sound.
    player_audible: bool,
    /// HakuCX paused the Player because a tab started playing.
    player_paused_by_us: bool,
    /// Bumped to cancel a pending resume.
    resume_generation: u64,
    /// The tabs shown last time the panes were laid out.
    last_visible: Vec<TabId>,
}

/// A media report from a page (see `media.js`).
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaReport {
    pub playing: bool,
    #[serde(default)]
    pub present: bool,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub video: bool,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub artwork: String,
    #[serde(default)]
    pub has_prev: bool,
    #[serde(default)]
    pub has_next: bool,
    #[serde(default)]
    pub poppable: bool,
}

/// A page's request to pop out its video.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PopoutRequest {
    pub src: String,
    pub time: f64,
    pub paused: bool,
    pub volume: f64,
    pub muted: bool,
    #[serde(default)]
    pub poster: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    #[serde(default)]
    pub auto: bool,
}

fn clip(mut s: String, max: usize) -> String {
    if s.len() > max {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s.truncate(end);
    }
    s
}

impl Browser {
    pub(crate) fn tab_media_report(&self, tab: &TabId, report: MediaReport) {
        let popped = self.popout_source().as_ref() == Some(tab);
        let info = report.present.then(|| MediaInfo {
            paused: report.paused,
            video: report.video,
            title: clip(report.title, 300),
            artist: clip(report.artist, 300),
            album: clip(report.album, 300),
            artwork: Some(report.artwork).filter(|a| a.starts_with("https://") && a.len() <= 2048),
            has_prev: report.has_prev,
            has_next: report.has_next,
            poppable: report.poppable,
            popped,
        });
        let started = {
            let mut views = self.views.borrow_mut();
            let Some(view) = views.get_mut(tab) else {
                return;
            };
            let started = report.playing && !view.audible;
            view.audible = report.playing;
            view.media = info;
            started
        };
        self.after_tab_update(tab);
        self.smart_home_media();
        if started {
            self.player_pause_for_tab();
        } else if !report.playing {
            self.player_maybe_resume();
        }
    }

    pub(crate) fn tab_popout_request(&self, tab: &TabId, r: PopoutRequest) {
        self.request_popout(PopoutSpec {
            tab: tab.clone(),
            src: clip(r.src, 8192),
            time: if r.time.is_finite() {
                r.time.max(0.0)
            } else {
                0.0
            },
            paused: r.paused,
            volume: if r.volume.is_finite() {
                r.volume.clamp(0.0, 1.0)
            } else {
                1.0
            },
            muted: r.muted,
            poster: clip(r.poster, 2048),
            title: clip(r.title, 300),
            width: r.width,
            height: r.height,
            auto: r.auto,
        });
    }

    /// Miniplayer buttons.
    pub(crate) fn media_command(&self, tab: &TabId, action: MediaAction) {
        let name = match action {
            MediaAction::Play => "play",
            MediaAction::Pause => "pause",
            MediaAction::Next => "next",
            MediaAction::Prev => "prev",
            MediaAction::PopOut => "popout",
            MediaAction::Return => {
                if self.popout_source().as_ref() == Some(tab) {
                    let webview = self.popout.borrow().as_ref().map(|p| p.webview.clone());
                    if let Some(webview) = webview {
                        // The page answers with its position (`popout-return`).
                        webview.evaluate_javascript(
                            "document.getElementById('back') && document.getElementById('back').click()",
                            |_| {},
                        );
                    }
                }
                return;
            }
        };
        self.tab_media_script(tab, name, &json!({}));
    }

    /// The panes now show `visible`: pop out a playing video whose tab was just left (when
    /// "Automatically pop out videos" is on), and bring an automatic pop out back when its
    /// tab is shown again.
    pub(crate) fn media_visibility_changed(&self, visible: Vec<TabId>) {
        let before = std::mem::replace(&mut self.media.borrow_mut().last_visible, visible.clone());
        let (popout, auto) = {
            let s = self.settings.borrow();
            (s.video.popout, s.video.auto_popout)
        };
        let source = self
            .popout
            .borrow()
            .as_ref()
            .map(|p| (p.spec.tab.clone(), p.spec.auto));
        if let Some((tab, true)) = &source
            && visible.contains(tab)
        {
            self.media_command(tab, MediaAction::Return);
        }
        if !(popout && auto) {
            return;
        }
        for left in before.iter().filter(|t| !visible.contains(t)) {
            let playing_video = self.views.borrow().get(left).is_some_and(|v| {
                v.audible
                    && v.media
                        .as_ref()
                        .is_some_and(|m| m.video && m.poppable && !m.paused)
            });
            if playing_video && source.as_ref().map(|s| &s.0) != Some(left) {
                self.tab_media_script(left, "popout", &json!({ "auto": true }));
            }
        }
    }

    // ------------------------------------------------------------------ Player auto-pause

    pub(crate) fn panel_media_report(&self, panel: &str, report: MediaReport) {
        if panel != PLAYER_PANEL {
            return;
        }
        let mut m = self.media.borrow_mut();
        m.player_audible = report.playing;
        if report.playing {
            // Playing again (the user pressed play, or HakuCX resumed it).
            m.player_paused_by_us = false;
        }
    }

    fn player_script(&self, action: &str) {
        if let Some(webview) = self.web_panels.borrow().get(PLAYER_PANEL) {
            webview.evaluate_javascript(
                page_scripts::media_action_script(action, &json!({})),
                |_| {},
            );
        }
    }

    fn any_tab_audible(&self) -> bool {
        self.views.borrow().values().any(|v| v.audible)
    }

    fn player_pause_for_tab(&self) {
        if !self.settings.borrow().player.auto_pause {
            return;
        }
        let pause = {
            let mut m = self.media.borrow_mut();
            m.resume_generation += 1;
            let pause = m.player_audible;
            if pause {
                m.player_paused_by_us = true;
            }
            pause
        };
        if pause {
            self.player_script("pause");
        }
    }

    fn player_maybe_resume(&self) {
        if self.any_tab_audible() {
            return;
        }
        let generation = {
            let mut m = self.media.borrow_mut();
            if !m.player_paused_by_us {
                return;
            }
            m.resume_generation += 1;
            m.resume_generation
        };
        let secs = self.settings.borrow().player.resume_after_secs as u64;
        self.background_sender()
            .post_after(Duration::from_secs(secs), move |b: &Browser| {
                let due = {
                    let m = b.media.borrow();
                    m.resume_generation == generation && m.player_paused_by_us
                };
                if due && !b.any_tab_audible() {
                    b.media.borrow_mut().player_paused_by_us = false;
                    b.player_script("play");
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_parse_with_defaults() {
        let r: MediaReport = serde_json::from_str(r#"{"playing":true}"#).unwrap();
        assert!(r.playing && !r.present && !r.video);
        let r: MediaReport = serde_json::from_str(
            r#"{"playing":false,"present":true,"paused":true,"video":true,"title":"T","artist":"A","album":"","artwork":"https://x/a.png","hasPrev":true,"hasNext":false,"poppable":true,"popped":false}"#,
        )
        .unwrap();
        assert!(r.present && r.paused && r.video && r.has_prev && r.poppable);
        assert_eq!(r.artwork, "https://x/a.png");
    }

    #[test]
    fn clip_respects_char_boundaries() {
        assert_eq!(clip("héllo".into(), 2), "h");
        assert_eq!(clip("abc".into(), 10), "abc");
    }
}

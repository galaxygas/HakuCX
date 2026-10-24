/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Smart home in the browser: keeps the MQTT connection (`crate::smart_home`) in step with the
//! settings and publishes the theme color and what is playing.

use haku_core::ipc::{Event, SmartHomeStatus};
use haku_core::themes;

use super::Browser;
use crate::smart_home::{self, SmartHome};

#[derive(Default)]
pub struct HomeState {
    client: Option<SmartHome>,
    /// (broker, prefix) the client was made for.
    config: Option<(String, String)>,
    status: SmartHomeStatus,
    /// Last published media state, to publish only changes.
    media: Option<(bool, String)>,
}

impl Browser {
    pub(crate) fn smart_home_apply(&self) {
        let (on, broker, prefix) = {
            let s = self.settings.borrow();
            (
                s.smart_home.mqtt,
                s.smart_home.broker_url.clone(),
                s.smart_home.topic_prefix.clone(),
            )
        };
        if !on || self.private || self.secondary {
            let mut st = self.home.borrow_mut();
            st.client = None;
            st.config = None;
            st.media = None;
            st.status = SmartHomeStatus {
                state: "off".into(),
                message: String::new(),
            };
            drop(st);
            self.smart_home_emit();
            return;
        }
        let config = (broker.clone(), prefix.clone());
        if self.home.borrow().config.as_ref() == Some(&config) {
            self.smart_home_publish_theme();
            return;
        }
        let mut st = self.home.borrow_mut();
        st.client = None;
        st.media = None;
        st.config = Some(config);
        match smart_home::parse_broker(&broker) {
            Err(message) => {
                st.status = SmartHomeStatus {
                    state: "error".into(),
                    message,
                };
            }
            Ok(address) => {
                let sender = self.background_sender();
                st.status = SmartHomeStatus {
                    state: "connecting".into(),
                    message: String::new(),
                };
                st.client = Some(SmartHome::connect(
                    &address,
                    &prefix,
                    Box::new(move |result| {
                        sender.send(Box::new(move |b: &Browser| b.smart_home_report(result)));
                    }),
                ));
            }
        }
        drop(st);
        self.smart_home_emit();
    }

    fn smart_home_report(&self, result: Result<(), String>) {
        let connected = result.is_ok();
        self.home.borrow_mut().status = match result {
            Ok(()) => SmartHomeStatus {
                state: "connected".into(),
                message: String::new(),
            },
            Err(e) => SmartHomeStatus {
                state: "error".into(),
                message: format!("Can't reach the broker: {e}. HakuCX keeps trying."),
            },
        };
        self.smart_home_emit();
        if connected {
            self.home.borrow_mut().media = None;
            self.smart_home_publish_theme();
            self.smart_home_media();
        }
    }

    fn smart_home_publish_theme(&self) {
        let home = self.home.borrow();
        let Some(client) = home.client.as_ref() else {
            return;
        };
        let settings = self.settings.borrow();
        let palette = themes::palette_for_settings(
            &settings.appearance,
            &self.user_themes.borrow().themes,
            self.system_dark.get(),
        );
        if let Some(accent) = palette.vars.get("--hx-accent") {
            client.publish("theme/accent", accent);
        }
    }

    /// Publish whether anything plays (after a tab's media changed).
    pub(crate) fn smart_home_media(&self) {
        if self.home.borrow().client.is_none() {
            return;
        }
        let playing = {
            let views = self.views.borrow();
            let session = self.session.borrow();
            views
                .iter()
                .filter(|(_, v)| v.audible)
                .map(|(id, v)| {
                    v.media
                        .as_ref()
                        .map(|m| m.title.clone())
                        .filter(|t| !t.is_empty())
                        .or_else(|| session.tab(id).map(|t| t.title.clone()))
                        .unwrap_or_default()
                })
                .next()
        };
        let state = (playing.is_some(), playing.unwrap_or_default());
        let mut home = self.home.borrow_mut();
        if home.media.as_ref() == Some(&state) {
            return;
        }
        if let Some(client) = home.client.as_ref() {
            client.publish("media/playing", if state.0 { "true" } else { "false" });
            client.publish("media/title", &state.1);
        }
        home.media = Some(state);
    }

    pub(crate) fn smart_home_emit(&self) {
        let status = self.home.borrow().status.clone();
        self.emit(Event::SmartHome { status });
    }

    pub(crate) fn smart_home_shutdown(&self) {
        let client = self.home.borrow_mut().client.take();
        drop(client);
    }
}

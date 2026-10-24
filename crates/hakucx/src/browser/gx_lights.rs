/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! GX Lights in the browser: turns settings and the theme into a lights configuration, and
//! browser events into light signals. The device work happens on the driver thread
//! (`crate::lights`).

use haku_core::ipc::{Event, LightsStatus};
use haku_core::themes;
use haku_lights::Rgb;
use haku_lights::effects::{Look, Signal, Style};

use super::Browser;
use crate::lights::{Config, Lights};

/// Driver state kept by the browser.
#[derive(Default)]
pub struct LightsState {
    pub driver: Option<Lights>,
    pub status: LightsStatus,
    /// RAM, CPU and network limiter switches, to notice one turning on.
    pub limiters: Option<(bool, bool, bool)>,
}

impl Browser {
    /// Lights belong to the profile's main window; private and extra windows (separate
    /// processes) leave the devices alone so two drivers never fight over them.
    fn lights_allowed(&self) -> bool {
        !self.private && !self.secondary
    }

    fn lights_config(&self) -> Config {
        let settings = self.settings.borrow();
        let l = &settings.lights;
        let palette = themes::palette_for_settings(
            &settings.appearance,
            &self.user_themes.borrow().themes,
            self.system_dark.get(),
        );
        let color = |key: &str, fallback: Rgb| {
            palette
                .vars
                .get(key)
                .and_then(|hex| Rgb::parse_hex(hex))
                .unwrap_or(fallback)
        };
        let primary = color("--hx-accent", Rgb::new(0xfa, 0x1e, 0x4e));
        Config {
            enabled: l.enabled,
            razer: l.razer_chroma,
            openrgb: l
                .openrgb
                .then(|| format!("{}:{}", l.openrgb_host.trim(), l.openrgb_port)),
            look: Look {
                primary,
                secondary: color("--hx-accent-dim", primary.scale(0.5)),
                style: Style::parse(&l.animation_set),
                brightness: l.brightness as f32 / 100.0,
                events: l.animations,
                music: l.music_visualizer,
                dim_in_fullscreen: l.dim_in_fullscreen,
            },
        }
    }

    /// Apply the lights settings (after start and whenever settings or the theme change).
    pub(crate) fn lights_apply(&self) {
        if !self.lights_allowed() {
            return;
        }
        let config = self.lights_config();
        let (ram, cpu, net) = {
            let s = self.settings.borrow();
            (
                s.control.ram_limiter,
                s.control.cpu_limiter,
                s.control.network_limiter,
            )
        };
        let mut state = self.lights.borrow_mut();
        if let Some((old_ram, old_cpu, old_net)) = state.limiters
            && ((ram && !old_ram) || (cpu && !old_cpu) || (net && !old_net))
            && let Some(driver) = &state.driver
        {
            driver.signal(Signal::LimiterOn);
        }
        state.limiters = Some((ram, cpu, net));
        if state.driver.is_none() {
            if !config.enabled {
                return;
            }
            let sender = self.background_sender();
            let driver = Lights::start(Box::new(move |status| {
                sender.send(Box::new(move |b: &Browser| b.lights_report(status)));
            }));
            driver.configure(config);
            driver.set_fullscreen(self.window.fullscreen().is_some());
            driver.signal(Signal::Startup);
            state.driver = Some(driver);
            return;
        }
        if let Some(driver) = &state.driver {
            driver.configure(config);
        }
    }

    pub(crate) fn lights_signal(&self, signal: Signal) {
        if let Some(driver) = &self.lights.borrow().driver {
            driver.signal(signal);
        }
    }

    pub(crate) fn lights_fullscreen(&self, on: bool) {
        if let Some(driver) = &self.lights.borrow().driver {
            driver.set_fullscreen(on);
        }
    }

    pub(crate) fn lights_reconnect(&self) {
        if let Some(driver) = &self.lights.borrow().driver {
            driver.reconnect();
        }
    }

    fn lights_report(&self, status: LightsStatus) {
        self.lights.borrow_mut().status = status;
        self.lights_emit();
    }

    pub(crate) fn lights_emit(&self) {
        let status = self.lights.borrow().status.clone();
        self.emit(Event::Lights { status });
    }

    /// Give the devices back before the process exits.
    pub(crate) fn lights_shutdown(&self) {
        let driver = self.lights.borrow_mut().driver.take();
        drop(driver);
    }
}

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! What the lights show: the theme's colors with an idle style, short flashes when something
//! happens in the browser (as Opera GX does: start, new tab, finished download, notification,
//! a limiter switching on), and optionally the level of the sound the computer is playing.
//!
//! Every device is treated as a row of LEDs from left to right; a frame is one color per LED.

use std::f32::consts::PI;
use std::time::{Duration, Instant};

use crate::Rgb;

/// The idle look ("Animation set" in settings).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    /// The theme color drifts slowly towards the secondary color and back.
    Gx,
    /// The theme color breathes.
    Pulse,
    /// A band of the secondary color runs across each device.
    Wave,
    /// The theme color, still.
    Static,
}

impl Style {
    /// The settings value; anything unknown falls back to [`Style::Gx`].
    pub fn parse(name: &str) -> Style {
        match name {
            "pulse" => Style::Pulse,
            "wave" => Style::Wave,
            "static" => Style::Static,
            _ => Style::Gx,
        }
    }
}

/// Browser events that flash the lights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    Startup,
    NewTab,
    DownloadDone,
    Notification,
    LimiterOn,
}

impl Signal {
    fn duration(self) -> Duration {
        Duration::from_millis(match self {
            Signal::Startup => 1500,
            Signal::NewTab => 450,
            Signal::DownloadDone => 1200,
            Signal::Notification => 1000,
            Signal::LimiterOn => 900,
        })
    }
}

/// Settings and theme colors, as the effects use them.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub primary: Rgb,
    pub secondary: Rgb,
    pub style: Style,
    /// 0..=1.
    pub brightness: f32,
    /// Flash on browser events.
    pub events: bool,
    /// Follow the sound level.
    pub music: bool,
    pub dim_in_fullscreen: bool,
}

const DOWNLOAD_GREEN: Rgb = Rgb::new(0x10, 0xe0, 0x60);
const LIMITER_RED: Rgb = Rgb::new(0xff, 0x24, 0x24);
/// How dark the lights get while a page or the window is fullscreen.
const FULLSCREEN_DIM: f32 = 0.15;

pub struct Effects {
    look: Look,
    epoch: Instant,
    flashes: Vec<(Signal, Instant)>,
    level: f32,
    fullscreen: bool,
}

impl Effects {
    pub fn new(look: Look, now: Instant) -> Self {
        Self {
            look,
            epoch: now,
            flashes: Vec::new(),
            level: 0.0,
            fullscreen: false,
        }
    }

    pub fn look(&self) -> &Look {
        &self.look
    }

    pub fn set_look(&mut self, look: Look) {
        if !look.events {
            self.flashes.clear();
        }
        self.look = look;
    }

    pub fn signal(&mut self, signal: Signal, now: Instant) {
        if !self.look.events {
            return;
        }
        self.flashes.retain(|(s, _)| *s != signal);
        self.flashes.push((signal, now));
        if self.flashes.len() > 4 {
            self.flashes.remove(0);
        }
    }

    pub fn set_fullscreen(&mut self, on: bool) {
        self.fullscreen = on;
    }

    /// Feed the latest peak level (0..=1) of the sound being played. Rises at once, falls
    /// smoothly, like a VU meter.
    pub fn set_level(&mut self, peak: f32) {
        let peak = peak.clamp(0.0, 1.0);
        self.level = if peak >= self.level {
            peak
        } else {
            self.level * 0.8 + peak * 0.2
        };
    }

    /// Whether frames change over time (so the driver must keep sending them).
    pub fn animating(&self, now: Instant) -> bool {
        self.look.style != Style::Static
            || self.look.music
            || self
                .flashes
                .iter()
                .any(|(s, at)| now.duration_since(*at) < s.duration())
    }

    /// One color per LED for a device with `leds` LEDs in a row.
    pub fn frame(&mut self, now: Instant, leds: usize) -> Vec<Rgb> {
        self.flashes
            .retain(|(s, at)| now.duration_since(*at) < s.duration());
        let t = now.duration_since(self.epoch).as_secs_f32();
        let look = &self.look;
        let mut overall = look.brightness.clamp(0.0, 1.0);
        if look.dim_in_fullscreen && self.fullscreen {
            overall *= FULLSCREEN_DIM;
        }
        if look.music {
            overall *= 0.2 + 0.8 * self.level;
        }
        (0..leds)
            .map(|i| {
                let x = if leds > 1 {
                    i as f32 / (leds - 1) as f32
                } else {
                    0.5
                };
                let mut c = idle(look, t, x);
                for (signal, at) in &self.flashes {
                    let p = now.duration_since(*at).as_secs_f32() / signal.duration().as_secs_f32();
                    c = flash(*signal, c, look, p.clamp(0.0, 1.0), x);
                }
                c.scale(overall)
            })
            .collect()
    }
}

fn wave(period: f32, t: f32) -> f32 {
    0.5 + 0.5 * (2.0 * PI * t / period).sin()
}

fn idle(look: &Look, t: f32, x: f32) -> Rgb {
    match look.style {
        Style::Static => look.primary,
        Style::Pulse => look.primary.scale(0.35 + 0.65 * wave(4.0, t)),
        Style::Gx => look.primary.lerp(look.secondary, wave(12.0, t + x * 2.0)),
        Style::Wave => {
            // A band 0.3 wide moving right, one pass every 3 s.
            let phase = (x - t / 3.0).rem_euclid(1.0);
            let d = (phase - 0.5).abs() * 2.0;
            look.primary.lerp(look.secondary, (1.0 - d / 0.3).max(0.0))
        }
    }
}

fn flash(signal: Signal, c: Rgb, look: &Look, p: f32, x: f32) -> Rgb {
    match signal {
        // The theme color fills the devices from left to right.
        Signal::Startup => {
            let edge = p * 1.25;
            if x <= edge {
                c
            } else {
                c.scale((1.0 - (x - edge) * 8.0).max(0.0))
            }
        }
        Signal::NewTab => c.lerp(Rgb::WHITE, 0.6 * (1.0 - p)),
        // Two green pulses.
        Signal::DownloadDone => c.lerp(
            DOWNLOAD_GREEN,
            (2.0 * PI * p * 2.0 - PI / 2.0).sin().max(0.0),
        ),
        // A bright band sweeps across.
        Signal::Notification => {
            let d = x - p;
            c.lerp(
                look.secondary.lerp(Rgb::WHITE, 0.5),
                (-(d * d) / 0.01).exp(),
            )
        }
        Signal::LimiterOn => c.lerp(LIMITER_RED, (PI * p).sin()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(style: Style) -> Look {
        Look {
            primary: Rgb::new(250, 30, 78),
            secondary: Rgb::new(20, 20, 200),
            style,
            brightness: 1.0,
            events: true,
            music: false,
            dim_in_fullscreen: true,
        }
    }

    #[test]
    fn static_style_is_the_theme_color() {
        let now = Instant::now();
        let mut fx = Effects::new(look(Style::Static), now);
        assert!(!fx.animating(now));
        assert_eq!(fx.frame(now, 3), vec![Rgb::new(250, 30, 78); 3]);
        fx.set_look(Look {
            brightness: 0.5,
            ..look(Style::Static)
        });
        assert_eq!(fx.frame(now, 1), vec![Rgb::new(125, 15, 39)]);
    }

    #[test]
    fn fullscreen_dims() {
        let now = Instant::now();
        let mut fx = Effects::new(look(Style::Static), now);
        fx.set_fullscreen(true);
        assert_eq!(
            fx.frame(now, 1)[0],
            Rgb::new(250, 30, 78).scale(FULLSCREEN_DIM)
        );
        fx.set_look(Look {
            dim_in_fullscreen: false,
            ..look(Style::Static)
        });
        assert_eq!(fx.frame(now, 1)[0], Rgb::new(250, 30, 78));
    }

    #[test]
    fn flashes_play_once_and_only_when_enabled() {
        let now = Instant::now();
        let mut fx = Effects::new(look(Style::Static), now);
        fx.signal(Signal::NewTab, now);
        assert!(fx.animating(now));
        // At the start of the flash the color is pushed towards white.
        assert_ne!(fx.frame(now, 1)[0], Rgb::new(250, 30, 78));
        let later = now + Duration::from_secs(2);
        assert!(!fx.animating(later));
        assert_eq!(fx.frame(later, 1)[0], Rgb::new(250, 30, 78));

        fx.set_look(Look {
            events: false,
            ..look(Style::Static)
        });
        fx.signal(Signal::DownloadDone, later);
        assert!(!fx.animating(later));
    }

    #[test]
    fn startup_fills_left_to_right() {
        let now = Instant::now();
        let mut fx = Effects::new(look(Style::Static), now);
        fx.signal(Signal::Startup, now);
        let early = fx.frame(now + Duration::from_millis(300), 10);
        assert_eq!(early[0], Rgb::new(250, 30, 78));
        assert_eq!(early[9], Rgb::BLACK);
    }

    #[test]
    fn music_follows_the_level() {
        let now = Instant::now();
        let mut fx = Effects::new(
            Look {
                music: true,
                ..look(Style::Static)
            },
            now,
        );
        assert!(fx.animating(now));
        let quiet = fx.frame(now, 1)[0];
        fx.set_level(1.0);
        let loud = fx.frame(now, 1)[0];
        assert_eq!(loud, Rgb::new(250, 30, 78));
        assert!(quiet.r < loud.r);
        // The level falls back gradually, not at once.
        fx.set_level(0.0);
        assert!(fx.frame(now, 1)[0].r > quiet.r);
    }

    #[test]
    fn animated_styles_change_over_time() {
        for style in [Style::Gx, Style::Pulse, Style::Wave] {
            let now = Instant::now();
            let mut fx = Effects::new(look(style), now);
            assert!(fx.animating(now));
            let a = fx.frame(now, 8);
            let b = fx.frame(now + Duration::from_millis(1500), 8);
            assert_ne!(a, b, "{style:?}");
        }
        assert_eq!(Style::parse("wave"), Style::Wave);
        assert_eq!(Style::parse("dark-magic"), Style::Gx);
    }
}

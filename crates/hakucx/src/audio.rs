/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Browser sounds, keyboard sounds and background music, synthesized in Rust (no sample files,
//! so there is nothing to license) and played through the default audio device with rodio.
//!
//! If the system has no audio device, every call is a no-op.

use std::f32::consts::TAU;
use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use log::warn;
use rodio::buffer::SamplesBuffer;
use rodio::{MixerDeviceSink, Source};

const RATE: u32 = 44_100;

pub struct Audio {
    sink: Option<MixerDeviceSink>,
    music_stop: Option<Arc<AtomicBool>>,
    music_volume: Arc<AtomicU32>,
    key_counter: std::cell::Cell<u32>,
}

/// One synthesized voice: frequency sweep, waveform, duration and loudness.
struct Voice {
    from_hz: f32,
    to_hz: f32,
    secs: f32,
    shape: Shape,
    gain: f32,
    delay: f32,
}

#[derive(Clone, Copy)]
enum Shape {
    Sine,
    Triangle,
    Square,
    Noise,
}

fn osc(shape: Shape, phase: f32, seed: &mut u32) -> f32 {
    match shape {
        Shape::Sine => (phase * TAU).sin(),
        Shape::Triangle => 4.0 * (phase - (phase + 0.5).floor()).abs() - 1.0,
        Shape::Square => {
            if phase.fract() < 0.5 {
                0.6
            } else {
                -0.6
            }
        }
        Shape::Noise => {
            // xorshift: deterministic per sound, no RNG dependency.
            *seed ^= *seed << 13;
            *seed ^= *seed >> 17;
            *seed ^= *seed << 5;
            (*seed as f32 / u32::MAX as f32) * 2.0 - 1.0
        }
    }
}

fn render(voices: &[Voice]) -> Vec<f32> {
    let total = voices.iter().map(|v| v.delay + v.secs).fold(0.0, f32::max);
    let len = (total * RATE as f32) as usize + 1;
    let mut out = vec![0.0f32; len];
    let mut seed = 0x9e37_79b9u32;
    for v in voices {
        let start = (v.delay * RATE as f32) as usize;
        let n = (v.secs * RATE as f32) as usize;
        let mut phase = 0.0f32;
        for i in 0..n {
            let t = i as f32 / n as f32;
            let hz = v.from_hz + (v.to_hz - v.from_hz) * t;
            phase += hz / RATE as f32;
            // Short attack, exponential release: clicks without pops.
            let attack = (i as f32 / (0.003 * RATE as f32)).min(1.0);
            let env = attack * (-5.0 * t).exp();
            if let Some(s) = out.get_mut(start + i) {
                *s += osc(v.shape, phase, &mut seed) * env * v.gain;
            }
        }
    }
    out
}

fn voice(from_hz: f32, to_hz: f32, secs: f32, shape: Shape, gain: f32, delay: f32) -> Voice {
    Voice {
        from_hz,
        to_hz,
        secs,
        shape,
        gain,
        delay,
    }
}

/// The sounds of one pack. `soft` is quieter and rounder.
fn sound(name: &str, soft: bool) -> Option<Vec<f32>> {
    let s = if soft { Shape::Sine } else { Shape::Triangle };
    let voices = match name {
        "click" => vec![voice(2200.0, 1800.0, 0.025, s, 0.35, 0.0)],
        "hover" => vec![voice(3200.0, 3000.0, 0.012, Shape::Sine, 0.12, 0.0)],
        "switch" => vec![voice(1200.0, 1500.0, 0.045, s, 0.3, 0.0)],
        "tab-open" => vec![
            voice(520.0, 620.0, 0.07, s, 0.35, 0.0),
            voice(780.0, 900.0, 0.09, s, 0.3, 0.05),
        ],
        "tab-close" => vec![
            voice(640.0, 480.0, 0.07, s, 0.35, 0.0),
            voice(420.0, 300.0, 0.1, s, 0.3, 0.05),
        ],
        "tab-slash" => vec![
            voice(0.0, 0.0, 0.11, Shape::Noise, 0.18, 0.0),
            voice(1800.0, 600.0, 0.11, Shape::Sine, 0.2, 0.0),
        ],
        "limiters" => vec![voice(
            150.0,
            130.0,
            0.16,
            if soft { Shape::Sine } else { Shape::Square },
            0.3,
            0.0,
        )],
        "download" => vec![
            voice(1046.5, 1046.5, 0.12, Shape::Sine, 0.3, 0.0),
            voice(1568.0, 1568.0, 0.2, Shape::Sine, 0.28, 0.1),
        ],
        "warn" => vec![
            voice(180.0, 170.0, 0.12, Shape::Square, 0.25, 0.0),
            voice(140.0, 130.0, 0.14, Shape::Square, 0.25, 0.12),
        ],
        "level" => vec![
            voice(523.25, 523.25, 0.1, s, 0.3, 0.0),
            voice(659.25, 659.25, 0.1, s, 0.3, 0.1),
            voice(783.99, 783.99, 0.18, s, 0.3, 0.2),
        ],
        _ => return None,
    };
    Some(render(&voices))
}

/// A mechanical key: a noise tick plus a pitched click, varied per keystroke.
fn key_sound(n: u32, pack: &str) -> Vec<f32> {
    let jitter = 1.0 + ((n.wrapping_mul(2_654_435_761) >> 24) as f32 / 255.0 - 0.5) * 0.12;
    match pack {
        "haku-soft-keys" => render(&[voice(
            900.0 * jitter,
            700.0 * jitter,
            0.03,
            Shape::Sine,
            0.22,
            0.0,
        )]),
        _ => render(&[
            voice(0.0, 0.0, 0.008, Shape::Noise, 0.25, 0.0),
            voice(
                3800.0 * jitter,
                2600.0 * jitter,
                0.018,
                Shape::Triangle,
                0.2,
                0.0,
            ),
            voice(
                160.0 * jitter,
                120.0 * jitter,
                0.03,
                Shape::Sine,
                0.18,
                0.004,
            ),
        ]),
    }
}

impl Audio {
    pub fn new() -> Self {
        let sink = match rodio::DeviceSinkBuilder::open_default_sink() {
            Ok(mut sink) => {
                sink.log_on_drop(false);
                Some(sink)
            }
            Err(e) => {
                warn!("no audio output ({e}); browser sounds are off");
                None
            }
        };
        Self {
            sink,
            music_stop: None,
            music_volume: Arc::new(AtomicU32::new(40)),
            key_counter: std::cell::Cell::new(0),
        }
    }

    fn play_samples(&self, samples: Vec<f32>, volume: f32) {
        let Some(sink) = &self.sink else { return };
        if volume <= 0.0 {
            return;
        }
        let buffer = SamplesBuffer::new(
            NonZero::new(1).expect("1"),
            NonZero::new(RATE).expect("rate"),
            samples,
        );
        sink.mixer().add(buffer.amplify(volume.clamp(0.0, 1.5)));
    }

    /// Play a browser sound at `volume` (0..1). Unknown names are ignored.
    pub fn play(&self, name: &str, pack: &str, volume: f32) {
        if let Some(samples) = sound(name, pack == "haku-soft") {
            self.play_samples(samples, volume);
        }
    }

    pub fn key(&self, pack: &str, volume: f32) {
        let n = self.key_counter.get().wrapping_add(1);
        self.key_counter.set(n);
        self.play_samples(key_sound(n, pack), volume);
    }

    pub fn set_music(&mut self, on: bool, track: &str, volume: f32) {
        self.music_volume
            .store((volume.clamp(0.0, 1.0) * 100.0) as u32, Ordering::Relaxed);
        if let Some(stop) = self.music_stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        let Some(sink) = &self.sink else { return };
        if !on {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        sink.mixer().add(
            Music::new(track, stop.clone(), self.music_volume.clone())
                .fade_in(Duration::from_secs(2)),
        );
        self.music_stop = Some(stop);
    }
}

/// Endless procedural pad music: a four-chord progression per track with slow swells.
struct Music {
    chords: Vec<[f32; 3]>,
    chord_secs: f32,
    shape: Shape,
    t: u64,
    phases: [f32; 4],
    stop: Arc<AtomicBool>,
    volume: Arc<AtomicU32>,
    seed: u32,
}

impl Music {
    fn new(track: &str, stop: Arc<AtomicBool>, volume: Arc<AtomicU32>) -> Self {
        let (chords, chord_secs, shape): (Vec<[f32; 3]>, f32, Shape) = match track {
            "chill" => (
                vec![
                    [174.6, 220.0, 261.6],
                    [196.0, 246.9, 293.7],
                    [164.8, 220.0, 246.9],
                    [146.8, 196.0, 246.9],
                ],
                4.0,
                Shape::Sine,
            ),
            "raid" => (
                vec![
                    [110.0, 165.0, 220.0],
                    [110.0, 164.8, 233.1],
                    [98.0, 146.8, 196.0],
                    [110.0, 174.6, 220.0],
                ],
                2.0,
                Shape::Triangle,
            ),
            "doom" => (
                vec![
                    [82.4, 123.5, 164.8],
                    [87.3, 130.8, 174.6],
                    [73.4, 110.0, 146.8],
                    [77.8, 116.5, 155.6],
                ],
                1.5,
                Shape::Square,
            ),
            _ => (
                vec![
                    [220.0, 329.6, 440.0],
                    [196.0, 293.7, 392.0],
                    [174.6, 261.6, 349.2],
                    [246.9, 329.6, 493.9],
                ],
                3.0,
                Shape::Sine,
            ),
        };
        Self {
            chords,
            chord_secs,
            shape,
            t: 0,
            phases: [0.0; 4],
            stop,
            volume,
            seed: 0x1234_5678,
        }
    }
}

impl Iterator for Music {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.stop.load(Ordering::Relaxed) {
            return None;
        }
        let secs = self.t as f32 / RATE as f32;
        self.t += 1;
        let index = (secs / self.chord_secs) as usize % self.chords.len();
        let within = (secs % self.chord_secs) / self.chord_secs;
        // Swell in and out within each chord to avoid hard changes.
        let swell = (within * std::f32::consts::PI).sin().powf(0.6);
        let chord = self.chords[index];
        let mut sample = 0.0;
        for (i, hz) in chord.iter().enumerate() {
            self.phases[i] = (self.phases[i] + hz / RATE as f32).fract();
            sample += osc(self.shape, self.phases[i], &mut self.seed) * 0.22;
        }
        // A sub-octave bass under the root.
        self.phases[3] = (self.phases[3] + chord[0] / 2.0 / RATE as f32).fract();
        sample += (self.phases[3] * TAU).sin() * 0.18;
        let volume = self.volume.load(Ordering::Relaxed) as f32 / 100.0;
        Some(sample * swell * volume * 0.5)
    }
}

impl Source for Music {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(1).expect("1")
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        NonZero::new(RATE).expect("rate")
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_renders_and_stays_in_range() {
        for name in [
            "click",
            "hover",
            "switch",
            "tab-open",
            "tab-close",
            "tab-slash",
            "limiters",
            "download",
            "warn",
            "level",
        ] {
            for soft in [false, true] {
                let s = sound(name, soft).unwrap();
                assert!(!s.is_empty(), "{name}");
                assert!(s.iter().all(|x| x.abs() <= 1.5), "{name} clips");
            }
        }
        assert!(sound("nope", false).is_none());
        assert!(!key_sound(3, "haku-mech").is_empty());
    }

    #[test]
    fn music_stops_on_flag() {
        let stop = Arc::new(AtomicBool::new(false));
        let mut m = Music::new("focus", stop.clone(), Arc::new(AtomicU32::new(50)));
        assert!(m.next().is_some());
        stop.store(true, Ordering::Relaxed);
        assert!(m.next().is_none());
    }
}

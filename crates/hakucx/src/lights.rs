/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The GX Lights driver: a background thread that keeps the Razer Chroma and OpenRGB
//! connections, renders [`Effects`] frames and sends them. Device I/O is slow and blocking, so
//! it never runs on the UI thread; the browser only sends it messages.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use haku_core::ipc::{LightLink, LightsStatus};
use haku_lights::effects::{Effects, Look, Signal};
use haku_lights::openrgb;
use haku_lights::razer::{self, Device};
use log::{info, warn};

/// What the lights should do.
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub enabled: bool,
    pub razer: bool,
    /// `host:port` of an OpenRGB SDK server, if wanted.
    pub openrgb: Option<String>,
    pub look: Look,
}

enum Msg {
    Configure(Config),
    Signal(Signal),
    Fullscreen(bool),
    Reconnect,
    Stop,
}

pub struct Lights {
    tx: Sender<Msg>,
    thread: Option<JoinHandle<()>>,
}

/// Frames per second while something animates.
const FPS: u64 = 30;
/// Razer's server is HTTP; it gets every other frame.
const RAZER_EVERY: u64 = 2;
const RETRY: Duration = Duration::from_secs(15);
/// A Razer device type that refused an effect (not connected) is tried again after this.
const RAZER_ABSENT: Duration = Duration::from_secs(30);

impl Lights {
    /// Start the driver. `report` is called (on the driver thread) when the status changes.
    pub fn start(report: Box<dyn Fn(LightsStatus) + Send>) -> Self {
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("gx-lights".into())
            .spawn(move || Driver::new(report).run(rx))
            .ok();
        Self { tx, thread }
    }

    pub fn configure(&self, config: Config) {
        let _ = self.tx.send(Msg::Configure(config));
    }

    pub fn signal(&self, signal: Signal) {
        let _ = self.tx.send(Msg::Signal(signal));
    }

    pub fn set_fullscreen(&self, on: bool) {
        let _ = self.tx.send(Msg::Fullscreen(on));
    }

    pub fn reconnect(&self) {
        let _ = self.tx.send(Msg::Reconnect);
    }
}

impl Drop for Lights {
    /// Give the devices back (OpenRGB modes restored, Razer session ended) before exiting.
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Driver {
    report: Box<dyn Fn(LightsStatus) + Send>,
    config: Option<Config>,
    effects: Option<Effects>,
    fullscreen: bool,
    razer: Option<razer::Session>,
    razer_absent: HashMap<Device, Instant>,
    razer_next_try: Instant,
    openrgb: Option<openrgb::Client>,
    openrgb_next_try: Instant,
    #[cfg(windows)]
    meter: Option<haku_lights::meter::Meter>,
    meter_next_try: Instant,
    status: LightsStatus,
    reported: Option<LightsStatus>,
    frame: u64,
    /// Something changed that static lights must show.
    dirty: bool,
    last_heartbeat: Instant,
    last_refresh: Instant,
}

impl Driver {
    fn new(report: Box<dyn Fn(LightsStatus) + Send>) -> Self {
        let now = Instant::now();
        Self {
            report,
            config: None,
            effects: None,
            fullscreen: false,
            razer: None,
            razer_absent: HashMap::new(),
            razer_next_try: now,
            openrgb: None,
            openrgb_next_try: now,
            #[cfg(windows)]
            meter: None,
            meter_next_try: now,
            status: LightsStatus::default(),
            reported: None,
            frame: 0,
            dirty: true,
            last_heartbeat: now,
            last_refresh: now,
        }
    }

    fn run(mut self, rx: Receiver<Msg>) {
        loop {
            let now = Instant::now();
            let active =
                self.enabled() && self.effects.as_ref().is_some_and(|fx| fx.animating(now));
            let wait = if active {
                Duration::from_millis(1000 / FPS)
            } else {
                Duration::from_millis(500)
            };
            match rx.recv_timeout(wait) {
                Ok(msg) => {
                    if !self.handle(msg) {
                        break;
                    }
                    // Take everything queued before drawing.
                    while let Ok(msg) = rx.try_recv() {
                        if !self.handle(msg) {
                            self.release();
                            return;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            self.tick();
        }
        self.release();
    }

    fn enabled(&self) -> bool {
        self.config.as_ref().is_some_and(|c| c.enabled)
    }

    /// Returns false to stop.
    fn handle(&mut self, msg: Msg) -> bool {
        let now = Instant::now();
        match msg {
            Msg::Stop => return false,
            Msg::Configure(config) => {
                let old = self.config.replace(config.clone());
                match &mut self.effects {
                    Some(fx) => fx.set_look(config.look.clone()),
                    None => {
                        let mut fx = Effects::new(config.look.clone(), now);
                        fx.set_fullscreen(self.fullscreen);
                        self.effects = Some(fx);
                    }
                }
                let old_razer = old.as_ref().is_some_and(|o| o.enabled && o.razer);
                let old_openrgb = old
                    .as_ref()
                    .filter(|o| o.enabled)
                    .and_then(|o| o.openrgb.clone());
                if !(config.enabled && config.razer) || !old_razer {
                    self.drop_razer();
                    self.razer_next_try = now;
                }
                let new_openrgb = config.openrgb.clone().filter(|_| config.enabled);
                if new_openrgb != old_openrgb {
                    self.drop_openrgb();
                    self.openrgb_next_try = now;
                }
                self.dirty = true;
            }
            Msg::Signal(signal) => {
                if let Some(fx) = &mut self.effects {
                    fx.signal(signal, now);
                }
            }
            Msg::Fullscreen(on) => {
                self.fullscreen = on;
                if let Some(fx) = &mut self.effects {
                    fx.set_fullscreen(on);
                }
                self.dirty = true;
            }
            Msg::Reconnect => {
                self.drop_razer();
                self.drop_openrgb();
                self.razer_absent.clear();
                self.razer_next_try = now;
                self.openrgb_next_try = now;
                self.dirty = true;
            }
        }
        true
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let Some(config) = self.config.clone() else {
            return;
        };
        if !config.enabled {
            self.drop_razer();
            self.drop_openrgb();
            self.status = LightsStatus::default();
            self.status.razer.state = "off".into();
            self.status.openrgb.state = "off".into();
            self.publish();
            return;
        }
        self.connect(&config, now);
        self.read_meter(&config, now);

        let Some(fx) = self.effects.as_mut() else {
            return;
        };
        let animating = fx.animating(now);
        // Static lights are re-sent every few seconds, in case another program changed them.
        let resend = self.frame.is_multiple_of(FPS * 3);
        let send = animating || self.dirty || resend;
        self.frame += 1;

        if send && let Some(client) = self.openrgb.as_mut() {
            let mut failed = None;
            for c in client.controllers.clone() {
                let colors = fx.frame(now, c.leds);
                if let Err(e) = client.set_leds(c.index, &colors) {
                    failed = Some(e);
                    break;
                }
            }
            if let Some(e) = failed {
                warn!("OpenRGB connection lost: {e}");
                self.openrgb = None;
                self.openrgb_next_try = now + RETRY;
                self.status.openrgb = LightLink {
                    state: "unavailable".into(),
                    devices: Vec::new(),
                    message: format!("Lost the OpenRGB server: {e}"),
                };
            }
        }
        let razer_turn = self.frame.is_multiple_of(RAZER_EVERY) || self.dirty;
        if send
            && razer_turn
            && let Some(session) = self.razer.as_ref()
        {
            // Synapse accepts effects for device types that are not plugged in, so it cannot
            // tell HakuCX which devices are lit; the status only says the session works.
            let mut lost = None;
            for device in Device::ALL {
                if self
                    .razer_absent
                    .get(&device)
                    .is_some_and(|at| now.duration_since(*at) < RAZER_ABSENT)
                {
                    continue;
                }
                let colors = fx.frame(now, device.grid().1);
                match session.set(device, &colors) {
                    Ok(()) => {}
                    Err(razer::Error::Code(code)) => {
                        log::debug!("Razer {device:?} refused the effect ({code})");
                        self.razer_absent.insert(device, now);
                    }
                    Err(e) => {
                        lost = Some(e);
                        break;
                    }
                }
            }
            if let Some(e) = lost {
                warn!("{e}");
                self.razer = None;
                self.razer_next_try = now + RETRY;
                self.status.razer = LightLink {
                    state: "unavailable".into(),
                    devices: Vec::new(),
                    message: e.to_string(),
                };
            }
        }
        if let Some(session) = self.razer.as_ref()
            && now.duration_since(self.last_heartbeat) >= Duration::from_secs(1)
        {
            self.last_heartbeat = now;
            if let Err(e) = session.heartbeat() {
                warn!("{e}");
                self.razer = None;
                self.razer_next_try = now + RETRY;
            }
        }
        if now.duration_since(self.last_refresh) >= Duration::from_secs(1) {
            self.last_refresh = now;
            if let Some(client) = self.openrgb.as_mut() {
                match client.refresh() {
                    Ok(true) => {
                        self.status.openrgb.devices =
                            client.controllers.iter().map(|c| c.name.clone()).collect();
                        self.dirty = true;
                    }
                    Ok(false) => {}
                    Err(e) => {
                        warn!("OpenRGB connection lost: {e}");
                        self.openrgb = None;
                        self.openrgb_next_try = now + RETRY;
                    }
                }
            }
        }
        if send {
            self.dirty = false;
        }
        self.publish();
    }

    fn connect(&mut self, config: &Config, now: Instant) {
        if !config.razer {
            self.status.razer = LightLink {
                state: "off".into(),
                ..Default::default()
            };
        } else if self.razer.is_none() && now >= self.razer_next_try {
            self.status.razer.state = "connecting".into();
            self.publish();
            match razer::Session::start() {
                Ok(session) => {
                    info!("connected to Razer Chroma");
                    // Razer's samples give a new session a moment before the first effect.
                    std::thread::sleep(Duration::from_millis(800));
                    self.razer = Some(session);
                    self.razer_absent.clear();
                    self.last_heartbeat = now;
                    self.status.razer = LightLink {
                        state: "connected".into(),
                        devices: Vec::new(),
                        message: String::new(),
                    };
                    self.dirty = true;
                }
                Err(e) => {
                    self.razer_next_try = now + RETRY;
                    self.status.razer = LightLink {
                        state: "unavailable".into(),
                        devices: Vec::new(),
                        message: match e {
                            razer::Error::Unreachable(_) => {
                                "Razer Synapse's Chroma SDK is not running on this computer.".into()
                            }
                            other => other.to_string(),
                        },
                    };
                }
            }
        }
        match &config.openrgb {
            None => {
                self.status.openrgb = LightLink {
                    state: "off".into(),
                    ..Default::default()
                }
            }
            Some(address) if self.openrgb.is_none() && now >= self.openrgb_next_try => {
                self.status.openrgb.state = "connecting".into();
                self.publish();
                match openrgb::Client::connect(address, Duration::from_secs(3)) {
                    Ok(client) => {
                        info!(
                            "connected to OpenRGB at {address} (protocol {}, {} devices)",
                            client.protocol(),
                            client.controllers.len()
                        );
                        self.status.openrgb = LightLink {
                            state: "connected".into(),
                            devices: client.controllers.iter().map(|c| c.name.clone()).collect(),
                            message: String::new(),
                        };
                        self.openrgb = Some(client);
                        self.dirty = true;
                    }
                    Err(e) => {
                        self.openrgb_next_try = now + RETRY;
                        self.status.openrgb = LightLink {
                            state: "unavailable".into(),
                            devices: Vec::new(),
                            message: format!(
                                "No OpenRGB SDK server at {address} ({e}). Start OpenRGB and turn on its SDK server."
                            ),
                        };
                    }
                }
            }
            Some(_) => {}
        }
    }

    #[cfg(windows)]
    fn read_meter(&mut self, config: &Config, now: Instant) {
        if !config.look.music {
            self.meter = None;
            return;
        }
        if self.meter.is_none() && now >= self.meter_next_try {
            self.meter = haku_lights::meter::Meter::open();
            // The default output device can change (headphones); reopen now and then.
            self.meter_next_try = now + Duration::from_secs(10);
        }
        let level = self.meter.as_ref().and_then(|m| m.peak());
        if level.is_none() {
            self.meter = None;
        }
        self.status.meter = self.meter.is_some();
        if let Some(fx) = &mut self.effects {
            fx.set_level(level.unwrap_or(0.0));
        }
    }

    #[cfg(not(windows))]
    fn read_meter(&mut self, _config: &Config, _now: Instant) {
        self.status.meter = false;
    }

    fn drop_razer(&mut self) {
        if let Some(session) = self.razer.take() {
            session.close();
        }
    }

    fn drop_openrgb(&mut self) {
        if let Some(mut client) = self.openrgb.take() {
            let _ = client.restore();
        }
    }

    fn release(&mut self) {
        self.drop_razer();
        self.drop_openrgb();
    }

    fn publish(&mut self) {
        if self.reported.as_ref() != Some(&self.status) {
            self.reported = Some(self.status.clone());
            (self.report)(self.status.clone());
        }
    }
}

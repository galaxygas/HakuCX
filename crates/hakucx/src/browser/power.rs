/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Battery saver: on by hand, or automatically on battery power (when unplugged, or below a
//! charge level). While it is on, background music pauses, background tabs are snoozed after
//! a few minutes, and the interface drops its animations (the UI reads `saver` from the
//! power status).

use haku_core::ipc::{Event, PowerStatus};

use super::Browser;

/// Minutes after which background tabs are snoozed while battery saver is on.
pub const SAVER_SNOOZE_MINUTES: i64 = 5;

/// The power source as Windows reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PowerSource {
    pub has_battery: bool,
    pub on_battery: bool,
    pub percent: Option<u8>,
}

#[cfg(windows)]
pub fn power_source() -> PowerSource {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut status = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut status) }.is_err() {
        return PowerSource::default();
    }
    // BatteryFlag 128: no system battery; 255: unknown. ACLineStatus 0: offline (on battery).
    let has_battery = status.BatteryFlag != 128 && status.BatteryFlag != 255;
    PowerSource {
        has_battery,
        on_battery: has_battery && status.ACLineStatus == 0,
        percent: (has_battery && status.BatteryLifePercent <= 100)
            .then_some(status.BatteryLifePercent),
    }
}

#[cfg(not(windows))]
pub fn power_source() -> PowerSource {
    PowerSource::default()
}

/// Whether battery saver is on, and whether it was turned on automatically.
pub fn saver_state(settings: &haku_core::settings::Battery, source: PowerSource) -> (bool, bool) {
    if settings.saver {
        return (true, false);
    }
    let low = settings.auto_below_percent > 0
        && source
            .percent
            .is_some_and(|p| (p as u32) <= settings.auto_below_percent);
    let auto = source.on_battery && (settings.auto_when_unplugged || low);
    (auto, auto)
}

impl Browser {
    /// Re-read the power source (every second, with the metrics) and apply a change.
    pub(crate) fn power_tick(&self) {
        let source = power_source();
        let (saver, automatic) = saver_state(&self.settings.borrow().battery, source);
        let status = PowerStatus {
            has_battery: source.has_battery,
            on_battery: source.on_battery,
            percent: source.percent,
            saver,
            automatic,
        };
        let changed_saver = self.power.borrow().saver != saver;
        if *self.power.borrow() != status {
            *self.power.borrow_mut() = status.clone();
            self.emit(Event::Power { status });
        }
        if changed_saver {
            log::info!("battery saver {}", if saver { "on" } else { "off" });
            self.apply_music();
        }
    }

    pub(crate) fn battery_saver_on(&self) -> bool {
        self.power.borrow().saver
    }

    pub(crate) fn power_emit(&self) {
        let status = self.power.borrow().clone();
        self.emit(Event::Power { status });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use haku_core::settings::Battery;

    #[test]
    fn automatic_saver() {
        let battery = Battery {
            saver: false,
            auto_when_unplugged: false,
            auto_below_percent: 20,
            show_toolbar_icon: false,
        };
        let plugged = PowerSource {
            has_battery: true,
            on_battery: false,
            percent: Some(10),
        };
        let low = PowerSource {
            has_battery: true,
            on_battery: true,
            percent: Some(15),
        };
        let fine = PowerSource {
            has_battery: true,
            on_battery: true,
            percent: Some(60),
        };
        assert_eq!(saver_state(&battery, plugged), (false, false));
        assert_eq!(saver_state(&battery, low), (true, true));
        assert_eq!(saver_state(&battery, fine), (false, false));
        let unplugged = Battery {
            auto_when_unplugged: true,
            ..battery.clone()
        };
        assert_eq!(saver_state(&unplugged, fine), (true, true));
        let manual = Battery {
            saver: true,
            ..battery
        };
        assert_eq!(saver_state(&manual, plugged), (true, false));
        // A desktop without a battery never turns it on by itself.
        let desktop = PowerSource::default();
        assert_eq!(saver_state(&unplugged, desktop), (false, false));
    }
}

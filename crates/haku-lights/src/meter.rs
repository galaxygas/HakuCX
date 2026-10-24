/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The peak level of the sound Windows is playing on the default output device (the same
//! meter the volume mixer shows), for the music visualizer. Only the level is read; no audio
//! is captured.

use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};

pub struct Meter {
    meter: IAudioMeterInformation,
}

impl Meter {
    /// The meter of the current default output device. Call from the thread that will read
    /// it; it joins that thread to COM's multithreaded apartment.
    pub fn open() -> Option<Self> {
        unsafe {
            // S_FALSE (already initialized) is fine; a different apartment is not.
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().ok()?;
            let devices: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let device = devices.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            let meter: IAudioMeterInformation = device.Activate(CLSCTX_ALL, None).ok()?;
            Some(Self { meter })
        }
    }

    /// 0..=1, or `None` if the device went away (open the meter again).
    pub fn peak(&self) -> Option<f32> {
        unsafe { self.meter.GetPeakValue().ok() }
    }
}

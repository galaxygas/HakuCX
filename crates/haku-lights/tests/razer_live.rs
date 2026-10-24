/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Against the real Razer Chroma SDK server (needs Razer Synapse). Run by hand:
//! `cargo test -p haku-lights --test razer_live -- --ignored --nocapture`.
//! Shows a red-to-blue gradient for about three seconds, then gives the lights back.

use std::time::Duration;

use haku_lights::Rgb;
use haku_lights::razer::{Device, Session};

#[test]
#[ignore = "changes the lights of connected Razer devices"]
fn razer_session() {
    let session = Session::start().expect("Razer Chroma SDK server");
    // Razer's samples wait for the new session to be ready before the first effect.
    std::thread::sleep(Duration::from_secs(1));
    let mut answered = Vec::new();
    for step in 0..6 {
        for device in Device::ALL {
            let cols = device.grid().1;
            let line: Vec<Rgb> = (0..cols)
                .map(|i| {
                    let t = (i as f32 / cols.max(2) as f32 + step as f32 / 6.0).fract();
                    Rgb::new(255, 0, 0).lerp(Rgb::new(0, 0, 255), t)
                })
                .collect();
            let result = session.set(device, &line);
            if step == 0 {
                println!("{device:?}: {result:?}");
                answered.push(result.is_ok());
            }
        }
        session.heartbeat().expect("heartbeat");
        std::thread::sleep(Duration::from_millis(500));
    }
    session.close();
    assert!(
        answered.iter().any(|ok| *ok),
        "no device type accepted an effect"
    );
}

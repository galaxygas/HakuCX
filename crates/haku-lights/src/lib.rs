/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! GX Lights: the lighting effects (theme colors, idle animations, browser event flashes, a
//! music visualizer) and the two ways HakuCX reaches RGB devices:
//!
//! - [`razer`]: the Razer Chroma SDK REST server that Razer Synapse runs on this computer.
//! - [`openrgb`]: an OpenRGB SDK server (ASUS, Corsair, Logitech, MSI, SteelSeries and many
//!   more), local or on the network.
//!
//! Nothing here talks to the internet.

pub mod effects;
#[cfg(windows)]
pub mod meter;
pub mod openrgb;
pub mod razer;

/// An sRGB color.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Rgb = Rgb::new(0, 0, 0);
    pub const WHITE: Rgb = Rgb::new(255, 255, 255);

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// `#rrggbb` (the form theme palettes use).
    pub fn parse_hex(hex: &str) -> Option<Rgb> {
        let h = hex.strip_prefix('#')?;
        if h.len() != 6 || !h.is_ascii() {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some(Rgb::new(byte(0)?, byte(2)?, byte(4)?))
    }

    /// Mix towards `other`: `t` = 0 gives `self`, 1 gives `other`.
    pub fn lerp(self, other: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgb::new(
            mix(self.r, other.r),
            mix(self.g, other.g),
            mix(self.b, other.b),
        )
    }

    /// Multiply the brightness by `k` (0..=1).
    pub fn scale(self, k: f32) -> Rgb {
        let k = k.clamp(0.0, 1.0);
        let s = |c: u8| (c as f32 * k).round() as u8;
        Rgb::new(s(self.r), s(self.g), s(self.b))
    }
}

#[cfg(test)]
mod tests {
    use super::Rgb;

    #[test]
    fn colors() {
        assert_eq!(Rgb::parse_hex("#fa1e4e"), Some(Rgb::new(0xfa, 0x1e, 0x4e)));
        assert_eq!(Rgb::parse_hex("fa1e4e"), None);
        assert_eq!(Rgb::parse_hex("#fff"), None);
        let red = Rgb::new(200, 0, 0);
        assert_eq!(red.lerp(Rgb::WHITE, 0.0), red);
        assert_eq!(red.lerp(Rgb::WHITE, 1.0), Rgb::WHITE);
        assert_eq!(red.scale(0.5), Rgb::new(100, 0, 0));
        assert_eq!(red.scale(2.0), red);
    }
}

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Built-in themes and the palette derivation that turns a theme (or a user's custom
//! primary/secondary colors) into the CSS custom properties the UI uses.
//!
//! The 13 GX-style themes use colors sampled pixel-exact from Opera GX research screenshots
//! (`galaxygas/operagx`, `Configure/themes examples`). Names describe the look; HakuCX is not
//! affiliated with Opera.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::settings::ColorMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn parse(hex: &str) -> Option<Rgb> {
        let h = hex.trim().trim_start_matches('#');
        let expand = |c: u8| c * 17;
        match h.len() {
            3 => {
                let v = u16::from_str_radix(h, 16).ok()?;
                Some(Rgb(
                    expand(((v >> 8) & 0xF) as u8),
                    expand(((v >> 4) & 0xF) as u8),
                    expand((v & 0xF) as u8),
                ))
            }
            6 => {
                let v = u32::from_str_radix(h, 16).ok()?;
                Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
            }
            _ => None,
        }
    }

    /// Linear blend toward `other`; `t = 0` is self, `t = 1` is other.
    pub fn mix(self, other: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgb(
            lerp(self.0, other.0),
            lerp(self.1, other.1),
            lerp(self.2, other.2),
        )
    }

    /// WCAG relative luminance.
    pub fn luminance(self) -> f32 {
        let channel = |c: u8| {
            let c = c as f32 / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(self.0) + 0.7152 * channel(self.1) + 0.0722 * channel(self.2)
    }

    pub fn contrast(self, other: Rgb) -> f32 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }

    pub fn rgba(self, alpha: f32) -> String {
        format!(
            "rgba({}, {}, {}, {})",
            self.0,
            self.1,
            self.2,
            (alpha * 1000.0).round() / 1000.0
        )
    }
}

impl fmt::Display for Rgb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

pub const BLACK: Rgb = Rgb(0, 0, 0);
pub const WHITE: Rgb = Rgb(255, 255, 255);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Variant {
    pub accent: Rgb,
    /// The muted accent used for selected tabs, slider tracks, toggle tracks.
    pub accent_dim: Rgb,
    pub background: Rgb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeFamily {
    Gx,
    Haku,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    pub family: ThemeFamily,
    pub dark: Variant,
    pub light: Variant,
    /// Default wallpaper id for this theme.
    pub wallpaper: &'static str,
}

const fn rgb(hex: u32) -> Rgb {
    Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn v(accent: u32, accent_dim: u32, background: u32) -> Variant {
    Variant {
        accent: rgb(accent),
        accent_dim: rgb(accent_dim),
        background: rgb(background),
    }
}

/// Light variants: accent and page background were sampled from the light screenshots; the dim
/// accent is derived (accent mixed 70% toward white).
const fn light(accent: u32, accent_dim: u32, background: u32) -> Variant {
    v(accent, accent_dim, background)
}

pub const THEMES: &[Theme] = &[
    Theme {
        id: "gx-classic",
        name: "GX Classic",
        family: ThemeFamily::Gx,
        dark: v(0xfa1e4e, 0x96122f, 0x120f19),
        light: light(0xfa1e4e, 0xfdbbca, 0xf4f3f7),
        wallpaper: "gx-ribbons",
    },
    Theme {
        id: "ultraviolet",
        name: "Ultraviolet",
        family: ThemeFamily::Gx,
        dark: v(0x7d46f2, 0x4b2a91, 0x121217),
        light: light(0x8635ff, 0xdbc2ff, 0xf1f2f8),
        wallpaper: "gx-ribbons",
    },
    Theme {
        id: "sub-zero",
        name: "Sub Zero",
        family: ThemeFamily::Gx,
        dark: v(0x4461ee, 0x293a8f, 0x131316),
        light: light(0x1652e9, 0xb9cbf8, 0xf3f5f7),
        wallpaper: "gx-ribbons",
    },
    Theme {
        id: "frutti-di-mare",
        name: "Frutti Di Mare",
        family: ThemeFamily::Gx,
        dark: v(0xff7070, 0x994343, 0x0c0f1d),
        light: light(0xff5959, 0xffcdcd, 0xf2f2f8),
        wallpaper: "gx-ribbons",
    },
    Theme {
        id: "purple-haze",
        name: "Purple Haze",
        family: ThemeFamily::Gx,
        dark: v(0xaeff3c, 0x689924, 0x160e1a),
        light: light(0x2cb40d, 0xc0e9b7, 0xf5f3f6),
        wallpaper: "gx-ribbons",
    },
    Theme {
        id: "vaporwave",
        name: "Vaporwave",
        family: ThemeFamily::Gx,
        dark: v(0x00e5c3, 0x008975, 0x191019),
        light: light(0x5540bf, 0xccc6ec, 0xf7f3f7),
        wallpaper: "gx-ribbons",
    },
    Theme {
        id: "rose-quartz",
        name: "Rose Quartz",
        family: ThemeFamily::Gx,
        dark: v(0xff708d, 0x994355, 0x1a0f17),
        light: light(0xeb4e9a, 0xf9cae1, 0xfaeff3),
        wallpaper: "gx-ribbons",
    },
    Theme {
        id: "coming-soon",
        name: "Coming Soon",
        family: ThemeFamily::Gx,
        dark: v(0xeede00, 0x8f8500, 0x0c1b1c),
        light: light(0x00a4cd, 0xb3e4f0, 0xf2f6f7),
        wallpaper: "gx-ribbons",
    },
    Theme {
        id: "hackerman",
        name: "Hackerman",
        family: ThemeFamily::Gx,
        dark: v(0x00ed3b, 0x008e23, 0x121713),
        light: light(0x009d5e, 0xb3e2cf, 0xf3f7f6),
        wallpaper: "gx-ribbons",
    },
    Theme {
        id: "lambda",
        name: "Lambda",
        family: ThemeFamily::Gx,
        dark: v(0xff9900, 0x995c00, 0x131615),
        light: light(0xf07c00, 0xfbd8b3, 0xf4f5f5),
        wallpaper: "gx-ribbons",
    },
    Theme {
        id: "after-eight",
        name: "After Eight",
        family: ThemeFamily::Gx,
        dark: v(0x5aedbc, 0x368e71, 0x171412),
        light: light(0x00a870, 0xb3e5d4, 0xf7f5f3),
        wallpaper: "gx-ribbons",
    },
    Theme {
        id: "pay-to-win",
        name: "Pay-To-Win",
        family: ThemeFamily::Gx,
        dark: v(0xf6c955, 0x947933, 0x171512),
        light: light(0xe79f03, 0xf8e2b3, 0xf7f5f3),
        wallpaper: "gx-ribbons",
    },
    Theme {
        id: "white-wolf",
        name: "White Wolf",
        family: ThemeFamily::Gx,
        dark: v(0xcccccc, 0xc2c2c2, 0x141414),
        light: light(0x3d3d3d, 0xc5c5c5, 0xf5f5f5),
        wallpaper: "gx-ribbons",
    },
    // ---- HakuCX originals ----
    Theme {
        id: "haku",
        name: "Haku",
        family: ThemeFamily::Haku,
        dark: v(0x7fe3ff, 0x2f6f86, 0x0b1016),
        light: light(0x0089b8, 0xb3dcea, 0xf2f6f8),
        wallpaper: "haku-tide",
    },
    Theme {
        id: "sakura",
        name: "Sakura",
        family: ThemeFamily::Haku,
        dark: v(0xff9ecb, 0x8f4d6c, 0x170f14),
        light: light(0xd94c8c, 0xf4c9dc, 0xfbf3f6),
        wallpaper: "haku-petals",
    },
    Theme {
        id: "nebula",
        name: "Nebula",
        family: ThemeFamily::Haku,
        dark: v(0xb57bff, 0x5d3d8c, 0x0d0a17),
        light: light(0x7a3fd1, 0xd7c5f1, 0xf4f1fa),
        wallpaper: "haku-nebula",
    },
    Theme {
        id: "solar-flare",
        name: "Solar Flare",
        family: ThemeFamily::Haku,
        dark: v(0xff5a1f, 0x8f3412, 0x150d0a),
        light: light(0xd94210, 0xf4c6b7, 0xfaf4f1),
        wallpaper: "haku-ember",
    },
    Theme {
        id: "abyss",
        name: "Abyss",
        family: ThemeFamily::Haku,
        dark: v(0x2ee6a6, 0x16735a, 0x05090b),
        light: light(0x0c9a6c, 0xb6e1d3, 0xf1f7f5),
        wallpaper: "haku-depths",
    },
    Theme {
        id: "carbon",
        name: "Carbon",
        family: ThemeFamily::Haku,
        dark: v(0xe8e8e8, 0x5a5a5a, 0x0a0a0a),
        light: light(0x1a1a1a, 0xbdbdbd, 0xfafafa),
        wallpaper: "haku-carbon",
    },
];

/// Primary color swatches offered by the custom theme editor (dark), as in Opera GX.
pub const PRIMARY_SWATCHES: &[&str] = &[
    "#ff0077", "#fa1e4e", "#ff6c22", "#ffe20f", "#cacaca", "#8d35e2", "#4d4dff", "#00ffff",
    "#00ffa2", "#36ff4e",
];

/// Secondary (background tint) swatches offered by the custom theme editor (dark).
pub const SECONDARY_SWATCHES: &[&str] = &[
    "#331f28", "#331f23", "#33261f", "#33301f", "#3d3d3d", "#291f33", "#1f1f33", "#1f3333",
    "#1f332b", "#1f3321",
];

pub fn theme(id: &str) -> Option<&'static Theme> {
    THEMES.iter().find(|t| t.id == id)
}

pub fn default_theme() -> &'static Theme {
    theme("ultraviolet").expect("ultraviolet is built in")
}

/// A user-made theme stored in the profile (`themes.json`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UserTheme {
    pub id: String,
    pub name: String,
    pub dark_primary: String,
    pub dark_secondary: String,
    pub light_primary: String,
    pub light_secondary: String,
    pub wallpaper: String,
}

/// Fully resolved palette handed to the UI as CSS custom properties.
#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct Palette {
    pub dark: bool,
    pub vars: BTreeMap<String, String>,
}

/// A theme as the UI sees it (colors as `#rrggbb`).
#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ThemeInfo {
    pub id: String,
    pub name: String,
    pub family: String,
    pub dark_accent: String,
    pub dark_background: String,
    pub light_accent: String,
    pub light_background: String,
    pub wallpaper: String,
    pub user: bool,
}

impl ThemeInfo {
    pub fn from_builtin(t: &Theme) -> Self {
        Self {
            id: t.id.into(),
            name: t.name.into(),
            family: match t.family {
                ThemeFamily::Gx => "gx".into(),
                ThemeFamily::Haku => "haku".into(),
            },
            dark_accent: t.dark.accent.to_string(),
            dark_background: t.dark.background.to_string(),
            light_accent: t.light.accent.to_string(),
            light_background: t.light.background.to_string(),
            wallpaper: t.wallpaper.into(),
            user: false,
        }
    }

    pub fn from_user(t: &UserTheme) -> Self {
        Self {
            id: t.id.clone(),
            name: t.name.clone(),
            family: "user".into(),
            dark_accent: t.dark_primary.clone(),
            dark_background: t.dark_secondary.clone(),
            light_accent: t.light_primary.clone(),
            light_background: t.light_secondary.clone(),
            wallpaper: t.wallpaper.clone(),
            user: true,
        }
    }
}

/// Built-in themes followed by the user's own.
pub fn all_themes(user: &[UserTheme]) -> Vec<ThemeInfo> {
    THEMES
        .iter()
        .map(ThemeInfo::from_builtin)
        .chain(user.iter().map(ThemeInfo::from_user))
        .collect()
}

/// Resolve the palette for whatever theme id the settings name (built-in or user theme).
pub fn palette_for_settings(
    appearance: &crate::settings::Appearance,
    user: &[UserTheme],
    system_dark: bool,
) -> Palette {
    if let Some(u) = user.iter().find(|u| u.id == appearance.theme) {
        let dark = match appearance.color_mode {
            ColorMode::Dark => true,
            ColorMode::Light => false,
            ColorMode::Auto => system_dark,
        };
        let (primary, secondary) =
            if dark { (&u.dark_primary, &u.dark_secondary) } else { (&u.light_primary, &u.light_secondary) };
        let base = default_theme();
        return palette_for(
            base,
            appearance.color_mode,
            system_dark,
            appearance.primary_color.as_deref().or(Some(primary)),
            appearance.secondary_color.as_deref().or(Some(secondary)),
        );
    }
    let theme = theme(&appearance.theme).unwrap_or_else(default_theme);
    palette_for(
        theme,
        appearance.color_mode,
        system_dark,
        appearance.primary_color.as_deref(),
        appearance.secondary_color.as_deref(),
    )
}

/// Build the palette from an accent and a background. `secondary` (if any) tints the
/// background the way Opera GX's "Secondary Color" does.
pub fn derive_palette(accent: Rgb, accent_dim: Rgb, background: Rgb, dark: bool) -> Palette {
    let mut vars = BTreeMap::new();
    let mut put = |k: &str, v: String| {
        vars.insert(k.to_owned(), v);
    };
    let (text, ink) = if dark {
        (WHITE, BLACK)
    } else {
        (rgb(0x16161a), WHITE)
    };
    put("--hx-accent", accent.to_string());
    put("--hx-accent-dim", accent_dim.to_string());
    put(
        "--hx-accent-soft",
        accent.rgba(if dark { 0.16 } else { 0.12 }),
    );
    put("--hx-accent-glow", accent.rgba(0.45));
    // Text drawn on top of an accent fill must stay readable on any accent.
    let on_accent = if accent.contrast(WHITE) >= accent.contrast(rgb(0x101014)) {
        WHITE
    } else {
        rgb(0x101014)
    };
    put("--hx-on-accent", on_accent.to_string());
    put("--hx-bg", background.to_string());
    if dark {
        put("--hx-frame", background.mix(BLACK, 0.92).to_string());
        put("--hx-chrome", background.mix(BLACK, 0.35).to_string());
        put("--hx-panel", background.mix(BLACK, 0.55).to_string());
        put("--hx-elev", background.mix(WHITE, 0.035).to_string());
        put("--hx-elev-2", background.mix(WHITE, 0.07).to_string());
        put("--hx-input", background.mix(BLACK, 0.5).to_string());
        put("--hx-hover", background.mix(WHITE, 0.09).to_string());
        put("--hx-selected", background.mix(WHITE, 0.11).to_string());
        put("--hx-line", WHITE.rgba(0.07));
        put("--hx-line-strong", WHITE.rgba(0.14));
        put("--hx-muted", text.mix(background, 0.38).to_string());
        put("--hx-dim", text.mix(background, 0.58).to_string());
        put("--hx-toggle-off", background.mix(WHITE, 0.06).to_string());
        put("--hx-shadow", BLACK.rgba(0.55));
    } else {
        put("--hx-frame", background.mix(WHITE, 0.6).to_string());
        put("--hx-chrome", background.mix(WHITE, 0.45).to_string());
        put("--hx-panel", background.mix(WHITE, 0.7).to_string());
        put("--hx-elev", WHITE.to_string());
        put("--hx-elev-2", background.mix(BLACK, 0.03).to_string());
        put("--hx-input", WHITE.to_string());
        put("--hx-hover", background.mix(BLACK, 0.05).to_string());
        put("--hx-selected", background.mix(BLACK, 0.08).to_string());
        put("--hx-line", BLACK.rgba(0.08));
        put("--hx-line-strong", BLACK.rgba(0.16));
        put("--hx-muted", text.mix(background, 0.35).to_string());
        put("--hx-dim", text.mix(background, 0.52).to_string());
        put("--hx-toggle-off", background.mix(BLACK, 0.12).to_string());
        put("--hx-shadow", BLACK.rgba(0.16));
    }
    put("--hx-text", text.to_string());
    put("--hx-ink", ink.to_string());
    put(
        "--hx-danger",
        if dark {
            "#ff4458".into()
        } else {
            "#d92035".into()
        },
    );
    put(
        "--hx-ok",
        if dark {
            "#3ddc84".into()
        } else {
            "#16a34a".into()
        },
    );
    put(
        "--hx-warn",
        if dark {
            "#ffb020".into()
        } else {
            "#c47f00".into()
        },
    );
    Palette { dark, vars }
}

/// Resolve the palette for a theme in the given mode. `system_dark` is the OS preference,
/// consulted only in `Auto` mode.
pub fn palette_for(
    theme: &Theme,
    mode: ColorMode,
    system_dark: bool,
    primary_override: Option<&str>,
    secondary_override: Option<&str>,
) -> Palette {
    let dark = match mode {
        ColorMode::Dark => true,
        ColorMode::Light => false,
        ColorMode::Auto => system_dark,
    };
    let base = if dark { theme.dark } else { theme.light };
    let accent = primary_override.and_then(Rgb::parse).unwrap_or(base.accent);
    let accent_dim = if primary_override.is_some() {
        if dark {
            accent.mix(BLACK, 0.4)
        } else {
            accent.mix(WHITE, 0.7)
        }
    } else {
        base.accent_dim
    };
    let background = match secondary_override.and_then(Rgb::parse) {
        // Opera GX's secondary swatches are mid-dark tints; scale them down to a page
        // background so text contrast stays above 12:1 in dark mode.
        Some(tint) if dark => tint.mix(BLACK, 0.55),
        Some(tint) => tint.mix(WHITE, 0.9),
        None => base.background,
    };
    derive_palette(accent, accent_dim, background, dark)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_format() {
        assert_eq!(Rgb::parse("#7d46f2"), Some(Rgb(0x7d, 0x46, 0xf2)));
        assert_eq!(Rgb::parse("fff"), Some(WHITE));
        assert_eq!(Rgb::parse("#12345"), None);
        assert_eq!(Rgb(0x7d, 0x46, 0xf2).to_string(), "#7d46f2");
    }

    #[test]
    fn all_gx_themes_present_with_unique_ids() {
        let gx: Vec<_> = THEMES
            .iter()
            .filter(|t| t.family == ThemeFamily::Gx)
            .collect();
        assert_eq!(gx.len(), 13);
        let mut ids: Vec<_> = THEMES.iter().map(|t| t.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), THEMES.len());
    }

    #[test]
    fn research_values_are_exact() {
        let uv = theme("ultraviolet").unwrap();
        assert_eq!(uv.dark.accent.to_string(), "#7d46f2");
        assert_eq!(uv.dark.accent_dim.to_string(), "#4b2a91");
        assert_eq!(uv.dark.background.to_string(), "#121217");
        assert_eq!(
            theme("gx-classic").unwrap().dark.accent.to_string(),
            "#fa1e4e"
        );
    }

    #[test]
    fn text_contrast_is_readable_in_every_theme() {
        for t in THEMES {
            for dark in [true, false] {
                let p = palette_for(
                    t,
                    if dark {
                        ColorMode::Dark
                    } else {
                        ColorMode::Light
                    },
                    true,
                    None,
                    None,
                );
                let text = Rgb::parse(&p.vars["--hx-text"]).unwrap();
                let bg = Rgb::parse(&p.vars["--hx-bg"]).unwrap();
                assert!(text.contrast(bg) >= 7.0, "{} dark={dark}", t.id);
                let accent = Rgb::parse(&p.vars["--hx-accent"]).unwrap();
                let on = Rgb::parse(&p.vars["--hx-on-accent"]).unwrap();
                assert!(accent.contrast(on) >= 2.5, "{} on-accent dark={dark}", t.id);
            }
        }
    }

    #[test]
    fn auto_mode_follows_system() {
        let t = default_theme();
        assert!(palette_for(t, ColorMode::Auto, true, None, None).dark);
        assert!(!palette_for(t, ColorMode::Auto, false, None, None).dark);
    }

    #[test]
    fn overrides_apply() {
        let p = palette_for(
            default_theme(),
            ColorMode::Dark,
            true,
            Some("#fa1e4e"),
            Some("#1f3333"),
        );
        assert_eq!(p.vars["--hx-accent"], "#fa1e4e");
        assert_ne!(p.vars["--hx-bg"], "#121217");
    }
}

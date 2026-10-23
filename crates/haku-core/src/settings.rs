/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The complete HakuCX settings document.
//!
//! Every option is persisted in one JSON document per profile (`settings.json`). Field names
//! are camelCase on disk and over IPC. Unknown keys are preserved in [`Settings::extra`] so a
//! newer UI or a mod never loses data when an older core rewrites the file.
//!
//! Sections follow the Opera GX settings layout (Appearance, Features, Video, Privacy, ...)
//! so the settings UI can map 1:1 onto this schema.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Bump when a migration in [`Settings::migrate`] is added.
pub const SETTINGS_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Settings {
    pub version: u32,
    pub appearance: Appearance,
    pub tabs: TabSettings,
    pub sidebar: SidebarSettings,
    pub start_page: StartPage,
    pub features: Features,
    pub sounds: Sounds,
    pub lights: Lights,
    pub battery: Battery,
    pub player: Player,
    pub video: Video,
    pub control: GxControl,
    pub cleaner: Cleaner,
    pub panic: Panic,
    pub privacy: Privacy,
    pub permissions: Permissions,
    pub search: Search,
    pub startup: Startup,
    pub languages: Languages,
    pub downloads: Downloads,
    pub accessibility: Accessibility,
    pub system: System,
    pub sync: Sync,
    pub smart_home: SmartHome,
    pub ai: Ai,
    pub shortcuts: Shortcuts,
    pub early_bird: EarlyBird,
    /// Keys this build does not know about, kept verbatim.
    #[serde(flatten)]
    #[ts(skip)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            appearance: Appearance::default(),
            tabs: TabSettings::default(),
            sidebar: SidebarSettings::default(),
            start_page: StartPage::default(),
            features: Features::default(),
            sounds: Sounds::default(),
            lights: Lights::default(),
            battery: Battery::default(),
            player: Player::default(),
            video: Video::default(),
            control: GxControl::default(),
            cleaner: Cleaner::default(),
            panic: Panic::default(),
            privacy: Privacy::default(),
            permissions: Permissions::default(),
            search: Search::default(),
            startup: Startup::default(),
            languages: Languages::default(),
            downloads: Downloads::default(),
            accessibility: Accessibility::default(),
            system: System::default(),
            sync: Sync::default(),
            smart_home: SmartHome::default(),
            ai: Ai::default(),
            shortcuts: Shortcuts::default(),
            early_bird: EarlyBird::default(),
            extra: BTreeMap::new(),
        }
    }
}

impl Settings {
    /// Parse a settings document, filling any missing field with its default.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        let mut settings: Settings = serde_json::from_str(text)?;
        settings.migrate();
        settings.clamp();
        Ok(settings)
    }

    /// Apply a JSON merge patch (RFC 7396) and re-validate. This is how the UI changes
    /// settings: it sends only the keys it touched.
    pub fn apply_patch(&mut self, patch: &serde_json::Value) -> Result<(), serde_json::Error> {
        let mut doc = serde_json::to_value(&*self)?;
        merge_patch(&mut doc, patch);
        let mut next: Settings = serde_json::from_value(doc)?;
        next.migrate();
        next.clamp();
        *self = next;
        Ok(())
    }

    fn migrate(&mut self) {
        // Version 1 is the first schema; future migrations go here, keyed on `self.version`.
        self.version = SETTINGS_VERSION;
    }

    /// Keep numeric settings inside the ranges the UI offers, so a hand-edited file or a
    /// malicious mod cannot push the browser into a broken state.
    pub fn clamp(&mut self) {
        let a = &mut self.appearance;
        a.opacity = a.opacity.clamp(0.2, 1.0);
        a.blur = a.blur.clamp(0.0, 40.0);
        a.elements_background = a.elements_background.clamp(0.0, 100.0);
        a.vignette = a.vignette.clamp(0.0, 1.0);
        a.roundness = a.roundness.clamp(0.0, 24.0);
        a.page_zoom = a.page_zoom.clamp(25, 500);
        a.ui_scale = a.ui_scale.clamp(75, 150);
        let c = &mut self.control;
        c.cpu_limit = c.cpu_limit.clamp(1, 100);
        c.ram_limit_mb = c.ram_limit_mb.clamp(256, 1024 * 1024);
        let s = &mut self.sounds;
        s.browser_volume = s.browser_volume.clamp(0, 100);
        s.keyboard_volume = s.keyboard_volume.clamp(0, 100);
        s.music_volume = s.music_volume.clamp(0, 100);
        for level in s.levels.values_mut() {
            *level = (*level).clamp(0, 100);
        }
        self.lights.brightness = self.lights.brightness.clamp(0, 100);
        self.start_page.dial_columns = self.start_page.dial_columns.clamp(2, 12);
        self.player.resume_after_secs = self.player.resume_after_secs.clamp(0, 60);
    }
}

/// RFC 7396 JSON merge patch.
pub fn merge_patch(target: &mut serde_json::Value, patch: &serde_json::Value) {
    use serde_json::Value;
    match patch {
        Value::Object(patch_map) => {
            if !target.is_object() {
                *target = Value::Object(serde_json::Map::new());
            }
            let target_map = target.as_object_mut().expect("just ensured object");
            for (key, value) in patch_map {
                if value.is_null() {
                    target_map.remove(key);
                } else {
                    merge_patch(target_map.entry(key.clone()).or_insert(Value::Null), value);
                }
            }
        }
        other => *target = other.clone(),
    }
}

// ---------------------------------------------------------------------------------------------
// Appearance
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ColorMode {
    Light,
    Auto,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum InterfaceMode {
    Auto,
    Custom,
}

/// Opera GX "Show window decoration": Outside / Outside except maximized / Inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum WindowDecoration {
    Outside,
    OutsideExceptMaximized,
    Inside,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum CornerStyle {
    Sharp,
    Cut,
    Round,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum FontSize {
    VerySmall,
    Small,
    Medium,
    Large,
    VeryLarge,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Appearance {
    /// Theme id from [`crate::themes`] or a user theme id (`user:<uuid>`).
    pub theme: String,
    pub color_mode: ColorMode,
    /// Primary (accent) color override, `#rrggbb`. `None` = theme default.
    pub primary_color: Option<String>,
    /// Secondary (background tint) color override, `#rrggbb`. `None` = theme default.
    pub secondary_color: Option<String>,
    pub show_wallpaper: bool,
    /// Built-in wallpaper id, or `custom`.
    pub wallpaper: String,
    /// Path inside the profile's `wallpapers/` directory when `wallpaper == "custom"`.
    pub custom_wallpaper: Option<String>,
    pub animated_wallpaper: bool,
    pub daily_wallpapers: bool,
    pub interface_mode: InterfaceMode,
    pub opacity: f32,
    pub blur: f32,
    pub elements_background: f32,
    pub vignette: f32,
    pub focus_mode: bool,
    pub window_decoration: WindowDecoration,
    /// Use the operating system's title bar instead of the GX-style frameless window.
    pub native_title_bar: bool,
    pub gx_messenger_icons: bool,
    pub colorize_icons: bool,
    pub show_bookmarks_bar: bool,
    pub split_screen: bool,
    pub split_screen_drag: bool,
    pub reduce_animations: bool,
    pub font_size: FontSize,
    pub font_standard: String,
    pub font_serif: String,
    pub font_sans: String,
    pub font_fixed: String,
    pub font_minimum: u32,
    /// Default page zoom, percent.
    pub page_zoom: u32,
    /// HakuCX extra: scale of the browser UI itself, percent.
    pub ui_scale: u32,
    pub corner_style: CornerStyle,
    pub roundness: f32,
    pub crosshair_cursor: bool,
    pub splash_screen: bool,
    /// HakuCX extra: user CSS applied to the browser UI.
    pub ui_css: String,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "ultraviolet".into(),
            color_mode: ColorMode::Dark,
            primary_color: None,
            secondary_color: None,
            show_wallpaper: true,
            wallpaper: "gx-ribbons".into(),
            custom_wallpaper: None,
            animated_wallpaper: true,
            daily_wallpapers: false,
            interface_mode: InterfaceMode::Auto,
            opacity: 1.0,
            blur: 0.0,
            elements_background: 18.0,
            vignette: 0.55,
            focus_mode: false,
            window_decoration: WindowDecoration::Outside,
            native_title_bar: false,
            gx_messenger_icons: false,
            colorize_icons: true,
            show_bookmarks_bar: true,
            split_screen: true,
            split_screen_drag: true,
            reduce_animations: false,
            font_size: FontSize::Medium,
            font_standard: "Segoe UI".into(),
            font_serif: "Times New Roman".into(),
            font_sans: "Arial".into(),
            font_fixed: "Consolas".into(),
            font_minimum: 0,
            page_zoom: 100,
            ui_scale: 100,
            corner_style: CornerStyle::Cut,
            roundness: 8.0,
            crosshair_cursor: false,
            splash_screen: true,
            ui_css: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tabs / user interface
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum TraceThreshold {
    Off,
    MoreThan10,
    MoreThan15,
    MoreThan30,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct TabSettings {
    pub auto_islands: bool,
    pub traces: TraceThreshold,
    pub snooze_inactive: bool,
    /// Minutes of inactivity before a background tab is snoozed.
    pub snooze_after_minutes: u32,
    pub snooze_on_memory_pressure: bool,
    pub address_suggestions: bool,
    pub suggestions_before_typing: bool,
    pub tab_previews: bool,
    pub enhanced_tooltips: bool,
    pub tooltip_same_domain: bool,
    pub search_in_tabs: bool,
    pub show_account_button: bool,
    pub mru_cycling: bool,
    pub show_full_url: bool,
    pub scroll_top_on_active_click: bool,
    pub close_external_app_tabs: bool,
    pub warn_close_window: bool,
    pub warn_quit: bool,
    pub vertical_tabs: bool,
    pub tab_emojis: bool,
}

impl Default for TabSettings {
    fn default() -> Self {
        Self {
            auto_islands: true,
            traces: TraceThreshold::MoreThan30,
            snooze_inactive: true,
            snooze_after_minutes: 30,
            snooze_on_memory_pressure: true,
            address_suggestions: true,
            suggestions_before_typing: true,
            tab_previews: true,
            enhanced_tooltips: true,
            tooltip_same_domain: true,
            search_in_tabs: true,
            show_account_button: true,
            mru_cycling: false,
            show_full_url: false,
            scroll_top_on_active_click: true,
            close_external_app_tabs: true,
            warn_close_window: true,
            warn_quit: true,
            vertical_tabs: false,
            tab_emojis: true,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct SidebarSettings {
    pub show: bool,
    pub messenger_notifications: bool,
    pub notification_badges: bool,
    pub center_icons: bool,
    pub corner_in_sidebar: bool,
    pub workspaces_enabled: bool,
    /// Ordered sidebar item ids (see `crate::catalog::SIDEBAR_ITEMS`).
    pub items: Vec<String>,
    /// Workspace ids hidden from the sidebar workspace switcher.
    pub hidden_workspaces: Vec<String>,
    /// Pin state of each web panel (stays open when clicking the page).
    pub pinned_panels: Vec<String>,
    /// Width in CSS px per panel id.
    pub panel_widths: BTreeMap<String, u32>,
}

impl Default for SidebarSettings {
    fn default() -> Self {
        Self {
            show: true,
            messenger_notifications: true,
            notification_badges: true,
            center_icons: false,
            corner_in_sidebar: true,
            workspaces_enabled: true,
            items: [
                "workspaces",
                "gx-control",
                "gx-cleaner",
                "shaders",
                "mods",
                "twitch",
                "discord",
                "whatsapp",
                "player",
                "history",
                "extensions",
                "settings",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            hidden_workspaces: Vec::new(),
            pinned_panels: Vec::new(),
            panel_widths: BTreeMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Start page
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum DialPosition {
    Auto,
    Top,
    Center,
    Bottom,
    Left,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum DialEffect {
    None,
    Skew,
    Glow,
    Tilt,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct StartPage {
    pub hide_search: bool,
    pub hide_speed_dial: bool,
    pub hide_add_site: bool,
    pub dial_position: DialPosition,
    pub dial_effect: DialEffect,
    pub show_dial_names: bool,
    pub bigger_tiles: bool,
    pub dial_suggestions: bool,
    pub game_strips: bool,
    pub show_news: bool,
    pub show_continue: bool,
    pub auto_columns: bool,
    pub dial_columns: u32,
    pub widgets: StartWidgets,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct StartWidgets {
    pub weather: bool,
    /// Free-text city resolved through Open-Meteo geocoding.
    pub weather_city: String,
    pub weather_lat: Option<f64>,
    pub weather_lon: Option<f64>,
    pub fahrenheit: bool,
    pub clock: bool,
    pub touch_grass: bool,
}

impl Default for StartWidgets {
    fn default() -> Self {
        Self {
            weather: true,
            weather_city: String::new(),
            weather_lat: None,
            weather_lon: None,
            fahrenheit: false,
            clock: false,
            touch_grass: false,
        }
    }
}

impl Default for StartPage {
    fn default() -> Self {
        Self {
            hide_search: false,
            hide_speed_dial: false,
            hide_add_site: false,
            dial_position: DialPosition::Auto,
            dial_effect: DialEffect::None,
            show_dial_names: true,
            bigger_tiles: false,
            dial_suggestions: true,
            game_strips: false,
            show_news: false,
            show_continue: true,
            auto_columns: true,
            dial_columns: 6,
            widgets: StartWidgets::default(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Features (GX basics, profiles, pinboards, easy files)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Features {
    pub force_dark_pages: bool,
    /// Sites excluded from force dark.
    pub force_dark_exceptions: Vec<String>,
    pub shaders_enabled: bool,
    /// Active shader id (`none` or a built-in / user shader id).
    pub shader: String,
    pub web_modding: bool,
    pub pinboards: bool,
    pub my_flow: bool,
    pub easy_files: bool,
    pub tab_traces: bool,
    pub rewards: bool,
    pub konami: bool,
}

impl Default for Features {
    fn default() -> Self {
        Self {
            force_dark_pages: false,
            force_dark_exceptions: Vec::new(),
            shaders_enabled: true,
            shader: "none".into(),
            web_modding: true,
            pinboards: true,
            my_flow: false,
            easy_files: true,
            tab_traces: true,
            rewards: true,
            konami: true,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Sounds
// ---------------------------------------------------------------------------------------------

/// Individual browser sounds that can be toggled and levelled (Opera GX "Browser sounds").
pub const BROWSER_SOUNDS: &[&str] = &[
    "tab-open",
    "tab-close",
    "limiters",
    "click",
    "hover",
    "tab-slash",
    "switch",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Sounds {
    pub browser_sounds: bool,
    pub browser_pack: String,
    pub browser_volume: u32,
    /// Per-sound level (0-100); a sound absent from the map plays at 100.
    pub levels: BTreeMap<String, u32>,
    /// Per-sound on/off; a sound absent from the map is on.
    pub enabled: BTreeMap<String, bool>,
    pub keyboard_sounds: bool,
    pub keyboard_pack: String,
    pub keyboard_volume: u32,
    pub background_music: bool,
    pub music_track: String,
    pub music_volume: u32,
}

impl Default for Sounds {
    fn default() -> Self {
        Self {
            browser_sounds: true,
            browser_pack: "haku-core".into(),
            browser_volume: 50,
            levels: BTreeMap::new(),
            enabled: BTreeMap::new(),
            keyboard_sounds: false,
            keyboard_pack: "haku-mech".into(),
            keyboard_volume: 100,
            background_music: false,
            music_track: "focus".into(),
            music_volume: 40,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// GX Lights (RGB peripherals)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Lights {
    pub enabled: bool,
    pub razer_chroma: bool,
    pub openrgb: bool,
    pub openrgb_host: String,
    pub openrgb_port: u16,
    pub brightness: u32,
    pub dim_in_fullscreen: bool,
    pub animations: bool,
    pub music_visualizer: bool,
    pub animation_set: String,
}

impl Default for Lights {
    fn default() -> Self {
        Self {
            enabled: false,
            razer_chroma: true,
            openrgb: true,
            openrgb_host: "127.0.0.1".into(),
            openrgb_port: 6742,
            brightness: 100,
            dim_in_fullscreen: true,
            animations: true,
            music_visualizer: false,
            animation_set: "dark-magic".into(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Battery saver
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Battery {
    pub saver: bool,
    pub auto_when_unplugged: bool,
    /// Turn on automatically below this charge percentage when unplugged (0 = off).
    pub auto_below_percent: u32,
    pub show_toolbar_icon: bool,
}

impl Default for Battery {
    fn default() -> Self {
        Self {
            saver: false,
            auto_when_unplugged: true,
            auto_below_percent: 20,
            show_toolbar_icon: false,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Player
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum MiniplayerMode {
    Off,
    InToolbar,
    InSidebar,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Player {
    pub enabled: bool,
    pub miniplayer: MiniplayerMode,
    pub miniplayer_background: bool,
    pub auto_pause: bool,
    pub resume_after_secs: u32,
    /// Selected service id (spotify, apple-music, ...).
    pub service: Option<String>,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            enabled: true,
            miniplayer: MiniplayerMode::InToolbar,
            miniplayer_background: true,
            auto_pause: true,
            resume_after_secs: 1,
            service: None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Video
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Video {
    pub rgx_enhance_videos: bool,
    pub rgx_button: bool,
    pub rgx_enhance_images: bool,
    pub video_skip: bool,
    pub popout: bool,
    pub auto_popout: bool,
    pub popout_transparent: bool,
    pub conference_popout: bool,
    pub conference_auto_detach: bool,
    pub conference_transparent: bool,
    pub video_pickup: bool,
}

impl Default for Video {
    fn default() -> Self {
        Self {
            rgx_enhance_videos: false,
            rgx_button: true,
            rgx_enhance_images: false,
            video_skip: true,
            popout: true,
            auto_popout: false,
            popout_transparent: false,
            conference_popout: true,
            conference_auto_detach: false,
            conference_transparent: false,
            video_pickup: true,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// GX Control
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum HotTabsSort {
    Cpu,
    Ram,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct GxControl {
    pub ram_limiter: bool,
    /// Memory limit for all browser processes together, MiB.
    pub ram_limit_mb: u64,
    /// Hard limit: the kernel refuses allocations above the cap (Job Object). Soft limit:
    /// HakuCX snoozes the heaviest background tabs to get back under it.
    pub ram_hard_limit: bool,
    pub cpu_limiter: bool,
    /// Percent of total CPU time the browser may use.
    pub cpu_limit: u32,
    pub network_limiter: bool,
    /// Bytes per second; 0 = unlimited.
    pub network_limit_bps: u64,
    pub hot_tabs_sort: HotTabsSort,
    pub keep_after_restart: bool,
}

impl Default for GxControl {
    fn default() -> Self {
        Self {
            ram_limiter: false,
            ram_limit_mb: 8 * 1024,
            ram_hard_limit: false,
            cpu_limiter: false,
            cpu_limit: 100,
            network_limiter: false,
            network_limit_bps: 25 * 1024 * 1024,
            hot_tabs_sort: HotTabsSort::Cpu,
            keep_after_restart: false,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// GX Cleaner
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum CleanerPreset {
    Min,
    Med,
    Max,
}

/// "Older than ..." thresholds offered by the cleaner, in seconds; 0 means everything.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum CleanerAge {
    All,
    OlderThanHour,
    OlderThanDay,
    OlderThanWeek,
    OlderThan30Days,
}

impl CleanerAge {
    pub fn seconds(self) -> u64 {
        match self {
            CleanerAge::All => 0,
            CleanerAge::OlderThanHour => 3_600,
            CleanerAge::OlderThanDay => 86_400,
            CleanerAge::OlderThanWeek => 7 * 86_400,
            CleanerAge::OlderThan30Days => 30 * 86_400,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct CleanerItem {
    pub enabled: bool,
    pub age: CleanerAge,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Cleaner {
    pub preset: CleanerPreset,
    pub cache: CleanerItem,
    pub cookies: CleanerItem,
    pub tabs: CleanerItem,
    pub history: CleanerItem,
    pub downloads: CleanerItem,
    pub delete_downloaded_files: bool,
    pub include_unfinished_downloads: bool,
    pub sidebar_icons: CleanerItem,
}

impl Default for CleanerItem {
    fn default() -> Self {
        Self {
            enabled: true,
            age: CleanerAge::OlderThanWeek,
        }
    }
}

impl Default for Cleaner {
    fn default() -> Self {
        Self::preset(CleanerPreset::Min)
    }
}

impl Cleaner {
    /// The three one-click presets. MIN only touches caches and stale data; MAX clears
    /// nearly everything except pinned tabs.
    pub fn preset(preset: CleanerPreset) -> Self {
        let item = |enabled, age| CleanerItem { enabled, age };
        match preset {
            CleanerPreset::Min => Self {
                preset,
                cache: item(true, CleanerAge::OlderThanDay),
                cookies: item(false, CleanerAge::OlderThanWeek),
                tabs: item(false, CleanerAge::OlderThanHour),
                history: item(true, CleanerAge::OlderThan30Days),
                downloads: item(true, CleanerAge::OlderThan30Days),
                delete_downloaded_files: false,
                include_unfinished_downloads: false,
                sidebar_icons: item(false, CleanerAge::OlderThanWeek),
            },
            CleanerPreset::Med => Self {
                preset,
                cache: item(true, CleanerAge::All),
                cookies: item(true, CleanerAge::OlderThanWeek),
                tabs: item(true, CleanerAge::OlderThanDay),
                history: item(true, CleanerAge::OlderThanWeek),
                downloads: item(true, CleanerAge::OlderThanWeek),
                delete_downloaded_files: false,
                include_unfinished_downloads: false,
                sidebar_icons: item(false, CleanerAge::OlderThanWeek),
            },
            CleanerPreset::Max => Self {
                preset,
                cache: item(true, CleanerAge::All),
                cookies: item(true, CleanerAge::All),
                tabs: item(true, CleanerAge::OlderThanHour),
                history: item(true, CleanerAge::All),
                downloads: item(true, CleanerAge::All),
                delete_downloaded_files: false,
                include_unfinished_downloads: true,
                sidebar_icons: item(true, CleanerAge::OlderThanWeek),
            },
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Panic button
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Panic {
    pub enabled: bool,
    /// Key in `keyboard_types::Code` notation, e.g. `F12`.
    pub key: String,
    /// The "boring" pages opened while panicking.
    pub urls: Vec<String>,
    pub mute_all: bool,
}

impl Default for Panic {
    fn default() -> Self {
        Self {
            enabled: true,
            key: "F12".into(),
            urls: [
                "https://www.youtube.com/",
                "https://x.com/",
                "https://www.wikipedia.org/",
                "https://docs.google.com/",
                "https://www.twitch.tv/",
                "https://www.reddit.com/",
                "https://mail.google.com/",
                "https://github.com/",
                "https://news.google.com/",
                "https://store.steampowered.com/",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            mute_all: true,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Privacy & security
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ThirdPartyCookies {
    Allow,
    BlockInPrivate,
    Block,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct FilterList {
    pub id: String,
    pub name: String,
    pub url: String,
    pub enabled: bool,
    /// `ads` or `trackers` (or `annoyances`, `malware`, `custom`).
    pub category: String,
    pub last_updated: Option<i64>,
}

impl Default for FilterList {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            url: String::new(),
            enabled: true,
            category: "custom".into(),
            last_updated: None,
        }
    }
}

/// Default filter lists. All are free, maintained by their communities, and fetched by HakuCX
/// over HTTPS; nothing is proxied through a HakuCX server (there is none).
pub fn default_filter_lists() -> Vec<FilterList> {
    let list = |id: &str, name: &str, url: &str, category: &str, enabled: bool| FilterList {
        id: id.into(),
        name: name.into(),
        url: url.into(),
        enabled,
        category: category.into(),
        last_updated: None,
    };
    vec![
        list(
            "easylist",
            "EasyList",
            "https://easylist.to/easylist/easylist.txt",
            "ads",
            true,
        ),
        list(
            "easyprivacy",
            "EasyPrivacy",
            "https://easylist.to/easylist/easyprivacy.txt",
            "trackers",
            true,
        ),
        list(
            "ublock-filters",
            "uBlock filters",
            "https://ublockorigin.github.io/uAssets/filters/filters.txt",
            "ads",
            false,
        ),
        list(
            "fanboy-annoyance",
            "Fanboy's Annoyance List",
            "https://secure.fanboy.co.nz/fanboy-annoyance.txt",
            "annoyances",
            false,
        ),
        list(
            "urlhaus",
            "URLhaus Malicious URL Blocklist",
            "https://malware-filter.gitlab.io/malware-filter/urlhaus-filter-agh.txt",
            "malware",
            true,
        ),
    ]
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct ClearOnExit {
    pub history: bool,
    pub cookies: bool,
    pub cache: bool,
    pub downloads: bool,
    pub autofill: bool,
    pub site_settings: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Privacy {
    pub block_ads: bool,
    pub block_trackers: bool,
    /// Sites where blocking is turned off (hostnames).
    pub ad_exceptions: Vec<String>,
    pub tracker_exceptions: Vec<String>,
    pub filter_lists: Vec<FilterList>,
    /// User-written filter rules (ABP / uBO syntax).
    pub custom_filters: String,
    pub cosmetic_filtering: bool,
    pub block_popups: bool,
    pub popup_allowlist: Vec<String>,
    pub do_not_track: bool,
    pub global_privacy_control: bool,
    pub third_party_cookies: ThirdPartyCookies,
    pub https_only: bool,
    /// Engine JIT compilers. Off trades speed for a smaller attack surface.
    pub javascript_optimization: bool,
    pub malicious_site_protection: bool,
    pub paste_protect_hijack: bool,
    pub paste_protect_injection: bool,
    pub paste_protect_allowlist: Vec<String>,
    pub clear_on_exit: ClearOnExit,
    pub show_blocked_stats: bool,
    /// HakuCX never phones home. These stay false and exist so the UI can say so.
    pub crash_reports: bool,
    pub usage_stats: bool,
    pub auto_remove_unused_permissions: bool,
}

impl Default for Privacy {
    fn default() -> Self {
        Self {
            block_ads: true,
            block_trackers: true,
            ad_exceptions: Vec::new(),
            tracker_exceptions: Vec::new(),
            filter_lists: default_filter_lists(),
            custom_filters: String::new(),
            cosmetic_filtering: true,
            block_popups: true,
            popup_allowlist: Vec::new(),
            do_not_track: true,
            global_privacy_control: true,
            third_party_cookies: ThirdPartyCookies::BlockInPrivate,
            https_only: false,
            javascript_optimization: true,
            malicious_site_protection: true,
            paste_protect_hijack: true,
            paste_protect_injection: true,
            paste_protect_allowlist: Vec::new(),
            clear_on_exit: ClearOnExit::default(),
            show_blocked_stats: true,
            crash_reports: false,
            usage_stats: false,
            auto_remove_unused_permissions: true,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Site permissions
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum PermissionSetting {
    Ask,
    Allow,
    Block,
}

/// Default behaviour per permission kind, plus per-origin overrides.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Permissions {
    /// Permission kind (e.g. `camera`, `geolocation`) -> default setting.
    pub defaults: BTreeMap<String, PermissionSetting>,
    /// Origin -> permission kind -> setting.
    pub sites: BTreeMap<String, BTreeMap<String, PermissionSetting>>,
    pub quiet_requests: bool,
}

/// Every permission kind shown in Opera GX's per-site settings page, in the same order.
pub const PERMISSION_KINDS: &[&str] = &[
    "geolocation",
    "camera",
    "microphone",
    "motion-sensors",
    "notifications",
    "javascript",
    "images",
    "popups",
    "background-sync",
    "sound",
    "automatic-downloads",
    "midi",
    "usb",
    "serial",
    "file-editing",
    "hid",
    "protected-content",
    "clipboard",
    "payment-handlers",
    "insecure-content",
    "javascript-jit",
    "third-party-sign-in",
    "augmented-reality",
    "virtual-reality",
    "idle-detection",
    "window-management",
    "local-fonts",
    "automatic-picture-in-picture",
    "captured-surface-control",
    "web-app-installation",
    "local-network",
    "embedded-content",
];

impl Default for Permissions {
    fn default() -> Self {
        let mut defaults = BTreeMap::new();
        for kind in PERMISSION_KINDS {
            let setting = match *kind {
                "javascript"
                | "images"
                | "sound"
                | "background-sync"
                | "javascript-jit"
                | "third-party-sign-in"
                | "embedded-content"
                | "automatic-picture-in-picture" => PermissionSetting::Allow,
                "popups" | "insecure-content" | "protected-content" => PermissionSetting::Block,
                _ => PermissionSetting::Ask,
            };
            defaults.insert(kind.to_string(), setting);
        }
        Self {
            defaults,
            sites: BTreeMap::new(),
            quiet_requests: true,
        }
    }
}

impl Permissions {
    /// Effective setting for an origin, falling back to the kind default, then to Ask.
    pub fn effective(&self, origin: &str, kind: &str) -> PermissionSetting {
        self.sites
            .get(origin)
            .and_then(|m| m.get(kind))
            .or_else(|| self.defaults.get(kind))
            .copied()
            .unwrap_or(PermissionSetting::Ask)
    }
}

// ---------------------------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct SearchEngine {
    pub id: String,
    pub name: String,
    /// Keyword typed in the address bar (`g`, `d`, `w`, ...).
    pub keyword: String,
    /// Search URL with `%s` as the query placeholder.
    pub url: String,
    /// Optional OpenSearch-style suggestion URL with `%s`.
    pub suggest_url: Option<String>,
    pub active: bool,
}

impl Default for SearchEngine {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            keyword: String::new(),
            url: String::new(),
            suggest_url: None,
            active: true,
        }
    }
}

pub fn default_search_engines() -> Vec<SearchEngine> {
    let engine =
        |id: &str, name: &str, keyword: &str, url: &str, suggest: Option<&str>| SearchEngine {
            id: id.into(),
            name: name.into(),
            keyword: keyword.into(),
            url: url.into(),
            suggest_url: suggest.map(Into::into),
            active: true,
        };
    vec![
        engine(
            "duckduckgo",
            "DuckDuckGo",
            "d",
            "https://duckduckgo.com/?q=%s",
            Some("https://duckduckgo.com/ac/?q=%s&type=list"),
        ),
        engine(
            "google",
            "Google",
            "g",
            "https://www.google.com/search?q=%s",
            Some("https://suggestqueries.google.com/complete/search?client=firefox&q=%s"),
        ),
        engine(
            "bing",
            "Bing",
            "b",
            "https://www.bing.com/search?q=%s",
            None,
        ),
        engine(
            "brave",
            "Brave Search",
            "br",
            "https://search.brave.com/search?q=%s",
            None,
        ),
        engine(
            "startpage",
            "Startpage",
            "s",
            "https://www.startpage.com/sp/search?query=%s",
            None,
        ),
        engine(
            "yahoo",
            "Yahoo!",
            "y",
            "https://search.yahoo.com/search?p=%s",
            None,
        ),
        engine(
            "wikipedia",
            "Wikipedia",
            "w",
            "https://en.wikipedia.org/wiki/Special:Search?search=%s",
            None,
        ),
        engine(
            "youtube",
            "YouTube",
            "yt",
            "https://www.youtube.com/results?search_query=%s",
            None,
        ),
        engine(
            "github",
            "GitHub",
            "gh",
            "https://github.com/search?q=%s",
            None,
        ),
        engine(
            "amazon",
            "Amazon",
            "z",
            "https://www.amazon.com/s?k=%s",
            None,
        ),
        engine(
            "twitch",
            "Twitch",
            "tw",
            "https://www.twitch.tv/search?term=%s",
            None,
        ),
        engine(
            "steam",
            "Steam",
            "st",
            "https://store.steampowered.com/search/?term=%s",
            None,
        ),
    ]
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Search {
    pub default_engine: String,
    pub private_engine: Option<String>,
    pub engines: Vec<SearchEngine>,
    pub keyword_shortcuts: bool,
    pub search_popup: bool,
    pub popup_currency: String,
    pub popup_units: bool,
    pub popup_time_zones: bool,
    pub image_search_menu: bool,
}

impl Default for Search {
    fn default() -> Self {
        Self {
            default_engine: "duckduckgo".into(),
            private_engine: None,
            engines: default_search_engines(),
            keyword_shortcuts: true,
            search_popup: true,
            popup_currency: "USD".into(),
            popup_units: true,
            popup_time_zones: true,
            image_search_menu: true,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// On startup
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum StartupMode {
    StartPage,
    RestoreSession,
    SpecificPages,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Startup {
    pub mode: StartupMode,
    pub show_start_page_first: bool,
    pub pages: Vec<String>,
    pub ask_when_started_with_url: bool,
    pub run_at_login: bool,
}

impl Default for Startup {
    fn default() -> Self {
        Self {
            mode: StartupMode::RestoreSession,
            show_start_page_first: false,
            pages: Vec::new(),
            ask_when_started_with_url: false,
            run_at_login: false,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Languages, downloads, accessibility, system
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Languages {
    /// UI language (BCP 47). Only locales shipped in `ui/locales` are selectable.
    pub ui_language: String,
    /// `Accept-Language` order for websites.
    pub accept_languages: Vec<String>,
    pub spellcheck: bool,
    pub spellcheck_languages: Vec<String>,
    pub translate_offer: bool,
    pub translate_target: String,
    pub never_translate: Vec<String>,
}

impl Default for Languages {
    fn default() -> Self {
        Self {
            ui_language: "en".into(),
            accept_languages: vec!["en-US".into(), "en".into()],
            spellcheck: true,
            spellcheck_languages: vec!["en-US".into()],
            translate_offer: true,
            translate_target: "en".into(),
            never_translate: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Downloads {
    /// Absolute path; empty means the user's Downloads folder.
    pub directory: String,
    pub ask_where: bool,
    pub open_when_done: bool,
    pub show_popup: bool,
}

impl Default for Downloads {
    fn default() -> Self {
        Self {
            directory: String::new(),
            ask_where: false,
            open_when_done: false,
            show_popup: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Accessibility {
    pub focus_highlight: bool,
    pub swipe_navigation: bool,
    pub copy_confirmations: bool,
    pub high_contrast_ui: bool,
}

impl Default for Accessibility {
    fn default() -> Self {
        Self {
            focus_highlight: false,
            swipe_navigation: true,
            copy_confirmations: true,
            high_contrast_ui: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ProxyMode {
    /// Use `HTTP(S)_PROXY` from the environment, like Servo does by default.
    System,
    Direct,
    Manual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum WebRtcPolicy {
    Default,
    DefaultPublicAndPrivate,
    DefaultPublicOnly,
    DisableNonProxiedUdp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct System {
    pub hardware_acceleration: bool,
    pub proxy_mode: ProxyMode,
    /// `http://host:port` used for http and https in manual mode.
    pub proxy_url: String,
    pub proxy_bypass: String,
    pub webrtc_policy: WebRtcPolicy,
    /// Servo multi-process mode (content processes separate from the UI). Needed for the GX
    /// Control RAM hard limit. Experimental in Servo, so off by default.
    pub multiprocess: bool,
    /// Keep the HTTP cache on disk between sessions.
    pub disk_cache: bool,
    pub webgpu: bool,
    pub text_antialiasing: bool,
    pub devtools_server: bool,
    pub devtools_port: u16,
}

impl Default for System {
    fn default() -> Self {
        Self {
            hardware_acceleration: true,
            proxy_mode: ProxyMode::System,
            proxy_url: String::new(),
            proxy_bypass: "localhost,127.0.0.1".into(),
            webrtc_policy: WebRtcPolicy::Default,
            multiprocess: false,
            disk_cache: true,
            webgpu: false,
            text_antialiasing: true,
            devtools_server: false,
            devtools_port: 7000,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Sync, smart home, AI
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Sync {
    /// Folder-based sync: HakuCX writes an encrypted bundle to this folder (e.g. a OneDrive or
    /// Syncthing folder) and merges bundles written by other devices.
    pub folder_sync: bool,
    pub folder: String,
    pub bookmarks: bool,
    pub history: bool,
    pub settings: bool,
    pub open_tabs: bool,
    pub passwords: bool,
    pub speed_dial: bool,
}

impl Default for Sync {
    fn default() -> Self {
        Self {
            folder_sync: false,
            folder: String::new(),
            bookmarks: true,
            history: true,
            settings: true,
            open_tabs: true,
            passwords: false,
            speed_dial: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct SmartHome {
    pub mqtt: bool,
    pub broker_url: String,
    pub topic_prefix: String,
}

impl Default for SmartHome {
    fn default() -> Self {
        Self {
            mqtt: false,
            broker_url: "mqtt://127.0.0.1:1883".into(),
            topic_prefix: "hakucx".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum AiProvider {
    None,
    Anthropic,
    OpenAiCompatible,
    Ollama,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Ai {
    pub enabled: bool,
    pub toolbar_button: bool,
    pub highlight_prompts: bool,
    pub provider: AiProvider,
    pub model: String,
    /// Base URL for OpenAI-compatible or Ollama endpoints.
    pub endpoint: String,
    /// The API key lives in the encrypted vault, never in settings.json.
    pub key_in_vault: bool,
}

impl Default for Ai {
    fn default() -> Self {
        Self {
            enabled: true,
            toolbar_button: true,
            highlight_prompts: true,
            provider: AiProvider::None,
            model: String::new(),
            endpoint: String::new(),
            key_in_vault: false,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Shortcuts & gestures
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Shortcuts {
    pub mouse_gestures: bool,
    pub rocker_gestures: bool,
    pub advanced_shortcuts: bool,
    /// Command id -> accelerator overrides (e.g. `"new-tab": "Ctrl+T"`). Empty string unbinds.
    pub overrides: BTreeMap<String, String>,
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self {
            mouse_gestures: false,
            rocker_gestures: false,
            advanced_shortcuts: true,
            overrides: BTreeMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Early Bird / Playground
// ---------------------------------------------------------------------------------------------

/// Experimental features. Each flag gates real code; nothing here is cosmetic.
pub const EARLY_BIRD_FEATURES: &[(&str, &str)] = &[
    ("advanced-audio-controls", "Advanced Audio Controls"),
    ("show-blocked-stats", "Show blocked stats"),
    ("extended-diagnostics", "Extended UI Diagnostics"),
    ("daily-wallpapers", "Daily wallpapers"),
    ("guest-tabs", "Guest Tabs"),
    ("live-wallpapers", "Live Wallpapers"),
    ("improved-video-skip", "Improved Video Skip Button"),
    ("tab-emojis", "Tab emojis"),
    ("vertical-tabs", "Vertical tabs"),
    ("panic-grass", "Touch grass after panic"),
];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct EarlyBird {
    pub enabled: bool,
    pub features: BTreeMap<String, bool>,
    /// Playground loadout: up to three experiment ids.
    pub loadout: Vec<String>,
    pub playground_intro_seen: bool,
}

impl EarlyBird {
    pub fn is_on(&self, feature: &str) -> bool {
        self.enabled && self.features.get(feature).copied().unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_document_yields_defaults() {
        let s = Settings::from_json("{}").unwrap();
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn round_trip_is_stable() {
        let s = Settings::default();
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(Settings::from_json(&text).unwrap(), s);
    }

    #[test]
    fn unknown_keys_survive() {
        let s = Settings::from_json(r#"{"futureThing":{"a":1}}"#).unwrap();
        let text = serde_json::to_string(&s).unwrap();
        assert!(text.contains("futureThing"));
    }

    #[test]
    fn patch_changes_only_given_keys() {
        let mut s = Settings::default();
        s.apply_patch(&serde_json::json!({"appearance": {"theme": "gx-classic"}}))
            .unwrap();
        assert_eq!(s.appearance.theme, "gx-classic");
        assert_eq!(s.appearance.color_mode, ColorMode::Dark);
        assert!(s.privacy.block_ads);
    }

    #[test]
    fn patch_values_are_clamped() {
        let mut s = Settings::default();
        s.apply_patch(
            &serde_json::json!({"control": {"cpuLimit": 900}, "appearance": {"opacity": -3}}),
        )
        .unwrap();
        assert_eq!(s.control.cpu_limit, 100);
        assert_eq!(s.appearance.opacity, 0.2);
    }

    #[test]
    fn invalid_enum_in_patch_is_rejected() {
        let mut s = Settings::default();
        let before = s.clone();
        assert!(
            s.apply_patch(&serde_json::json!({"appearance": {"colorMode": "purple"}}))
                .is_err()
        );
        assert_eq!(s, before);
    }

    #[test]
    fn permission_fallbacks() {
        let mut p = Permissions::default();
        assert_eq!(
            p.effective("https://a.test", "camera"),
            PermissionSetting::Ask
        );
        assert_eq!(
            p.effective("https://a.test", "popups"),
            PermissionSetting::Block
        );
        p.sites
            .entry("https://a.test".into())
            .or_default()
            .insert("camera".into(), PermissionSetting::Allow);
        assert_eq!(
            p.effective("https://a.test", "camera"),
            PermissionSetting::Allow
        );
        assert_eq!(
            p.effective("https://b.test", "unknown-kind"),
            PermissionSetting::Ask
        );
    }

    #[test]
    fn cleaner_presets_escalate() {
        let min = Cleaner::preset(CleanerPreset::Min);
        let max = Cleaner::preset(CleanerPreset::Max);
        assert!(!min.cookies.enabled);
        assert!(max.cookies.enabled);
        assert_eq!(max.history.age, CleanerAge::All);
    }

    #[test]
    fn merge_patch_null_removes() {
        let mut v = serde_json::json!({"a": 1, "b": {"c": 2}});
        merge_patch(&mut v, &serde_json::json!({"a": null, "b": {"d": 3}}));
        assert_eq!(v, serde_json::json!({"b": {"c": 2, "d": 3}}));
    }
}

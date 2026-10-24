/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Static catalogs the UI offers: sidebar items, web panels, wallpapers, shaders, sounds.

export type SidebarGroup = "gx" | "ai" | "messengers" | "social" | "special" | "tools" | "extensions";

export type SidebarItem = {
  id: string;
  label: string;
  group: SidebarGroup;
  icon: string;
  /** Web panels load a site in a sidebar page (messengers, social, AI). */
  web?: string;
  /** Internal page this tool opens instead of a panel. */
  page?: string;
  brand?: string;
};

export const SIDEBAR_GROUPS: { id: SidebarGroup; label: string }[] = [
  { id: "gx", label: "GX features" },
  { id: "ai", label: "AI services" },
  { id: "messengers", label: "Messengers" },
  { id: "social", label: "Social media" },
  { id: "special", label: "Special features" },
  { id: "tools", label: "Browser tools" },
  { id: "extensions", label: "Sidebar extensions" },
];

export const SIDEBAR_ITEMS: SidebarItem[] = [
  { id: "gx-control", label: "GX Control", group: "gx", icon: "gauge" },
  { id: "gx-cleaner", label: "GX Cleaner", group: "gx", icon: "sparkles" },
  { id: "shaders", label: "Shaders", group: "gx", icon: "aperture" },
  { id: "mods", label: "Mods", group: "gx", icon: "box", page: "haku://mods/" },
  { id: "twitch", label: "Twitch", group: "gx", icon: "twitch", web: "https://www.twitch.tv/", brand: "#9146ff" },
  { id: "chatgpt", label: "ChatGPT", group: "ai", icon: "bot", web: "https://chatgpt.com/", brand: "#10a37f" },
  { id: "gemini", label: "Gemini", group: "ai", icon: "sparkle", web: "https://gemini.google.com/", brand: "#8ab4f8" },
  { id: "claude", label: "Claude", group: "ai", icon: "asterisk", web: "https://claude.ai/", brand: "#d97757" },
  { id: "messenger", label: "Facebook Messenger", group: "messengers", icon: "message-circle", web: "https://www.messenger.com/", brand: "#0084ff" },
  { id: "whatsapp", label: "WhatsApp", group: "messengers", icon: "phone", web: "https://web.whatsapp.com/", brand: "#25d366" },
  { id: "discord", label: "Discord", group: "messengers", icon: "gamepad", web: "https://discord.com/app", brand: "#5865f2" },
  { id: "telegram", label: "Telegram", group: "messengers", icon: "send", web: "https://web.telegram.org/", brand: "#2aabee" },
  { id: "slack", label: "Slack", group: "messengers", icon: "hash", web: "https://app.slack.com/client", brand: "#e01e5a" },
  { id: "instagram", label: "Instagram", group: "social", icon: "instagram", web: "https://www.instagram.com/", brand: "#e1306c" },
  { id: "tiktok", label: "TikTok", group: "social", icon: "music", web: "https://www.tiktok.com/", brand: "#fe2c55" },
  { id: "x", label: "X", group: "social", icon: "x-logo", web: "https://x.com/", brand: "#e7e9ea" },
  { id: "bluesky", label: "Bluesky", group: "social", icon: "cloud", web: "https://bsky.app/", brand: "#1185fe" },
  { id: "reddit", label: "Reddit", group: "social", icon: "reddit", web: "https://www.reddit.com/", brand: "#ff4500" },
  { id: "rewards", label: "GX Rewards", group: "special", icon: "gift" },
  { id: "player", label: "Player", group: "special", icon: "play-circle" },
  { id: "my-flow", label: "My Flow", group: "special", icon: "waves" },
  { id: "corner", label: "GX Corner", group: "special", icon: "gamepad-2", page: "haku://corner/" },
  { id: "speed-dial", label: "Speed Dial", group: "tools", icon: "layout-grid", page: "haku://start/" },
  { id: "bookmarks", label: "Bookmarks", group: "tools", icon: "bookmark" },
  { id: "news", label: "Personal news", group: "tools", icon: "newspaper" },
  { id: "pinboards", label: "Pinboards", group: "tools", icon: "pin" },
  { id: "tabs", label: "Tabs", group: "tools", icon: "panels-top-left" },
  { id: "history", label: "History", group: "tools", icon: "clock" },
  { id: "downloads", label: "Downloads", group: "tools", icon: "download" },
  { id: "extensions", label: "Extensions", group: "tools", icon: "puzzle", page: "haku://extensions/" },
  { id: "settings", label: "Settings", group: "tools", icon: "settings", page: "haku://settings/" },
  { id: "my-games", label: "My Games", group: "extensions", icon: "percent", page: "haku://games/" },
];

export function sidebarItem(id: string): SidebarItem | undefined {
  return SIDEBAR_ITEMS.find((i) => i.id === id);
}

export const WALLPAPERS: { id: string; name: string }[] = [
  { id: "gx-ribbons", name: "GX Ribbons" },
  { id: "haku-tide", name: "Haku Tide" },
  { id: "haku-nebula", name: "Nebula" },
  { id: "haku-petals", name: "Petals" },
  { id: "haku-ember", name: "Ember" },
  { id: "haku-depths", name: "Depths" },
  { id: "haku-carbon", name: "Carbon" },
  { id: "neon-grid", name: "Neon Grid" },
  { id: "solid", name: "Solid" },
];

export const SHADERS: { id: string; name: string; description: string }[] = [
  { id: "none", name: "None", description: "Pages look the way their authors made them." },
  { id: "shift-fx", name: "ShiftFX", description: "Rotates the color wheel for a fresh palette." },
  { id: "midnight-fx", name: "MidnightFX", description: "Cool, dim, low-glare." },
  { id: "fade-fx", name: "FadeFX", description: "Softer contrast and muted colors." },
  { id: "boost-fx", name: "BoostFX", description: "Punchier contrast and saturation." },
  { id: "mono-core", name: "MonoCore", description: "Black and white." },
  { id: "duo-fx", name: "DuoFX", description: "Two-tone, tinted." },
];

export const BROWSER_SOUNDS: { id: string; label: string }[] = [
  { id: "tab-open", label: "Tab open" },
  { id: "tab-close", label: "Tab close" },
  { id: "limiters", label: "Limiters" },
  { id: "click", label: "Click" },
  { id: "hover", label: "Hover" },
  { id: "tab-slash", label: "Tab slash" },
  { id: "switch", label: "Switch" },
  { id: "download", label: "Download complete" },
];

export const SOUND_PACKS = [
  { id: "haku-core", name: "Haku Core" },
  { id: "haku-soft", name: "Haku Soft" },
];

export const KEYBOARD_PACKS = [
  { id: "haku-mech", name: "Haku Mech" },
  { id: "haku-soft-keys", name: "Haku Soft Keys" },
];

export const MUSIC_TRACKS = [
  { id: "focus", name: "Focus" },
  { id: "chill", name: "Chill" },
  { id: "raid", name: "Raid" },
  { id: "doom", name: "Doom" },
];

export const WORKSPACE_ICONS = ["home", "game", "moon", "book", "star", "code", "music", "heart", "briefcase", "rocket"];

export const PERMISSION_LABELS: Record<string, string> = {
  geolocation: "Location",
  camera: "Camera",
  microphone: "Microphone",
  "motion-sensors": "Motion sensors",
  notifications: "Notifications",
  javascript: "JavaScript",
  images: "Images",
  popups: "Pop-ups and redirects",
  "background-sync": "Background sync",
  sound: "Sound",
  "automatic-downloads": "Automatic downloads",
  midi: "MIDI device control & reprogram",
  usb: "USB devices",
  serial: "Serial ports",
  "file-editing": "File editing",
  hid: "HID devices",
  "protected-content": "Protected content IDs",
  clipboard: "Clipboard",
  "payment-handlers": "Payment handlers",
  "insecure-content": "Insecure content",
  "javascript-jit": "JavaScript optimization & security",
  "third-party-sign-in": "Third-party sign-in",
  "augmented-reality": "Augmented reality",
  "virtual-reality": "Virtual reality",
  "idle-detection": "Your device use",
  "window-management": "Window management",
  "local-fonts": "Fonts",
  "automatic-picture-in-picture": "Automatic picture-in-picture",
  "captured-surface-control": "Scrolling and zooming shared tabs",
  "web-app-installation": "Web app installations",
  "local-network": "Local network",
  "embedded-content": "Embedded content",
  "device-info": "Device info",
  "persistent-storage": "Persistent storage",
  "wake-lock": "Keep screen awake",
  bluetooth: "Bluetooth",
  gamepad: "Game controllers",
};

/**
 * Permission kinds Servo 0.7 actually asks the browser about. The others are listed in the
 * site settings for completeness, but marked as not used by the engine.
 */
export const ENGINE_PERMISSIONS = new Set([
  "geolocation",
  "notifications",
  "midi",
  "camera",
  "microphone",
  "sound",
  "device-info",
  "background-sync",
  "bluetooth",
  "persistent-storage",
  "wake-lock",
  "gamepad",
]);

export const EARLY_BIRD = [
  { id: "advanced-audio-controls", name: "Advanced Audio Controls", description: "Per-tab volume sliders in the tab context menu." },
  { id: "show-blocked-stats", name: "Show blocked stats", description: "Shows how many ads and trackers were blocked, per page and in total." },
  { id: "daily-wallpapers", name: "Daily wallpapers", description: "A new built-in wallpaper every day." },
  { id: "guest-tabs", name: "Guest Tabs", description: "Open a tab that shares nothing with your profile." },
  { id: "live-wallpapers", name: "Live Wallpapers", description: "Animated wallpapers that react to the music visualizer." },
  { id: "tab-emojis", name: "Tab emojis", description: "Pick an emoji for any tab from its context menu." },
  { id: "vertical-tabs", name: "Vertical tabs", description: "Show tabs in a column next to the sidebar." },
  { id: "panic-grass", name: "Touch grass after panic", description: "After using the panic button, suggest a short break." },
];

/** Keyboard shortcuts with their defaults. Mirrors `SHORTCUTS` in `crates/hakucx/src/browser/input.rs`. */
export const SHORTCUT_LABELS: { id: string; label: string; default: string }[] = [
  { id: "new-tab", label: "New tab", default: "Ctrl+T" },
  { id: "close-tab", label: "Close tab", default: "Ctrl+W" },
  { id: "close-tab-alt", label: "Close tab (alternative)", default: "Ctrl+F4" },
  { id: "reopen-tab", label: "Reopen last closed tab", default: "Ctrl+Shift+T" },
  { id: "next-tab", label: "Next tab", default: "Ctrl+Tab" },
  { id: "previous-tab", label: "Previous tab", default: "Ctrl+Shift+Tab" },
  { id: "next-tab-alt", label: "Next tab (alternative)", default: "Ctrl+PageDown" },
  { id: "previous-tab-alt", label: "Previous tab (alternative)", default: "Ctrl+PageUp" },
  { id: "focus-address", label: "Go to address bar", default: "Ctrl+L" },
  { id: "focus-address-alt", label: "Go to address bar (alternative)", default: "Alt+D" },
  { id: "focus-address-f6", label: "Go to address bar (F6)", default: "F6" },
  { id: "find", label: "Find in page", default: "Ctrl+F" },
  { id: "find-next", label: "Find next", default: "F3" },
  { id: "reload", label: "Reload", default: "Ctrl+R" },
  { id: "reload-f5", label: "Reload (F5)", default: "F5" },
  { id: "hard-reload", label: "Reload, bypassing the cache", default: "Ctrl+Shift+R" },
  { id: "hard-reload-f5", label: "Reload, bypassing the cache (alternative)", default: "Ctrl+F5" },
  { id: "back", label: "Back", default: "Alt+Left" },
  { id: "forward", label: "Forward", default: "Alt+Right" },
  { id: "home", label: "Start page", default: "Alt+Home" },
  { id: "bookmark", label: "Bookmark this page", default: "Ctrl+D" },
  { id: "history", label: "History", default: "Ctrl+H" },
  { id: "downloads", label: "Downloads", default: "Ctrl+J" },
  { id: "bookmarks", label: "Bookmarks", default: "Ctrl+Shift+O" },
  { id: "settings", label: "Settings", default: "Alt+P" },
  { id: "extensions", label: "Extensions", default: "Ctrl+Shift+E" },
  { id: "new-window", label: "New window", default: "Ctrl+N" },
  { id: "private-window", label: "New private window", default: "Ctrl+Shift+N" },
  { id: "zoom-in", label: "Zoom in", default: "Ctrl+=" },
  { id: "zoom-in-plus", label: "Zoom in (alternative)", default: "Ctrl+Shift+=" },
  { id: "zoom-out", label: "Zoom out", default: "Ctrl+-" },
  { id: "zoom-reset", label: "Reset zoom", default: "Ctrl+0" },
  { id: "fullscreen", label: "Full screen", default: "F11" },
  { id: "tab-search", label: "Search in tabs", default: "Ctrl+Space" },
  { id: "command-palette", label: "Command palette", default: "Ctrl+Shift+A" },
  { id: "save-page", label: "Save page", default: "Ctrl+S" },
  { id: "view-source", label: "View page source", default: "Ctrl+U" },
  { id: "devtools", label: "Developer tools", default: "Ctrl+Shift+I" },
  { id: "clear-data", label: "Delete browsing data", default: "Ctrl+Shift+Delete" },
  { id: "toggle-bookmarks-bar", label: "Show or hide the bookmarks bar", default: "Ctrl+Shift+B" },
  { id: "toggle-sidebar", label: "Show or hide the sidebar", default: "Ctrl+Shift+S" },
  { id: "snapshot", label: "Snapshot", default: "Ctrl+Shift+5" },
  { id: "mute-tab", label: "Mute tab", default: "Ctrl+M" },
  { id: "easy-setup", label: "Easy Setup", default: "Ctrl+Shift+Y" },
  { id: "quit", label: "Quit HakuCX", default: "Ctrl+Shift+Q" },
];

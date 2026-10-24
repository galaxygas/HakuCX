/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Icons come from the Lucide icon font (lucide-static). Servo draws inline SVG as a separate
// image, so `currentColor` and CSS variables never reach it; a font glyph takes the text color
// like any other text, including hover and active states.

import codepoints from "lucide-static/font/codepoints.json";
import { useStore } from "../state/store";

/** HakuCX icon names -> Lucide icon names. */
const ICONS: Record<string, string> = {
  aperture: "aperture", window: "app-window", archive: "archive", back: "arrow-left", forward: "arrow-right",
  asterisk: "asterisk", battery: "battery", "battery-charging": "battery-charging", bell: "bell",
  book: "book-open", bookmark: "bookmark", bot: "bot", box: "box", briefcase: "briefcase",
  calendar: "calendar", camera: "camera", check: "check", "chevron-down": "chevron-down",
  "chevron-left": "chevron-left", "chevron-right": "chevron-right", "chevron-up": "chevron-up",
  "play-circle": "circle-play", clock: "clock", cloud: "cloud", code: "code", split: "columns-2",
  copy: "copy", cpu: "cpu", crop: "crop", download: "download", ellipsis: "ellipsis",
  external: "external-link", eye: "eye", "eye-off": "eye-off", file: "file", "file-text": "file-text",
  film: "film", flame: "flame", folder: "folder", "folder-open": "folder-open", gamepad: "gamepad",
  "gamepad-2": "gamepad-2", game: "gamepad-2", gauge: "gauge", ghost: "ghost", gift: "gift", globe: "globe",
  grip: "grip-vertical", hash: "hash", headphones: "headphones", heart: "heart", home: "house",
  image: "image", info: "info", keyboard: "keyboard", languages: "languages", layers: "layers",
  "layout-grid": "layout-grid", lightbulb: "lightbulb", link: "link", lock: "lock", "lock-open": "lock-open",
  "map-pin": "map-pin", maximize: "maximize", megaphone: "megaphone", memory: "memory-stick",
  "message-circle": "message-circle", mic: "mic", minimize: "minimize", minus: "minus", moon: "moon",
  mouse: "mouse", newspaper: "newspaper", palette: "palette", "panels-top-left": "panels-top-left",
  pause: "pause", pencil: "pencil", percent: "percent", phone: "phone", pip: "picture-in-picture-2",
  pin: "pin", play: "play", plus: "plus", puzzle: "puzzle", qr: "qr-code", refresh: "refresh-cw",
  rocket: "rocket", reload: "rotate-cw", rss: "rss", search: "search", send: "send", settings: "settings",
  shield: "shield", "shield-check": "shield-check", shuffle: "shuffle", skull: "skull",
  sliders: "sliders-horizontal", sparkle: "sparkle", sparkles: "sparkles", square: "square",
  edit: "square-pen", star: "star", store: "store", sun: "sun", swords: "swords", tag: "tag",
  trophy: "trophy", undo: "undo-2", upload: "upload", user: "user", volume: "volume-2",
  "volume-x": "volume-x", wifi: "wifi", x: "x", zap: "zap", music: "headphones", waves: "waves-horizontal",
  trash: "trash", tv: "tv", chat: "message-square", activity: "activity", "arrow-down": "arrow-down-to-line",
  "arrow-up": "arrow-up-from-line", alert: "circle-alert", ok: "circle-check", card: "credit-card",
  key: "key-round", "layout-list": "layout-list", list: "list", monitor: "monitor",
  "panel-left": "panel-left", repeat: "repeat", prev: "skip-back", next: "skip-forward",
  warn: "triangle-alert", users: "users", wand: "wand-sparkles", grid: "grid-3x3", brush: "paintbrush",
  server: "server", phone2: "smartphone",
};

const GLYPHS = codepoints as Record<string, number>;

export function glyph(name: string): string {
  const code = GLYPHS[ICONS[name] ?? name] ?? GLYPHS.square!;
  return String.fromCodePoint(code);
}

export function Icon({ name, size = 18, className, title }: { name: string; size?: number; stroke?: number; className?: string; title?: string }) {
  return (
    <span className={className ? `icon ${className}` : "icon"} style={{ fontSize: size, width: size, height: size }} aria-hidden={title ? undefined : true} aria-label={title} role={title ? "img" : undefined}>
      {glyph(name)}
    </span>
  );
}

/**
 * A theme color as a concrete value, for SVG drawings (which do not see CSS variables in
 * Servo). Follows theme changes because the palette lives in the store.
 */
export function useThemeColor(name: string, fallback: string): string {
  return useStore((s) => s.palette?.vars[name] ?? fallback);
}

/**
 * A neutral lettermark for third-party services (no trademarked logos). In "GX icons" mode it
 * is drawn in the theme accent; otherwise in the service's own color.
 */
export function Lettermark({ label, color, size = 26, gx }: { label: string; color?: string; size?: number; gx: boolean }) {
  const letter = label.trim()[0]?.toUpperCase() ?? "?";
  const style: React.CSSProperties = gx
    ? { width: size, height: size, color: "var(--hx-accent)", border: "1.5px solid var(--hx-accent)", background: "transparent" }
    : { width: size, height: size, color: "#fff", background: color ?? "var(--hx-accent)" };
  return (
    <span className="lettermark" style={style}>
      {letter}
    </span>
  );
}

/** The HakuCX logo mark: a notched rounded square with an "H". */
export function Logo({ size = 22 }: { size?: number }) {
  const accent = useThemeColor("--hx-accent", "#7d46f2");
  const dim = useThemeColor("--hx-accent-dim", "#4b2a91");
  const on = useThemeColor("--hx-on-accent", "#ffffff");
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" aria-label="HakuCX">
      <defs>
        <linearGradient id="hx-logo" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor={accent} />
          <stop offset="1" stopColor={dim} />
        </linearGradient>
      </defs>
      <path d="M5 1h14a4 4 0 0 1 4 4v12l-6 6H5a4 4 0 0 1-4-4V5a4 4 0 0 1 4-4z" fill="url(#hx-logo)" />
      <path d="M7.5 6v12M16.5 6v12M7.5 12h9" stroke={on} strokeWidth="2.4" strokeLinecap="round" fill="none" />
    </svg>
  );
}

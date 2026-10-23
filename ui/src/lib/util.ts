/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import type { Settings } from "../ipc/generated/Settings";
import { useStore } from "../state/store";

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null || !Number.isFinite(bytes)) return "";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = bytes;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

export function formatRate(bps: number): string {
  return `${formatBytes(bps)}/s`;
}

export function hostOf(url: string): string {
  try {
    const u = new URL(url);
    if (u.protocol === "haku:") return "HakuCX";
    return u.hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}

export function isInternal(url: string | undefined | null): boolean {
  return !!url && (url.startsWith("haku:") || url.startsWith("about:"));
}

export function internalPage(url: string): string {
  try {
    return new URL(url).host || "start";
  } catch {
    return "start";
  }
}

export function timeOfDay(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

export function dayLabel(ms: number): string {
  const d = new Date(ms);
  const today = new Date();
  const start = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const diff = Math.round((start(today) - start(d)) / 86_400_000);
  if (diff === 0) return "Today";
  if (diff === 1) return "Yesterday";
  return d.toLocaleDateString([], { weekday: "long", month: "long", day: "numeric", year: "numeric" });
}

export function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

/** A deep partial of the settings document, for `patch-settings`. */
export type SettingsPatch = { [K in keyof Settings]?: Partial<Settings[K]> };

export function patch(p: SettingsPatch): void {
  useStore.getState().patchSettings(p);
}

/** Read a value from the settings by "section.key". */
export function useSetting<K extends keyof Settings, F extends keyof Settings[K]>(section: K, key: F): Settings[K][F] | undefined {
  return useStore((s) => s.settings?.[section]?.[key]);
}

export function letterOf(title: string, url: string): string {
  const source = (title || hostOf(url) || "?").trim();
  return (source[0] ?? "?").toUpperCase();
}

/** A stable color for sites without a favicon. */
export function colorFor(text: string): string {
  let h = 0;
  for (const c of text) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return `hsl(${h % 360} 55% 45%)`;
}

export function clsx(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(" ");
}

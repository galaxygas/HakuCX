/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useEffect, useState } from "react";
import { Checkbox, Select, Toggle } from "../components/controls";
import { Icon } from "../components/Icon";
import { onEvent, send } from "../ipc/bridge";
import type { Cleaner } from "../ipc/generated/Cleaner";
import type { CleanerAge } from "../ipc/generated/CleanerAge";
import type { CleanerItem } from "../ipc/generated/CleanerItem";
import type { CleanerPreset } from "../ipc/generated/CleanerPreset";
import type { CleanerStats } from "../ipc/generated/CleanerStats";
import { clsx, formatBytes, patch } from "../lib/util";
import { useStore } from "../state/store";

const AGES: { value: CleanerAge; label: string }[] = [
  { value: "all", label: "Everything" },
  { value: "older-than-hour", label: "Older than an hour" },
  { value: "older-than-day", label: "Older than a day" },
  { value: "older-than-week", label: "Older than a week" },
  { value: "older-than30-days", label: "Older than 30 days" },
];

type Key = "cache" | "cookies" | "tabs" | "history" | "downloads" | "sidebarIcons";

const ROWS: { key: Key; label: string; description: string }[] = [
  { key: "cache", label: "Cache", description: "Removes cached images and files. The engine clears its whole cache; age filters do not apply." },
  { key: "cookies", label: "Cookies", description: "Clear site cookies (may sign you out of sites). The engine clears all cookies; age filters do not apply." },
  { key: "tabs", label: "Tabs", description: "Close unused tabs (keeps pinned tabs and the tab you are on)." },
  { key: "history", label: "Browsing history", description: "Clear history and auto-complete data." },
  { key: "downloads", label: "Downloads", description: "Remove downloads history and optionally delete files." },
  { key: "sidebarIcons", label: "Sidebar icons", description: "Hide sidebar icons you have not opened recently." },
];

/** When each sidebar item was last opened (kept by the UI, per profile). */
export function noteSidebarUse(id: string) {
  try {
    const raw = JSON.parse(localStorage.getItem("hx-sidebar-use") ?? "{}") as Record<string, number>;
    raw[id] = Date.now();
    localStorage.setItem("hx-sidebar-use", JSON.stringify(raw));
  } catch {
    // Storage unavailable: the cleaner simply keeps all icons.
  }
}

const AGE_MS: Record<CleanerAge, number> = {
  all: 0,
  "older-than-hour": 3_600_000,
  "older-than-day": 86_400_000,
  "older-than-week": 7 * 86_400_000,
  "older-than30-days": 30 * 86_400_000,
};

export function CleanerPanel() {
  const cleaner = useStore((s) => s.settings?.cleaner);
  const sidebar = useStore((s) => s.settings?.sidebar);
  const [stats, setStats] = useState<CleanerStats | null>(null);
  const [open, setOpen] = useState<Key | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const off = onEvent((e) => {
      if (e.type === "cleaner-stats") {
        setStats(e.stats);
        setBusy(false);
      }
    });
    send({ type: "cleaner-stats" });
    return off;
  }, []);

  if (!cleaner || !sidebar) return null;

  const setPreset = (preset: CleanerPreset) => send({ type: "patch-settings", patch: { cleaner: presetValues(preset) } });
  const setItem = (key: Key, value: Partial<CleanerItem>) => patch({ cleaner: { [key]: { ...cleaner[key], ...value } } as Partial<Cleaner> });

  const run = () => {
    setBusy(true);
    send({ type: "run-cleaner", cleaner });
    if (cleaner.sidebarIcons.enabled) {
      // Sidebar icon usage is tracked by the UI, so this part runs here.
      let used: Record<string, number> = {};
      try {
        used = JSON.parse(localStorage.getItem("hx-sidebar-use") ?? "{}");
      } catch {
        used = {};
      }
      const cutoff = Date.now() - AGE_MS[cleaner.sidebarIcons.age];
      // Settings stays reachable no matter what.
      const keep = sidebar.items.filter((id) => (used[id] ?? 0) >= cutoff || id === "settings");
      if (keep.length !== sidebar.items.length) patch({ sidebar: { items: keep } });
    }
  };

  return (
    <div className="panel-scroll cleaner-panel">
      <div className="cleaner-hero">
        <div className="octagon big">
          <small className="display">Browser files</small>
          <strong>{stats ? formatBytes(stats.profileBytes) : "..."}</strong>
        </div>
        <div className="cleaner-stats">
          <div className="octagon">
            <strong>{stats ? stats.historyVisits.toLocaleString() : "..."}</strong>
            <small className="display">History</small>
          </div>
          <div className="octagon">
            <strong>{stats ? formatBytes(stats.downloadBytes) : "..."}</strong>
            <small className="display">Downloads</small>
          </div>
          <div className="octagon">
            <strong>{stats ? stats.sitesWithData.toLocaleString() : "..."}</strong>
            <small className="display">Sites with data</small>
          </div>
          <div className="octagon">
            <strong>{stats ? stats.cacheEntries.toLocaleString() : "..."}</strong>
            <small className="display">Cached items</small>
          </div>
        </div>
      </div>
      <div className="preset-row">
        {(["min", "med", "max"] as CleanerPreset[]).map((p) => (
          <button key={p} className={clsx("octagon-btn display", cleaner.preset === p && "active")} onClick={() => setPreset(p)}>
            {p}
          </button>
        ))}
      </div>
      <button className="btn btn-accent wide" disabled={busy} onClick={run}>
        {busy ? "Cleaning..." : "Start cleaning"}
      </button>
      <h3 className="display section-label">Advanced setup</h3>
      {ROWS.map((row) => {
        const item = cleaner[row.key];
        return (
          <div key={row.key} className={clsx("cleaner-row", open === row.key && "open")}>
            <div className="cleaner-row-head">
              <Toggle on={item.enabled} onChange={(v) => setItem(row.key, { enabled: v })} label={row.label} />
              <span className="grow">{row.label}</span>
              <button className="icon-btn" onClick={() => setOpen(open === row.key ? null : row.key)} title="Details">
                <Icon name={open === row.key ? "chevron-up" : "chevron-down"} size={15} />
              </button>
            </div>
            {open === row.key && (
              <div className="cleaner-row-body">
                <p className="muted">{row.description}</p>
                <Select value={item.age} options={AGES} onChange={(v) => setItem(row.key, { age: v })} />
                {row.key === "downloads" && (
                  <div className="col">
                    <Checkbox checked={cleaner.deleteDownloadedFiles} onChange={(v) => patch({ cleaner: { deleteDownloadedFiles: v } })} label="Delete downloaded files" />
                    <Checkbox checked={cleaner.includeUnfinishedDownloads} onChange={(v) => patch({ cleaner: { includeUnfinishedDownloads: v } })} label="Include unfinished downloads" />
                  </div>
                )}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

function presetValues(preset: CleanerPreset): Partial<Cleaner> {
  const item = (enabled: boolean, age: CleanerAge): CleanerItem => ({ enabled, age });
  switch (preset) {
    case "min":
      return { preset, cache: item(true, "older-than-day"), cookies: item(false, "older-than-week"), tabs: item(false, "older-than-hour"), history: item(true, "older-than30-days"), downloads: item(true, "older-than30-days"), deleteDownloadedFiles: false, includeUnfinishedDownloads: false, sidebarIcons: item(false, "older-than-week") };
    case "med":
      return { preset, cache: item(true, "all"), cookies: item(true, "older-than-week"), tabs: item(true, "older-than-day"), history: item(true, "older-than-week"), downloads: item(true, "older-than-week"), deleteDownloadedFiles: false, includeUnfinishedDownloads: false, sidebarIcons: item(false, "older-than-week") };
    case "max":
      return { preset, cache: item(true, "all"), cookies: item(true, "all"), tabs: item(true, "older-than-hour"), history: item(true, "all"), downloads: item(true, "all"), deleteDownloadedFiles: false, includeUnfinishedDownloads: true, sidebarIcons: item(true, "older-than-week") };
  }
}

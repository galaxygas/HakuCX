/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Counts the local statistics GX Rewards levels and achievements are based on. Stored in this
// profile's UI storage only; nothing leaves the device.

import { onEvent } from "../ipc/bridge";
import { readRewards, type RewardStats } from "../panels/MiscPanels";
import { useStore } from "../state/store";

const seenUrls = new Map<string, string>();
const seenDownloads = new Set<string>();
let lastBlockedTotal = 0;

function update(fn: (s: RewardStats) => void) {
  if (!useStore.getState().settings?.features.rewards) return;
  const stats = readRewards();
  fn(stats);
  try {
    localStorage.setItem("hx-rewards", JSON.stringify(stats));
  } catch {
    // Not persisted.
  }
}

export function startRewardTracking() {
  onEvent((e) => {
    switch (e.type) {
      case "init":
        // Downloads finished in earlier sessions were already counted.
        for (const d of e.downloads) if (d.state.state === "completed") seenDownloads.add(d.id);
        break;
      case "tab-meta": {
        const { tab, url, loading } = e.meta;
        if (!loading && /^https?:/.test(url) && seenUrls.get(tab) !== url) {
          seenUrls.set(tab, url);
          update((s) => s.pages++);
        }
        break;
      }
      case "blocked": {
        // `total` counts this session only; add what is new since the last event.
        const delta = Math.max(0, e.total - lastBlockedTotal);
        lastBlockedTotal = e.total;
        if (delta) update((s) => (s.blocked += delta));
        break;
      }
      case "downloads": {
        const fresh = e.items.filter((d) => d.state.state === "completed" && !seenDownloads.has(d.id));
        fresh.forEach((d) => seenDownloads.add(d.id));
        if (fresh.length) update((s) => (s.downloads += fresh.length));
        break;
      }
      case "session":
        update((s) => (s.tabs = Math.max(s.tabs, e.session.tabs.length)));
        break;
      case "settings": {
        const theme = e.settings.appearance.theme;
        update((s) => {
          if (!s.themes.includes(theme)) s.themes.push(theme);
        });
        break;
      }
    }
  });
}

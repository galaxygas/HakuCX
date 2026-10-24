/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// The card shown when the pointer rests on a tab: its title and address ("Show enhanced tab
// tooltips") and a preview of the page ("Show tab previews"; the core captures it while the
// tab is on screen).

import { hostOf, isInternal } from "../lib/util";
import { useStore } from "../state/store";

export function TabHoverCard() {
  const hover = useStore((s) => s.hoverTab);
  const tab = useStore((s) => s.session?.tabs.find((t) => t.id === hover?.tab));
  const meta = useStore((s) => (hover ? s.meta[hover.tab] : undefined));
  const thumb = useStore((s) => (hover ? s.thumbs[hover.tab] : undefined));
  const previews = useStore((s) => s.settings?.tabs.tabPreviews ?? false);
  const enhanced = useStore((s) => s.settings?.tabs.enhancedTooltips ?? false);
  const island = useStore((s) => s.session?.islands.find((i) => i.id === tab?.islandId));
  if (!hover || !tab) return null;
  const url = meta?.url || tab.url;
  const title = meta?.title || tab.title || (isInternal(url) ? "Speed Dial" : hostOf(url));
  const status = [
    meta?.audible && !tab.muted ? "Playing sound" : null,
    tab.muted ? "Muted" : null,
    tab.snoozed ? "Snoozed to save memory" : null,
    meta?.loading ? "Loading" : null,
    island ? `Island: ${island.name || "unnamed"}` : null,
  ].filter(Boolean);
  const width = 280;
  const left = Math.max(8, Math.min(window.innerWidth - width - 8, hover.x));
  return (
    <div className="tab-card" style={{ left, top: hover.y + 6, width }}>
      {previews && thumb && <img className="tab-card-thumb" src={thumb} alt="" />}
      <div className="tab-card-text">
        <strong className="tab-card-title">{title}</strong>
        <span className="tab-card-url">{enhanced ? url : isInternal(url) ? url : hostOf(url)}</span>
        {enhanced && status.length > 0 && <span className="tab-card-status">{status.join(" · ")}</span>}
      </div>
    </div>
  );
}

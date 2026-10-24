/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// The miniplayer: what tabs are playing, with play/pause, previous/next (when the site offers
// them through its Media Session) and video pop out. Shown in the toolbar or the sidebar,
// per Settings > Features > Player.

import { useState } from "react";
import { Icon } from "../components/Icon";
import { send } from "../ipc/bridge";
import type { MediaAction } from "../ipc/generated/MediaAction";
import type { TabMeta } from "../ipc/generated/TabMeta";
import { clsx, hostOf } from "../lib/util";
import { useStore } from "../state/store";

type MediaTab = TabMeta & { media: NonNullable<TabMeta["media"]> };

/** Tabs with audio or video, playing ones first (in tab order). */
export function useMediaTabs(): MediaTab[] {
  const meta = useStore((s) => s.meta);
  const session = useStore((s) => s.session);
  const order = session?.tabs.map((t) => t.id) ?? [];
  return order
    .map((id) => meta[id])
    .filter((m): m is MediaTab => !!m?.media)
    .sort((a, b) => Number(a.media.paused && !a.media.popped) - Number(b.media.paused && !b.media.popped));
}

const act = (tab: string, action: MediaAction) => send({ type: "media", tab, action });

function Artwork({ tab, size }: { tab: MediaTab; size: number }) {
  // A site's artwork address can fail to load; fall back to the tab icon, then a glyph.
  const [failed, setFailed] = useState<string[]>([]);
  const src = [tab.media.artwork, tab.favicon].find((u): u is string => !!u && !failed.includes(u));
  return (
    <span className="mini-art" style={{ width: size, height: size }}>
      {src ? <img src={src} alt="" onError={() => setFailed((f) => [...f, src])} /> : <Icon name={tab.media.video ? "film" : "music"} size={size * 0.5} />}
    </span>
  );
}

export function MediaCard({ tab }: { tab: MediaTab }) {
  const background = useStore((s) => s.settings?.player.miniplayerBackground ?? true);
  const popoutOn = useStore((s) => s.settings?.video.popout ?? true);
  const m = tab.media;
  const title = m.title || tab.title || hostOf(tab.url);
  const subtitle = [m.artist, m.album].filter(Boolean).join(" · ") || hostOf(tab.url);
  const playing = !m.paused || m.popped;
  return (
    <div className={clsx("mini-card", background && "with-bg")}>
      {background && m.artwork && <div className="mini-card-bg" style={{ backgroundImage: `url("${m.artwork.replace(/"/g, "%22")}")` }} />}
      <button className="mini-info" title="Go to tab" onClick={() => { send({ type: "activate-tab", tab: tab.tab }); useStore.getState().openPopup(null); }}>
        <Artwork tab={tab} size={44} />
        <span className="mini-text">
          <span className="mini-title ellipsis">{title}</span>
          <span className="mini-sub ellipsis muted">{m.popped ? "Playing in the pop-out window" : subtitle}</span>
        </span>
      </button>
      <div className="mini-controls">
        {m.hasPrev && (
          <button className="icon-btn" title="Previous" onClick={() => act(tab.tab, "prev")}>
            <Icon name="prev" size={16} />
          </button>
        )}
        {m.popped ? (
          <button className="icon-btn" title="Back to tab" onClick={() => act(tab.tab, "return")}>
            <Icon name="pip" size={16} />
          </button>
        ) : (
          <button className="icon-btn mini-play" title={playing ? "Pause" : "Play"} onClick={() => act(tab.tab, playing ? "pause" : "play")}>
            <Icon name={playing ? "pause" : "play"} size={17} />
          </button>
        )}
        {m.hasNext && (
          <button className="icon-btn" title="Next" onClick={() => act(tab.tab, "next")}>
            <Icon name="next" size={16} />
          </button>
        )}
        {m.video && m.poppable && popoutOn && !m.popped && (
          <button className="icon-btn" title="Pop out video" onClick={() => act(tab.tab, "pop-out")}>
            <Icon name="pip" size={16} />
          </button>
        )}
      </div>
    </div>
  );
}

export function MediaPopupBody() {
  const tabs = useMediaTabs();
  if (!tabs.length) return <p className="muted pad">Nothing is playing.</p>;
  return (
    <div className="mini-list">
      {tabs.map((t) => (
        <MediaCard key={t.tab} tab={t} />
      ))}
    </div>
  );
}

/** Toolbar button (miniplayer "In toolbar"). */
export function ToolbarMiniplayer() {
  const mode = useStore((s) => s.settings?.player.miniplayer);
  const popup = useStore((s) => s.popup);
  const openPopup = useStore((s) => s.openPopup);
  const tabs = useMediaTabs();
  if (mode !== "in-toolbar" || !tabs.length) return null;
  const playing = tabs.some((t) => !t.media.paused || t.media.popped);
  return (
    <button
      className={clsx("tool-btn", "mini-btn", popup === "media" && "active", playing && "playing")}
      title="Playing media"
      onClick={(e) => openPopup("media", (e.currentTarget as HTMLElement).getBoundingClientRect())}
    >
      <Icon name="music" size={17} />
    </button>
  );
}

/** Sidebar miniplayer ("In sidebar"): the first tab's artwork and play/pause. */
export function SidebarMiniplayer() {
  const mode = useStore((s) => s.settings?.player.miniplayer);
  const openPopup = useStore((s) => s.openPopup);
  const tabs = useMediaTabs();
  const first = tabs[0];
  if (mode !== "in-sidebar" || !first) return null;
  const playing = !first.media.paused || first.media.popped;
  return (
    <div className="sidebar-mini">
      <button className="sidebar-item" title="Playing media" onClick={(e) => openPopup("media", (e.currentTarget as HTMLElement).getBoundingClientRect())}>
        <Artwork tab={first} size={26} />
      </button>
      {!first.media.popped && (
        <button className="sidebar-item" title={playing ? "Pause" : "Play"} onClick={() => act(first.tab, playing ? "pause" : "play")}>
          <Icon name={playing ? "pause" : "play"} size={17} />
        </button>
      )}
    </div>
  );
}

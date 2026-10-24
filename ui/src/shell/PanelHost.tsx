/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useEffect, useRef } from "react";
import { Icon, Logo } from "../components/Icon";
import { send } from "../ipc/bridge";
import { sidebarItem } from "../lib/catalog";
import { clsx } from "../lib/util";
import { AiPanel } from "../panels/AiPanel";
import { CleanerPanel } from "../panels/CleanerPanel";
import { ControlPanel } from "../panels/ControlPanel";
import { BookmarksPanel, DownloadsPanel, HistoryPanel, TabsPanel } from "../panels/ListPanels";
import { FlowPanel, NewsPanel, PinboardsPanel, PlayerPanel, RewardsPanel } from "../panels/MiscPanels";
import { ShadersPanel } from "../panels/ShadersPanel";
import { useStore } from "../state/store";

const TITLES: Record<string, string> = {
  "gx-control": "GX Control",
  "gx-cleaner": "GX Cleaner",
  shaders: "Shaders",
  player: "Player",
  history: "History",
  downloads: "Downloads",
  bookmarks: "Bookmarks",
  pinboards: "Pinboards",
  news: "Personal news",
  rewards: "GX Rewards",
  "my-flow": "My Flow",
  tabs: "Tabs",
  ai: "AI",
};

export function PanelHost() {
  const panel = useStore((s) => s.panel);
  const settings = useStore((s) => s.settings);
  const setPanel = useStore((s) => s.setPanel);
  const item = panel ? sidebarItem(panel) : undefined;
  const webUrl = item?.web;

  // Keep the core's web panel in step with the open sidebar item.
  const shownWeb = useRef<string | null>(null);
  useEffect(() => {
    if (panel && webUrl) {
      send({ type: "open-web-panel", id: panel, url: webUrl });
      shownWeb.current = panel;
    } else if (shownWeb.current && panel !== "player") {
      // The Player panel manages its own web panel (child effects run before this one).
      send({ type: "hide-web-panel" });
      shownWeb.current = null;
    }
    window.dispatchEvent(new CustomEvent("hx-layout"));
  }, [panel, webUrl]);

  if (!panel || !settings) return null;
  const pinned = settings.sidebar.pinnedPanels.includes(panel);
  const width = settings.sidebar.panelWidths[panel] ?? (webUrl ? 440 : 400);
  const title = item?.label ?? TITLES[panel] ?? panel;

  const togglePin = () => {
    const pins = pinned ? settings.sidebar.pinnedPanels.filter((p) => p !== panel) : [...settings.sidebar.pinnedPanels, panel];
    send({ type: "patch-settings", patch: { sidebar: { pinnedPanels: pins } } });
  };

  return (
    <section className={clsx("panel", pinned ? "pinned" : "floating", webUrl && "web-panel")} style={{ width }}>
      <header className="panel-header">
        <span className="panel-logo">
          <Logo size={18} />
        </span>
        <h2 className="display panel-title">{title}</h2>
        <span className="grow" />
        {webUrl && (
          <>
            <button className="icon-btn" title="Back" onClick={() => send({ type: "web-panel-navigate", id: panel, action: "back" })}>
              <Icon name="back" size={16} />
            </button>
            <button className="icon-btn" title="Reload" onClick={() => send({ type: "web-panel-navigate", id: panel, action: "reload" })}>
              <Icon name="reload" size={15} />
            </button>
          </>
        )}
        <button className={clsx("icon-btn", pinned && "active")} title={pinned ? "Unpin panel" : "Pin panel"} onClick={togglePin}>
          <Icon name="pin" size={15} />
        </button>
        <button className="icon-btn" title="Close" onClick={() => setPanel(null)}>
          <Icon name="x" size={16} />
        </button>
      </header>
      <div className="panel-body">{webUrl ? <div className="web-panel-hole" data-web-panel={panel} /> : <PanelContent id={panel} />}</div>
      <ResizeHandle panel={panel} width={width} />
    </section>
  );
}

function PanelContent({ id }: { id: string }) {
  switch (id) {
    case "gx-control":
      return <ControlPanel />;
    case "gx-cleaner":
      return <CleanerPanel />;
    case "shaders":
      return <ShadersPanel />;
    case "history":
      return <HistoryPanel />;
    case "downloads":
      return <DownloadsPanel />;
    case "bookmarks":
      return <BookmarksPanel />;
    case "tabs":
      return <TabsPanel />;
    case "pinboards":
      return <PinboardsPanel />;
    case "player":
      return <PlayerPanel />;
    case "news":
      return <NewsPanel />;
    case "rewards":
      return <RewardsPanel />;
    case "my-flow":
      return <FlowPanel />;
    case "ai":
      return <AiPanel />;
    default:
      return <div className="panel-empty">This panel is not available.</div>;
  }
}

function ResizeHandle({ panel, width }: { panel: string; width: number }) {
  return (
    <div
      className="panel-resize"
      onMouseDown={(e) => {
        e.preventDefault();
        const startX = e.clientX;
        const el = (e.currentTarget as HTMLElement).parentElement!;
        let w = width;
        const move = (ev: MouseEvent) => {
          w = Math.max(300, Math.min(900, width + ev.clientX - startX));
          el.style.width = `${w}px`;
          window.dispatchEvent(new CustomEvent("hx-layout"));
        };
        const up = () => {
          window.removeEventListener("mousemove", move);
          window.removeEventListener("mouseup", up);
          const widths = { ...(useStore.getState().settings?.sidebar.panelWidths ?? {}), [panel]: Math.round(w) };
          send({ type: "patch-settings", patch: { sidebar: { panelWidths: widths } } });
        };
        window.addEventListener("mousemove", move);
        window.addEventListener("mouseup", up);
      }}
    />
  );
}

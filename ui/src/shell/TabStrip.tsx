/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { Fragment, useMemo, useRef, useState, type ReactElement } from "react";
import { Icon, Logo } from "../components/Icon";
import { send } from "../ipc/bridge";
import type { Island } from "../ipc/generated/Island";
import type { Tab } from "../ipc/generated/Tab";
import { clsx, hostOf, isInternal } from "../lib/util";
import { useStore } from "../state/store";

const TRACE_THRESHOLDS: Record<string, number> = { "more-than10": 10, "more-than15": 15, "more-than30": 30 };

export function TabStrip() {
  const session = useStore((s) => s.session);
  const settings = useStore((s) => s.settings);
  const maximized = useStore((s) => s.maximized);
  const openPopup = useStore((s) => s.openPopup);
  const setPalette = useStore((s) => s.setPalette);
  const stripRef = useRef<HTMLDivElement>(null);
  const [drag, setDrag] = useState<{ tab: string; over: number } | null>(null);
  // Dragging a tab down onto the page: which half it would take in a split screen.
  const [splitDrop, setSplitDrop] = useState<{ tab: string; side: "left" | "right" | null } | null>(null);

  const ws = session?.activeWorkspace ?? "";
  const tabs = useMemo(() => session?.tabs.filter((t) => t.workspaceId === ws) ?? [], [session, ws]);
  const active = session?.activeTabs[ws];
  const islands = useMemo(() => new Map((session?.islands ?? []).map((i) => [i.id, i])), [session]);
  const frameless = settings ? !settings.appearance.nativeTitleBar : true;

  // Tab traces: the most recently used tabs get a fading accent underline.
  const traces = useMemo(() => {
    const threshold = TRACE_THRESHOLDS[settings?.tabs.traces ?? "off"];
    const map = new Map<string, number>();
    if (!threshold || !settings?.features.tabTraces || tabs.length <= threshold) return map;
    (session?.mru ?? []).filter((id) => tabs.some((t) => t.id === id)).slice(1, 6).forEach((id, i) => map.set(id, 1 - i * 0.18));
    return map;
  }, [settings, session, tabs]);

  const startDrag = (e: React.MouseEvent, tab: Tab, index: number) => {
    if (e.button !== 0) return;
    const startX = e.clientX;
    const startY = e.clientY;
    let moved = false;
    let toPage = false;
    const splitByDrag = !!settings?.appearance.splitScreen && !!settings.appearance.splitScreenDrag;
    const move = (ev: MouseEvent) => {
      if (!moved && Math.abs(ev.clientX - startX) < 6 && Math.abs(ev.clientY - startY) < 6) return;
      moved = true;
      const strip = stripRef.current;
      if (!strip) return;
      const viewport = document.querySelector(".viewport")?.getBoundingClientRect();
      // Below the tab strip: offer the page halves (the overlay takes the pointer from the
      // page so the drag can continue there).
      if (splitByDrag && viewport && ev.clientY > strip.getBoundingClientRect().bottom + 24) {
        toPage = true;
        const side = ev.clientY < viewport.top ? null : ev.clientX < viewport.left + viewport.width / 2 ? "left" : "right";
        setSplitDrop({ tab: tab.id, side });
        setDrag(null);
        window.dispatchEvent(new CustomEvent("hx-layout"));
        return;
      }
      if (toPage) {
        toPage = false;
        setSplitDrop(null);
        window.dispatchEvent(new CustomEvent("hx-layout"));
      }
      const items = Array.from(strip.querySelectorAll<HTMLElement>("[data-tab-index]"));
      let over = items.length;
      for (const el of items) {
        const r = el.getBoundingClientRect();
        if (ev.clientX < r.left + r.width / 2) {
          over = Number(el.dataset.tabIndex);
          break;
        }
      }
      setDrag({ tab: tab.id, over });
    };
    const up = () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      if (toPage) {
        setSplitDrop((drop) => {
          const s = useStore.getState().session;
          const current = s?.activeTabs[s.activeWorkspace];
          // Pair with the active tab, or (dragging the active tab) with the one used before it.
          const other = current && current !== tab.id ? current : s?.mru.find((id) => id !== tab.id && s.tabs.some((t) => t.id === id && t.workspaceId === s.activeWorkspace));
          if (drop?.side && other) {
            const [left, right] = drop.side === "left" ? [tab.id, other] : [other, tab.id];
            send({ type: "set-split", left, right });
          }
          return null;
        });
        window.dispatchEvent(new CustomEvent("hx-layout"));
        return;
      }
      setDrag((d) => {
        if (moved && d) {
          const target = d.over > index ? d.over - 1 : d.over;
          if (target !== index) send({ type: "move-tab", tab: tab.id, index: target });
        }
        return null;
      });
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  const rendered: ReactElement[] = [];
  let lastIsland: string | null = null;
  tabs.forEach((tab, index) => {
    const island = tab.islandId ? islands.get(tab.islandId) : undefined;
    if (island && island.id !== lastIsland) {
      rendered.push(<IslandChip key={`island-${island.id}`} island={island} count={tabs.filter((t) => t.islandId === island.id).length} />);
    }
    lastIsland = island?.id ?? null;
    if (island?.collapsed && tab.id !== active) return;
    rendered.push(
      <Fragment key={tab.id}>
        {drag && drag.over === index && drag.tab !== tab.id && <div className="tab-drop" />}
        <TabButton
          tab={tab}
          index={index}
          active={tab.id === active}
          island={island}
          trace={traces.get(tab.id)}
          dragging={drag?.tab === tab.id}
          onMouseDown={(e) => startDrag(e, tab, index)}
        />
      </Fragment>,
    );
  });

  return (
    <>
    {splitDrop && <SplitDropOverlay side={splitDrop.side} />}
    <header
      className="tabstrip"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && e.button === 0 && frameless) send({ type: "window", action: "drag" });
      }}
      onDoubleClick={(e) => {
        if (e.target === e.currentTarget && frameless) send({ type: "window", action: "toggle-maximize" });
      }}
    >
      <button
        className="gx-menu-button"
        title="Menu"
        onClick={(e) => openPopup("main-menu", (e.currentTarget as HTMLElement).getBoundingClientRect())}
      >
        <Logo size={22} />
      </button>
      {settings?.sidebar.workspacesEnabled && (
        <button className="workspace-button" title="Workspaces" onClick={(e) => openPopup("workspaces", (e.currentTarget as HTMLElement).getBoundingClientRect())}>
          <Icon name={session?.workspaces.find((w) => w.id === ws)?.icon ?? "home"} size={16} />
        </button>
      )}
      <div className="tabs" ref={stripRef}>
        {rendered}
        {drag && drag.over >= tabs.length && <div className="tab-drop" />}
        <button className="new-tab" title="New tab (Ctrl+T)" onClick={() => send({ type: "new-tab", url: null, background: false, relativeTo: null })}>
          <Icon name="plus" size={16} />
        </button>
      </div>
      <div
        className="tabstrip-drag"
        onMouseDown={(e) => e.button === 0 && frameless && send({ type: "window", action: "drag" })}
        onDoubleClick={() => frameless && send({ type: "window", action: "toggle-maximize" })}
      />
      {settings?.tabs.searchInTabs && (
        <button className="tabstrip-icon" title="Search tabs (Ctrl+Space)" onClick={() => setPalette("tabs")}>
          <Icon name="search" size={16} />
        </button>
      )}
      {frameless && (
        <div className="window-controls">
          <button title="Minimize" onClick={() => send({ type: "window", action: "minimize" })}>
            <Icon name="minus" size={16} />
          </button>
          <button title={maximized ? "Restore" : "Maximize"} onClick={() => send({ type: "window", action: "toggle-maximize" })}>
            <Icon name={maximized ? "minimize" : "square"} size={14} />
          </button>
          <button
            className="close"
            title="Close"
            onClick={() => {
              const s = useStore.getState();
              if (s.settings?.tabs.warnCloseWindow && (s.session?.tabs.length ?? 0) > 1) s.openModal({ kind: "quit", window: true });
              else send({ type: "window", action: "close" });
            }}
          >
            <Icon name="x" size={17} />
          </button>
        </div>
      )}
    </header>
    </>
  );
}

function IslandChip({ island, count }: { island: Island; count: number }) {
  const openMenu = useStore((s) => s.openMenu);
  return (
    <button
      className={clsx("island-chip", island.collapsed && "collapsed")}
      style={{ "--island": island.color ?? "var(--hx-accent)" } as React.CSSProperties}
      title={island.collapsed ? `Expand ${island.name}` : `Collapse ${island.name}`}
      onClick={() => send({ type: "update-island", island: island.id, name: null, emoji: null, color: null, collapsed: !island.collapsed })}
      onContextMenu={(e) => {
        e.preventDefault();
        openMenu({ kind: "island", x: e.clientX, y: e.clientY, island: island.id });
      }}
    >
      {island.emoji && <span className="island-emoji">{island.emoji}</span>}
      <span className="ellipsis">{island.name || "Island"}</span>
      {island.collapsed && <span className="island-count">{count}</span>}
    </button>
  );
}

function TabButton({ tab, index, active, island, trace, dragging, onMouseDown }: { tab: Tab; index: number; active: boolean; island?: Island; trace?: number; dragging: boolean; onMouseDown: (e: React.MouseEvent) => void }) {
  const meta = useStore((s) => s.meta[tab.id]);
  const openMenu = useStore((s) => s.openMenu);
  const previews = useStore((s) => s.settings?.tabs.tabPreviews);
  const enhanced = useStore((s) => s.settings?.tabs.enhancedTooltips);
  const setHoverTab = useStore((s) => s.setHoverTab);
  const hoverTimer = useRef(0);
  const title = meta?.title || tab.title || (isInternal(tab.url) ? "Speed Dial" : hostOf(tab.url));
  const loading = meta?.loading;
  // With enhanced tooltips or previews, a hover card replaces the plain tooltip.
  const card = previews || enhanced;
  return (
    <div
      data-tab-index={index}
      className={clsx("tab", active && "active", tab.pinned && "pinned", island && "in-island", dragging && "dragging", tab.snoozed && "snoozed")}
      style={{ "--island": island?.color ?? "var(--hx-accent)", "--trace": trace ?? 0 } as React.CSSProperties}
      title={card ? undefined : title}
      onMouseEnter={(e) => {
        if (!card) return;
        const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
        window.clearTimeout(hoverTimer.current);
        hoverTimer.current = window.setTimeout(() => setHoverTab({ tab: tab.id, x: r.left, y: r.bottom }), 450);
      }}
      onMouseLeave={() => {
        window.clearTimeout(hoverTimer.current);
        if (useStore.getState().hoverTab?.tab === tab.id) setHoverTab(null);
      }}
      onMouseDown={(e) => {
        window.clearTimeout(hoverTimer.current);
        setHoverTab(null);
        if (e.button === 1) {
          e.preventDefault();
          send({ type: "close-tab", tab: tab.id });
          return;
        }
        if (e.button === 0 && !active) send({ type: "activate-tab", tab: tab.id });
        else if (e.button === 0 && useStore.getState().settings?.tabs.scrollTopOnActiveClick) {
          send({ type: "page", action: "scroll-to-top", tab: tab.id, data: null });
        }
        onMouseDown(e);
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        openMenu({ kind: "tab", x: e.clientX, y: e.clientY, tab: tab.id });
      }}
    >
      <span className="tab-icon">
        {loading ? (
          <span className="spinner" />
        ) : tab.emoji ? (
          <span className="tab-emoji">{tab.emoji}</span>
        ) : meta?.favicon ? (
          <img src={meta.favicon} alt="" width={16} height={16} />
        ) : isInternal(tab.url) ? (
          <Logo size={15} />
        ) : (
          <Icon name="globe" size={14} />
        )}
      </span>
      {!tab.pinned && <span className="tab-title ellipsis">{title}</span>}
      {(meta?.audible || tab.muted) && (
        <button
          className="tab-audio"
          title={tab.muted ? "Unmute tab" : "Mute tab"}
          onMouseDown={(e) => e.stopPropagation()}
          onClick={() => send({ type: "mute-tab", tab: tab.id, muted: !tab.muted })}
        >
          <Icon name={tab.muted ? "volume-x" : "volume"} size={13} />
        </button>
      )}
      {!tab.pinned && (
        <button
          className="tab-close"
          title="Close tab (Ctrl+W)"
          onMouseDown={(e) => e.stopPropagation()}
          onClick={() => send({ type: "close-tab", tab: tab.id })}
        >
          <Icon name="x" size={13} />
        </button>
      )}
      {trace !== undefined && <span className="tab-trace" />}
    </div>
  );
}

/** Drop zones over the page while a tab is dragged down from the strip (split screen by drag
 * and drop). `data-hit` makes the browser send the pointer here instead of to the page. */
function SplitDropOverlay({ side }: { side: "left" | "right" | null }) {
  const r = document.querySelector(".viewport")?.getBoundingClientRect();
  if (!r) return null;
  return (
    <div className="split-drop" data-hit style={{ left: r.left, top: r.top, width: r.width, height: r.height }}>
      <div className={clsx("split-drop-half", side === "left" && "on")}>
        <span>Open on the left</span>
      </div>
      <div className={clsx("split-drop-half", side === "right" && "on")}>
        <span>Open on the right</span>
      </div>
    </div>
  );
}

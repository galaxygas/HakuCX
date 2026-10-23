/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useEffect, useMemo, useRef, useState } from "react";
import { Icon } from "../components/Icon";
import { send } from "../ipc/bridge";
import { clsx, hostOf } from "../lib/util";
import { openInternal, useStore } from "../state/store";
import { Input } from "../components/Field";

type Entry = { id: string; icon: string; title: string; detail?: string; run: () => void };

function commands(): Entry[] {
  const s = useStore.getState();
  const toggle = (section: string, key: string, current: boolean | undefined) => () => send({ type: "patch-settings", patch: { [section]: { [key]: !current } } });
  return [
    { id: "new-tab", icon: "plus", title: "New tab", detail: "Ctrl+T", run: () => send({ type: "new-tab", url: null, background: false, relativeTo: null }) },
    { id: "private", icon: "ghost", title: "New private window", detail: "Ctrl+Shift+N", run: () => send({ type: "window", action: "new-private-window" }) },
    { id: "reopen", icon: "undo", title: "Reopen closed tab", detail: "Ctrl+Shift+T", run: () => send({ type: "reopen-closed" }) },
    { id: "settings", icon: "settings", title: "Open settings", run: () => openInternal("haku://settings/") },
    { id: "history", icon: "clock", title: "Open history", detail: "Ctrl+H", run: () => openInternal("haku://history/") },
    { id: "downloads", icon: "download", title: "Open downloads", detail: "Ctrl+J", run: () => openInternal("haku://downloads/") },
    { id: "bookmarks", icon: "bookmark", title: "Open bookmarks", run: () => openInternal("haku://bookmarks/") },
    { id: "extensions", icon: "puzzle", title: "Open extensions", run: () => openInternal("haku://extensions/") },
    { id: "control", icon: "gauge", title: "GX Control", run: () => s.setPanel("gx-control") },
    { id: "cleaner", icon: "sparkles", title: "GX Cleaner", run: () => s.setPanel("gx-cleaner") },
    { id: "easy", icon: "sliders", title: "Easy Setup", run: () => s.setDrawer("easy-setup") },
    { id: "panic", icon: "zap", title: "Panic button", detail: s.settings?.panic.key, run: () => send({ type: "panic" }) },
    { id: "music", icon: "music", title: `${s.settings?.sounds.backgroundMusic ? "Stop" : "Play"} background music`, run: toggle("sounds", "backgroundMusic", s.settings?.sounds.backgroundMusic) },
    { id: "sounds", icon: "volume", title: `${s.settings?.sounds.browserSounds ? "Turn off" : "Turn on"} browser sounds`, run: toggle("sounds", "browserSounds", s.settings?.sounds.browserSounds) },
    { id: "dark", icon: "moon", title: `${s.settings?.features.forceDarkPages ? "Stop forcing" : "Force"} dark pages`, run: toggle("features", "forceDarkPages", s.settings?.features.forceDarkPages) },
    { id: "adblock", icon: "shield", title: `${s.settings?.privacy.blockAds ? "Turn off" : "Turn on"} ad blocker`, run: toggle("privacy", "blockAds", s.settings?.privacy.blockAds) },
    { id: "bar", icon: "bookmark", title: "Toggle bookmarks bar", detail: "Ctrl+Shift+B", run: toggle("appearance", "showBookmarksBar", s.settings?.appearance.showBookmarksBar) },
    { id: "sidebar", icon: "panel-left", title: "Toggle sidebar", detail: "Ctrl+Shift+S", run: toggle("sidebar", "show", s.settings?.sidebar.show) },
    { id: "snapshot", icon: "crop", title: "Take a snapshot", run: () => send({ type: "page", action: "screenshot", tab: null, data: null }) },
    { id: "clear", icon: "trash", title: "Clear browsing data", run: () => s.openModal({ kind: "clear-data" }) },
    { id: "fullscreen", icon: "maximize", title: "Toggle full screen", detail: "F11", run: () => send({ type: "window", action: "toggle-fullscreen" }) },
    { id: "theme", icon: "palette", title: "Change theme", run: () => openInternal("haku://settings/themes") },
    { id: "corner", icon: "gamepad-2", title: "GX Corner", run: () => openInternal("haku://corner/") },
  ];
}

export function Palette() {
  const open = useStore((s) => s.palette_open);
  const session = useStore((s) => s.session);
  const meta = useStore((s) => s.meta);
  const setPalette = useStore((s) => s.setPalette);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setQuery("");
    setIndex(0);
    if (open) window.setTimeout(() => input.current?.focus(), 0);
  }, [open]);

  const entries = useMemo<Entry[]>(() => {
    if (!open || !session) return [];
    const q = query.trim().toLowerCase();
    const tabs: Entry[] = session.tabs.map((t) => {
      const ws = session.workspaces.find((w) => w.id === t.workspaceId);
      return {
        id: `tab-${t.id}`,
        icon: "panels-top-left",
        title: meta[t.id]?.title || t.title || hostOf(t.url),
        detail: `${hostOf(t.url)} · ${ws?.name ?? ""}`,
        run: () => send({ type: "activate-tab", tab: t.id }),
      };
    });
    const list = open === "tabs" ? tabs : [...commands(), ...tabs];
    if (!q) return list.slice(0, 50);
    return list.filter((e) => e.title.toLowerCase().includes(q) || e.detail?.toLowerCase().includes(q)).slice(0, 50);
  }, [open, session, meta, query]);

  if (!open) return null;
  const run = (e: Entry | undefined) => {
    if (!e) return;
    setPalette(null);
    e.run();
  };
  return (
    <div className="modal-backdrop palette-backdrop" onMouseDown={() => setPalette(null)}>
      <div className="palette" onMouseDown={(e) => e.stopPropagation()}>
        <div className="palette-input">
          <Icon name="search" size={16} />
          <Input
            ref={input}
            value={query}
            placeholder={open === "tabs" ? "Search tabs" : "Type a command or tab name"}
            onChange={(e) => {
              setQuery(e.target.value);
              setIndex(0);
            }}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setIndex((i) => Math.min(entries.length - 1, i + 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setIndex((i) => Math.max(0, i - 1));
              } else if (e.key === "Enter") run(entries[index]);
              else if (e.key === "Escape") setPalette(null);
            }}
          />
        </div>
        <div className="palette-list">
          {entries.length === 0 && <div className="panel-empty">Nothing found</div>}
          {entries.map((e, i) => (
            <button key={e.id} className={clsx("palette-item", i === index && "selected")} onMouseEnter={() => setIndex(i)} onClick={() => run(e)}>
              <Icon name={e.icon} size={15} />
              <span className="ellipsis grow">{e.title}</span>
              {e.detail && <span className="dim ellipsis">{e.detail}</span>}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useLayoutEffect, useRef, useState } from "react";
import { Icon } from "../components/Icon";
import { send } from "../ipc/bridge";
import type { PageAction } from "../ipc/generated/PageAction";
import { sidebarItem } from "../lib/catalog";
import { clsx } from "../lib/util";
import { openInternal, useStore, type Menu } from "../state/store";

const EMOJIS = ["🔥", "🎮", "🎧", "⚡", "💀", "🌙", "🎯", "🧪", "📌", "🛡️", "🚀", "👾", "💬", "📚", "🛒", "⭐"];

type Item = { label: string; icon?: string; shortcut?: string; action?: () => void; disabled?: boolean; title?: string; danger?: boolean; children?: Item[] } | "sep";

function close() {
  const s = useStore.getState();
  if (s.menu?.kind === "page") send({ type: "context-menu-closed", id: s.menu.info.id });
  s.openMenu(null);
}

function MenuList({ items, x, y }: { items: Item[]; x: number; y: number }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });
  const [sub, setSub] = useState<number | null>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPos({
      x: Math.max(4, Math.min(x, window.innerWidth - r.width - 4)),
      y: Math.max(4, Math.min(y, window.innerHeight - r.height - 4)),
    });
  }, [x, y]);
  return (
    <div ref={ref} className="popup menu context-menu" style={{ left: pos.x, top: pos.y }} onMouseDown={(e) => e.stopPropagation()}>
      {items.map((item, i) =>
        item === "sep" ? (
          <div key={i} className="menu-sep" />
        ) : (
          <div key={i} className="menu-item-wrap" onMouseEnter={() => setSub(item.children ? i : null)}>
            <button
              className={clsx("menu-item", item.danger && "danger")}
              disabled={item.disabled}
              title={item.title}
              onClick={() => {
                if (item.children) return;
                item.action?.();
                close();
              }}
            >
              {item.icon ? <Icon name={item.icon} size={15} /> : <span className="menu-icon-space" />}
              <span className="grow">{item.label}</span>
              {item.shortcut && <kbd>{item.shortcut}</kbd>}
              {item.children && <Icon name="chevron-right" size={13} />}
            </button>
            {item.children && sub === i && (
              <div className="popup menu submenu">
                {item.children.map((c, j) =>
                  c === "sep" ? (
                    <div key={j} className="menu-sep" />
                  ) : (
                    <button
                      key={j}
                      className="menu-item"
                      disabled={c.disabled}
                      onClick={() => {
                        c.action?.();
                        close();
                      }}
                    >
                      {c.icon ? <Icon name={c.icon} size={14} /> : <span className="menu-icon-space" />}
                      <span className="grow">{c.label}</span>
                    </button>
                  ),
                )}
              </div>
            )}
          </div>
        ),
      )}
    </div>
  );
}

const page = (action: PageAction, tab: string | null = null, data: string | null = null) => send({ type: "page", action, tab, data });

export function Menus() {
  const menu = useStore((s) => s.menu);
  if (!menu) return null;
  const items = itemsFor(menu);
  if (!items) return null;
  return <MenuList items={items} x={menu.x} y={menu.y} />;
}

function itemsFor(menu: Menu): Item[] | null {
  const s = useStore.getState();
  const session = s.session;
  if (!session) return null;
  switch (menu.kind) {
    case "page": {
      const info = menu.info;
      const items: Item[] = [];
      if (info.linkUrl) {
        const link = info.linkUrl;
        items.push(
          { label: "Open link in new tab", icon: "plus", action: () => page("open-link-in-new-tab", info.tab, link) },
          { label: "Open link in background tab", icon: "layers", action: () => page("open-link-in-background-tab", info.tab, link) },
          { label: "Open link in private window", icon: "ghost", action: () => page("open-link-in-private-window", info.tab, link) },
          { label: "Copy link address", icon: "link", action: () => page("copy-link", info.tab, link) },
          { label: "Save link as...", icon: "download", action: () => page("save-link-as", info.tab, link) },
          "sep",
        );
      }
      if (info.imageUrl) {
        const img = info.imageUrl;
        items.push(
          { label: "Open image in new tab", icon: "image", action: () => page("open-image-in-new-tab", info.tab, img) },
          { label: "Copy image address", icon: "link", action: () => page("copy-image-link", info.tab, img) },
          { label: "Save image as...", icon: "download", action: () => page("save-image-as", info.tab, img) },
          "sep",
        );
      }
      if (info.editable) {
        items.push(
          { label: "Cut", icon: "crop", shortcut: "Ctrl+X", action: () => page("engine-cut") },
          { label: "Copy", icon: "copy", shortcut: "Ctrl+C", action: () => page("engine-copy") },
          { label: "Paste", icon: "file-text", shortcut: "Ctrl+V", action: () => page("engine-paste") },
          { label: "Select all", shortcut: "Ctrl+A", action: () => page("engine-select-all") },
          "sep",
        );
      } else if (info.selection) {
        items.push(
          { label: "Copy", icon: "copy", shortcut: "Ctrl+C", action: () => page("engine-copy") },
          { label: "Search the web for the selection", icon: "search", action: () => page("search-selection", info.tab, null) },
          "sep",
        );
      }
      items.push(
        { label: "Back", icon: "back", shortcut: "Alt+Left", disabled: !info.canGoBack, action: () => send({ type: "back", tab: info.tab }) },
        { label: "Forward", icon: "forward", shortcut: "Alt+Right", disabled: !info.canGoForward, action: () => send({ type: "forward", tab: info.tab }) },
        { label: "Reload", icon: "reload", shortcut: "F5", action: () => send({ type: "reload", tab: info.tab, bypassCache: false }) },
        "sep",
        { label: "Add to Speed Dial", icon: "layout-grid", action: () => page("add-to-speed-dial", info.tab) },
        { label: "Add to bookmarks...", icon: "star", action: () => page("bookmark", info.tab) },
        "sep",
        { label: "Enter full screen", icon: "maximize", shortcut: "F11", action: () => send({ type: "window", action: "toggle-fullscreen" }) },
        { label: "Copy address", icon: "copy", action: () => page("copy-address", info.tab) },
        { label: "Save as...", icon: "download", shortcut: "Ctrl+S", action: () => page("save-page", info.tab) },
        { label: "Save as PDF...", icon: "file-text", disabled: true, title: "The Servo engine cannot print or make PDFs yet" },
        { label: "Force dark page", icon: "moon", action: () => page("toggle-force-dark", info.tab) },
        { label: "Print...", icon: "file", shortcut: "Ctrl+P", disabled: true, title: "The Servo engine cannot print yet" },
        { label: "Take snapshot", icon: "crop", action: () => page("screenshot", info.tab) },
        { label: "Create QR code for this page", icon: "qr", action: () => page("qr-code", info.tab) },
        { label: "Translate page", icon: "languages", action: () => page("translate", info.tab) },
        { label: "Wayback Machine", icon: "clock", action: () => page("wayback", info.tab) },
        "sep",
        { label: "Page source", icon: "code", shortcut: "Ctrl+U", action: () => page("view-source", info.tab) },
        { label: "Inspect element", icon: "search", shortcut: "Ctrl+Shift+I", action: () => s.openModal({ kind: "devtools" }) },
      );
      return items;
    }
    case "tab": {
      const tab = session.tabs.find((t) => t.id === menu.tab);
      if (!tab) return null;
      const islands = session.islands.filter((i) => i.workspaceId === tab.workspaceId);
      const inSplit = session.split && (session.split[0] === tab.id || session.split[1] === tab.id);
      const others = session.workspaces.filter((w) => w.id !== tab.workspaceId);
      const emojiAllowed = s.settings?.tabs.tabEmojis ?? true;
      const items: Item[] = [
        { label: "New tab to the right", icon: "plus", action: () => send({ type: "new-tab", url: null, background: false, relativeTo: tab.id }) },
        "sep",
        { label: "Reload", icon: "reload", action: () => send({ type: "reload", tab: tab.id, bypassCache: false }) },
        { label: "Duplicate", icon: "copy", action: () => send({ type: "duplicate-tab", tab: tab.id }) },
        { label: tab.pinned ? "Unpin tab" : "Pin tab", icon: "pin", action: () => send({ type: "pin-tab", tab: tab.id, pinned: !tab.pinned }) },
        { label: tab.muted ? "Unmute tab" : "Mute tab", icon: tab.muted ? "volume" : "volume-x", shortcut: "Ctrl+M", action: () => send({ type: "mute-tab", tab: tab.id, muted: !tab.muted }) },
      ];
      if (emojiAllowed) {
        items.push({
          label: "Tab emoji",
          icon: "sparkle",
          children: [
            ...EMOJIS.map((e) => ({ label: e, action: () => send({ type: "set-tab-emoji", tab: tab.id, emoji: e }) })),
            "sep" as const,
            { label: "Remove emoji", action: () => send({ type: "set-tab-emoji", tab: tab.id, emoji: null }) },
          ],
        });
      }
      items.push("sep");
      if (!tab.pinned) {
        items.push({
          label: "Add to island",
          icon: "layers",
          children: [
            { label: "New island", icon: "plus", action: () => send({ type: "create-island", tabs: [tab.id], name: null }) },
            ...(islands.length ? ["sep" as const] : []),
            ...islands.map((i) => ({ label: `${i.emoji ?? ""} ${i.name || "Island"}`.trim(), action: () => send({ type: "add-to-island", tab: tab.id, island: i.id }) })),
          ],
        });
      }
      if (tab.islandId) items.push({ label: "Remove from island", icon: "x", action: () => send({ type: "remove-from-island", tab: tab.id }) });
      if (others.length) {
        items.push({
          label: "Move to workspace",
          icon: "briefcase",
          children: others.map((w) => ({ label: w.name, icon: w.icon, action: () => send({ type: "move-tab-to-workspace", tab: tab.id, workspace: w.id }) })),
        });
      }
      if (s.settings?.appearance.splitScreen) {
        items.push(
          inSplit
            ? { label: "Exit split screen", icon: "split", action: () => send({ type: "clear-split" }) }
            : {
                label: "Split screen with next tab",
                icon: "split",
                action: () => {
                  const list = session.tabs.filter((t) => t.workspaceId === tab.workspaceId);
                  const i = list.findIndex((t) => t.id === tab.id);
                  const next = list[(i + 1) % list.length];
                  if (next && next.id !== tab.id) send({ type: "set-split", left: tab.id, right: next.id });
                },
              },
        );
      }
      items.push(
        { label: "Copy address", icon: "link", action: () => page("copy-address", tab.id) },
        "sep",
        { label: "Close tab", icon: "x", shortcut: "Ctrl+W", action: () => send({ type: "close-tab", tab: tab.id }) },
        { label: "Close other tabs", action: () => send({ type: "close-others", tab: tab.id }) },
        { label: "Close tabs to the right", action: () => send({ type: "close-to-right", tab: tab.id }) },
        { label: "Reopen closed tab", icon: "undo", shortcut: "Ctrl+Shift+T", disabled: session.closed.length === 0, action: () => send({ type: "reopen-closed" }) },
      );
      return items;
    }
    case "island": {
      const island = session.islands.find((i) => i.id === menu.island);
      if (!island) return null;
      return [
        { label: island.collapsed ? "Expand island" : "Collapse island", icon: "layers", action: () => send({ type: "update-island", island: island.id, name: null, emoji: null, color: null, collapsed: !island.collapsed }) },
        { label: "Edit island...", icon: "pencil", action: () => s.openModal({ kind: "island-edit", island: island.id }) },
        { label: "Ungroup", icon: "split", action: () => send({ type: "ungroup-island", island: island.id }) },
        "sep",
        { label: "Close all tabs in island", icon: "x", danger: true, action: () => send({ type: "close-island", island: island.id }) },
      ];
    }
    case "bookmark":
    case "dial": {
      const node = s.bookmarks.nodes.find((n) => n.id === menu.id);
      if (!node) return null;
      const items: Item[] = [];
      if (node.url) {
        const url = node.url;
        items.push(
          { label: "Open in new tab", icon: "plus", action: () => send({ type: "navigate", tab: null, input: url, disposition: "new-tab" }) },
          { label: "Open in background tab", icon: "layers", action: () => send({ type: "navigate", tab: null, input: url, disposition: "background-tab" }) },
          { label: "Open in private window", icon: "ghost", action: () => send({ type: "navigate", tab: null, input: url, disposition: "private-window" }) },
          "sep",
        );
      }
      items.push(
        { label: "Edit...", icon: "pencil", action: () => s.openModal(menu.kind === "dial" ? { kind: "dial-edit", id: node.id, parent: node.parent ?? "speed-dial" } : { kind: "bookmark-edit", id: node.id }) },
        { label: "Delete", icon: "trash", danger: true, action: () => send({ type: "delete-bookmark", id: node.id }) },
      );
      return items;
    }
    case "sidebar": {
      const item = sidebarItem(menu.item);
      const sb = s.settings?.sidebar;
      if (!item || !sb) return null;
      const items: Item[] = [];
      if (item.web) {
        items.push(
          { label: "Reload panel", icon: "reload", action: () => send({ type: "web-panel-navigate", id: item.id, action: "reload" }) },
          { label: "Log out / close panel page", icon: "x", action: () => send({ type: "close-web-panel", id: item.id }) },
          "sep",
        );
      }
      items.push(
        { label: "Move up", icon: "chevron-up", disabled: sb.items.indexOf(item.id) <= 0, action: () => moveSidebar(item.id, -1) },
        { label: "Move down", icon: "chevron-down", disabled: sb.items.indexOf(item.id) >= sb.items.length - 1, action: () => moveSidebar(item.id, 1) },
        { label: "Hide from sidebar", icon: "eye-off", action: () => send({ type: "patch-settings", patch: { sidebar: { items: sb.items.filter((i) => i !== item.id) } } }) },
        "sep",
        { label: "Sidebar setup", icon: "sliders", action: () => s.setDrawer("sidebar-setup") },
      );
      if (item.page) items.unshift({ label: "Open", icon: "external", action: () => openInternal(item.page!) }, "sep");
      return items;
    }
  }
}

function moveSidebar(id: string, delta: number) {
  const items = [...(useStore.getState().settings?.sidebar.items ?? [])];
  const i = items.indexOf(id);
  const j = i + delta;
  if (i < 0 || j < 0 || j >= items.length) return;
  [items[i], items[j]] = [items[j]!, items[i]!];
  send({ type: "patch-settings", patch: { sidebar: { items } } });
}


/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useEffect, useMemo, useRef, useState } from "react";
import { Icon, useThemeColor } from "../components/Icon";
import { send } from "../ipc/bridge";
import type { Suggestion } from "../ipc/generated/Suggestion";
import { clsx, isInternal } from "../lib/util";
import { useStore } from "../state/store";
import { Input } from "../components/Field";

let requestId = 1;

export function Toolbar() {
  const session = useStore((s) => s.session);
  const settings = useStore((s) => s.settings);
  const downloads = useStore((s) => s.downloads);
  const popup = useStore((s) => s.popup);
  const openPopup = useStore((s) => s.openPopup);
  const drawer = useStore((s) => s.drawer);
  const setDrawer = useStore((s) => s.setDrawer);
  const togglePanel = useStore((s) => s.togglePanel);
  const activeId = session?.activeTabs[session.activeWorkspace];
  const meta = useStore((s) => (activeId ? s.meta[activeId] : undefined));

  const running = downloads.filter((d) => d.state.state === "in-progress");
  const progress = useMemo(() => {
    const total = running.reduce((a, d) => a + (d.totalBytes ?? 0), 0);
    const got = running.reduce((a, d) => a + d.receivedBytes, 0);
    return total > 0 ? got / total : running.length ? 0.15 : 0;
  }, [running]);

  const anchor = (e: React.MouseEvent) => (e.currentTarget as HTMLElement).getBoundingClientRect();
  const accent = useThemeColor("--hx-accent", "#7d46f2");
  const track = useThemeColor("--hx-elev-2", "#1d1d23");

  return (
    <nav className="toolbar">
      <button className="nav-btn" title="Back (Alt+Left)" disabled={!meta?.canGoBack} onClick={() => send({ type: "back", tab: null })}>
        <Icon name="back" size={18} />
      </button>
      <button className="nav-btn" title="Forward (Alt+Right)" disabled={!meta?.canGoForward} onClick={() => send({ type: "forward", tab: null })}>
        <Icon name="forward" size={18} />
      </button>
      <button className="nav-btn" title="Reload (F5)" onClick={() => send({ type: "reload", tab: null, bypassCache: false })}>
        <Icon name="reload" size={17} />
      </button>
      <AddressBar />
      <button className="tool-btn" title="Snapshot (Ctrl+Shift+5)" onClick={() => send({ type: "page", action: "screenshot", tab: null, data: null })}>
        <Icon name="crop" size={17} />
      </button>
      <span className="toolbar-divider" />
      <button className={clsx("tool-btn", popup === "extensions" && "active")} title="Extensions" onClick={(e) => openPopup("extensions", anchor(e))}>
        <Icon name="puzzle" size={17} />
      </button>
      {settings?.tabs.showAccountButton && (
        <button className={clsx("tool-btn", popup === "profile" && "active")} title="Profile" onClick={(e) => openPopup("profile", anchor(e))}>
          <Icon name="user" size={17} />
        </button>
      )}
      <button className={clsx("tool-btn", "downloads-btn", popup === "downloads" && "active")} title="Downloads (Ctrl+J)" onClick={(e) => openPopup("downloads", anchor(e))}>
        <Icon name="download" size={17} />
        {running.length > 0 && (
          <svg className="download-ring" viewBox="0 0 36 36">
            <circle cx="18" cy="18" r="15" stroke={track} strokeWidth="3" fill="none" />
            <circle
              cx="18"
              cy="18"
              r="15"
              stroke={accent}
              strokeWidth="3"
              fill="none"
              strokeDasharray={`${Math.max(0.03, progress) * 94.2} 94.2`}
              transform="rotate(-90 18 18)"
              strokeLinecap="round"
            />
          </svg>
        )}
      </button>
      {settings?.ai.enabled && settings.ai.toolbarButton && (
        <button className="tool-btn ai-btn" title="AI" onClick={() => togglePanel("ai")}>
          <Icon name="sparkles" size={16} />
          <span>AI</span>
        </button>
      )}
      {settings?.battery.showToolbarIcon && (
        <button
          className={clsx("tool-btn", settings.battery.saver && "active")}
          title={settings.battery.saver ? "Battery saver is on" : "Battery saver is off"}
          onClick={() => send({ type: "patch-settings", patch: { battery: { saver: !settings.battery.saver } } })}
        >
          <Icon name={settings.battery.saver ? "battery-charging" : "battery"} size={17} />
        </button>
      )}
      <button className={clsx("tool-btn", drawer === "easy-setup" && "active")} title="Easy Setup" onClick={() => setDrawer(drawer === "easy-setup" ? null : "easy-setup")}>
        <Icon name="sliders" size={17} />
      </button>
    </nav>
  );
}

function AddressBar() {
  const session = useStore((s) => s.session);
  const settings = useStore((s) => s.settings);
  const bookmarks = useStore((s) => s.bookmarks);
  const suggestions = useStore((s) => s.suggestions);
  const addressFocus = useStore((s) => s.addressFocus);
  const setAddressOpen = useStore((s) => s.setAddressOpen);
  const openPopup = useStore((s) => s.openPopup);
  const showBlocked = useStore((s) => s.settings?.privacy.showBlockedStats);
  const activeId = session?.activeTabs[session.activeWorkspace];
  const tab = session?.tabs.find((t) => t.id === activeId);
  const meta = useStore((s) => (activeId ? s.meta[activeId] : undefined));
  const url = meta?.url ?? tab?.url ?? "";
  const [text, setText] = useState("");
  const [editing, setEditing] = useState(false);
  const [selected, setSelected] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const wasFocused = useRef(false);

  useEffect(() => {
    if (addressFocus > 0) {
      input.current?.focus();
      input.current?.select();
    }
  }, [addressFocus]);

  useEffect(() => {
    if (!editing) setText(displayUrl(url, settings?.tabs.showFullUrl ?? false));
  }, [url, editing, settings?.tabs.showFullUrl]);

  const items: Suggestion[] = useMemo(() => {
    if (!editing || !text.trim() || suggestions.text !== text.trim()) return [];
    const remote = suggestions.remote.filter((r) => !suggestions.items.some((i) => i.title === r.title));
    return [...suggestions.items, ...remote].slice(0, 12);
  }, [editing, text, suggestions]);

  const open = editing && items.length > 0;
  useEffect(() => setAddressOpen(open), [open, setAddressOpen]);

  const bookmarked = bookmarks.nodes.some((n) => n.url === url && n.parent !== "trash");
  const security = meta?.security ?? "none";
  const zoom = meta?.zoom ?? tab?.zoom ?? 1;

  const commit = (value: string, s?: Suggestion) => {
    setEditing(false);
    input.current?.blur();
    if (s?.kind === "open-tab" && s.tab) {
      send({ type: "activate-tab", tab: s.tab });
      return;
    }
    if (s?.kind === "calculator" || s?.kind === "conversion") {
      useStore.getState().toast(s.title.replace(/^= /, ""), "info");
      return;
    }
    const target = s ? s.url : value;
    if (target.trim()) send({ type: "navigate", tab: null, input: target, disposition: "current-tab" });
  };

  return (
    <div className={clsx("address", editing && "editing", open && "open")}>
      <button
        className={clsx("address-security", security)}
        title={securityLabel(security)}
        onClick={(e) => openPopup("site-info", (e.currentTarget as HTMLElement).getBoundingClientRect())}
      >
        <Icon name={security === "secure" ? "lock" : security === "insecure" ? "lock-open" : security === "internal" ? "shield-check" : editing ? "search" : "globe"} size={14} />
      </button>
      <Input
        ref={input}
        value={text}
        placeholder="Enter search or web address"
        spellCheck={false}
        onMouseDown={(e) => {
          wasFocused.current = document.activeElement === e.currentTarget;
        }}
        onMouseUp={(e) => {
          // The first click selects the whole address, like every browser. Done here because
          // Servo delivers the focus event after keys typed right after the click.
          if (!wasFocused.current) e.currentTarget.select();
        }}
        onFocus={(e) => {
          setEditing(true);
          const el = e.currentTarget;
          // Swap the short display form for the full URL, unless the user already typed.
          if (el.value === displayUrl(url, settings?.tabs.showFullUrl ?? false)) {
            const full = isInternal(url) && url.startsWith("haku://start") ? "" : url;
            setText(full);
            el.value = full;
            el.select();
          } else {
            setText(el.value);
          }
        }}
        onBlur={() => window.setTimeout(() => setEditing(false), 120)}
        onChange={(e) => {
          setText(e.target.value);
          setSelected(0);
          const value = e.target.value.trim();
          if (value) send({ type: "suggest", request: requestId++, text: value });
        }}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" && open) {
            e.preventDefault();
            setSelected((i) => Math.min(items.length - 1, i + 1));
          } else if (e.key === "ArrowUp" && open) {
            e.preventDefault();
            setSelected((i) => Math.max(0, i - 1));
          } else if (e.key === "Enter") {
            e.preventDefault();
            const disposition = e.altKey || e.ctrlKey ? "new-tab" : "current-tab";
            if (disposition === "new-tab") {
              send({ type: "navigate", tab: null, input: text, disposition });
              setEditing(false);
              input.current?.blur();
            } else commit(text, open && selected > 0 ? items[selected] : undefined);
          } else if (e.key === "Escape") {
            setEditing(false);
            setText(displayUrl(url, false));
            input.current?.blur();
          }
        }}
      />
      {Math.abs(zoom - 1) > 0.001 && (
        <button className="zoom-pill" title="Reset zoom (Ctrl+0)" onClick={() => send({ type: "zoom-step", delta: 0 })}>
          {Math.round(zoom * 100)}%
        </button>
      )}
      {showBlocked && (meta?.blocked ?? 0) > 0 && (
        <button className="blocked-pill" title="Ads and trackers blocked on this page" onClick={(e) => openPopup("blocked", (e.currentTarget as HTMLElement).getBoundingClientRect())}>
          <Icon name="shield" size={13} />
          {meta?.blocked}
        </button>
      )}
      <button className={clsx("address-star", bookmarked && "on")} title={bookmarked ? "Remove bookmark (Ctrl+D)" : "Bookmark this page (Ctrl+D)"} onClick={() => send({ type: "toggle-bookmark" })}>
        <Icon name="star" size={15} />
      </button>
      {open && (
        <div className="suggestions">
          {items.map((s, i) => (
            <button
              key={`${s.kind}-${s.url}-${s.title}-${i}`}
              className={clsx("suggestion", i === selected && "selected")}
              onMouseDown={(e) => {
                e.preventDefault();
                commit(text, s);
              }}
              onMouseEnter={() => setSelected(i)}
            >
              <Icon name={suggestionIcon(s.kind)} size={15} />
              <span className="suggestion-title ellipsis">{s.title}</span>
              {s.detail && <span className="suggestion-detail ellipsis">{s.detail}</span>}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function displayUrl(url: string, full: boolean): string {
  if (!url || url.startsWith("haku://start") || url === "about:blank") return "";
  if (full) return url;
  try {
    const u = new URL(url);
    if (u.protocol === "https:" || u.protocol === "http:") {
      const rest = u.pathname === "/" && !u.search && !u.hash ? "" : u.pathname + u.search + u.hash;
      return u.host + rest;
    }
  } catch {
    // Not a URL: show as is.
  }
  return url;
}

function securityLabel(s: string): string {
  switch (s) {
    case "secure":
      return "Connection is secure";
    case "insecure":
      return "Connection is not secure";
    case "internal":
      return "HakuCX page";
    case "local":
      return "Local file";
    default:
      return "Site information";
  }
}

function suggestionIcon(kind: string): string {
  switch (kind) {
    case "search":
      return "search";
    case "history":
      return "clock";
    case "bookmark":
      return "star";
    case "open-tab":
      return "panels-top-left";
    case "calculator":
      return "hash";
    case "conversion":
      return "repeat";
    default:
      return "globe";
  }
}

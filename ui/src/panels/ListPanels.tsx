/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useEffect, useMemo, useState } from "react";
import { Icon } from "../components/Icon";
import { send } from "../ipc/bridge";
import type { DownloadRecord } from "../ipc/generated/DownloadRecord";
import type { Visit } from "../ipc/generated/Visit";
import { clsx, formatBytes, formatRate, hostOf, timeOfDay } from "../lib/util";
import { openInternal, NONE, useStore } from "../state/store";
import { Input } from "../components/Field";

let historyRequest = 1000;

/** Query history through the core and keep the latest result for this component. */
export function useHistory(text: string, from: number | null, limit = 200): Visit[] | null {
  const [id, setId] = useState<number | null>(null);
  const results = useStore((s) => (id !== null ? s.historyResults[id] : undefined));
  useEffect(() => {
    const request = historyRequest++;
    setId(request);
    const timer = window.setTimeout(() => send({ type: "query-history", request, text, from, to: null, limit, offset: 0 }), text ? 150 : 0);
    return () => window.clearTimeout(timer);
  }, [text, from, limit]);
  return results ?? null;
}

export function Favicon({ url, size = 16 }: { url: string; size?: number }) {
  const meta = useStore((s) => Object.values(s.meta).find((m) => m.url === url && m.favicon));
  return meta?.favicon ? <img src={meta.favicon} width={size} height={size} alt="" /> : <Icon name="globe" size={size - 2} />;
}

export function HistoryPanel() {
  const [text, setText] = useState("");
  const closed = useStore((s) => s.session?.closed ?? NONE);
  const startOfDay = useMemo(() => {
    const d = new Date();
    return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  }, []);
  const visits = useHistory(text, text ? null : startOfDay);
  return (
    <div className="panel-scroll">
      <div className="search-box">
        <Icon name="search" size={15} />
        <Input placeholder="Search history" value={text} onChange={(e) => setText(e.target.value)} />
      </div>
      {!text && closed.length > 0 && (
        <>
          <h3 className="display section-label">Recently closed</h3>
          {closed.slice(0, 6).map((c, i) => (
            <button key={`${c.tab.id}-${i}`} className="list-row" onClick={() => i === 0 && send({ type: "reopen-closed" })} title={i === 0 ? "Reopen" : c.tab.url}>
              <span className="list-time">{timeOfDay(c.closedAt)}</span>
              <Favicon url={c.tab.url} />
              <span className="ellipsis grow">{c.tab.title || hostOf(c.tab.url)}</span>
              <span className="list-host dim">{hostOf(c.tab.url)}</span>
            </button>
          ))}
        </>
      )}
      <h3 className="display section-label">{text ? "Results" : "Today"}</h3>
      {visits === null && <div className="panel-empty">Loading...</div>}
      {visits?.length === 0 && <div className="panel-empty">{text ? "No matches" : "Nothing visited today yet"}</div>}
      {visits?.map((v) => (
        <button key={v.id} className="list-row" onClick={() => send({ type: "navigate", tab: null, input: v.url, disposition: "current-tab" })}>
          <span className="list-time">{timeOfDay(v.visitedAt)}</span>
          <Favicon url={v.url} />
          <span className="ellipsis grow">{v.title || v.url}</span>
          <span className="list-host dim">{hostOf(v.url)}</span>
        </button>
      ))}
      <button className="btn wide" onClick={() => openInternal("haku://history/")}>
        Open full History view
      </button>
    </div>
  );
}

export function DownloadRow({ d, compact }: { d: DownloadRecord; compact?: boolean }) {
  const speed = useStore((s) => s.downloadSpeed[d.id]);
  const state = d.state.state;
  const pct = d.totalBytes ? Math.min(100, (d.receivedBytes / d.totalBytes) * 100) : null;
  const status =
    state === "in-progress"
      ? `${formatBytes(d.receivedBytes)}${d.totalBytes ? ` of ${formatBytes(d.totalBytes)}` : ""}${speed ? ` · ${formatRate(speed)}` : ""}`
      : state === "completed"
        ? `Download complete · ${formatBytes(d.totalBytes ?? d.receivedBytes)}`
        : state === "paused"
          ? `Paused · ${formatBytes(d.receivedBytes)}`
          : state === "cancelled"
            ? "Canceled"
            : `Failed: ${d.state.state === "failed" ? d.state.reason : ""}`;
  const act = (action: "pause" | "resume" | "cancel" | "retry" | "open" | "show-in-folder" | "remove" | "delete-file") => send({ type: "download", id: d.id, action });
  return (
    <div className={clsx("download-row", state, compact && "compact")}>
      <Icon name={fileIcon(d.fileName, d.mime)} size={20} />
      <div className="grow download-text">
        <button className="ellipsis download-name" disabled={state !== "completed"} onClick={() => act("open")} title={d.path}>
          {d.fileName || "Starting..."}
        </button>
        <small className={clsx(state === "failed" || state === "cancelled" ? "danger" : "muted")}>{status}</small>
        {state === "in-progress" && (
          <div className="progress">
            <div className={clsx("progress-fill", pct === null && "indeterminate")} style={pct !== null ? { width: `${pct}%` } : undefined} />
          </div>
        )}
      </div>
      {state === "in-progress" && (
        <>
          <button className="icon-btn" title="Pause" onClick={() => act("pause")}>
            <Icon name="pause" size={14} />
          </button>
          <button className="icon-btn" title="Cancel" onClick={() => act("cancel")}>
            <Icon name="x" size={14} />
          </button>
        </>
      )}
      {state === "paused" && (
        <button className="icon-btn" title="Resume" onClick={() => act("resume")}>
          <Icon name="play" size={14} />
        </button>
      )}
      {(state === "failed" || state === "cancelled") && (
        <button className="icon-btn" title="Retry" onClick={() => act("retry")}>
          <Icon name="refresh" size={14} />
        </button>
      )}
      {state === "completed" && (
        <button className="icon-btn" title="Show in folder" onClick={() => act("show-in-folder")}>
          <Icon name="folder-open" size={14} />
        </button>
      )}
    </div>
  );
}

export function fileIcon(name: string, mime: string): string {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  if (/^(png|jpe?g|gif|webp|avif|bmp|svg|ico)$/.test(ext) || mime.startsWith("image/")) return "image";
  if (/^(mp4|mkv|webm|mov|avi)$/.test(ext) || mime.startsWith("video/")) return "film";
  if (/^(mp3|wav|flac|ogg|opus|m4a)$/.test(ext) || mime.startsWith("audio/")) return "music";
  if (/^(zip|7z|rar|tar|gz|xz)$/.test(ext)) return "archive";
  if (/^(exe|msi|msix)$/.test(ext)) return "window";
  if (/^(pdf|docx?|txt|md|xlsx?|pptx?)$/.test(ext)) return "file-text";
  return "file";
}

export function DownloadsPanel() {
  const downloads = useStore((s) => s.downloads);
  return (
    <div className="panel-scroll">
      {downloads.length === 0 && <div className="panel-empty">No downloads yet</div>}
      {downloads.slice(0, 50).map((d) => (
        <DownloadRow key={d.id} d={d} />
      ))}
      <div className="row">
        <button className="btn" onClick={() => send({ type: "clear-downloads" })}>
          Clear
        </button>
        <span className="grow" />
        <button className="btn" onClick={() => openInternal("haku://downloads/")}>
          Show all
        </button>
      </div>
    </div>
  );
}

export function BookmarksPanel() {
  const nodes = useStore((s) => s.bookmarks.nodes);
  const [open, setOpen] = useState<Record<string, boolean>>({ bar: true });
  const [text, setText] = useState("");
  const render = (parent: string, depth: number): React.ReactNode =>
    nodes
      .filter((n) => n.parent === parent)
      .map((n) => (
        <div key={n.id}>
          <button
            className="list-row"
            style={{ paddingLeft: 8 + depth * 14 }}
            onClick={() => (n.url ? send({ type: "navigate", tab: null, input: n.url, disposition: "current-tab" }) : setOpen((o) => ({ ...o, [n.id]: !o[n.id] })))}
          >
            <Icon name={n.url ? "globe" : open[n.id] ? "folder-open" : "folder"} size={14} />
            <span className="ellipsis grow">{n.title || n.url}</span>
          </button>
          {!n.url && open[n.id] && render(n.id, depth + 1)}
        </div>
      ));
  const q = text.trim().toLowerCase();
  return (
    <div className="panel-scroll">
      <div className="search-box">
        <Icon name="search" size={15} />
        <Input placeholder="Search bookmarks" value={text} onChange={(e) => setText(e.target.value)} />
      </div>
      {q
        ? nodes
            .filter((n) => n.url && n.parent !== "trash" && (n.title.toLowerCase().includes(q) || n.url.toLowerCase().includes(q)))
            .map((n) => (
              <button key={n.id} className="list-row" onClick={() => send({ type: "navigate", tab: null, input: n.url!, disposition: "current-tab" })}>
                <Icon name="globe" size={14} />
                <span className="ellipsis grow">{n.title || n.url}</span>
              </button>
            ))
        : ["bar", "speed-dial", "other"].map((root) => (
            <div key={root}>
              <button className="list-row root" onClick={() => setOpen((o) => ({ ...o, [root]: !o[root] }))}>
                <Icon name={open[root] ? "folder-open" : "folder"} size={14} />
                <span className="display">{root === "bar" ? "Bookmarks bar" : root === "speed-dial" ? "Speed Dial" : "Other bookmarks"}</span>
              </button>
              {open[root] && render(root, 1)}
            </div>
          ))}
      <button className="btn wide" onClick={() => openInternal("haku://bookmarks/")}>
        Manage bookmarks
      </button>
    </div>
  );
}

export function TabsPanel() {
  const session = useStore((s) => s.session);
  if (!session) return null;
  return (
    <div className="panel-scroll">
      {session.workspaces.map((w) => {
        const tabs = session.tabs.filter((t) => t.workspaceId === w.id);
        return (
          <div key={w.id}>
            <h3 className="display section-label">
              <Icon name={w.icon} size={13} /> {w.name} <span className="dim">{tabs.length}</span>
            </h3>
            {tabs.map((t) => (
              <div key={t.id} className={clsx("list-row", session.activeTabs[w.id] === t.id && "current")}>
                <button className="row grow" onClick={() => send({ type: "activate-tab", tab: t.id })}>
                  <Favicon url={t.url} />
                  <span className="ellipsis grow">{t.title || hostOf(t.url)}</span>
                </button>
                <button className="icon-btn" title="Close" onClick={() => send({ type: "close-tab", tab: t.id })}>
                  <Icon name="x" size={13} />
                </button>
              </div>
            ))}
          </div>
        );
      })}
    </div>
  );
}

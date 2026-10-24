/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useState } from "react";
import { Button, Toggle } from "../components/controls";
import { Icon } from "../components/Icon";
import { send } from "../ipc/bridge";
import type { BookmarkNode } from "../ipc/generated/BookmarkNode";
import { clsx, colorFor, hostOf, letterOf, patch } from "../lib/util";
import { useStore } from "../state/store";
import { Input } from "../components/Field";

const ROOTS = [
  { id: "bar", label: "Bookmarks bar" },
  { id: "speed-dial", label: "Speed Dial" },
  { id: "other", label: "Other bookmarks" },
  { id: "trash", label: "Trash" },
];

export function BookmarksPage() {
  const nodes = useStore((s) => s.bookmarks.nodes);
  const showBar = useStore((s) => s.settings?.appearance.showBookmarksBar ?? false);
  const openModal = useStore((s) => s.openModal);
  const openMenu = useStore((s) => s.openMenu);
  const [folder, setFolder] = useState("bar");
  const [view, setView] = useState<"grid" | "small" | "list">("grid");
  const [query, setQuery] = useState("");
  const q = query.trim().toLowerCase();
  const inTrash = folder === "trash";
  const items: BookmarkNode[] = q
    ? nodes.filter((n) => n.parent !== "trash" && n.parent !== null && (n.title.toLowerCase().includes(q) || (n.url ?? "").toLowerCase().includes(q)))
    : nodes.filter((n) => n.parent === folder);
  const subfolders = (parent: string): BookmarkNode[] => nodes.filter((n) => n.parent === parent && !n.url);
  const crumbs: BookmarkNode[] = [];
  let cur = nodes.find((n) => n.id === folder);
  while (cur) {
    crumbs.unshift(cur);
    cur = cur.parent ? nodes.find((n) => n.id === cur!.parent) : undefined;
  }

  const tree = (parent: string, depth: number): React.ReactNode =>
    subfolders(parent).map((f) => (
      <div key={f.id}>
        <button className={clsx("hub-side-link", folder === f.id && "active")} style={{ paddingLeft: 12 + depth * 12 }} onClick={() => setFolder(f.id)}>
          <Icon name="folder" size={13} /> <span className="ellipsis">{f.title}</span>
        </button>
        {tree(f.id, depth + 1)}
      </div>
    ));

  return (
    <div className="hub-two-col">
      <aside className="hub-side card">
        {ROOTS.map((r) => (
          <div key={r.id}>
            <button className={clsx("hub-side-link display", folder === r.id && "active")} onClick={() => setFolder(r.id)}>
              <Icon name={r.id === "trash" ? "trash" : "chevron-right"} size={12} /> {r.label}
            </button>
            {r.id !== "trash" && tree(r.id, 1)}
          </div>
        ))}
        <span className="grow" />
        <Button onClick={() => send({ type: "import-bookmarks" })}>Import bookmarks</Button>
        <Button onClick={() => send({ type: "export-bookmarks" })}>Export bookmarks</Button>
      </aside>
      <section className="hub-main">
        <div className="row bookmarks-toolbar">
          <div className="crumbs">
            {crumbs.map((c, i) => (
              <span key={c.id}>
                {i > 0 && <Icon name="chevron-right" size={12} />}
                <button className="link-btn" onClick={() => setFolder(c.id)}>
                  {c.title}
                </button>
              </span>
            ))}
          </div>
          <span className="grow" />
          <div className="search-box small">
            <Icon name="search" size={14} />
            <Input placeholder="Search bookmarks" value={query} onChange={(e) => setQuery(e.target.value)} />
          </div>
          {(["grid", "small", "list"] as const).map((v) => (
            <button key={v} className={clsx("icon-btn", view === v && "active")} title={`${v} view`} onClick={() => setView(v)}>
              <Icon name={v === "list" ? "list" : v === "small" ? "grid" : "layout-grid"} size={15} />
            </button>
          ))}
          {!inTrash && (
            <>
              <button className="icon-btn" title="New folder" onClick={() => send({ type: "add-bookmark", parent: folder, title: "New folder", url: null, index: null })}>
                <Icon name="folder" size={15} />
              </button>
              <button className="icon-btn" title="Add bookmark" onClick={() => openModal({ kind: "dial-edit", id: null, parent: folder })}>
                <Icon name="plus" size={15} />
              </button>
            </>
          )}
          {inTrash && <Button onClick={() => send({ type: "empty-trash" })}>Empty trash</Button>}
          <span className="row small">
            Show the bookmarks bar <Toggle on={showBar} onChange={(v) => patch({ appearance: { showBookmarksBar: v } })} />
          </span>
        </div>
        {items.length === 0 && <p className="muted">{q ? "No bookmarks match." : inTrash ? "Trash is empty." : "This folder is empty."}</p>}
        <div className={clsx("bookmark-items", view)}>
          {items.map((n) => (
            <div
              key={n.id}
              className="bookmark-card notch"
              onContextMenu={(e) => {
                e.preventDefault();
                openMenu({ kind: "bookmark", x: e.clientX, y: e.clientY, id: n.id });
              }}
            >
              <button
                className="bookmark-main"
                onClick={() => (n.url ? send({ type: "navigate", tab: null, input: n.url, disposition: "current-tab" }) : setFolder(n.id))}
              >
                {view !== "list" && (
                  <span className="bookmark-banner" style={{ background: n.url ? n.color ?? colorFor(n.url) : "var(--hx-elev-2)" }}>
                    {n.url ? letterOf(n.title, n.url) : <Icon name="folder" size={22} />}
                  </span>
                )}
                <span className="bookmark-text">
                  <span className="ellipsis">{n.title || n.url}</span>
                  {n.url && <small className="dim ellipsis">{hostOf(n.url)}</small>}
                </span>
              </button>
              <div className="bookmark-actions">
                {inTrash ? (
                  <>
                    <button className="icon-btn" title="Restore" onClick={() => send({ type: "restore-bookmark", id: n.id })}>
                      <Icon name="undo" size={13} />
                    </button>
                    <button className="icon-btn" title="Delete forever" onClick={() => send({ type: "delete-bookmark", id: n.id })}>
                      <Icon name="trash" size={13} />
                    </button>
                  </>
                ) : (
                  <>
                    <button className="icon-btn" title="Edit" onClick={() => openModal({ kind: "bookmark-edit", id: n.id })}>
                      <Icon name="pencil" size={13} />
                    </button>
                    <button className="icon-btn" title="Delete" onClick={() => send({ type: "delete-bookmark", id: n.id })}>
                      <Icon name="trash" size={13} />
                    </button>
                  </>
                )}
              </div>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}

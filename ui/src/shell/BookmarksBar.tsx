/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useState } from "react";
import { Icon } from "../components/Icon";
import { send } from "../ipc/bridge";
import type { BookmarkNode } from "../ipc/generated/BookmarkNode";
import { hostOf } from "../lib/util";
import { useStore } from "../state/store";

export function BookmarksBar() {
  const nodes = useStore((s) => s.bookmarks.nodes);
  const openMenu = useStore((s) => s.openMenu);
  const [folder, setFolder] = useState<{ id: string; x: number; y: number } | null>(null);
  const items = nodes.filter((n) => n.parent === "bar");

  const open = (n: BookmarkNode, e: React.MouseEvent) => {
    if (!n.url) return;
    const background = e.ctrlKey || e.button === 1;
    if (background) send({ type: "navigate", tab: null, input: n.url, disposition: "background-tab" });
    else send({ type: "navigate", tab: null, input: n.url, disposition: "current-tab" });
  };

  return (
    <div className="bookmarks-bar">
      {items.length === 0 && <span className="bookmarks-empty">Bookmark pages with the star (Ctrl+D) to see them here.</span>}
      {items.map((n) => (
        <button
          key={n.id}
          className="bookmark-item"
          title={n.url ?? n.title}
          onMouseUp={(e) => {
            if (n.url) open(n, e);
            else setFolder(folder?.id === n.id ? null : { id: n.id, x: (e.currentTarget as HTMLElement).getBoundingClientRect().left, y: 0 });
          }}
          onContextMenu={(e) => {
            e.preventDefault();
            openMenu({ kind: "bookmark", x: e.clientX, y: e.clientY, id: n.id });
          }}
        >
          <Icon name={n.url ? "globe" : "folder"} size={13} />
          <span className="ellipsis">{n.title || (n.url ? hostOf(n.url) : "Folder")}</span>
        </button>
      ))}
      {folder && (
        <div className="bookmark-folder-menu" style={{ left: folder.x }} onMouseLeave={() => setFolder(null)}>
          {nodes
            .filter((n) => n.parent === folder.id)
            .map((n) => (
              <button
                key={n.id}
                className="menu-item"
                onClick={(e) => {
                  open(n, e);
                  setFolder(null);
                }}
              >
                <Icon name={n.url ? "globe" : "folder"} size={13} />
                <span className="ellipsis">{n.title || n.url}</span>
              </button>
            ))}
          {nodes.filter((n) => n.parent === folder.id).length === 0 && <div className="menu-empty">Empty folder</div>}
        </div>
      )}
    </div>
  );
}

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useMemo, useState } from "react";
import { Button } from "../components/controls";
import { Icon } from "../components/Icon";
import { send } from "../ipc/bridge";
import type { DownloadRecord } from "../ipc/generated/DownloadRecord";
import { clsx, dayLabel, formatBytes } from "../lib/util";
import { DownloadRow, fileIcon } from "../panels/ListPanels";
import { useStore } from "../state/store";
import { Input } from "../components/Field";

type Filter = "all" | "documents" | "images" | "videos" | "music" | "others";

function category(d: DownloadRecord): Filter {
  const icon = fileIcon(d.fileName, d.mime);
  if (icon === "image") return "images";
  if (icon === "film") return "videos";
  if (icon === "music") return "music";
  if (icon === "file-text") return "documents";
  return "others";
}

export function DownloadsPage() {
  const downloads = useStore((s) => s.downloads);
  const settings = useStore((s) => s.settings);
  const [filter, setFilter] = useState<Filter>("all");
  const [range, setRange] = useState<"all" | "today" | "yesterday">("all");
  const [query, setQuery] = useState("");
  const day = useMemo(() => {
    const d = new Date();
    return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  }, []);
  const list = downloads.filter((d) => {
    if (filter !== "all" && category(d) !== filter) return false;
    if (range === "today" && d.startedAt < day) return false;
    if (range === "yesterday" && (d.startedAt >= day || d.startedAt < day - 86_400_000)) return false;
    if (query && !d.fileName.toLowerCase().includes(query.toLowerCase()) && !d.url.toLowerCase().includes(query.toLowerCase())) return false;
    return true;
  });
  const groups = new Map<string, DownloadRecord[]>();
  for (const d of list) groups.set(dayLabel(d.startedAt), [...(groups.get(dayLabel(d.startedAt)) ?? []), d]);

  return (
    <div className="hub-two-col">
      <aside className="hub-side card">
        {(["all", "today", "yesterday"] as const).map((r) => (
          <button key={r} className={clsx("hub-side-link display", range === r && "active")} onClick={() => setRange(r)}>
            <Icon name="chevron-right" size={12} /> {r === "all" ? "All downloads" : r}
          </button>
        ))}
        <span className="grow" />
        <div className="muted small">Saving to {settings?.downloads.directory || "your Downloads folder"}</div>
        <Button onClick={() => send({ type: "clear-downloads" })}>Clear downloads</Button>
      </aside>
      <section className="hub-main">
        <div className="row">
          <h2 className="display">Downloads</h2>
          <span className="grow" />
          <div className="search-box small">
            <Icon name="search" size={14} />
            <Input placeholder="Search downloads" value={query} onChange={(e) => setQuery(e.target.value)} />
          </div>
        </div>
        <div className="chip-row">
          {(["all", "documents", "images", "videos", "music", "others"] as Filter[]).map((f) => (
            <button key={f} className={clsx("chip display", filter === f && "accent")} onClick={() => setFilter(f)}>
              {f}
            </button>
          ))}
        </div>
        {list.length === 0 && <p className="muted">No downloads here.</p>}
        {[...groups.entries()].map(([label, items]) => (
          <div key={label}>
            <h3 className="display section-label">{label}</h3>
            <div className="download-cards">
              {items.map((d) => (
                <div key={d.id} className="download-card notch">
                  <DownloadRow d={d} />
                  <div className="row download-card-foot">
                    <small className="dim ellipsis grow">{d.state.state === "completed" ? `Downloaded to: ${d.path}` : d.url}</small>
                    {d.state.state === "completed" && (
                      <>
                        <Button onClick={() => send({ type: "download", id: d.id, action: "open" })}>Open</Button>
                        <button className="icon-btn" title="Delete file" onClick={() => send({ type: "download", id: d.id, action: "delete-file" })}>
                          <Icon name="trash" size={13} />
                        </button>
                      </>
                    )}
                    <button className="icon-btn" title="Remove from list" onClick={() => send({ type: "download", id: d.id, action: "remove" })}>
                      <Icon name="x" size={13} />
                    </button>
                  </div>
                </div>
              ))}
            </div>
            <small className="dim">{formatBytes(items.reduce((a, d) => a + (d.totalBytes ?? d.receivedBytes), 0))} total</small>
          </div>
        ))}
      </section>
    </div>
  );
}

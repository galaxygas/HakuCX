/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useMemo, useState } from "react";
import { Button } from "../components/controls";
import { Icon } from "../components/Icon";
import { send } from "../ipc/bridge";
import { clsx, dayLabel, hostOf, timeOfDay } from "../lib/util";
import { Favicon, useHistory } from "../panels/ListPanels";
import { useStore } from "../state/store";
import { Input } from "../components/Field";

type Range = "today" | "yesterday" | "older" | "all";

export function HistoryPage() {
  const [text, setText] = useState("");
  const [range, setRange] = useState<Range>("all");
  const [limit, setLimit] = useState(300);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const openModal = useStore((s) => s.openModal);
  const redeem = useStore((s) => !!s.settings?.earlyBird.enabled && !!s.settings?.earlyBird.features["fake-my-history"]);
  const day = useMemo(() => {
    const d = new Date();
    return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  }, []);
  const from = range === "today" ? day : range === "yesterday" ? day - 86_400_000 : null;
  const visits = useHistory(text, from, limit);
  const filtered = (visits ?? []).filter((v) => {
    if (range === "yesterday") return v.visitedAt < day;
    if (range === "older") return v.visitedAt < day - 86_400_000;
    return true;
  });
  const groups = useMemo(() => {
    const map = new Map<string, typeof filtered>();
    for (const v of filtered) {
      const key = dayLabel(v.visitedAt);
      map.set(key, [...(map.get(key) ?? []), v]);
    }
    return [...map.entries()];
  }, [filtered]);

  const toggle = (id: number) =>
    setSelected((s) => {
      const n = new Set(s);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });

  return (
    <div className="hub-two-col">
      <aside className="hub-side card">
        <div className="search-box">
          <Icon name="search" size={15} />
          <Input placeholder="Search history" value={text} onChange={(e) => setText(e.target.value)} />
        </div>
        {(["all", "today", "yesterday", "older"] as Range[]).map((r) => (
          <button key={r} className={clsx("hub-side-link display", range === r && "active")} onClick={() => setRange(r)}>
            <Icon name="chevron-right" size={12} /> {r === "all" ? "Everything" : r}
          </button>
        ))}
        <span className="grow" />
        <Button kind="accent" onClick={() => send({ type: "delete-history-range", from: day, to: Date.now() + 1 })}>
          Forget today
        </Button>
        <Button onClick={() => openModal({ kind: "clear-data" })}>Delete browsing data...</Button>
        {redeem && (
          <Button
            onClick={() => {
              send({ type: "redeem-history" });
              setLimit((l) => l + 1);
            }}
          >
            Redeem my soul
          </Button>
        )}
      </aside>
      <section className="hub-main">
        <div className="row">
          <h2 className="display">History</h2>
          <span className="grow" />
          {selected.size > 0 && (
            <Button
              kind="danger"
              icon="trash"
              onClick={() => {
                send({ type: "delete-history-visits", ids: [...selected] });
                setSelected(new Set());
                setText((t) => t + "");
                setLimit((l) => l + 1);
              }}
            >
              Delete {selected.size} selected
            </Button>
          )}
        </div>
        {visits === null && <p className="muted">Loading...</p>}
        {visits !== null && filtered.length === 0 && <p className="muted">{text ? "Nothing matches your search." : "No history in this range."}</p>}
        {groups.map(([label, list]) => (
          <div key={label} className="history-day card">
            <h3 className="display section-label">{label}</h3>
            {list.map((v) => (
              <div key={v.id} className={clsx("history-row", selected.has(v.id) && "selected")}>
                <input type="checkbox" checked={selected.has(v.id)} onChange={() => toggle(v.id)} aria-label="Select" />
                <span className="list-time">{timeOfDay(v.visitedAt)}</span>
                <Favicon url={v.url} />
                <button className="history-title ellipsis" onClick={() => send({ type: "navigate", tab: null, input: v.url, disposition: "current-tab" })}>
                  {v.title || v.url}
                </button>
                <span className="dim ellipsis history-host">{hostOf(v.url)}</span>
                <button className="icon-btn" title="Remove from history" onClick={() => { send({ type: "delete-history-visits", ids: [v.id] }); setLimit((l) => l + 1); }}>
                  <Icon name="x" size={13} />
                </button>
              </div>
            ))}
          </div>
        ))}
        {visits && visits.length >= limit && (
          <Button onClick={() => setLimit((l) => l + 300)}>Show more</Button>
        )}
      </section>
    </div>
  );
}

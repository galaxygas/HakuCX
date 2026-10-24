/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useEffect, useMemo, useRef, useState } from "react";
import { Icon } from "../components/Icon";
import { Wallpaper } from "../components/Wallpaper";
import { onEvent, send } from "../ipc/bridge";
import type { BookmarkNode } from "../ipc/generated/BookmarkNode";
import type { UrlStat } from "../ipc/generated/UrlStat";
import { clsx, colorFor, hostOf, letterOf } from "../lib/util";
import { useFeeds } from "../panels/MiscPanels";
import { NONE, useStore } from "../state/store";
import { useFreeGames } from "./CornerPage";
import { Input } from "../components/Field";

const WEATHER_CODES: Record<number, [string, string]> = {
  0: ["Clear", "sun"],
  1: ["Mostly clear", "sun"],
  2: ["Partly cloudy", "cloud"],
  3: ["Overcast", "cloud"],
  45: ["Fog", "cloud"],
  48: ["Fog", "cloud"],
  51: ["Drizzle", "cloud"],
  61: ["Rain", "cloud"],
  63: ["Rain", "cloud"],
  65: ["Heavy rain", "cloud"],
  71: ["Snow", "cloud"],
  80: ["Showers", "cloud"],
  95: ["Thunderstorm", "zap"],
};

let req = 70_000;
function httpGet(url: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const request = req++;
    const off = onEvent((e) => {
      if (e.type === "http-result" && e.request === request) {
        off();
        if (e.error || e.status >= 400) reject(new Error(e.error ?? `HTTP ${e.status}`));
        else resolve(e.body);
      }
    });
    send({ type: "http-get", request, url });
  });
}
export { httpGet };

function Weather() {
  const widgets = useStore((s) => s.settings?.startPage.widgets);
  const [data, setData] = useState<{ temp: number; code: number; city: string } | null>(null);
  const [error, setError] = useState(false);
  useEffect(() => {
    if (!widgets?.weather) return;
    let cancelled = false;
    (async () => {
      try {
        let lat = widgets.weatherLat;
        let lon = widgets.weatherLon;
        let city = widgets.weatherCity;
        if ((lat == null || lon == null) && city) {
          const geo = JSON.parse(await httpGet(`https://geocoding-api.open-meteo.com/v1/search?name=${encodeURIComponent(city)}&count=1`));
          const hit = geo.results?.[0];
          if (!hit) throw new Error("city not found");
          lat = hit.latitude;
          lon = hit.longitude;
          city = hit.name;
          send({ type: "patch-settings", patch: { startPage: { widgets: { ...widgets, weatherLat: lat, weatherLon: lon, weatherCity: city } } } });
        }
        if (lat == null || lon == null) return;
        const unit = widgets.fahrenheit ? "&temperature_unit=fahrenheit" : "";
        const w = JSON.parse(await httpGet(`https://api.open-meteo.com/v1/forecast?latitude=${lat}&longitude=${lon}&current=temperature_2m,weather_code${unit}`));
        if (!cancelled) setData({ temp: Math.round(w.current.temperature_2m), code: w.current.weather_code, city: city ?? "" });
      } catch {
        if (!cancelled) setError(true);
      }
    })();
    return () => {
      cancelled = true;
    };
    // Only the weather fields matter: toggling the clock must not refetch the forecast.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [widgets?.weather, widgets?.weatherCity, widgets?.weatherLat, widgets?.weatherLon, widgets?.fahrenheit]);
  if (!widgets?.weather) return null;
  if (!widgets.weatherCity && widgets.weatherLat == null) {
    return (
      <button className="widget weather" onClick={() => send({ type: "navigate", tab: null, input: "haku://settings/start-page", disposition: "current-tab" })}>
        <Icon name="sun" size={18} /> <span>Set your city for weather</span>
      </button>
    );
  }
  if (error) return null;
  if (!data) return <div className="widget weather muted">Weather...</div>;
  const [label, icon] = WEATHER_CODES[data.code] ?? WEATHER_CODES[Math.floor(data.code / 10) * 10] ?? ["", "cloud"];
  return (
    <div className="widget weather" title="Weather data: Open-Meteo">
      <Icon name={icon} size={20} />
      <strong>
        {data.temp}°{widgets.fahrenheit ? "F" : "C"}
      </strong>
      <span className="muted">
        {data.city} · {label}
      </span>
    </div>
  );
}

function Clock() {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const t = window.setInterval(() => setNow(new Date()), 15_000);
    return () => window.clearInterval(t);
  }, []);
  return <div className="widget clock display">{now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</div>;
}

function DialTile({ node, index, onDragStart, dragging }: { node: BookmarkNode; index: number; onDragStart: (e: React.MouseEvent, index: number) => void; dragging: boolean }) {
  const openMenu = useStore((s) => s.openMenu);
  const showNames = useStore((s) => s.settings?.startPage.showDialNames ?? true);
  const nodes = useStore((s) => s.bookmarks.nodes);
  const folder = !node.url;
  const color = node.color ?? colorFor(node.url ?? node.title);
  const [openFolder, setOpenFolder] = useState(false);
  const open = (e: React.MouseEvent) => {
    if (!node.url) return setOpenFolder(true);
    send({ type: "navigate", tab: null, input: node.url, disposition: e.ctrlKey || e.button === 1 ? "background-tab" : "current-tab" });
  };
  const children = folder ? nodes.filter((n) => n.parent === node.id) : [];
  return (
    <>
      <button
        data-dial-index={index}
        className={clsx("dial notch", folder && "folder", dragging && "dragging")}
        style={{ "--dial": color } as React.CSSProperties}
        onMouseDown={(e) => e.button === 0 && onDragStart(e, index)}
        onMouseUp={(e) => e.button === 1 && open(e)}
        onClick={open}
        onContextMenu={(e) => {
          e.preventDefault();
          openMenu({ kind: "dial", x: e.clientX, y: e.clientY, id: node.id });
        }}
        title={node.url ?? node.title}
      >
        <span className="dial-art">
          {folder ? (
            <span className="dial-folder-grid">
              {children.slice(0, 4).map((c) => (
                <span key={c.id} style={{ background: c.color ?? colorFor(c.url ?? c.title) }}>
                  {letterOf(c.title, c.url ?? "")}
                </span>
              ))}
            </span>
          ) : (
            <span className="dial-letter">{letterOf(node.title, node.url ?? "")}</span>
          )}
        </span>
        {showNames && <span className="dial-name ellipsis">{node.title || hostOf(node.url ?? "")}</span>}
      </button>
      {openFolder && (
        <div className="modal-backdrop" onMouseDown={() => setOpenFolder(false)}>
          <div className="dial-folder-pop" onMouseDown={(e) => e.stopPropagation()}>
            <h3 className="display">{node.title}</h3>
            <div className="dial-grid">
              {children.map((c, i) => (
                <DialTile key={c.id} node={c} index={i} onDragStart={() => {}} dragging={false} />
              ))}
              {children.length === 0 && <p className="muted">Empty folder. Drag a tile onto it to add.</p>}
            </div>
          </div>
        </div>
      )}
    </>
  );
}

export function StartPage() {
  const settings = useStore((s) => s.settings);
  const nodes = useStore((s) => s.bookmarks.nodes);
  const closed = useStore((s) => s.session?.closed ?? NONE);
  const search = useStore((s) => s.settings?.search);
  const openModal = useStore((s) => s.openModal);
  const [text, setText] = useState("");
  const [top, setTop] = useState<UrlStat[]>([]);
  const [drag, setDrag] = useState<{ from: number; over: number } | null>(null);
  const grid = useRef<HTMLDivElement>(null);
  const dials = useMemo(() => nodes.filter((n) => n.parent === "speed-dial"), [nodes]);

  useEffect(() => {
    if (!settings?.startPage.dialSuggestions) return;
    const off = onEvent((e) => {
      if (e.type === "top-sites") setTop(e.sites);
    });
    send({ type: "top-sites", limit: 12 });
    return off;
  }, [settings?.startPage.dialSuggestions]);

  const feeds = useMemo(() => ["https://www.pcgamer.com/rss/", "https://www.rockpapershotgun.com/feed"], []);
  const news = useFeeds(settings?.startPage.showNews ? feeds : []);
  const games = useFreeGames(!!settings?.startPage.gameStrips);

  if (!settings) return null;
  const sp = settings.startPage;
  const engine = search?.engines.find((e) => e.id === search.defaultEngine);
  const suggestions = top.filter((t) => !dials.some((d) => d.url && hostOf(d.url) === hostOf(t.url))).slice(0, 6);

  const startDrag = (e: React.MouseEvent, from: number) => {
    const startX = e.clientX;
    const startY = e.clientY;
    let moved = false;
    const move = (ev: MouseEvent) => {
      if (!moved && Math.hypot(ev.clientX - startX, ev.clientY - startY) < 8) return;
      moved = true;
      const tiles = Array.from(grid.current?.querySelectorAll<HTMLElement>("[data-dial-index]") ?? []);
      let over = tiles.length;
      for (const t of tiles) {
        const r = t.getBoundingClientRect();
        if (ev.clientY < r.bottom && ev.clientX < r.left + r.width / 2) {
          over = Number(t.dataset.dialIndex);
          break;
        }
      }
      setDrag({ from, over });
    };
    const up = (ev: MouseEvent) => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      if (moved) {
        ev.preventDefault();
        setDrag((d) => {
          if (d && d.over !== d.from && d.over !== d.from + 1) {
            const node = dials[d.from];
            if (node) send({ type: "move-bookmark", id: node.id, parent: "speed-dial", index: d.over > d.from ? d.over - 1 : d.over });
          }
          return null;
        });
        // Swallow the click that follows a drag.
        window.addEventListener("click", (c) => c.stopPropagation(), { capture: true, once: true });
      }
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  };

  return (
    <div className={clsx("start-page", `pos-${sp.dialPosition}`, `effect-${sp.dialEffect}`, sp.biggerTiles && "big-tiles")}>
      <Wallpaper />
      <div className="start-widgets">
        {sp.widgets.clock && <Clock />}
        <Weather />
      </div>
      <div className="start-content">
        {!sp.hideSearch && (
          <form
            className="start-search"
            onSubmit={(e) => {
              e.preventDefault();
              if (text.trim()) send({ type: "navigate", tab: null, input: text, disposition: "current-tab" });
            }}
          >
            <span className="engine-badge">{engine?.name[0] ?? "S"}</span>
            <Input value={text} onChange={(e) => setText(e.target.value)} placeholder={`Search with ${engine?.name ?? "the web"}`} />
            <button type="submit" className="icon-btn" title="Search">
              <Icon name="search" size={17} />
            </button>
          </form>
        )}

        {!sp.hideSpeedDial && (
          <div className="dial-grid" ref={grid} style={!sp.autoColumns ? { gridTemplateColumns: `repeat(${sp.dialColumns}, ${sp.biggerTiles ? 200 : 150}px)`, maxWidth: "none" } : undefined}>
            {dials.map((d, i) => (
              <div key={d.id} className="dial-slot">
                {drag && drag.over === i && drag.from !== i && <span className="dial-drop" />}
                <DialTile node={d} index={i} onDragStart={startDrag} dragging={drag?.from === i} />
              </div>
            ))}
            {!sp.hideAddSite && (
              <button className="dial notch add" onClick={() => openModal({ kind: "dial-edit", id: null, parent: "speed-dial" })}>
                <span className="dial-art">
                  <Icon name="plus" size={26} />
                </span>
                {sp.showDialNames && <span className="dial-name">Add a site</span>}
              </button>
            )}
          </div>
        )}

        {sp.dialSuggestions && suggestions.length > 0 && (
          <section className="start-row">
            <h3 className="display section-label">Suggestions</h3>
            <div className="chip-row">
              {suggestions.map((s) => (
                <span key={s.url} className="suggestion-chip">
                  <button onClick={() => send({ type: "navigate", tab: null, input: s.url, disposition: "current-tab" })}>{s.title || hostOf(s.url)}</button>
                  <button className="add" title="Add to Speed Dial" onClick={() => send({ type: "add-bookmark", parent: "speed-dial", title: s.title || hostOf(s.url), url: s.url, index: null })}>
                    <Icon name="plus" size={12} />
                  </button>
                </span>
              ))}
            </div>
          </section>
        )}

        {sp.showContinue && closed.length > 0 && (
          <section className="start-row">
            <h3 className="display section-label">Continue on</h3>
            <div className="continue-row">
              {closed.slice(0, 5).map((c, i) => (
                <button key={`${c.tab.id}-${i}`} className="continue-card notch" onClick={() => send({ type: "navigate", tab: null, input: c.tab.url, disposition: "current-tab" })}>
                  <span className="ellipsis">{c.tab.title || hostOf(c.tab.url)}</span>
                  <small className="dim">{hostOf(c.tab.url)}</small>
                </button>
              ))}
            </div>
          </section>
        )}

        {sp.gameStrips && games.length > 0 && (
          <section className="start-row">
            <h3 className="display section-label">Free to claim now</h3>
            <div className="game-strip">
              {games.slice(0, 6).map((g) => (
                <button key={g.url} className="game-card notch" onClick={() => send({ type: "navigate", tab: null, input: g.url, disposition: "new-tab" })}>
                  {g.image && <img src={g.image} alt="" />}
                  <span className="ellipsis">{g.title}</span>
                  <small className="dim">{g.store}</small>
                </button>
              ))}
            </div>
          </section>
        )}

        {sp.showNews && news.items.length > 0 && (
          <section className="start-row">
            <h3 className="display section-label">News</h3>
            <div className="news-grid">
              {news.items.slice(0, 6).map((n) => (
                <button key={n.link} className="news-card" onClick={() => send({ type: "navigate", tab: null, input: n.link, disposition: "current-tab" })}>
                  <small className="accent">{n.source}</small>
                  <span>{n.title}</span>
                </button>
              ))}
            </div>
          </section>
        )}
      </div>
    </div>
  );
}

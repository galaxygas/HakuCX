/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useEffect, useMemo, useState } from "react";
import { Button } from "../components/controls";
import { Icon } from "../components/Icon";
import { onEvent, send } from "../ipc/bridge";
import type { Pinboard } from "../ipc/generated/Pinboard";
import type { PinItem } from "../ipc/generated/PinItem";
import { clsx, hostOf } from "../lib/util";
import { useStore } from "../state/store";
import { Input } from "../components/Field";

const PIN_COLORS = ["#7d46f2", "#fa1e4e", "#00e5c3", "#f6c955", "#4461ee", "#aeff3c", "#ff708d"];

function newId() {
  return Math.random().toString(36).slice(2) + Date.now().toString(36);
}

function activePage(): { url: string; title: string } | null {
  const s = useStore.getState();
  const id = s.session?.activeTabs[s.session.activeWorkspace];
  const tab = s.session?.tabs.find((t) => t.id === id);
  if (!tab || tab.url.startsWith("haku:")) return null;
  return { url: s.meta[tab.id]?.url ?? tab.url, title: s.meta[tab.id]?.title ?? tab.title };
}

// ------------------------------------------------------------------ Pinboards

export function PinboardsPanel() {
  const pinboards = useStore((s) => s.pinboards);
  const [boardId, setBoardId] = useState<string | null>(null);
  const [note, setNote] = useState("");
  const board = pinboards.boards.find((b) => b.id === boardId) ?? pinboards.boards[0];
  const save = (boards: Pinboard[]) => send({ type: "save-pinboards", pinboards: { boards } });
  const updateBoard = (b: Pinboard) => save(pinboards.boards.map((x) => (x.id === b.id ? b : x)));
  const addItem = (item: Partial<PinItem>) => {
    if (!board) return;
    const full: PinItem = { id: newId(), kind: "note", title: "", url: null, text: "", color: null, done: false, created: Date.now(), ...item };
    updateBoard({ ...board, items: [full, ...board.items] });
  };
  if (pinboards.boards.length === 0) {
    return (
      <div className="panel-scroll">
        <div className="panel-empty">
          <Icon name="pin" size={28} />
          <p>Pinboards collect links, notes and to-dos in one place.</p>
          <Button kind="accent" onClick={() => save([{ id: newId(), name: "My board", emoji: null, color: PIN_COLORS[0]!, items: [], created: Date.now() }])}>
            Create a pinboard
          </Button>
        </div>
      </div>
    );
  }
  return (
    <div className="panel-scroll">
      <div className="chip-row">
        {pinboards.boards.map((b) => (
          <button key={b.id} className={clsx("chip", b.id === board?.id && "accent")} onClick={() => setBoardId(b.id)}>
            {b.emoji} {b.name}
          </button>
        ))}
        <button className="chip" title="New board" onClick={() => save([...pinboards.boards, { id: newId(), name: `Board ${pinboards.boards.length + 1}`, emoji: null, color: PIN_COLORS[pinboards.boards.length % PIN_COLORS.length]!, items: [], created: Date.now() }])}>
          <Icon name="plus" size={12} />
        </button>
      </div>
      {board && (
        <>
          <Input className="text-input board-name" value={board.name} onChange={(e) => updateBoard({ ...board, name: e.target.value })} />
          <div className="row">
            <Button icon="link" onClick={() => {
              const page = activePage();
              if (page) addItem({ kind: "link", title: page.title, url: page.url });
              else useStore.getState().toast("Open a web page to pin it.", "info");
            }}>
              Pin this page
            </Button>
            <Button icon="check" onClick={() => addItem({ kind: "todo", text: "New to-do" })}>
              To-do
            </Button>
            <span className="grow" />
            <Button icon="trash" title="Delete board" onClick={() => save(pinboards.boards.filter((b) => b.id !== board.id))} />
          </div>
          <div className="row">
            <Input className="text-input" placeholder="Write a note and press Enter" value={note} onChange={(e) => setNote(e.target.value)} onKeyDown={(e) => {
              if (e.key === "Enter" && note.trim()) {
                addItem({ kind: "note", text: note.trim(), color: board.color });
                setNote("");
              }
            }} />
          </div>
          <div className="pin-grid">
            {board.items.map((item) => (
              <div key={item.id} className={clsx("pin-card", item.kind)} style={{ borderColor: item.color ?? "var(--hx-line)" }}>
                {item.kind === "link" && item.url && (
                  <button className="pin-link" onClick={() => send({ type: "navigate", tab: null, input: item.url!, disposition: "new-tab" })}>
                    <Icon name="globe" size={14} />
                    <span className="ellipsis">{item.title || item.url}</span>
                    <small className="dim">{hostOf(item.url)}</small>
                  </button>
                )}
                {item.kind === "note" && <p className="pin-note">{item.text}</p>}
                {item.kind === "todo" && (
                  <label className="pin-todo">
                    <input type="checkbox" checked={item.done} onChange={(e) => updateBoard({ ...board, items: board.items.map((x) => (x.id === item.id ? { ...x, done: e.target.checked } : x)) })} />
                    <Input className="pin-todo-text" value={item.text} onChange={(e) => updateBoard({ ...board, items: board.items.map((x) => (x.id === item.id ? { ...x, text: e.target.value } : x)) })} />
                  </label>
                )}
                <button className="pin-remove icon-btn" title="Remove" onClick={() => updateBoard({ ...board, items: board.items.filter((x) => x.id !== item.id) })}>
                  <Icon name="x" size={12} />
                </button>
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

// ------------------------------------------------------------------ Player

const PLAYER_SERVICES = [
  { id: "spotify", name: "Spotify", url: "https://open.spotify.com/", color: "#1db954", drm: true },
  { id: "apple-music", name: "Apple Music", url: "https://music.apple.com/", color: "#fa2d48", drm: true },
  { id: "youtube-music", name: "YouTube Music", url: "https://music.youtube.com/", color: "#ff0000", drm: false },
  { id: "soundcloud", name: "SoundCloud", url: "https://soundcloud.com/", color: "#ff5500", drm: false },
  { id: "deezer", name: "Deezer", url: "https://www.deezer.com/", color: "#a238ff", drm: true },
  { id: "tidal", name: "Tidal", url: "https://listen.tidal.com/", color: "#00c4c4", drm: true },
];

export function PlayerPanel() {
  const player = useStore((s) => s.settings?.player);
  const [service, setService] = useState<string | null>(player?.service ?? null);
  const current = PLAYER_SERVICES.find((s) => s.id === service);
  useEffect(() => {
    if (current) send({ type: "open-web-panel", id: "player", url: current.url });
    window.dispatchEvent(new CustomEvent("hx-layout"));
    return () => {
      if (current) send({ type: "hide-web-panel" });
    };
  }, [current]);
  if (current) {
    return (
      <div className="player-web">
        <div className="row player-bar">
          <button className="btn btn-ghost" onClick={() => setService(null)}>
            <Icon name="chevron-left" size={14} /> Services
          </button>
          <span className="display">{current.name}</span>
        </div>
        {current.drm && <p className="note warn">{current.name} uses DRM (Widevine), which the Servo engine does not support yet. Browsing works; playback will not.</p>}
        <div className="web-panel-hole" data-web-panel="player" />
      </div>
    );
  }
  return (
    <div className="panel-scroll">
      <div className="player-hero">
        <div className="eq">
          {Array.from({ length: 12 }, (_, i) => (
            <span key={i} style={{ animationDelay: `${(i % 5) * 0.13}s` }} />
          ))}
        </div>
        <h3 className="display">A world of music and podcasts at your fingertips</h3>
      </div>
      <div className="service-grid">
        {PLAYER_SERVICES.map((s) => (
          <button
            key={s.id}
            className="service-tile notch"
            style={{ "--brand": s.color } as React.CSSProperties}
            onClick={() => {
              setService(s.id);
              send({ type: "patch-settings", patch: { player: { service: s.id } } });
            }}
          >
            <span className="lettermark" style={{ width: 34, height: 34, background: s.color, color: "#fff" }}>
              {s.name[0]}
            </span>
            <span>{s.name}</span>
            {s.drm && <small className="dim">needs DRM</small>}
          </button>
        ))}
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ Personal news

const DEFAULT_FEEDS = [
  "https://www.pcgamer.com/rss/",
  "https://www.rockpapershotgun.com/feed",
  "https://feeds.arstechnica.com/arstechnica/gaming",
  "https://www.theverge.com/rss/index.xml",
];

type FeedItem = { title: string; link: string; date: number; source: string };

export function parseFeed(xml: string, source: string): FeedItem[] {
  const decode = (s: string) =>
    s
      .replace(/<!\[CDATA\[([\s\S]*?)\]\]>/g, "$1")
      .replace(/<[^>]+>/g, "")
      .replace(/&lt;/g, "<")
      .replace(/&gt;/g, ">")
      .replace(/&quot;/g, '"')
      .replace(/&#39;|&apos;/g, "'")
      .replace(/&amp;/g, "&")
      .trim();
  const tag = (block: string, name: string) => {
    const m = block.match(new RegExp(`<${name}[^>]*>([\\s\\S]*?)</${name}>`, "i"));
    return m ? decode(m[1]!) : "";
  };
  const items: FeedItem[] = [];
  const blocks = xml.match(/<item[\s>][\s\S]*?<\/item>/gi) ?? xml.match(/<entry[\s>][\s\S]*?<\/entry>/gi) ?? [];
  for (const block of blocks) {
    let link = tag(block, "link");
    if (!link) link = block.match(/<link[^>]*href="([^"]+)"/i)?.[1] ?? "";
    const date = Date.parse(tag(block, "pubDate") || tag(block, "updated") || tag(block, "published")) || 0;
    const title = tag(block, "title");
    if (title && /^https?:/.test(link)) items.push({ title, link, date, source });
  }
  return items;
}

function loadFeeds(): string[] {
  try {
    const raw = localStorage.getItem("hx-news-feeds");
    return raw ? (JSON.parse(raw) as string[]) : DEFAULT_FEEDS;
  } catch {
    return DEFAULT_FEEDS;
  }
}

let feedRequest = 50_000;

export function useFeeds(feeds: string[]): { items: FeedItem[]; loading: boolean } {
  const [items, setItems] = useState<FeedItem[]>([]);
  const [pending, setPending] = useState(0);
  useEffect(() => {
    const ids = new Map<number, string>();
    const collected: FeedItem[] = [];
    setPending(feeds.length);
    const off = onEvent((e) => {
      if (e.type !== "http-result" || !ids.has(e.request)) return;
      const source = hostOf(ids.get(e.request)!);
      ids.delete(e.request);
      if (!e.error) collected.push(...parseFeed(e.body, source));
      setItems([...collected].sort((a, b) => b.date - a.date).slice(0, 60));
      setPending(ids.size);
    });
    for (const url of feeds) {
      const request = feedRequest++;
      ids.set(request, url);
      send({ type: "http-get", request, url });
    }
    return off;
  }, [feeds]);
  return { items, loading: pending > 0 };
}

export function NewsPanel() {
  const [feeds, setFeeds] = useState<string[]>(loadFeeds);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const { items, loading } = useFeeds(feeds);
  const saveFeeds = (next: string[]) => {
    setFeeds(next);
    try {
      localStorage.setItem("hx-news-feeds", JSON.stringify(next));
    } catch {
      // Not persisted; still shown this session.
    }
  };
  return (
    <div className="panel-scroll">
      <div className="row">
        <span className="muted grow">{loading ? "Loading feeds..." : `${items.length} stories from ${feeds.length} feeds`}</span>
        <button className="icon-btn" title="Edit feeds" onClick={() => setEditing((e) => !e)}>
          <Icon name="settings" size={15} />
        </button>
      </div>
      {editing && (
        <div className="feed-editor">
          {feeds.map((f) => (
            <div key={f} className="row">
              <span className="ellipsis grow">{f}</span>
              <button className="icon-btn" onClick={() => saveFeeds(feeds.filter((x) => x !== f))}>
                <Icon name="x" size={13} />
              </button>
            </div>
          ))}
          <div className="row">
            <Input className="text-input" placeholder="https://example.com/feed.xml" value={draft} onChange={(e) => setDraft(e.target.value)} />
            <Button onClick={() => {
              if (/^https?:\/\//.test(draft.trim())) {
                saveFeeds([...feeds, draft.trim()]);
                setDraft("");
              }
            }}>Add</Button>
          </div>
          <p className="note">Feeds are fetched directly from each site by HakuCX; nothing goes through a HakuCX server.</p>
        </div>
      )}
      {items.map((item) => (
        <button key={item.link} className="news-row" onClick={() => send({ type: "navigate", tab: null, input: item.link, disposition: "new-tab" })}>
          <span className="news-source">{item.source}</span>
          <span className="news-title">{item.title}</span>
          {item.date > 0 && <small className="dim">{new Date(item.date).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}</small>}
        </button>
      ))}
    </div>
  );
}

// ------------------------------------------------------------------ GX Rewards (local)

const ACHIEVEMENTS = [
  { id: "first-steps", name: "First steps", detail: "Visit 10 pages", metric: "pages", goal: 10 },
  { id: "explorer", name: "Explorer", detail: "Visit 250 pages", metric: "pages", goal: 250 },
  { id: "ad-slayer", name: "Ad slayer", detail: "Block 1,000 ads and trackers", metric: "blocked", goal: 1000 },
  { id: "hoarder", name: "Hoarder", detail: "Download 20 files", metric: "downloads", goal: 20 },
  { id: "tab-tamer", name: "Tab tamer", detail: "Have 30 tabs open at once", metric: "tabs", goal: 30 },
  { id: "decorator", name: "Decorator", detail: "Try 5 different themes", metric: "themes", goal: 5 },
];

export type RewardStats = { pages: number; blocked: number; downloads: number; tabs: number; themes: string[] };

export function readRewards(): RewardStats {
  try {
    return { pages: 0, blocked: 0, downloads: 0, tabs: 0, themes: [], ...(JSON.parse(localStorage.getItem("hx-rewards") ?? "{}") as Partial<RewardStats>) };
  } catch {
    return { pages: 0, blocked: 0, downloads: 0, tabs: 0, themes: [] };
  }
}

export function levelOf(stats: RewardStats): { level: number; xp: number; next: number } {
  const xp = stats.pages + Math.floor(stats.blocked / 10) + stats.downloads * 3 + stats.themes.length * 5;
  const level = Math.floor(Math.sqrt(xp / 15)) + 1;
  return { level, xp, next: Math.pow(level, 2) * 15 };
}

export function RewardsPanel() {
  const enabled = useStore((s) => s.settings?.features.rewards);
  const stats = useMemo(readRewards, []);
  const { level, xp, next } = levelOf(stats);
  if (!enabled) return <div className="panel-empty">GX Rewards is turned off in Settings &gt; Features.</div>;
  const metric = (m: string) => (m === "themes" ? stats.themes.length : ((stats as unknown as Record<string, number>)[m] ?? 0));
  return (
    <div className="panel-scroll">
      <div className="level-card">
        <div className="octagon big">
          <small className="display">Level</small>
          <strong>{level}</strong>
        </div>
        <div className="grow">
          <div className="row">
            <span className="display">{xp} XP</span>
            <span className="grow" />
            <span className="dim">{next} XP for level {level + 1}</span>
          </div>
          <div className="progress">
            <div className="progress-fill" style={{ width: `${Math.min(100, (xp / next) * 100)}%` }} />
          </div>
          <small className="muted">XP is counted on this device only. Nothing is sent anywhere.</small>
        </div>
      </div>
      <h3 className="display section-label">Achievements</h3>
      {ACHIEVEMENTS.map((a) => {
        const value = metric(a.metric);
        const done = value >= a.goal;
        return (
          <div key={a.id} className={clsx("achievement", done && "done")}>
            <Icon name={done ? "trophy" : "star"} size={18} />
            <div className="grow">
              <div>{a.name}</div>
              <small className="muted">{a.detail}</small>
              <div className="progress small">
                <div className="progress-fill" style={{ width: `${Math.min(100, (value / a.goal) * 100)}%` }} />
              </div>
            </div>
            <span className="dim">
              {Math.min(value, a.goal)}/{a.goal}
            </span>
          </div>
        );
      })}
    </div>
  );
}

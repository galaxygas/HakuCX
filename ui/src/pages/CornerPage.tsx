/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// GX Corner and My Games, built only on public, keyless sources fetched by HakuCX itself:
// Epic Games Store promotions, CheapShark deals, the Steam store, and RSS news.

import { useEffect, useMemo, useState } from "react";
import { Icon, Logo } from "../components/Icon";
import { send } from "../ipc/bridge";
import { clsx } from "../lib/util";
import { useFeeds } from "../panels/MiscPanels";
import { httpGet } from "./StartPage";
import { Input } from "../components/Field";

export type FreeGame = { title: string; url: string; image: string | null; store: string; until: number | null; upcoming: boolean };
type Deal = { id: string; title: string; sale: number; normal: number; savings: number; store: string; storeLogo: string | null; thumb: string; rating: number; url: string };
type SteamItem = { id: number; name: string; image: string; url: string; discount: number; price: number | null };

const open = (url: string) => send({ type: "navigate", tab: null, input: url, disposition: "new-tab" });

export function useFreeGames(enabled = true): FreeGame[] {
  const [games, setGames] = useState<FreeGame[]>([]);
  useEffect(() => {
    if (!enabled) return;
    httpGet("https://store-site-backend-static.ak.epicgames.com/freeGamesPromotions?locale=en-US&country=US&allowCountries=US")
      .then((body) => {
        const elements = JSON.parse(body)?.data?.Catalog?.searchStore?.elements ?? [];
        const out: FreeGame[] = [];
        for (const el of elements) {
          const now = el.promotions?.promotionalOffers?.[0]?.promotionalOffers?.[0];
          const next = el.promotions?.upcomingPromotionalOffers?.[0]?.promotionalOffers?.[0];
          const offer = now ?? next;
          if (!offer || offer.discountSetting?.discountPercentage !== 0) continue;
          const slug = el.catalogNs?.mappings?.[0]?.pageSlug ?? el.productSlug ?? el.urlSlug;
          const image = (el.keyImages ?? []).find((k: { type: string }) => k.type === "OfferImageWide" || k.type === "DieselStoreFrontWide")?.url ?? el.keyImages?.[0]?.url ?? null;
          out.push({
            title: el.title,
            url: slug ? `https://store.epicgames.com/en-US/p/${slug}` : "https://store.epicgames.com/en-US/free-games",
            image,
            store: now ? "Epic Games Store · free now" : "Epic Games Store · free soon",
            until: Date.parse(offer.endDate) || null,
            upcoming: !now,
          });
        }
        setGames(out.sort((a, b) => Number(a.upcoming) - Number(b.upcoming)));
      })
      .catch(() => setGames([]));
  }, [enabled]);
  return games;
}

function useDeals(query: string): { deals: Deal[]; loading: boolean; error: boolean } {
  const [state, setState] = useState<{ deals: Deal[]; loading: boolean; error: boolean }>({ deals: [], loading: true, error: false });
  useEffect(() => {
    let cancelled = false;
    setState((s) => ({ ...s, loading: true }));
    const stores = httpGet("https://www.cheapshark.com/api/1.0/stores").then((b) => JSON.parse(b) as { storeID: string; storeName: string; images: { logo: string } }[]);
    const q = query.trim() ? `&title=${encodeURIComponent(query.trim())}` : "";
    const deals = httpGet(`https://www.cheapshark.com/api/1.0/deals?sortBy=Deal%20Rating&pageSize=30${q}`).then((b) => JSON.parse(b) as Record<string, string>[]);
    Promise.all([stores, deals])
      .then(([storeList, dealList]) => {
        if (cancelled) return;
        const byId = new Map(storeList.map((s) => [s.storeID, s]));
        setState({
          loading: false,
          error: false,
          deals: dealList.map((d) => ({
            id: d.dealID!,
            title: d.title!,
            sale: Number(d.salePrice),
            normal: Number(d.normalPrice),
            savings: Math.round(Number(d.savings)),
            store: byId.get(d.storeID!)?.storeName ?? "Store",
            storeLogo: byId.get(d.storeID!) ? `https://www.cheapshark.com${byId.get(d.storeID!)!.images.logo}` : null,
            thumb: d.thumb!,
            rating: Number(d.steamRatingPercent) || 0,
            url: `https://www.cheapshark.com/redirect?dealID=${encodeURIComponent(d.dealID!)}`,
          })),
        });
      })
      .catch(() => !cancelled && setState({ deals: [], loading: false, error: true }));
    return () => {
      cancelled = true;
    };
  }, [query]);
  return state;
}

function useSteam(): { specials: SteamItem[]; upcoming: SteamItem[]; top: SteamItem[] } {
  const [data, setData] = useState<{ specials: SteamItem[]; upcoming: SteamItem[]; top: SteamItem[] }>({ specials: [], upcoming: [], top: [] });
  useEffect(() => {
    httpGet("https://store.steampowered.com/api/featuredcategories?cc=us&l=english")
      .then((body) => {
        const j = JSON.parse(body);
        const map = (items: Record<string, unknown>[] | undefined): SteamItem[] =>
          (items ?? []).map((i) => ({
            id: Number(i.id),
            name: String(i.name),
            image: String(i.header_image ?? i.large_capsule_image ?? ""),
            url: `https://store.steampowered.com/app/${i.id}/`,
            discount: Number(i.discount_percent ?? 0),
            price: i.final_price != null ? Number(i.final_price) / 100 : null,
          }));
        setData({ specials: map(j.specials?.items), upcoming: map(j.coming_soon?.items), top: map(j.top_sellers?.items) });
      })
      .catch(() => {});
  }, []);
  return data;
}

function DealCard({ d }: { d: Deal }) {
  return (
    <button className="deal-card notch" onClick={() => open(d.url)}>
      <img src={d.thumb} alt="" />
      <div className="deal-info">
        <span className="ellipsis deal-title">{d.title}</span>
        <small className="dim">
          {d.store}
          {d.rating ? ` · ${d.rating}% positive on Steam` : ""}
        </small>
      </div>
      <div className="deal-price">
        {d.savings > 0 && <span className="deal-badge">-{d.savings}%</span>}
        <strong>${d.sale.toFixed(2)}</strong>
        {d.savings > 0 && <s className="dim">${d.normal.toFixed(2)}</s>}
      </div>
    </button>
  );
}

const SECTIONS = [
  { id: "freebies", label: "Freebies" },
  { id: "deals", label: "Deals" },
  { id: "steam", label: "Steam" },
  { id: "upcoming", label: "Upcoming" },
  { id: "news", label: "News" },
];

export function CornerPage() {
  const free = useFreeGames();
  const { deals, loading, error } = useDeals("");
  const steam = useSteam();
  const feeds = useMemo(() => ["https://www.pcgamer.com/rss/", "https://www.rockpapershotgun.com/feed", "https://www.gamespot.com/feeds/news/"], []);
  const news = useFeeds(feeds);
  const scrollTo = (id: string) => document.getElementById(`corner-${id}`)?.scrollIntoView({ behavior: "smooth", block: "start" });
  return (
    <div className="corner">
      <header className="corner-head">
        <Logo size={24} />
        <h1 className="display">
          <span className="accent">GX</span> Corner
        </h1>
        <nav>
          {SECTIONS.map((s) => (
            <button key={s.id} className="display" onClick={() => scrollTo(s.id)}>
              {s.label}
            </button>
          ))}
        </nav>
      </header>
      <div className="corner-scroll">
        <section id="corner-freebies" className="corner-section">
          <h2 className="display">
            Free <span className="accent">games</span>
          </h2>
          {free.length === 0 && <p className="muted">Checking the Epic Games Store...</p>}
          <div className="free-grid">
            {free.map((g) => (
              <button key={g.url + g.title} className={clsx("free-card notch", g.upcoming && "upcoming")} onClick={() => open(g.url)}>
                {g.image && <img src={g.image} alt="" />}
                <div className="free-info">
                  <span className="ellipsis">{g.title}</span>
                  <small className="accent">{g.store}</small>
                  {g.until && <small className="dim">until {new Date(g.until).toLocaleDateString([], { month: "short", day: "numeric" })}</small>}
                </div>
              </button>
            ))}
          </div>
        </section>
        <section id="corner-deals" className="corner-section">
          <h2 className="display">
            Deals <span className="accent">aggregator</span>
          </h2>
          {loading && <p className="muted">Loading deals from CheapShark...</p>}
          {error && <p className="muted">Deals could not be loaded right now.</p>}
          <div className="deal-list">
            {deals.slice(0, 12).map((d) => (
              <DealCard key={d.id} d={d} />
            ))}
          </div>
          <small className="dim">Prices from CheapShark. HakuCX earns nothing from these links.</small>
        </section>
        <section id="corner-steam" className="corner-section">
          <h2 className="display">
            Top on <span className="accent">Steam</span>
          </h2>
          <div className="steam-strip">
            {[...steam.specials, ...steam.top].slice(0, 12).map((s, i) => (
              <button key={`${s.id}-${i}`} className="steam-card notch" onClick={() => open(s.url)}>
                <img src={s.image} alt="" />
                <span className="ellipsis">{s.name}</span>
                {s.discount > 0 && <span className="deal-badge">-{s.discount}%</span>}
              </button>
            ))}
          </div>
        </section>
        <section id="corner-upcoming" className="corner-section">
          <h2 className="display">
            Release <span className="accent">radar</span>
          </h2>
          <div className="steam-strip">
            {steam.upcoming.slice(0, 12).map((s) => (
              <button key={s.id} className="steam-card notch" onClick={() => open(s.url)}>
                <img src={s.image} alt="" />
                <span className="ellipsis">{s.name}</span>
                <small className="dim">Coming soon on Steam</small>
              </button>
            ))}
          </div>
        </section>
        <section id="corner-news" className="corner-section">
          <h2 className="display">
            News <span className="accent">feed</span>
          </h2>
          <div className="news-grid">
            {news.items.slice(0, 12).map((n) => (
              <button key={n.link} className="news-card" onClick={() => open(n.link)}>
                <small className="accent">{n.source}</small>
                <span>{n.title}</span>
              </button>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}

function loadWishlist(): string[] {
  try {
    return JSON.parse(localStorage.getItem("hx-wishlist") ?? "[]") as string[];
  } catch {
    return [];
  }
}

export function GamesPage() {
  const [tab, setTab] = useState<"deals" | "wishlist">("deals");
  const [query, setQuery] = useState("");
  const [search, setSearch] = useState("");
  const { deals, loading, error } = useDeals(search);
  const [wishlist, setWishlist] = useState<string[]>(loadWishlist);
  const [prices, setPrices] = useState<Record<string, { price: string; url: string } | null>>({});
  const saveWish = (next: string[]) => {
    setWishlist(next);
    try {
      localStorage.setItem("hx-wishlist", JSON.stringify(next));
    } catch {
      // Not persisted.
    }
  };
  useEffect(() => {
    if (tab !== "wishlist") return;
    for (const title of wishlist) {
      if (title in prices) continue;
      httpGet(`https://www.cheapshark.com/api/1.0/games?title=${encodeURIComponent(title)}&limit=1`)
        .then((b) => {
          const hit = (JSON.parse(b) as { cheapest: string; cheapestDealID: string }[])[0];
          setPrices((p) => ({ ...p, [title]: hit ? { price: hit.cheapest, url: `https://www.cheapshark.com/redirect?dealID=${encodeURIComponent(hit.cheapestDealID)}` } : null }));
        })
        .catch(() => setPrices((p) => ({ ...p, [title]: null })));
    }
  }, [tab, wishlist, prices]);
  return (
    <div className="games-page">
      <header className="corner-head">
        <Icon name="percent" size={22} />
        <h1 className="display">My games</h1>
        <nav>
          <button className={clsx("display", tab === "deals" && "active")} onClick={() => setTab("deals")}>
            Deals
          </button>
          <button className={clsx("display", tab === "wishlist" && "active")} onClick={() => setTab("wishlist")}>
            Wishlist
          </button>
        </nav>
      </header>
      <div className="corner-scroll">
        {tab === "deals" && (
          <>
            <form
              className="search-box"
              onSubmit={(e) => {
                e.preventDefault();
                setSearch(query);
              }}
            >
              <Icon name="search" size={15} />
              <Input placeholder="Search game deals" value={query} onChange={(e) => setQuery(e.target.value)} />
            </form>
            <h2 className="display section-label">{search ? `Deals for "${search}"` : "Top deals"}</h2>
            {loading && <p className="muted">Loading...</p>}
            {error && <p className="muted">Deals could not be loaded right now.</p>}
            <div className="deal-list">
              {deals.map((d) => (
                <div key={d.id} className="row">
                  <div className="grow">
                    <DealCard d={d} />
                  </div>
                  <button className="icon-btn" title="Add to wishlist" onClick={() => !wishlist.includes(d.title) && saveWish([...wishlist, d.title])}>
                    <Icon name={wishlist.includes(d.title) ? "check" : "heart"} size={15} />
                  </button>
                </div>
              ))}
            </div>
          </>
        )}
        {tab === "wishlist" && (
          <>
            {wishlist.length === 0 && <p className="muted">Add games from Deals with the heart button. Their lowest price shows here.</p>}
            {wishlist.map((t) => (
              <div key={t} className="list-row">
                <Icon name="heart" size={14} />
                <span className="grow ellipsis">{t}</span>
                {prices[t] === undefined ? (
                  <span className="dim">checking...</span>
                ) : prices[t] ? (
                  <button className="chip accent" onClick={() => open(prices[t]!.url)}>
                    lowest ${prices[t]!.price}
                  </button>
                ) : (
                  <span className="dim">no deal found</span>
                )}
                <button className="icon-btn" onClick={() => saveWish(wishlist.filter((x) => x !== t))}>
                  <Icon name="x" size={13} />
                </button>
              </div>
            ))}
          </>
        )}
      </div>
    </div>
  );
}

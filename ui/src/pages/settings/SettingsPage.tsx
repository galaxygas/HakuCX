/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useState } from "react";
import { Button, Segmented, Select, Slider, Toggle } from "../../components/controls";
import { Icon } from "../../components/Icon";
import { clsx, patch } from "../../lib/util";
import { ThemeCard } from "../../overlays/EasySetup";
import { useStore } from "../../state/store";
import { go } from "../InternalPage";
import { SiteModsList } from "../ModsPage";
import { RowView, rowText, type SectionDef } from "./rows";
import { SECTIONS } from "./sections";
import {
  BrowserSoundsPage,
  FilterListsPage,
  KeyboardSoundsPage,
  LightsPage,
  PanicPage,
  SearchEnginesPage,
  ShortcutsPage,
  SitePage,
  SiteSettingsPage,
} from "./subpages";
import { Input } from "../../components/Field";
import { AddressesPage, PasswordsPage, PaymentMethodsPage } from "./autofill";

const GROUP_BREAKS = [0, 1, 2, 3];

export function SettingsPage({ section, params }: { section: string; params: URLSearchParams }) {
  const settings = useStore((s) => s.settings);
  const [query, setQuery] = useState("");
  if (!settings) return null;
  const q = query.trim().toLowerCase();
  const sub = SUBPAGES[section];
  const current = SECTIONS.find((s) => s.id === section) ?? SECTIONS[0]!;

  return (
    <div className="settings">
      <nav className="settings-nav card">
        {GROUP_BREAKS.map((g) => (
          <div key={g} className="settings-nav-group">
            {SECTIONS.filter((s) => s.group === g).map((s) => (
              <button key={s.id} className={clsx("settings-nav-item display", s.id === current.id && !sub && !q && "active")} onClick={() => { setQuery(""); go(`haku://settings/${s.id}`); }}>
                <Icon name={s.icon} size={15} />
                {s.label}
              </button>
            ))}
          </div>
        ))}
        <div className="settings-nav-group">
          <button className="settings-nav-item display" onClick={() => go("haku://about/")}>
            <Icon name="info" size={15} />
            About HakuCX
            <Icon name="external" size={12} />
          </button>
        </div>
      </nav>
      <div className="settings-main">
        <div className="search-box settings-search">
          <Icon name="search" size={15} />
          <Input placeholder="Search settings" value={query} onChange={(e) => setQuery(e.target.value)} />
        </div>
        {q ? (
          <SearchResults query={q} />
        ) : sub ? (
          <div className="settings-sub">
            <div className="sub-head">
              <button className="icon-btn" title="Back" onClick={() => go(`haku://settings/${sub.parent}`)}>
                <Icon name="back" size={18} />
              </button>
              <h2 className="display">{sub.title}</h2>
            </div>
            {sub.render(params)}
          </div>
        ) : (
          <SectionView section={current} />
        )}
      </div>
    </div>
  );
}

function SectionView({ section }: { section: SectionDef }) {
  const settings = useStore((s) => s.settings)!;
  return (
    <>
      {section.cards.map((card) => (
        <div key={card.title} className="settings-card-wrap">
          <h3 className="display section-label">{card.title}</h3>
          <div className="settings-card card">
            {card.rows.map((row, i) => (
              <RowView key={`${row.label}-${i}`} row={row} settings={settings} />
            ))}
          </div>
        </div>
      ))}
    </>
  );
}

function SearchResults({ query }: { query: string }) {
  const settings = useStore((s) => s.settings)!;
  const hits = SECTIONS.flatMap((s) => s.cards.flatMap((c) => c.rows.filter((r) => rowText(r).includes(query)).map((r) => ({ section: s, card: c, row: r }))));
  if (hits.length === 0) return <p className="muted">No settings match "{query}".</p>;
  return (
    <div className="settings-card card">
      {hits.map(({ section, card, row }, i) => (
        <div key={i}>
          <small className="dim display">
            {section.label} › {card.title}
          </small>
          <RowView row={row} settings={settings} />
        </div>
      ))}
    </div>
  );
}

// ------------------------------------------------------------------ themes sub-page

function ThemesPage() {
  const themes = useStore((s) => s.themes);
  const appearance = useStore((s) => s.settings?.appearance);
  const palette = useStore((s) => s.palette);
  const openModal = useStore((s) => s.openModal);
  const [filter, setFilter] = useState<"all" | "gx" | "haku" | "user">("all");
  const [mode, setMode] = useState<"dark" | "light">(palette?.dark === false ? "light" : "dark");
  if (!appearance) return null;
  const list = themes.filter((t) => filter === "all" || t.family === filter);
  return (
    <>
      <div className="row">
        <Segmented value={filter} options={[{ value: "all", label: "All" }, { value: "gx", label: "GX" }, { value: "haku", label: "HakuCX" }, { value: "user", label: "Mine" }]} onChange={setFilter} />
        <span className="grow" />
        <Segmented value={mode} options={[{ value: "light", label: "Light" }, { value: "dark", label: "Dark" }]} onChange={(v) => { setMode(v); patch({ appearance: { colorMode: v } }); }} />
      </div>
      <button className="setting-line link-line card" onClick={() => openModal({ kind: "theme-editor", themeId: null })}>
        <Icon name="palette" size={16} />
        <div className="setting-line-text">
          <span>Custom theme</span>
          <small className="muted">Pick your own primary and secondary colors</small>
        </div>
        <Icon name="chevron-right" size={16} />
      </button>
      <div className="theme-grid">
        {list.map((t) => (
          <div key={t.id} className="theme-grid-item">
            <ThemeCard theme={t} dark={mode === "dark"} active={appearance.theme === t.id} onPick={() => patch({ appearance: { theme: t.id, primaryColor: null, secondaryColor: null } })} />
            {t.user && (
              <button className="link-btn" onClick={() => openModal({ kind: "theme-editor", themeId: t.id })}>
                Edit
              </button>
            )}
          </div>
        ))}
        {filter === "user" && list.length === 0 && <p className="muted">Themes you make appear here.</p>}
      </div>
    </>
  );
}

function ShadersPage() {
  const features = useStore((s) => s.settings?.features);
  if (!features) return null;
  return (
    <div className="settings-card card">
      <div className="setting-line">
        <div className="setting-line-text">
          <span>Shaders</span>
          <small className="muted">Shaders apply visual effects on web pages. This may impact performance.</small>
        </div>
        <Toggle on={features.shadersEnabled} onChange={(v) => patch({ features: { shadersEnabled: v } })} />
      </div>
      <Button onClick={() => useStore.getState().setPanel("shaders")}>Open the Shaders panel</Button>
    </div>
  );
}

function WebModdingPage() {
  const on = useStore((s) => s.settings?.features.webModding ?? true);
  return (
    <div className="settings-card card">
      <div className="setting-line">
        <div className="setting-line-text">
          <span>Web modding</span>
          <small className="muted">Add your own CSS to any site. Styles apply to the domain and its subdomains.</small>
        </div>
        <Toggle on={on} onChange={(v) => patch({ features: { webModding: v } })} />
      </div>
      <h3 className="display section-label">Domains</h3>
      <SiteModsList />
    </div>
  );
}

function MusicPage() {
  const sounds = useStore((s) => s.settings?.sounds);
  if (!sounds) return null;
  return (
    <div className="settings-card card">
      <div className="setting-line">
        <span>Background music</span>
        <Toggle on={sounds.backgroundMusic} onChange={(v) => patch({ sounds: { backgroundMusic: v } })} />
      </div>
      <div className="setting-line">
        <span>Track</span>
        <Select value={sounds.musicTrack} options={[{ value: "focus", label: "Focus" }, { value: "chill", label: "Chill" }, { value: "raid", label: "Raid" }, { value: "doom", label: "Doom" }]} onChange={(v) => patch({ sounds: { musicTrack: v } })} />
      </div>
      <div className="setting-line slider-line">
        <span>Volume</span>
        <div className="slider-wrap">
          <Slider value={sounds.musicVolume} min={0} max={100} onCommit={(v) => patch({ sounds: { musicVolume: v } })} label="Music volume" />
          <span className="slider-value">{sounds.musicVolume}</span>
        </div>
      </div>
      <p className="note">HakuCX composes its music live from chord progressions; it uses no recordings.</p>
    </div>
  );
}

type Sub = { title: string; parent: string; render: (params: URLSearchParams) => React.ReactNode };

const SUBPAGES: Record<string, Sub> = {
  themes: { title: "Themes", parent: "appearance", render: () => <ThemesPage /> },
  "browser-sounds": { title: "Browser sounds", parent: "features", render: () => <BrowserSoundsPage /> },
  "keyboard-sounds": { title: "Keyboard sounds", parent: "features", render: () => <KeyboardSoundsPage /> },
  music: { title: "Background music", parent: "features", render: () => <MusicPage /> },
  lights: { title: "GX Lights", parent: "features", render: () => <LightsPage /> },
  shaders: { title: "Shaders", parent: "features", render: () => <ShadersPage /> },
  "web-modding": { title: "Web modding", parent: "features", render: () => <WebModdingPage /> },
  panic: { title: "Panic button", parent: "features", render: () => <PanicPage /> },
  shortcuts: { title: "Keyboard shortcuts", parent: "features", render: () => <ShortcutsPage /> },
  "site-settings": { title: "Site settings", parent: "privacy", render: () => <SiteSettingsPage /> },
  site: { title: "Site", parent: "site-settings", render: (p) => <SitePage origin={p.get("origin") ?? ""} /> },
  "search-engines": { title: "Manage search engines and site search", parent: "search", render: () => <SearchEnginesPage /> },
  "filter-lists": { title: "Manage lists", parent: "privacy", render: () => <FilterListsPage /> },
  passwords: { title: "Password manager", parent: "autofill", render: () => <PasswordsPage /> },
  "payment-methods": { title: "Payment methods", parent: "autofill", render: () => <PaymentMethodsPage /> },
  addresses: { title: "Addresses and more", parent: "autofill", render: () => <AddressesPage /> },
};

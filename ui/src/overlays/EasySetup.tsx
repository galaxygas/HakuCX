/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useState, type ReactNode } from "react";
import { Button, Radio, Segmented, Select, Slider, Toggle } from "../components/controls";
import { Icon } from "../components/Icon";
import { send } from "../ipc/bridge";
import type { ThemeInfo } from "../ipc/generated/ThemeInfo";
import { WALLPAPERS } from "../lib/catalog";
import { clsx, patch, type SettingsPatch } from "../lib/util";
import { openInternal, useStore } from "../state/store";

/** Presets: one click sets a whole look. Each is an ordinary settings patch. */
export const PRESETS: { id: string; name: string; theme: string; patch: SettingsPatch }[] = [
  {
    id: "gx-classic",
    name: "GX Classic",
    theme: "gx-classic",
    patch: { appearance: { theme: "gx-classic", colorMode: "dark", interfaceMode: "auto", cornerStyle: "cut", roundness: 8, wallpaper: "gx-ribbons", showWallpaper: true, crosshairCursor: false }, sounds: { browserSounds: true, musicTrack: "focus" } },
  },
  {
    id: "ultraviolet",
    name: "Ultraviolet",
    theme: "ultraviolet",
    patch: { appearance: { theme: "ultraviolet", colorMode: "dark", interfaceMode: "auto", cornerStyle: "cut", roundness: 8, wallpaper: "gx-ribbons", showWallpaper: true } },
  },
  {
    id: "underwave",
    name: "Underwave",
    theme: "nebula",
    patch: { appearance: { theme: "nebula", colorMode: "dark", interfaceMode: "custom", opacity: 0.86, blur: 12, elementsBackground: 30, vignette: 0.4, cornerStyle: "round", roundness: 14, wallpaper: "haku-nebula", showWallpaper: true }, sounds: { musicTrack: "chill" } },
  },
  {
    id: "doom",
    name: "Doom",
    theme: "solar-flare",
    patch: { appearance: { theme: "solar-flare", colorMode: "dark", interfaceMode: "custom", opacity: 1, blur: 0, vignette: 0.85, cornerStyle: "sharp", roundness: 2, crosshairCursor: true, wallpaper: "neon-grid" }, sounds: { musicTrack: "doom" } },
  },
  {
    id: "haku",
    name: "Haku",
    theme: "haku",
    patch: { appearance: { theme: "haku", colorMode: "dark", interfaceMode: "auto", cornerStyle: "round", roundness: 10, wallpaper: "haku-tide", showWallpaper: true } },
  },
  {
    id: "soft-light",
    name: "Soft Light",
    theme: "sakura",
    patch: { appearance: { theme: "sakura", colorMode: "light", interfaceMode: "auto", cornerStyle: "round", roundness: 12, wallpaper: "haku-petals", showWallpaper: true } },
  },
];

export function Section({ title, children, extra }: { title: string; children: ReactNode; extra?: ReactNode }) {
  return (
    <section className="drawer-section">
      <div className="drawer-section-head">
        <h3 className="display">{title}</h3>
        <span className="grow" />
        {extra}
      </div>
      {children}
    </section>
  );
}

export function Line({ label, children, beta, description }: { label: ReactNode; children: ReactNode; beta?: boolean; description?: ReactNode }) {
  return (
    <div className="setting-line">
      <div className="setting-line-text">
        <span>
          {label} {beta && <span className="chip accent beta">BETA</span>}
        </span>
        {description && <small className="muted">{description}</small>}
      </div>
      {children}
    </div>
  );
}

export function ThemeCard({ theme, active, dark, onPick }: { theme: ThemeInfo; active: boolean; dark: boolean; onPick: () => void }) {
  const accent = dark ? theme.darkAccent : theme.lightAccent;
  const bg = dark ? theme.darkBackground : theme.lightBackground;
  return (
    <button className={clsx("theme-card notch", active && "active")} style={{ "--card-accent": accent, "--card-bg": bg } as React.CSSProperties} onClick={onPick} title={theme.name}>
      <span className="theme-card-preview">
        <span />
        <span />
        <span />
      </span>
      <span className="theme-card-name">
        <span className="ellipsis grow">{theme.name}</span>
        <span className={clsx("radio-dot", active && "on")} />
      </span>
    </button>
  );
}

/**
 * GX's theme carousel: a page of theme cards with arrows and a position bar. It pages instead
 * of scrolling because wide overflowing rows stall Servo's rendering updates (see AGENTS.md).
 */
export function ThemeCarousel({ perPage }: { perPage: number }) {
  const themes = useStore((s) => s.themes);
  const current = useStore((s) => s.settings?.appearance.theme);
  const dark = useStore((s) => s.palette?.dark ?? true);
  const pages = Math.max(1, Math.ceil(themes.length / perPage));
  const activeIndex = Math.max(0, themes.findIndex((t) => t.id === current));
  const [page, setPage] = useState(() => Math.floor(activeIndex / perPage));
  const shown = Math.min(page, pages - 1);
  const visible = themes.slice(shown * perPage, shown * perPage + perPage);
  return (
    <div className="theme-carousel">
      <div className="theme-carousel-row">
        <button className="carousel-arrow" title="Previous themes" disabled={shown === 0} onClick={() => setPage(shown - 1)}>
          <Icon name="chevron-left" size={16} />
        </button>
        <div className="theme-carousel-page" style={{ gridTemplateColumns: `repeat(${perPage}, minmax(0, 1fr))` }}>
          {visible.map((t) => (
            <ThemeCard key={t.id} theme={t} dark={dark} active={current === t.id} onPick={() => patch({ appearance: { theme: t.id, primaryColor: null, secondaryColor: null } })} />
          ))}
        </div>
        <button className="carousel-arrow" title="More themes" disabled={shown >= pages - 1} onClick={() => setPage(shown + 1)}>
          <Icon name="chevron-right" size={16} />
        </button>
      </div>
      <div className="carousel-track" aria-hidden>
        <span style={{ width: `${100 / pages}%`, marginLeft: `${(shown * 100) / pages}%` }} />
      </div>
    </div>
  );
}

export function EasySetup() {
  const open = useStore((s) => s.drawer === "easy-setup");
  const settings = useStore((s) => s.settings);
  const themes = useStore((s) => s.themes);
  const setDrawer = useStore((s) => s.setDrawer);
  if (!open || !settings) return null;
  const a = settings.appearance;
  const shuffle = () => {
    const pool = themes.filter((t) => t.id !== a.theme);
    const pick = pool[Math.floor(Math.random() * pool.length)];
    if (pick) patch({ appearance: { theme: pick.id, primaryColor: null, secondaryColor: null } });
  };
  return (
    <aside className="drawer easy-setup" onMouseDown={(e) => e.stopPropagation()}>
      <header className="drawer-head">
        <h2 className="display accent">Easy Setup</h2>
        <span className="grow" />
        <button className="icon-btn" title="All settings" onClick={() => { openInternal("haku://settings/"); setDrawer(null); }}>
          <Icon name="settings" size={16} />
        </button>
        <button className="icon-btn" title="Close" onClick={() => setDrawer(null)}>
          <Icon name="x" size={16} />
        </button>
      </header>
      <div className="drawer-body">
        <Section title="Presets" extra={<button className="link-btn" onClick={() => openInternal("haku://mods/")}>Show all</button>}>
          <div className="preset-grid">
            {PRESETS.map((p) => {
              const t = themes.find((x) => x.id === p.theme);
              return (
                <button key={p.id} className="preset-tile notch" style={{ "--card-accent": t?.darkAccent ?? "var(--hx-accent)" } as React.CSSProperties} onClick={() => patch(p.patch)}>
                  <span className="display">{p.name}</span>
                </button>
              );
            })}
          </div>
        </Section>

        <Section
          title="Themes"
          extra={
            <>
              <button className="icon-btn" title="Shuffle" onClick={shuffle}>
                <Icon name="shuffle" size={15} />
              </button>
              <button className="link-btn" onClick={() => openInternal("haku://settings/themes")}>
                Show all
              </button>
            </>
          }
        >
          <ThemeCarousel perPage={3} />
          <Segmented wide value={a.colorMode} options={[{ value: "light", label: "Light" }, { value: "auto", label: "Auto" }, { value: "dark", label: "Dark" }]} onChange={(v) => patch({ appearance: { colorMode: v } })} />
        </Section>

        <Section title="Wallpapers" extra={<Toggle on={a.showWallpaper} onChange={(v) => patch({ appearance: { showWallpaper: v } })} />}>
          <Radio checked={a.wallpaper !== "custom"} onChange={() => patch({ appearance: { wallpaper: a.wallpaper === "custom" ? "gx-ribbons" : a.wallpaper } })} label="GX wallpaper" />
          {a.wallpaper !== "custom" && (
            <Select value={a.wallpaper} options={WALLPAPERS.map((w) => ({ value: w.id, label: w.name }))} onChange={(v) => patch({ appearance: { wallpaper: v } })} />
          )}
          <Radio checked={a.wallpaper === "custom"} onChange={() => send({ type: "pick-wallpaper" })} label="Custom" description="Choose an image from your computer" />
        </Section>

        <Section title="Appearance">
          <Line label="Interface">
            <Select value={a.interfaceMode} options={[{ value: "auto", label: "Auto" }, { value: "custom", label: "Custom" }]} onChange={(v) => patch({ appearance: { interfaceMode: v } })} width={130} />
          </Line>
          {a.interfaceMode === "custom" && (
            <div className="custom-sliders">
              <Line label="Opacity">
                <Slider value={Math.round(a.opacity * 100)} min={20} max={100} onCommit={(v) => patch({ appearance: { opacity: v / 100 } })} label="Opacity" />
              </Line>
              <Line label="Wallpaper blur">
                <Slider value={a.blur} min={0} max={40} onCommit={(v) => patch({ appearance: { blur: v } })} label="Blur" />
              </Line>
              <Line label="Elements background">
                <Slider value={a.elementsBackground} min={0} max={100} onCommit={(v) => patch({ appearance: { elementsBackground: v } })} label="Elements background" />
              </Line>
              <Line label="Vignette">
                <Slider value={Math.round(a.vignette * 100)} min={0} max={100} onCommit={(v) => patch({ appearance: { vignette: v / 100 } })} label="Vignette" />
              </Line>
              <Line label="Focus mode" description="Dims the browser frame so the page stands out">
                <Toggle on={a.focusMode} onChange={(v) => patch({ appearance: { focusMode: v } })} />
              </Line>
            </div>
          )}
          <Line label="Show window decoration">
            <Select
              value={a.windowDecoration}
              options={[{ value: "outside", label: "Outside" }, { value: "outside-except-maximized", label: "Outside except maximized" }, { value: "inside", label: "Inside" }]}
              onChange={(v) => patch({ appearance: { windowDecoration: v } })}
              width={200}
            />
          </Line>
          <Line label="Use GX messenger icons">
            <Toggle on={a.gxMessengerIcons} onChange={(v) => patch({ appearance: { gxMessengerIcons: v } })} />
          </Line>
          <Line label="Colorize icons using primary color">
            <Toggle on={a.colorizeIcons} onChange={(v) => patch({ appearance: { colorizeIcons: v } })} />
          </Line>
          <Line label="Show bookmarks bar">
            <Toggle on={a.showBookmarksBar} onChange={(v) => patch({ appearance: { showBookmarksBar: v } })} />
          </Line>
        </Section>

        <Section title="GX features">
          <Line label="Browser sounds">
            <Toggle on={settings.sounds.browserSounds} onChange={(v) => patch({ sounds: { browserSounds: v } })} />
          </Line>
          <Line label="Keyboard sounds">
            <Toggle on={settings.sounds.keyboardSounds} onChange={(v) => patch({ sounds: { keyboardSounds: v } })} />
          </Line>
          <Line label="Background music">
            <Toggle on={settings.sounds.backgroundMusic} onChange={(v) => patch({ sounds: { backgroundMusic: v } })} />
          </Line>
          <Line label="Show sidebar">
            <Toggle on={settings.sidebar.show} onChange={(v) => patch({ sidebar: { show: v } })} />
          </Line>
          <Line label="Force dark pages" beta>
            <Toggle on={settings.features.forceDarkPages} onChange={(v) => patch({ features: { forceDarkPages: v } })} />
          </Line>
          <Line label="Shaders">
            <Toggle on={settings.features.shadersEnabled} onChange={(v) => patch({ features: { shadersEnabled: v } })} />
          </Line>
          <Line label="Web modding">
            <Toggle on={settings.features.webModding} onChange={(v) => patch({ features: { webModding: v } })} />
          </Line>
          <Line label="Splash screen on startup">
            <Toggle on={a.splashScreen} onChange={(v) => patch({ appearance: { splashScreen: v } })} />
          </Line>
        </Section>

        <Section title="Early Bird">
          <p className="muted">Try features that are still being built.</p>
          <Button kind="accent" onClick={() => { openInternal("haku://settings/early-bird"); setDrawer(null); }}>
            Enable in Settings
          </Button>
        </Section>

        <Section title="Start page">
          <Line label="Speed Dial position">
            <Select
              value={settings.startPage.dialPosition}
              options={[{ value: "auto", label: "Auto" }, { value: "top", label: "Top" }, { value: "center", label: "Center" }, { value: "bottom", label: "Bottom" }, { value: "left", label: "Left" }]}
              onChange={(v) => patch({ startPage: { dialPosition: v } })}
              width={130}
            />
          </Line>
          <Line label="Speed Dial effect">
            <Select
              value={settings.startPage.dialEffect}
              options={[{ value: "none", label: "None" }, { value: "skew", label: "Skew" }, { value: "glow", label: "Glow" }, { value: "tilt", label: "Tilt" }]}
              onChange={(v) => patch({ startPage: { dialEffect: v } })}
              width={130}
            />
          </Line>
          <Line label="Show Speed Dial names">
            <Toggle on={settings.startPage.showDialNames} onChange={(v) => patch({ startPage: { showDialNames: v } })} />
          </Line>
          <Line label="Use bigger Speed Dial tiles">
            <Toggle on={settings.startPage.biggerTiles} onChange={(v) => patch({ startPage: { biggerTiles: v } })} />
          </Line>
          <Line label="Show Speed Dial suggestions">
            <Toggle on={settings.startPage.dialSuggestions} onChange={(v) => patch({ startPage: { dialSuggestions: v } })} />
          </Line>
          <Line label="Show news">
            <Toggle on={settings.startPage.showNews} onChange={(v) => patch({ startPage: { showNews: v } })} />
          </Line>
          <Line label="Game strips">
            <Toggle on={settings.startPage.gameStrips} onChange={(v) => patch({ startPage: { gameStrips: v } })} />
          </Line>
        </Section>

        <Section title="Privacy & security">
          <Line label="Block ads">
            <Toggle on={settings.privacy.blockAds} onChange={(v) => patch({ privacy: { blockAds: v } })} />
          </Line>
          <Line label="Block trackers">
            <Toggle on={settings.privacy.blockTrackers} onChange={(v) => patch({ privacy: { blockTrackers: v } })} />
          </Line>
          <Line label="VPN / proxy" description="Use your own proxy or VPN provider">
            <Button onClick={() => { openInternal("haku://settings/vpn"); setDrawer(null); }}>Enable in Settings</Button>
          </Line>
          <Line label="Browsing data">
            <Button icon="trash" onClick={() => useStore.getState().openModal({ kind: "clear-data" })}>
              Clear
            </Button>
          </Line>
        </Section>

        <Section title="AI services" extra={<Toggle on={settings.ai.enabled} onChange={(v) => patch({ ai: { enabled: v } })} />}>
          <Line label="AI button in the toolbar">
            <Toggle on={settings.ai.toolbarButton} onChange={(v) => patch({ ai: { toolbarButton: v } })} disabled={!settings.ai.enabled} />
          </Line>
          <Line label="AI prompts in text highlight popup">
            <Toggle on={settings.ai.highlightPrompts} onChange={(v) => patch({ ai: { highlightPrompts: v } })} disabled={!settings.ai.enabled} />
          </Line>
        </Section>
      </div>
    </aside>
  );
}

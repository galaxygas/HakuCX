/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Settings sub-pages: the detail screens behind "›" rows.

import { useState } from "react";
import { Button, Select, Slider, Toggle } from "../../components/controls";
import { Icon } from "../../components/Icon";
import { send } from "../../ipc/bridge";
import type { FilterList } from "../../ipc/generated/FilterList";
import type { LightLink } from "../../ipc/generated/LightLink";
import type { PermissionSetting } from "../../ipc/generated/PermissionSetting";
import type { SearchEngine } from "../../ipc/generated/SearchEngine";
import { BROWSER_SOUNDS, ENGINE_PERMISSIONS, KEYBOARD_PACKS, PERMISSION_LABELS, SHORTCUT_LABELS, SOUND_PACKS } from "../../lib/catalog";
import { clsx, dayLabel, hostOf, patch } from "../../lib/util";
import { useStore } from "../../state/store";
import { go } from "../InternalPage";
import { Input } from "../../components/Field";

function Line({ label, description, children, warn }: { label: string; description?: string; children?: React.ReactNode; warn?: string }) {
  return (
    <div className="setting-line">
      <div className="setting-line-text">
        <span>{label}</span>
        {description && <small className="muted">{description}</small>}
        {warn && <small className="warn-text">{warn}</small>}
      </div>
      {children}
    </div>
  );
}

function SliderLine({ label, value, min, max, onCommit, format, disabled }: { label: string; value: number; min: number; max: number; onCommit: (v: number) => void; format?: (v: number) => string; disabled?: boolean }) {
  return (
    <div className="setting-line slider-line">
      <div className="setting-line-text">
        <span>{label}</span>
      </div>
      <div className="slider-wrap">
        <Slider value={value} min={min} max={max} onCommit={onCommit} label={label} disabled={disabled} />
        <span className="slider-value">{format ? format(value) : value}</span>
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ sounds

export function BrowserSoundsPage() {
  const sounds = useStore((s) => s.settings?.sounds);
  if (!sounds) return null;
  return (
    <>
      <div className="settings-card card">
        <Line label="Browser sounds" description="Short sounds for tabs, clicks, downloads and limiters">
          <Toggle on={sounds.browserSounds} onChange={(v) => patch({ sounds: { browserSounds: v } })} label="Browser sounds" />
        </Line>
        <Line label="Sound pack">
          <Select value={sounds.browserPack} options={SOUND_PACKS.map((p) => ({ value: p.id, label: p.name }))} onChange={(v) => patch({ sounds: { browserPack: v } })} />
        </Line>
        <SliderLine label="Volume" value={sounds.browserVolume} min={0} max={100} onCommit={(v) => patch({ sounds: { browserVolume: v } })} format={(v) => `${v}%`} />
      </div>
      <h3 className="display section-label">Sounds</h3>
      <div className="settings-card card">
        {BROWSER_SOUNDS.map((sound) => {
          const on = sounds.enabled[sound.id] ?? true;
          const level = sounds.levels[sound.id] ?? 100;
          return (
            <div key={sound.id} className="setting-line sound-line">
              <Toggle on={on} onChange={(v) => patch({ sounds: { enabled: { ...sounds.enabled, [sound.id]: v } } })} label={sound.label} disabled={!sounds.browserSounds} />
              <span className="sound-name">{sound.label}</span>
              <div className="slider-wrap">
                <Slider value={level} min={0} max={100} onCommit={(v) => patch({ sounds: { levels: { ...sounds.levels, [sound.id]: v } } })} label={`${sound.label} volume`} disabled={!sounds.browserSounds || !on} />
                <span className="slider-value">{level}</span>
              </div>
              <Button kind="ghost" icon="play" title={`Play ${sound.label}`} disabled={!sounds.browserSounds || !on} onClick={() => send({ type: "play-sound", sound: sound.id })} />
            </div>
          );
        })}
      </div>
    </>
  );
}

export function KeyboardSoundsPage() {
  const sounds = useStore((s) => s.settings?.sounds);
  const [sample, setSample] = useState("");
  if (!sounds) return null;
  return (
    <div className="settings-card card">
      <Line label="Keyboard sounds" description="A key sound each time you type, in the browser and on pages">
        <Toggle on={sounds.keyboardSounds} onChange={(v) => patch({ sounds: { keyboardSounds: v } })} label="Keyboard sounds" />
      </Line>
      <Line label="Sound pack">
        <Select value={sounds.keyboardPack} options={KEYBOARD_PACKS.map((p) => ({ value: p.id, label: p.name }))} onChange={(v) => patch({ sounds: { keyboardPack: v } })} />
      </Line>
      <SliderLine label="Volume" value={sounds.keyboardVolume} min={0} max={100} onCommit={(v) => patch({ sounds: { keyboardVolume: v } })} format={(v) => `${v}%`} />
      <Line label="Try it">
        <Input className="text-input" style={{ width: 240 }} value={sample} placeholder="Type here to hear the pack" onChange={(e) => setSample(e.target.value)} />
      </Line>
    </div>
  );
}

// ------------------------------------------------------------------ GX Lights

const LINK_TEXT: Record<string, string> = { off: "Off", connecting: "Connecting...", connected: "Connected", unavailable: "Not available" };

function LinkStatus({ link, connected }: { link: LightLink | undefined; connected: string }) {
  if (!link || link.state === "off") return null;
  const text = link.state === "connected" ? connected : link.message || LINK_TEXT[link.state] || link.state;
  return <small className={link.state === "unavailable" ? "warn-text" : "muted"}>{text}</small>;
}

export function LightsPage() {
  const lights = useStore((s) => s.settings?.lights);
  const status = useStore((s) => s.lights);
  if (!lights) return null;
  const off = !lights.enabled;
  const openrgbDevices = status?.openrgb.devices ?? [];
  return (
    <>
      <div className="settings-card card">
        <Line label="GX Lights" description="Light your keyboard, mouse and other RGB devices in your theme's colors, with flashes when tabs open, downloads finish or a limiter turns on.">
          <Toggle on={lights.enabled} onChange={(v) => patch({ lights: { enabled: v } })} label="GX Lights" />
        </Line>
        <div className="setting-line">
          <div className="setting-line-text">
            <span>Razer Chroma</span>
            <small className="muted">Razer devices, through Razer Synapse's Chroma SDK on this computer. Synapse takes the lights back when HakuCX closes.</small>
            {!off && lights.razerChroma && <LinkStatus link={status?.razer} connected="Connected to Razer Chroma" />}
          </div>
          <Toggle on={lights.razerChroma} onChange={(v) => patch({ lights: { razerChroma: v } })} label="Razer Chroma" disabled={off} />
        </div>
        <div className="setting-line">
          <div className="setting-line-text">
            <span>OpenRGB</span>
            <small className="muted">ASUS, Corsair, Logitech, MSI, SteelSeries and more, through OpenRGB with its SDK server turned on. Each device gets its own effect back when HakuCX closes.</small>
            {!off && lights.openrgb && (
              <LinkStatus link={status?.openrgb} connected={openrgbDevices.length ? `Lighting ${openrgbDevices.join(", ")}` : "Connected; OpenRGB reports no devices"} />
            )}
          </div>
          <Toggle on={lights.openrgb} onChange={(v) => patch({ lights: { openrgb: v } })} label="OpenRGB" disabled={off} />
        </div>
        <Line label="OpenRGB server" description="Address and port of the OpenRGB SDK server (6742 unless you changed it)">
          <div className="row">
            <input className="text-input" style={{ width: 150 }} defaultValue={lights.openrgbHost} disabled={off || !lights.openrgb} onBlur={(e) => patch({ lights: { openrgbHost: e.target.value.trim() || "127.0.0.1" } })} />
            <input className="text-input" style={{ width: 80 }} type="number" min={1} max={65535} defaultValue={lights.openrgbPort} disabled={off || !lights.openrgb} onBlur={(e) => patch({ lights: { openrgbPort: Number(e.target.value) || 6742 } })} />
          </div>
        </Line>
        <Line label="Connect again" description="HakuCX retries on its own every 15 seconds">
          <Button icon="refresh" disabled={off} onClick={() => send({ type: "lights-reconnect" })}>
            Reconnect
          </Button>
        </Line>
      </div>
      <div className="settings-card card">
        <SliderLine label="Brightness" value={lights.brightness} min={0} max={100} onCommit={(v) => patch({ lights: { brightness: v } })} format={(v) => `${v}%`} disabled={off} />
        <Line label="Animation set" description="How the lights look while nothing is happening">
          <Select
            value={lights.animationSet}
            options={[
              { value: "gx", label: "GX (slow color drift)" },
              { value: "pulse", label: "Pulse (breathing)" },
              { value: "wave", label: "Wave (a band runs across)" },
              { value: "static", label: "Static" },
            ]}
            onChange={(v) => patch({ lights: { animationSet: v } })}
            disabled={off}
          />
        </Line>
        <Line label="Browser event effects" description="Flash when HakuCX starts, a tab opens, a download finishes, a site sends a notification or a limiter turns on">
          <Toggle on={lights.animations} onChange={(v) => patch({ lights: { animations: v } })} label="Browser event effects" disabled={off} />
        </Line>
        <div className="setting-line">
          <div className="setting-line-text">
            <span>Music visualizer</span>
            <small className="muted">The lights follow how loud your computer is playing (the level Windows' volume mixer shows; no sound is recorded).</small>
            {!off && lights.musicVisualizer && status && !status.meter && <small className="warn-text">No sound output device found.</small>}
          </div>
          <Toggle on={lights.musicVisualizer} onChange={(v) => patch({ lights: { musicVisualizer: v } })} label="Music visualizer" disabled={off} />
        </div>
        <Line label="Dim lights in fullscreen" description="Lower the lights while a video or page is fullscreen">
          <Toggle on={lights.dimInFullscreen} onChange={(v) => patch({ lights: { dimInFullscreen: v } })} label="Dim lights in fullscreen" disabled={off} />
        </Line>
      </div>
    </>
  );
}

// ------------------------------------------------------------------ panic button

export function PanicPage() {
  const panic = useStore((s) => s.settings?.panic);
  const [capturing, setCapturing] = useState(false);
  if (!panic) return null;
  return (
    <div className="settings-card card">
      <Line label="Panic button" description="One key hides every tab behind a boring page. Press it again to come back.">
        <Toggle on={panic.enabled} onChange={(v) => patch({ panic: { enabled: v } })} label="Panic button" />
      </Line>
      <Line label="Shortcut">
        <button
          className={clsx("key-capture", capturing && "capturing")}
          onClick={() => {
            setCapturing(true);
            send({ type: "key-capture", on: true });
          }}
          onBlur={() => {
            setCapturing(false);
            send({ type: "key-capture", on: false });
          }}
          onKeyDown={(e) => {
            if (!capturing) return;
            e.preventDefault();
            const acc = accelerator(e);
            if (acc) {
              patch({ panic: { key: acc } });
              e.currentTarget.blur();
            }
          }}
        >
          {capturing ? "Press a key..." : panic.key}
        </button>
      </Line>
      <Line label="Sound" description="While the panic page shows, your tabs are unloaded, so nothing keeps playing; they load again when you press the key a second time." />
      <div className="setting-block">
        <div className="setting-line-text">
          <span>Pages to open</span>
          <small className="muted">One address per line. HakuCX picks one at random.</small>
        </div>
        <textarea
          className="text-input"
          rows={5}
          defaultValue={panic.urls.join("\n")}
          onBlur={(e) => patch({ panic: { urls: e.target.value.split("\n").map((l) => l.trim()).filter((l) => /^https?:\/\//.test(l)) } })}
        />
      </div>
      <Button onClick={() => send({ type: "panic" })}>Try it now</Button>
    </div>
  );
}

/** A key event as an accelerator string the Rust side parses (`Ctrl+Shift+K`, `F12`). */
export function accelerator(e: React.KeyboardEvent): string | null {
  if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return null;
  const named: Record<string, string> = { ArrowLeft: "Left", ArrowRight: "Right", ArrowUp: "Up", ArrowDown: "Down", " ": "Space", Escape: "Escape" };
  let key = named[e.key] ?? (e.key.length === 1 ? e.key.toUpperCase() : e.key);
  if (e.code.startsWith("Digit")) key = e.code.slice(5);
  const parts = [e.ctrlKey && "Ctrl", e.shiftKey && "Shift", e.altKey && "Alt"].filter(Boolean) as string[];
  return [...parts, key].join("+");
}

// ------------------------------------------------------------------ shortcuts

export function ShortcutsPage() {
  const shortcuts = useStore((s) => s.settings?.shortcuts);
  const [editing, setEditing] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  if (!shortcuts) return null;
  const set = (id: string, acc: string | null) => {
    const overrides = { ...shortcuts.overrides };
    if (acc === null) delete overrides[id];
    else overrides[id] = acc;
    patch({ shortcuts: { overrides } });
  };
  const bindings = SHORTCUT_LABELS.map((s) => ({ ...s, binding: shortcuts.overrides[s.id] ?? s.default }));
  const conflicts = new Map<string, number>();
  for (const b of bindings) if (b.binding) conflicts.set(b.binding.toLowerCase(), (conflicts.get(b.binding.toLowerCase()) ?? 0) + 1);
  const q = filter.trim().toLowerCase();
  return (
    <>
      <div className="row">
        <div className="search-box" style={{ flex: 1 }}>
          <Icon name="search" size={15} />
          <Input placeholder="Search shortcuts" value={filter} onChange={(e) => setFilter(e.target.value)} />
        </div>
        <Button kind="ghost" onClick={() => patch({ shortcuts: { overrides: {} } })}>
          Reset all
        </Button>
      </div>
      <div className="settings-card card">
        {bindings
          .filter((b) => !q || b.label.toLowerCase().includes(q) || b.binding.toLowerCase().includes(q))
          .map((b) => {
            const changed = b.id in shortcuts.overrides;
            const clash = b.binding && (conflicts.get(b.binding.toLowerCase()) ?? 0) > 1;
            return (
              <div key={b.id} className="setting-line">
                <div className="setting-line-text">
                  <span>{b.label}</span>
                  {clash && <small className="warn-text">Used by another shortcut too</small>}
                </div>
                <div className="row">
                  <button
                    className={clsx("key-capture", editing === b.id && "capturing", changed && "changed")}
                    onClick={() => {
                      setEditing(b.id);
                      send({ type: "key-capture", on: true });
                    }}
                    onBlur={() => {
                      setEditing(null);
                      send({ type: "key-capture", on: false });
                    }}
                    onKeyDown={(e) => {
                      if (editing !== b.id) return;
                      e.preventDefault();
                      if (e.key === "Escape") return e.currentTarget.blur();
                      if (e.key === "Backspace" || e.key === "Delete") {
                        set(b.id, "");
                        return e.currentTarget.blur();
                      }
                      const acc = accelerator(e);
                      if (acc) {
                        set(b.id, acc === b.default ? null : acc);
                        e.currentTarget.blur();
                      }
                    }}
                  >
                    {editing === b.id ? "Press keys (Backspace clears)" : b.binding || "None"}
                  </button>
                  {changed && <Button kind="ghost" icon="undo" title="Restore default" onClick={() => set(b.id, null)} />}
                </div>
              </div>
            );
          })}
      </div>
    </>
  );
}

// ------------------------------------------------------------------ site settings

const ENFORCED_ELSEWHERE: Record<string, string> = {
  popups: "Pop-ups are controlled by the pop-up blocker and its per-site exceptions",
  "javascript-jit": "Set for every site in Privacy & security",
};

function permissionNote(kind: string): string | undefined {
  if (ENFORCED_ELSEWHERE[kind]) return ENFORCED_ELSEWHERE[kind];
  if (!ENGINE_PERMISSIONS.has(kind)) return "The engine never asks for this, so the setting has no effect yet";
  return undefined;
}

const SETTING_OPTIONS: { value: PermissionSetting; label: string }[] = [
  { value: "ask", label: "Ask" },
  { value: "allow", label: "Allow" },
  { value: "block", label: "Block" },
];

export function SiteSettingsPage() {
  const permissions = useStore((s) => s.settings?.permissions);
  const privacy = useStore((s) => s.settings?.privacy);
  const [showAll, setShowAll] = useState(false);
  if (!permissions || !privacy) return null;
  const kinds = Object.keys(PERMISSION_LABELS).filter((k) => k in permissions.defaults || ENGINE_PERMISSIONS.has(k));
  const shown = showAll ? kinds : kinds.filter((k) => ENGINE_PERMISSIONS.has(k));
  const sites = Object.keys(permissions.sites).filter((o) => Object.keys(permissions.sites[o] ?? {}).length > 0);
  return (
    <>
      <h3 className="display section-label">Sites with their own settings</h3>
      <div className="settings-card card">
        {sites.length === 0 && <p className="muted">When you allow or block something for a site, it is listed here.</p>}
        {sites.map((origin) => (
          <button key={origin} className="setting-line link-line" onClick={() => go(`haku://settings/site?origin=${encodeURIComponent(origin)}`)}>
            <Icon name="globe" size={16} />
            <div className="setting-line-text">
              <span>{hostOf(origin) || origin}</span>
              <small className="muted">{Object.keys(permissions.sites[origin] ?? {}).length} settings</small>
            </div>
            <Icon name="chevron-right" size={16} />
          </button>
        ))}
      </div>
      <h3 className="display section-label">Permissions</h3>
      <div className="settings-card card">
        <Line label="Use quieter messaging" description="Permission requests show as a small icon instead of a prompt">
          <Toggle on={permissions.quietRequests} onChange={(v) => patch({ permissions: { quietRequests: v } })} label="Quieter messaging" />
        </Line>
        {shown.map((kind) => (
          <Line key={kind} label={PERMISSION_LABELS[kind] ?? kind} warn={permissionNote(kind)}>
            <Select
              value={permissions.defaults[kind] ?? "ask"}
              options={SETTING_OPTIONS}
              onChange={(v) => patch({ permissions: { defaults: { ...permissions.defaults, [kind]: v } } })}
              width={120}
            />
          </Line>
        ))}
        <Button kind="ghost" onClick={() => setShowAll((v) => !v)}>
          {showAll ? "Show only permissions the engine uses" : "Show all permissions"}
        </Button>
      </div>
      <h3 className="display section-label">Pop-ups allowed on</h3>
      <div className="settings-card card">
        <HostList list={privacy.popupAllowlist} onChange={(popupAllowlist) => patch({ privacy: { popupAllowlist } })} placeholder="example.com" />
      </div>
    </>
  );
}

function HostList({ list, onChange, placeholder }: { list: string[]; onChange: (l: string[]) => void; placeholder: string }) {
  const [text, setText] = useState("");
  const add = () => {
    const host = hostOf(text.includes("://") ? text : `https://${text}`).replace(/^www\./, "");
    if (host && !list.includes(host)) onChange([...list, host]);
    setText("");
  };
  return (
    <>
      {list.map((h) => (
        <div key={h} className="setting-line">
          <span>{h}</span>
          <Button kind="ghost" icon="x" title={`Remove ${h}`} onClick={() => onChange(list.filter((x) => x !== h))} />
        </div>
      ))}
      <div className="row">
        <Input className="text-input" style={{ flex: 1 }} value={text} placeholder={placeholder} onChange={(e) => setText(e.target.value)} onKeyDown={(e) => e.key === "Enter" && add()} />
        <Button onClick={add} disabled={!text.trim()}>
          Add
        </Button>
      </div>
    </>
  );
}

export function SitePage({ origin }: { origin: string }) {
  const permissions = useStore((s) => s.settings?.permissions);
  const privacy = useStore((s) => s.settings?.privacy);
  if (!permissions || !privacy) return null;
  if (!origin) return <p className="muted">No site selected.</p>;
  const site = permissions.sites[origin] ?? {};
  const host = hostOf(origin).replace(/^www\./, "");
  const setKind = (kind: string, value: string) => {
    const next = { ...site };
    if (value === "default") delete next[kind];
    else next[kind] = value as PermissionSetting;
    patch({ permissions: { sites: { ...permissions.sites, [origin]: next } } });
  };
  const popupsAllowed = privacy.popupAllowlist.includes(host);
  const adsOff = privacy.adExceptions.includes(host);
  return (
    <>
      <div className="row">
        <Icon name="globe" size={18} />
        <h3 className="display">{origin}</h3>
        <span className="grow" />
        <Button
          kind="danger"
          onClick={() => {
            const sites = { ...permissions.sites };
            delete sites[origin];
            patch({ permissions: { sites } });
          }}
        >
          Reset permissions
        </Button>
      </div>
      <div className="settings-card card">
        <Line label="Pop-ups and redirects">
          <Toggle on={popupsAllowed} onChange={(v) => patch({ privacy: { popupAllowlist: v ? [...privacy.popupAllowlist, host] : privacy.popupAllowlist.filter((h) => h !== host) } })} label="Allow pop-ups" />
        </Line>
        <Line label="Block ads and trackers on this site">
          <Toggle on={!adsOff} onChange={(v) => setBlockingOff(host, !v)} label="Block ads on this site" />
        </Line>
        {Object.keys(PERMISSION_LABELS)
          .filter((k) => ENGINE_PERMISSIONS.has(k))
          .map((kind) => {
            const def = permissions.defaults[kind] ?? "ask";
            return (
              <Line key={kind} label={PERMISSION_LABELS[kind] ?? kind}>
                <Select
                  value={site[kind] ?? "default"}
                  options={[{ value: "default", label: `Default (${def[0]!.toUpperCase()}${def.slice(1)})` }, ...SETTING_OPTIONS]}
                  onChange={(v) => setKind(kind, v)}
                  width={170}
                />
              </Line>
            );
          })}
      </div>
    </>
  );
}

// ------------------------------------------------------------------ search engines

export function SearchEnginesPage() {
  const search = useStore((s) => s.settings?.search);
  const [adding, setAdding] = useState(false);
  if (!search) return null;
  const save = (engines: SearchEngine[]) => patch({ search: { engines } });
  const update = (id: string, change: Partial<SearchEngine>) => save(search.engines.map((e) => (e.id === id ? { ...e, ...change } : e)));
  return (
    <>
      <div className="settings-card card">
        <div className="engine-row engine-head muted">
          <span>Name</span>
          <span>Shortcut</span>
          <span>URL with %s in place of query</span>
          <span />
        </div>
        {search.engines.map((engine) => (
          <EngineRow
            key={engine.id}
            engine={engine}
            isDefault={search.defaultEngine === engine.id}
            onChange={(c) => update(engine.id, c)}
            onDefault={() => patch({ search: { defaultEngine: engine.id } })}
            onDelete={() => save(search.engines.filter((e) => e.id !== engine.id))}
          />
        ))}
      </div>
      {adding ? (
        <NewEngine
          taken={search.engines.map((e) => e.keyword)}
          onCancel={() => setAdding(false)}
          onAdd={(engine) => {
            save([...search.engines, engine]);
            setAdding(false);
          }}
        />
      ) : (
        <Button icon="plus" onClick={() => setAdding(true)}>
          Add search engine
        </Button>
      )}
    </>
  );
}

function EngineRow({ engine, isDefault, onChange, onDefault, onDelete }: { engine: SearchEngine; isDefault: boolean; onChange: (c: Partial<SearchEngine>) => void; onDefault: () => void; onDelete: () => void }) {
  return (
    <div className={clsx("engine-row", !engine.active && "inactive")}>
      <input className="text-input" defaultValue={engine.name} onBlur={(e) => e.target.value.trim() && e.target.value !== engine.name && onChange({ name: e.target.value.trim() })} />
      <input className="text-input" defaultValue={engine.keyword} onBlur={(e) => e.target.value !== engine.keyword && onChange({ keyword: e.target.value.trim() })} />
      <input className="text-input" defaultValue={engine.url} onBlur={(e) => e.target.value.includes("%s") && e.target.value !== engine.url && onChange({ url: e.target.value.trim() })} />
      <div className="row">
        {isDefault ? (
          <span className="chip accent">Default</span>
        ) : (
          <>
            <Toggle on={engine.active} onChange={(active) => onChange({ active })} label={`Use ${engine.name}`} />
            <Button kind="ghost" onClick={onDefault} disabled={!engine.active}>
              Make default
            </Button>
            <Button kind="ghost" icon="trash" title={`Delete ${engine.name}`} onClick={onDelete} />
          </>
        )}
      </div>
    </div>
  );
}

function NewEngine({ taken, onAdd, onCancel }: { taken: string[]; onAdd: (e: SearchEngine) => void; onCancel: () => void }) {
  const [name, setName] = useState("");
  const [keyword, setKeyword] = useState("");
  const [url, setUrl] = useState("");
  const error = !name.trim()
    ? "Give it a name"
    : keyword && taken.includes(keyword.trim())
      ? "That shortcut is taken"
      : !/^https?:\/\/.+%s/.test(url.trim())
        ? "The URL must start with http(s):// and contain %s"
        : null;
  return (
    <div className="settings-card card">
      <div className="engine-row">
        <Input className="text-input" placeholder="Name" value={name} onChange={(e) => setName(e.target.value)} />
        <Input className="text-input" placeholder="Shortcut" value={keyword} onChange={(e) => setKeyword(e.target.value)} />
        <Input className="text-input" placeholder="https://example.com/search?q=%s" value={url} onChange={(e) => setUrl(e.target.value)} />
        <div className="row">
          <Button kind="ghost" onClick={onCancel}>
            Cancel
          </Button>
          <Button kind="accent" disabled={!!error} onClick={() => onAdd({ id: `user-${crypto.randomUUID()}`, name: name.trim(), keyword: keyword.trim(), url: url.trim(), suggestUrl: null, active: true })}>
            Add
          </Button>
        </div>
      </div>
      {error && (name || url) && <small className="warn-text">{error}</small>}
    </div>
  );
}

// ------------------------------------------------------------------ filter lists

const CATEGORY_LABELS: Record<string, string> = { ads: "Ads", trackers: "Trackers", annoyances: "Annoyances", malware: "Malicious sites", custom: "Custom" };

export function FilterListsPage() {
  const privacy = useStore((s) => s.settings?.privacy);
  const [url, setUrl] = useState("");
  if (!privacy) return null;
  const save = (filterLists: FilterList[]) => patch({ privacy: { filterLists } });
  const validUrl = /^https:\/\/\S+$/.test(url.trim()) && !privacy.filterLists.some((l) => l.url === url.trim());
  const categories = [...new Set(privacy.filterLists.map((l) => l.category))];
  return (
    <>
      <p className="note">Lists download from their publishers and refresh every four days. Changes apply after the lists are rebuilt, which takes a moment.</p>
      {categories.map((category) => (
        <div key={category}>
          <h3 className="display section-label">{CATEGORY_LABELS[category] ?? category}</h3>
          <div className="settings-card card">
            {privacy.filterLists
              .filter((l) => l.category === category)
              .map((list) => (
                <div key={list.id} className="setting-line">
                  <div className="setting-line-text">
                    <span>{list.name}</span>
                    <small className="muted ellipsis">
                      {list.url} · {list.lastUpdated ? `updated ${dayLabel(list.lastUpdated)}` : "not downloaded yet"}
                    </small>
                  </div>
                  <div className="row">
                    {list.category === "custom" && <Button kind="ghost" icon="trash" title={`Remove ${list.name}`} onClick={() => save(privacy.filterLists.filter((l) => l.id !== list.id))} />}
                    <Toggle on={list.enabled} onChange={(enabled) => save(privacy.filterLists.map((l) => (l.id === list.id ? { ...l, enabled } : l)))} label={list.name} />
                  </div>
                </div>
              ))}
          </div>
        </div>
      ))}
      <h3 className="display section-label">Add a list</h3>
      <div className="settings-card card">
        <div className="row">
          <Input className="text-input" style={{ flex: 1 }} placeholder="https://example.com/filters.txt" value={url} onChange={(e) => setUrl(e.target.value)} />
          <Button
            disabled={!validUrl}
            onClick={() => {
              const u = url.trim();
              save([...privacy.filterLists, { id: `custom-${crypto.randomUUID()}`, name: hostOf(u) || u, url: u, enabled: true, category: "custom", lastUpdated: null }]);
              setUrl("");
            }}
          >
            Add
          </Button>
        </div>
        <small className="muted">Adblock Plus and uBlock Origin syntax. Only HTTPS addresses.</small>
      </div>
      <h3 className="display section-label">My filters</h3>
      <div className="settings-card card">
        <textarea className="text-input mono" rows={8} defaultValue={privacy.customFilters} placeholder={"||ads.example.com^\nexample.com##.banner"} onBlur={(e) => e.target.value !== privacy.customFilters && patch({ privacy: { customFilters: e.target.value } })} />
      </div>
      <h3 className="display section-label">Sites where blocking is off</h3>
      <div className="settings-card card">
        <HostList
          list={privacy.adExceptions}
          onChange={(next) => {
            for (const h of privacy.adExceptions) if (!next.includes(h)) setBlockingOff(h, false);
            for (const h of next) if (!privacy.adExceptions.includes(h)) setBlockingOff(h, true);
          }}
          placeholder="example.com"
        />
      </div>
    </>
  );
}

/** Turn ad and tracker blocking off (or back on) for one site, as the shield popup does. */
function setBlockingOff(host: string, off: boolean) {
  const privacy = useStore.getState().settings?.privacy;
  if (!privacy) return;
  const toggle = (list: string[]) => (off ? (list.includes(host) ? list : [...list, host]) : list.filter((h) => h !== host));
  patch({ privacy: { adExceptions: toggle(privacy.adExceptions), trackerExceptions: toggle(privacy.trackerExceptions) } });
}



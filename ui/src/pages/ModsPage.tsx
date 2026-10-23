/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Mods: shareable bundles of look-and-feel settings (theme, colors, wallpaper, interface,
// sounds, shaders, UI CSS). Saved per profile; exported and imported as .hakumod JSON files.

import { useEffect, useState } from "react";
import { Button } from "../components/controls";
import { Icon, Logo } from "../components/Icon";
import { onEvent, send } from "../ipc/bridge";
import type { Settings } from "../ipc/generated/Settings";
import { clsx, patch, type SettingsPatch } from "../lib/util";
import { PRESETS } from "../overlays/EasySetup";
import { useStore } from "../state/store";
import { Input } from "../components/Field";

export type Mod = { format: "hakumod/1"; id: string; name: string; author: string; description: string; patch: SettingsPatch; created: number };

const APPEARANCE_KEYS = ["theme", "colorMode", "primaryColor", "secondaryColor", "wallpaper", "showWallpaper", "animatedWallpaper", "interfaceMode", "opacity", "blur", "elementsBackground", "vignette", "focusMode", "windowDecoration", "cornerStyle", "roundness", "crosshairCursor", "uiCss", "gxMessengerIcons", "colorizeIcons"] as const;
const SOUND_KEYS = ["browserSounds", "browserPack", "browserVolume", "keyboardSounds", "keyboardPack", "backgroundMusic", "musicTrack", "musicVolume"] as const;

function capture(settings: Settings): SettingsPatch {
  const appearance: Record<string, unknown> = {};
  for (const k of APPEARANCE_KEYS) appearance[k] = settings.appearance[k];
  const sounds: Record<string, unknown> = {};
  for (const k of SOUND_KEYS) sounds[k] = settings.sounds[k];
  return {
    appearance: appearance as SettingsPatch["appearance"],
    sounds: sounds as SettingsPatch["sounds"],
    features: { shader: settings.features.shader, shadersEnabled: settings.features.shadersEnabled, forceDarkPages: settings.features.forceDarkPages },
  };
}

function loadMods(): Mod[] {
  try {
    return JSON.parse(localStorage.getItem("hx-mods") ?? "[]") as Mod[];
  } catch {
    return [];
  }
}

function saveMods(mods: Mod[]) {
  try {
    localStorage.setItem("hx-mods", JSON.stringify(mods));
  } catch {
    useStore.getState().toast("Could not save mods in this profile.", "error");
  }
}

/** Only keys a mod is allowed to change: never privacy, search, sync or system settings. */
export function sanitizeModPatch(p: unknown): SettingsPatch {
  const src = (p ?? {}) as Record<string, Record<string, unknown>>;
  const out: Record<string, Record<string, unknown>> = {};
  const allow: Record<string, readonly string[]> = { appearance: APPEARANCE_KEYS, sounds: SOUND_KEYS, features: ["shader", "shadersEnabled", "forceDarkPages"] };
  for (const [section, keys] of Object.entries(allow)) {
    const s = src[section];
    if (!s || typeof s !== "object") continue;
    for (const k of keys) if (k in s) (out[section] ??= {})[k] = s[k];
  }
  return out as SettingsPatch;
}

/** How far a category is customized from the defaults (Opera GX's mod meter). */
function percent(settings: Settings, section: "appearance" | "sounds" | "features", keys: readonly string[], defaults: Record<string, unknown>): number {
  const values = settings[section] as unknown as Record<string, unknown>;
  const changed = keys.filter((k) => JSON.stringify(values[k]) !== JSON.stringify(defaults[k])).length;
  return Math.round((changed / keys.length) * 100);
}

const DEFAULTS = {
  appearance: { theme: "ultraviolet", colorMode: "dark", primaryColor: null, secondaryColor: null, wallpaper: "gx-ribbons", showWallpaper: true, animatedWallpaper: true, interfaceMode: "auto", opacity: 1, blur: 0, elementsBackground: 18, vignette: 0.55, focusMode: false, windowDecoration: "outside", cornerStyle: "cut", roundness: 8, crosshairCursor: false, uiCss: "", gxMessengerIcons: false, colorizeIcons: true },
  sounds: { browserSounds: true, browserPack: "haku-core", browserVolume: 50, keyboardSounds: false, keyboardPack: "haku-mech", backgroundMusic: false, musicTrack: "focus", musicVolume: 40 },
  features: { shader: "none", shadersEnabled: true, forceDarkPages: false },
};

let fileRequest = 30_000;

export function ModsPage() {
  const settings = useStore((s) => s.settings);
  const themes = useStore((s) => s.themes);
  const [mods, setMods] = useState<Mod[]>(loadMods);
  const [name, setName] = useState("");
  const pending = useState<number | null>(null);
  const [importRequest, setImportRequest] = pending;

  useEffect(
    () =>
      onEvent((e) => {
        if (e.type !== "text-file" || e.request !== importRequest) return;
        setImportRequest(null);
        try {
          const parsed = JSON.parse(e.content) as Partial<Mod>;
          if (parsed.format !== "hakumod/1" || typeof parsed.name !== "string") throw new Error("not a HakuCX mod");
          const mod: Mod = { format: "hakumod/1", id: Math.random().toString(36).slice(2), name: parsed.name.slice(0, 60), author: String(parsed.author ?? "").slice(0, 60), description: String(parsed.description ?? "").slice(0, 300), patch: sanitizeModPatch(parsed.patch), created: Date.now() };
          const next = [...mods, mod];
          setMods(next);
          saveMods(next);
          useStore.getState().toast(`Installed mod "${mod.name}"`, "info");
        } catch (err) {
          useStore.getState().toast(`That file is not a valid mod (${String(err)})`, "error");
        }
      }),
    [importRequest, mods, setImportRequest],
  );

  if (!settings) return null;
  const effects = percent(settings, "features", ["shader", "shadersEnabled", "forceDarkPages"], DEFAULTS.features);
  const iface = percent(settings, "appearance", APPEARANCE_KEYS, DEFAULTS.appearance);
  const sounds = percent(settings, "sounds", SOUND_KEYS, DEFAULTS.sounds);
  const overall = Math.round((effects + iface + sounds) / 3);

  const create = () => {
    const mod: Mod = { format: "hakumod/1", id: Math.random().toString(36).slice(2), name: name.trim() || `My mod ${mods.length + 1}`, author: useStore.getState().engine?.profile.name ?? "", description: "", patch: capture(settings), created: Date.now() };
    const next = [...mods, mod];
    setMods(next);
    saveMods(next);
    setName("");
  };

  return (
    <div className="mods-page">
      <div className="mods-circuit">
        <div className="mods-core octagon big">
          <Logo size={34} />
          <strong>{overall}%</strong>
          <small className="display">Customized</small>
        </div>
        <div className="mods-nodes">
          {[
            ["Effects", effects, "aperture"],
            ["Interface", iface, "palette"],
            ["Sounds", sounds, "volume"],
          ].map(([label, value, icon]) => (
            <div key={label as string} className="mods-node">
              <Icon name={icon as string} size={18} />
              <span className="display">{label}</span>
              <strong>{value}%</strong>
            </div>
          ))}
        </div>
      </div>

      <section className="card">
        <h3 className="display section-label">Presets</h3>
        <div className="preset-grid wide">
          {PRESETS.map((p) => {
            const t = themes.find((x) => x.id === p.theme);
            return (
              <button key={p.id} className="preset-tile notch" style={{ "--card-accent": t?.darkAccent ?? "var(--hx-accent)" } as React.CSSProperties} onClick={() => patch(p.patch)}>
                <span className="display">{p.name}</span>
              </button>
            );
          })}
        </div>
      </section>

      <section className="card">
        <div className="row">
          <h3 className="display section-label grow">My mods</h3>
          <Button
            icon="upload"
            onClick={() => {
              const request = fileRequest++;
              setImportRequest(request);
              send({ type: "open-text-file", request, extensions: ["hakumod", "json"] });
            }}
          >
            Import
          </Button>
        </div>
        <div className="row">
          <Input className="text-input" placeholder="Name your current look" value={name} onChange={(e) => setName(e.target.value)} />
          <Button kind="accent" icon="plus" onClick={create}>
            Create mod from current look
          </Button>
        </div>
        {mods.length === 0 && <p className="muted">Mods you create or import appear here. A mod changes only appearance, sounds and shaders, never privacy or system settings.</p>}
        <div className="mod-list">
          {mods.map((m) => (
            <div key={m.id} className="mod-row">
              <Icon name="box" size={18} />
              <div className="grow">
                <div>{m.name}</div>
                <small className="muted">{m.author ? `by ${m.author}` : "Local mod"}</small>
              </div>
              <Button kind="accent" onClick={() => patch(m.patch)}>
                Apply
              </Button>
              <Button icon="download" title="Export" onClick={() => send({ type: "save-text-file", suggestedName: `${m.name.replace(/[^\w -]+/g, "")}.hakumod`, content: JSON.stringify(m, null, 2) })} />
              <Button
                icon="trash"
                title="Delete"
                onClick={() => {
                  const next = mods.filter((x) => x.id !== m.id);
                  setMods(next);
                  saveMods(next);
                }}
              />
            </div>
          ))}
        </div>
      </section>

      <section className="card">
        <h3 className="display section-label">Web mods</h3>
        <SiteModsList />
      </section>
    </div>
  );
}

export function SiteModsList() {
  const mods = useStore((s) => s.siteMods.mods);
  const enabled = useStore((s) => s.settings?.features.webModding ?? true);
  const openModal = useStore((s) => s.openModal);
  return (
    <div className="col">
      {!enabled && <p className="note">Web modding is turned off. Turn it on in Settings &gt; Features.</p>}
      {mods.map((m) => (
        <div key={m.id} className={clsx("mod-row", !m.enabled && "off")}>
          <Icon name="brush" size={16} />
          <span className="grow ellipsis">{m.domain}</span>
          <small className="dim">{m.css.length} chars</small>
          <Button onClick={() => send({ type: "save-site-mods", mods: { mods: mods.map((x) => (x.id === m.id ? { ...x, enabled: !x.enabled } : x)) } })}>{m.enabled ? "Disable" : "Enable"}</Button>
          <Button icon="pencil" title="Edit" onClick={() => openModal({ kind: "site-mod", id: m.id })} />
          <Button icon="trash" title="Delete" onClick={() => send({ type: "save-site-mods", mods: { mods: mods.filter((x) => x.id !== m.id) } })} />
        </div>
      ))}
      <Button icon="plus" onClick={() => openModal({ kind: "site-mod", id: null })}>
        Add style for a site
      </Button>
    </div>
  );
}

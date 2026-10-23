/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import QRCode from "qrcode";
import { useEffect, useState, type ReactNode } from "react";
import { Button, Checkbox, Segmented } from "../components/controls";
import { Icon } from "../components/Icon";
import { copyText, send } from "../ipc/bridge";
import { WORKSPACE_ICONS } from "../lib/catalog";
import { clsx } from "../lib/util";
import { useStore } from "../state/store";
import { Input, TextArea } from "../components/Field";

const PRIMARY_SWATCHES = ["#ff0077", "#fa1e4e", "#ff6c22", "#ffe20f", "#cacaca", "#8d35e2", "#4d4dff", "#00ffff", "#00ffa2", "#36ff4e"];
const SECONDARY_SWATCHES = ["#331f28", "#331f23", "#33261f", "#33301f", "#3d3d3d", "#291f33", "#1f1f33", "#1f3333", "#1f332b", "#1f3321"];
const ISLAND_COLORS = ["#7d46f2", "#fa1e4e", "#00e5c3", "#f6c955", "#4461ee", "#aeff3c", "#ff708d", "#ff9900"];

export function ModalFrame({ title, children, onClose, width = 440, footer }: { title: string; children: ReactNode; onClose: () => void; width?: number; footer?: ReactNode }) {
  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div className="modal" style={{ width }} onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h2 className="display">{title}</h2>
          <button className="icon-btn" title="Close" onClick={onClose}>
            <Icon name="x" size={16} />
          </button>
        </div>
        <div className="modal-body">{children}</div>
        {footer && <div className="modal-foot">{footer}</div>}
      </div>
    </div>
  );
}

export function Modals() {
  const modal = useStore((s) => s.modal);
  const close = () => useStore.getState().openModal(null);
  if (!modal) return null;
  switch (modal.kind) {
    case "quit":
      return <QuitModal close={close} />;
    case "qr":
      return <QrModal url={modal.url} close={close} />;
    case "clear-data":
      return <ClearDataModal close={close} />;
    case "dial-edit":
      return <DialEditModal id={modal.id} parent={modal.parent} close={close} />;
    case "bookmark-edit":
      return <DialEditModal id={modal.id} parent="other" close={close} bookmark />;
    case "island-edit":
      return <IslandModal island={modal.island} close={close} />;
    case "workspace-edit":
      return <WorkspaceModal workspace={modal.workspace} close={close} />;
    case "theme-editor":
      return <ThemeEditor themeId={modal.themeId} close={close} />;
    case "site-mod":
      return <SiteModModal id={modal.id} close={close} />;
    case "devtools":
      return <DevtoolsModal close={close} />;
    case "profiles":
      return <ProfilesModal close={close} />;
  }
}

function QuitModal({ close }: { close: () => void }) {
  const tabs = useStore((s) => s.session?.tabs.length ?? 0);
  return (
    <ModalFrame
      title="Close HakuCX?"
      onClose={close}
      footer={
        <>
          <Button onClick={close}>Cancel</Button>
          <Button kind="accent" onClick={() => send({ type: "window", action: "quit" })}>
            Close
          </Button>
        </>
      }
    >
      <p>You have {tabs} tabs open. They will be restored next time if "On startup" is set to retain tabs.</p>
      <Checkbox
        checked={!useStore.getState().settings?.tabs.warnQuit}
        onChange={(v) => send({ type: "patch-settings", patch: { tabs: { warnQuit: !v } } })}
        label="Don't ask again"
      />
    </ModalFrame>
  );
}

function QrModal({ url, close }: { url: string; close: () => void }) {
  const [svg, setSvg] = useState("");
  useEffect(() => {
    QRCode.toString(url, { type: "svg", margin: 1, color: { dark: "#000000", light: "#ffffff" } }).then(setSvg, () => setSvg(""));
  }, [url]);
  return (
    <ModalFrame title="QR code" onClose={close} width={360} footer={<Button onClick={() => copyText(url)}>Copy address</Button>}>
      <div className="qr-box" dangerouslySetInnerHTML={{ __html: svg }} />
      <p className="muted ellipsis">{url}</p>
    </ModalFrame>
  );
}

const RANGES: { value: string; label: string; ms: number | null }[] = [
  { value: "15m", label: "Last 15 min", ms: 15 * 60_000 },
  { value: "1h", label: "Last hour", ms: 3_600_000 },
  { value: "24h", label: "Last 24 hours", ms: 86_400_000 },
  { value: "7d", label: "Last 7 days", ms: 7 * 86_400_000 },
  { value: "all", label: "All time", ms: null },
];

function ClearDataModal({ close }: { close: () => void }) {
  const [range, setRange] = useState("1h");
  const [kinds, setKinds] = useState({ history: true, cookies: true, cache: true, downloads: false, siteSettings: false });
  const set = (k: keyof typeof kinds) => (v: boolean) => setKinds((x) => ({ ...x, [k]: v }));
  const ms = RANGES.find((r) => r.value === range)?.ms ?? null;
  return (
    <ModalFrame
      title="Delete browsing data"
      onClose={close}
      width={520}
      footer={
        <>
          <Button onClick={close}>Cancel</Button>
          <Button
            kind="accent"
            onClick={() => {
              send({ type: "clear-browsing-data", from: ms === null ? null : Date.now() - ms, history: kinds.history, cookies: kinds.cookies, cache: kinds.cache, downloads: kinds.downloads, siteSettings: kinds.siteSettings });
              close();
            }}
          >
            Delete from this device
          </Button>
        </>
      }
    >
      <Segmented value={range} options={RANGES.map(({ value, label }) => ({ value, label }))} onChange={setRange} />
      <div className="col clear-kinds">
        <Checkbox checked={kinds.history} onChange={set("history")} label={<span>Browsing history <small className="muted">Pages you visited and recently closed tabs</small></span>} />
        <Checkbox checked={kinds.cookies} onChange={set("cookies")} label={<span>Cookies and other site data <small className="muted">Signs you out of most sites. The engine deletes all cookies, not just this time range.</small></span>} />
        <Checkbox checked={kinds.cache} onChange={set("cache")} label={<span>Cached images and files <small className="muted">The whole engine cache</small></span>} />
        <Checkbox checked={kinds.downloads} onChange={set("downloads")} label={<span>Download history <small className="muted">The list only; files stay on disk</small></span>} />
        <Checkbox checked={kinds.siteSettings} onChange={set("siteSettings")} label={<span>Site settings <small className="muted">Permissions you granted or blocked</small></span>} />
      </div>
    </ModalFrame>
  );
}

function DialEditModal({ id, parent, close, bookmark }: { id: string | null; parent: string; close: () => void; bookmark?: boolean }) {
  const node = useStore((s) => (id ? s.bookmarks.nodes.find((n) => n.id === id) : undefined));
  const [title, setTitle] = useState(node?.title ?? "");
  const [url, setUrl] = useState(node?.url ?? "https://");
  const [color, setColor] = useState(node?.color ?? "");
  const folder = node && !node.url;
  const valid = folder || /^(https?|file|ftp|haku):\/\//.test(url.trim());
  const save = () => {
    if (!valid) return;
    if (id) send({ type: "update-bookmark", id, title, url: folder ? null : url.trim(), color: color || null });
    else send({ type: "add-bookmark", parent, title: title || url, url: url.trim(), index: null });
    close();
  };
  return (
    <ModalFrame
      title={id ? (bookmark ? "Edit bookmark" : "Edit Speed Dial") : "Add a site"}
      onClose={close}
      footer={
        <>
          <Button onClick={close}>Cancel</Button>
          <Button kind="accent" disabled={!valid} onClick={save}>
            Save
          </Button>
        </>
      }
    >
      <label className="field">
        <span>Name</span>
        <Input className="text-input" value={title} onChange={(e) => setTitle(e.target.value)} autoFocus />
      </label>
      {!folder && (
        <label className="field">
          <span>Address</span>
          <Input className="text-input" value={url} onChange={(e) => setUrl(e.target.value)} onKeyDown={(e) => e.key === "Enter" && save()} />
        </label>
      )}
      {!bookmark && (
        <div className="field">
          <span>Tile color</span>
          <div className="swatches">
            <button className={clsx("swatch auto", !color && "selected")} onClick={() => setColor("")} title="Automatic" />
            {ISLAND_COLORS.map((c) => (
              <button key={c} className={clsx("swatch", color === c && "selected")} style={{ background: c }} onClick={() => setColor(c)} />
            ))}
          </div>
        </div>
      )}
    </ModalFrame>
  );
}

function IslandModal({ island, close }: { island: string; close: () => void }) {
  const data = useStore((s) => s.session?.islands.find((i) => i.id === island));
  const [name, setName] = useState(data?.name ?? "");
  const [emoji, setEmoji] = useState(data?.emoji ?? "");
  const [color, setColor] = useState(data?.color ?? "");
  if (!data) return null;
  return (
    <ModalFrame
      title="Edit island"
      onClose={close}
      footer={
        <>
          <Button onClick={close}>Cancel</Button>
          <Button kind="accent" onClick={() => { send({ type: "update-island", island, name, emoji, color, collapsed: null }); close(); }}>
            Save
          </Button>
        </>
      }
    >
      <label className="field">
        <span>Name</span>
        <Input className="text-input" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
      </label>
      <label className="field">
        <span>Emoji</span>
        <Input className="text-input" value={emoji} maxLength={4} onChange={(e) => setEmoji(e.target.value)} placeholder="🎮" />
      </label>
      <div className="field">
        <span>Color</span>
        <div className="swatches">
          <button className={clsx("swatch auto", !color && "selected")} onClick={() => setColor("")} title="Theme accent" />
          {ISLAND_COLORS.map((c) => (
            <button key={c} className={clsx("swatch", color === c && "selected")} style={{ background: c }} onClick={() => setColor(c)} />
          ))}
        </div>
      </div>
    </ModalFrame>
  );
}

function WorkspaceModal({ workspace, close }: { workspace: string | null; close: () => void }) {
  const ws = useStore((s) => s.session?.workspaces.find((w) => w.id === workspace));
  const count = useStore((s) => s.session?.workspaces.length ?? 0);
  const [name, setName] = useState(ws?.name ?? "");
  const [icon, setIcon] = useState(ws?.icon ?? "star");
  const save = () => {
    if (!name.trim()) return;
    if (workspace) send({ type: "update-workspace", workspace, name: name.trim(), icon });
    else send({ type: "add-workspace", name: name.trim(), icon });
    close();
  };
  return (
    <ModalFrame
      title={workspace ? "Edit workspace" : "New workspace"}
      onClose={close}
      footer={
        <>
          {workspace && count > 1 && (
            <Button kind="danger" onClick={() => { send({ type: "remove-workspace", workspace }); close(); }}>
              Delete
            </Button>
          )}
          <span className="grow" />
          <Button onClick={close}>Cancel</Button>
          <Button kind="accent" disabled={!name.trim()} onClick={save}>
            Save
          </Button>
        </>
      }
    >
      <label className="field">
        <span>Name</span>
        <Input className="text-input" value={name} onChange={(e) => setName(e.target.value)} autoFocus onKeyDown={(e) => e.key === "Enter" && save()} />
      </label>
      <div className="field">
        <span>Icon</span>
        <div className="icon-picker">
          {WORKSPACE_ICONS.map((i) => (
            <button key={i} className={clsx("icon-btn", icon === i && "active")} onClick={() => setIcon(i)}>
              <Icon name={i} size={16} />
            </button>
          ))}
        </div>
      </div>
      {workspace && count > 1 && <p className="note">Deleting a workspace closes its tabs (you can reopen them from history).</p>}
    </ModalFrame>
  );
}

function ThemeEditor({ themeId, close }: { themeId: string | null; close: () => void }) {
  const themes = useStore((s) => s.themes);
  const existing = themes.find((t) => t.id === themeId && t.user);
  const [mode, setMode] = useState<"dark" | "light">("dark");
  const [name, setName] = useState(existing?.name ?? "My theme");
  const [dark, setDark] = useState({ primary: existing?.darkAccent ?? "#fa1e4e", secondary: existing?.darkBackground ?? "#291f33" });
  const [light, setLight] = useState({ primary: existing?.lightAccent ?? "#fa1e4e", secondary: existing?.lightBackground ?? "#f4f3f7" });
  const cur = mode === "dark" ? dark : light;
  const setCur = mode === "dark" ? setDark : setLight;
  const save = () => {
    send({
      type: "save-user-theme",
      theme: { id: existing?.id ?? "", name, darkPrimary: dark.primary, darkSecondary: dark.secondary, lightPrimary: light.primary, lightSecondary: light.secondary, wallpaper: "gx-ribbons" },
    });
    close();
  };
  return (
    <ModalFrame
      title="Advanced configuration"
      onClose={close}
      width={560}
      footer={
        <>
          {existing && (
            <Button kind="danger" onClick={() => { send({ type: "delete-user-theme", id: existing.id }); close(); }}>
              Delete theme
            </Button>
          )}
          <span className="grow" />
          <Button onClick={close}>Cancel</Button>
          <Button kind="accent" onClick={save}>
            Save and use
          </Button>
        </>
      }
    >
      <label className="field">
        <span>Theme name</span>
        <Input className="text-input" value={name} onChange={(e) => setName(e.target.value)} />
      </label>
      <Segmented value={mode} options={[{ value: "light", label: "Light" }, { value: "dark", label: "Dark" }]} onChange={setMode} />
      <div className="field">
        <span>Primary color ({mode} theme)</span>
        <div className="swatches">
          {PRIMARY_SWATCHES.map((c) => (
            <button key={c} className={clsx("swatch", cur.primary === c && "selected")} style={{ background: c }} onClick={() => setCur({ ...cur, primary: c })}>
              {cur.primary === c && <Icon name="check" size={12} stroke={3} />}
            </button>
          ))}
          <HexInput value={cur.primary} onChange={(v) => setCur({ ...cur, primary: v })} />
        </div>
      </div>
      <div className="field">
        <span>Secondary color ({mode} theme)</span>
        <div className="swatches">
          {(mode === "dark" ? SECONDARY_SWATCHES : ["#f4f3f7", "#f1f2f8", "#faeff3", "#f2f6f7", "#f7f5f3", "#f5f5f5"]).map((c) => (
            <button key={c} className={clsx("swatch", cur.secondary === c && "selected")} style={{ background: c }} onClick={() => setCur({ ...cur, secondary: c })}>
              {cur.secondary === c && <Icon name="check" size={12} stroke={3} />}
            </button>
          ))}
          <HexInput value={cur.secondary} onChange={(v) => setCur({ ...cur, secondary: v })} />
        </div>
      </div>
      <div className="theme-preview" style={{ background: cur.secondary, borderColor: cur.primary }}>
        <span style={{ background: cur.primary }} />
        <span style={{ background: cur.primary, opacity: 0.6 }} />
        <span style={{ background: cur.primary, opacity: 0.3 }} />
      </div>
    </ModalFrame>
  );
}

/** RGB entry as hex: the UI shell has no native color dialog. */
export function HexInput({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return (
    <span className="hex-input">
      <span className="hex-preview" style={{ background: /^#[0-9a-f]{6}$/i.test(text) ? text : value }} />
      <Input
        className="text-input"
        value={text}
        maxLength={7}
        onChange={(e) => {
          const v = e.target.value.startsWith("#") ? e.target.value : `#${e.target.value}`;
          setText(v);
          if (/^#[0-9a-f]{6}$/i.test(v)) onChange(v.toLowerCase());
        }}
      />
    </span>
  );
}

function SiteModModal({ id, close }: { id: string | null; close: () => void }) {
  const mods = useStore((s) => s.siteMods.mods);
  const existing = mods.find((m) => m.id === id);
  const [domain, setDomain] = useState(existing?.domain ?? "");
  const [css, setCss] = useState(existing?.css ?? "");
  const save = () => {
    const d = domain.trim().toLowerCase().replace(/^https?:\/\//, "").replace(/\/.*$/, "");
    if (!d) return;
    const next = existing
      ? mods.map((m) => (m.id === existing.id ? { ...m, domain: d, css } : m))
      : [...mods, { id: Math.random().toString(36).slice(2), domain: d, css, enabled: true }];
    send({ type: "save-site-mods", mods: { mods: next } });
    close();
  };
  return (
    <ModalFrame
      title="Add style"
      onClose={close}
      width={560}
      footer={
        <>
          <Button onClick={close}>Cancel</Button>
          <Button kind="accent" onClick={save}>
            OK
          </Button>
        </>
      }
    >
      <label className="field">
        <span>Domain</span>
        <Input className="text-input" value={domain} placeholder="youtube.com" onChange={(e) => setDomain(e.target.value)} autoFocus />
      </label>
      <label className="field">
        <span>CSS</span>
        <TextArea className="text-input" value={css} placeholder="CSS goes here" onChange={(e) => setCss(e.target.value)} rows={10} />
      </label>
    </ModalFrame>
  );
}

function DevtoolsModal({ close }: { close: () => void }) {
  const system = useStore((s) => s.settings?.system);
  if (!system) return null;
  return (
    <ModalFrame title="Developer tools" onClose={close} width={520}>
      <p>
        HakuCX's engine, Servo, speaks the Firefox DevTools protocol. Turn on the DevTools server, then in Firefox open <code>about:debugging</code>, choose
        "Setup", and connect to <code>localhost:{system.devtoolsPort}</code>.
      </p>
      <Checkbox
        checked={system.devtoolsServer}
        onChange={(v) => send({ type: "patch-settings", patch: { system: { devtoolsServer: v } } })}
        label="Enable the DevTools server (local connections only)"
      />
      <p className="note">The server listens on 127.0.0.1 only. Turn it off when you are done: anything connected to it can inspect and control your pages.</p>
    </ModalFrame>
  );
}

function ProfilesModal({ close }: { close: () => void }) {
  const engine = useStore((s) => s.engine);
  if (!engine) return null;
  return (
    <ModalFrame title="Profiles" onClose={close} width={520}>
      <p className="muted">Each profile has its own tabs, history, bookmarks, logins and settings. Opening another profile starts a new window.</p>
      {engine.profiles.map((p) => (
        <div key={p.id} className="profile-row">
          <span className="avatar" style={{ background: p.color }}>
            {p.name[0]?.toUpperCase()}
          </span>
          <div className="grow">
            <div>{p.name}</div>
            <small className="muted">
              {p.kind === "streaming" ? "Streaming: starts with tabs muted" : p.kind === "roguelike" ? "Roguelike: nothing is saved between launches" : "Standard"}
            </small>
          </div>
          {p.id === engine.profile.id ? <span className="chip accent">Current</span> : <Button onClick={() => send({ type: "open-profile", id: p.id })}>Open</Button>}
        </div>
      ))}
      <div className="section-label display">Add a profile</div>
      <div className="row">
        <Button icon="plus" onClick={() => send({ type: "add-profile", name: "Streaming", kind: "streaming" })}>Streaming</Button>
        <Button icon="plus" onClick={() => send({ type: "add-profile", name: "Roguelike", kind: "roguelike" })}>Roguelike</Button>
        <Button icon="plus" onClick={() => send({ type: "add-profile", name: "Standard", kind: "standard" })}>Standard</Button>
      </div>
    </ModalFrame>
  );
}

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// The extensions manager (haku://extensions/): install, enable, pin, allow in private,
// shortcuts, rule sets, errors, and developer mode.

import { useState } from "react";
import { Button, Toggle } from "../components/controls";
import { Icon } from "../components/Icon";
import { Input } from "../components/Field";
import { send } from "../ipc/bridge";
import type { ExtensionInfo } from "../ipc/generated/ExtensionInfo";
import { clsx } from "../lib/util";
import { useStore } from "../state/store";
import { accelerator } from "./settings/subpages";

export function ExtensionsPage() {
  const extensions = useStore((s) => s.extensions);
  const developer = useStore((s) => s.extDeveloperMode);
  const isPrivate = useStore((s) => s.engine?.private ?? false);
  const [filter, setFilter] = useState("");
  const [open, setOpen] = useState<string | null>(null);
  const q = filter.trim().toLowerCase();
  const shown = extensions.filter((e) => !q || e.name.toLowerCase().includes(q) || e.description.toLowerCase().includes(q) || e.id.includes(q));

  return (
    <div className="hub-main ext-page">
      <div className="card ext-head">
        <div className="row">
          <h2 className="display grow">Extensions</h2>
          <span className="muted">Developer mode</span>
          <Toggle on={developer} onChange={(on) => send({ type: "ext-set-developer-mode", on })} label="Developer mode" />
        </div>
        <p className="muted">
          HakuCX runs Chrome (Manifest V2 and V3) and Firefox extensions. Install a <code>.crx</code>, <code>.xpi</code> or <code>.zip</code> package
          {developer ? ", or load an extension folder" : ""}. Each extension asks for its permissions before it is added.
        </p>
        <div className="row ext-actions">
          <Button kind="accent" icon="plus" disabled={isPrivate} title={isPrivate ? "Install extensions from a normal window" : undefined} onClick={() => send({ type: "ext-install-file" })}>
            Install from file
          </Button>
          {developer && (
            <Button icon="folder" disabled={isPrivate} onClick={() => send({ type: "ext-load-unpacked" })}>
              Load unpacked
            </Button>
          )}
          <span className="grow" />
          <div className="search-box">
            <Icon name="search" size={15} />
            <Input placeholder="Search extensions" value={filter} onChange={(e) => setFilter(e.target.value)} />
          </div>
        </div>
      </div>

      {extensions.length === 0 ? (
        <div className="card ext-empty">
          <Icon name="puzzle" size={36} />
          <h3 className="display">No extensions yet</h3>
          <p className="muted">
            Download an extension package (Firefox Add-ons offers <code>.xpi</code> files directly) and choose <b>Install from file</b>.
          </p>
        </div>
      ) : (
        shown.map((ext) => <ExtensionCard key={ext.id} ext={ext} developer={developer} expanded={open === ext.id} onToggle={() => setOpen(open === ext.id ? null : ext.id)} />)
      )}
    </div>
  );
}

function ExtensionCard({ ext, developer, expanded, onToggle }: { ext: ExtensionInfo; developer: boolean; expanded: boolean; onToggle: () => void }) {
  const [confirmRemove, setConfirmRemove] = useState(false);
  const failed = !ext.enabled && ext.errors.length > 0 && !ext.version;
  return (
    <div className={clsx("card ext-card", !ext.enabled && "disabled")}>
      <div className="ext-card-head">
        <ExtIcon src={ext.icon} size={40} />
        <div className="ext-card-text">
          <div className="row">
            <span className="ext-name">{ext.name || ext.id}</span>
            {ext.version && <span className="ext-version">{ext.version}</span>}
            {ext.unpacked && <span className="ext-tag">Unpacked</span>}
            {ext.manifestVersion === 2 && <span className="ext-tag">MV2</span>}
          </div>
          <p className="muted ext-desc">{failed ? "This extension could not be loaded." : ext.description}</p>
        </div>
        <Toggle on={ext.enabled} onChange={(enabled) => send({ type: "ext-set-enabled", id: ext.id, enabled })} label={`Enable ${ext.name}`} />
      </div>

      {ext.errors.length > 0 && (
        <div className="ext-errors">
          <Icon name="alert" size={14} />
          <span className="grow">{ext.errors[ext.errors.length - 1]}</span>
          {ext.errors.length > 1 && <span className="muted">+{ext.errors.length - 1} more</span>}
        </div>
      )}

      <div className="row ext-card-actions">
        <Button kind="ghost" icon={expanded ? "chevron-up" : "chevron-down"} onClick={onToggle}>
          Details
        </Button>
        {ext.hasOptions && ext.enabled && (
          <Button kind="ghost" icon="settings" onClick={() => send({ type: "ext-open-options", id: ext.id })}>
            Options
          </Button>
        )}
        {ext.unpacked && (
          <Button kind="ghost" icon="reload" title="Reload from its folder" onClick={() => send({ type: "ext-reload", id: ext.id })}>
            Reload
          </Button>
        )}
        <span className="grow" />
        {confirmRemove ? (
          <>
            <span className="muted">Remove {ext.name}?</span>
            <Button kind="danger" onClick={() => send({ type: "ext-remove", id: ext.id })}>
              Remove
            </Button>
            <Button kind="ghost" onClick={() => setConfirmRemove(false)}>
              Keep
            </Button>
          </>
        ) : (
          <Button kind="ghost" icon="trash" onClick={() => setConfirmRemove(true)}>
            Remove
          </Button>
        )}
      </div>

      {expanded && <ExtensionDetails ext={ext} developer={developer} />}
    </div>
  );
}

function ExtensionDetails({ ext, developer }: { ext: ExtensionInfo; developer: boolean }) {
  const [editing, setEditing] = useState<string | null>(null);
  return (
    <div className="ext-details">
      <div className="setting-line">
        <div className="setting-line-text">
          <span>Pin to toolbar</span>
          <small className="muted">Show its button next to the address bar</small>
        </div>
        <Toggle on={ext.pinned} onChange={(pinned) => send({ type: "ext-set-pinned", id: ext.id, pinned })} label="Pin to toolbar" />
      </div>
      <div className="setting-line">
        <div className="setting-line-text">
          <span>Allow in private windows</span>
          <small className="muted">HakuCX can't stop extensions from recording your browsing history in private windows</small>
        </div>
        <Toggle on={ext.allowPrivate} onChange={(allow) => send({ type: "ext-set-private", id: ext.id, allow })} label="Allow in private" />
      </div>

      {ext.warnings.length > 0 && (
        <div className="ext-section">
          <h4>Permissions</h4>
          <ul className="ext-list">
            {ext.warnings.map((w) => (
              <li key={w}>{w}</li>
            ))}
          </ul>
        </div>
      )}

      {ext.commands.length > 0 && (
        <div className="ext-section">
          <h4>Keyboard shortcuts</h4>
          {ext.commands.map((c) => (
            <div key={c.name} className="setting-line">
              <div className="setting-line-text">
                <span>{c.description || c.name}</span>
              </div>
              <button
                className={clsx("key-capture", editing === c.name && "capturing")}
                onClick={() => {
                  setEditing(c.name);
                  send({ type: "key-capture", on: true });
                }}
                onBlur={() => {
                  setEditing(null);
                  send({ type: "key-capture", on: false });
                }}
                onKeyDown={(e) => {
                  if (editing !== c.name) return;
                  e.preventDefault();
                  if (e.key === "Escape") return e.currentTarget.blur();
                  if (e.key === "Backspace" || e.key === "Delete") {
                    send({ type: "ext-set-shortcut", id: ext.id, command: c.name, accelerator: "" });
                    return e.currentTarget.blur();
                  }
                  const acc = accelerator(e);
                  if (acc) {
                    send({ type: "ext-set-shortcut", id: ext.id, command: c.name, accelerator: acc });
                    e.currentTarget.blur();
                  }
                }}
              >
                {editing === c.name ? "Press keys (Backspace clears)" : c.shortcut || "Not set"}
              </button>
            </div>
          ))}
        </div>
      )}

      {ext.rulesets.length > 0 && (
        <div className="ext-section">
          <h4>Rule sets</h4>
          {ext.rulesets.map(([id, on]) => (
            <div key={id} className="setting-line">
              <div className="setting-line-text">
                <span>{id}</span>
              </div>
              <Toggle on={on} onChange={(enabled) => send({ type: "ext-toggle-ruleset", id: ext.id, ruleset: id, enabled })} label={`Rule set ${id}`} />
            </div>
          ))}
        </div>
      )}

      {ext.unsupported.length > 0 && (
        <div className="ext-section">
          <h4>Not available in HakuCX yet</h4>
          <ul className="ext-list muted">
            {ext.unsupported.map((u) => (
              <li key={u}>{u}</li>
            ))}
          </ul>
        </div>
      )}

      {ext.errors.length > 0 && (
        <div className="ext-section">
          <h4>Errors</h4>
          <pre className="ext-log">{ext.errors.join("\n")}</pre>
        </div>
      )}

      <div className="ext-section ext-meta">
        {ext.homepage && (
          <button className="link-btn" onClick={() => send({ type: "new-tab", url: ext.homepage, background: false, relativeTo: null })}>
            <Icon name="external" size={13} /> Homepage
          </button>
        )}
        {developer && (
          <>
            <div>
              <span className="muted">ID</span> <code>{ext.runtimeId || ext.id}</code>
            </div>
            <div>
              <span className="muted">Folder</span> <code>{ext.path}</code>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

/** An extension icon, or a puzzle piece when it has none (or it fails to load). */
export function ExtIcon({ src, size = 20 }: { src: string | null; size?: number }) {
  const [broken, setBroken] = useState(false);
  if (!src || broken) {
    return (
      <span className="ext-icon placeholder" style={{ width: size, height: size }}>
        <Icon name="puzzle" size={Math.round(size * 0.6)} />
      </span>
    );
  }
  return <img className="ext-icon" src={src} width={size} height={size} alt="" onError={() => setBroken(true)} />;
}

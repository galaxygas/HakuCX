/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import type { ReactNode } from "react";
import { Icon } from "../components/Icon";
import { send } from "../ipc/bridge";
import { PERMISSION_LABELS } from "../lib/catalog";
import { clsx, hostOf } from "../lib/util";
import { DownloadRow } from "../panels/ListPanels";
import { ExtIcon } from "../pages/ExtensionsPage";
import { loginsFor } from "../pages/settings/autofill";
import { openInternal, useStore } from "../state/store";

function PopupFrame({ children, className, align = "right", width = 340 }: { children: ReactNode; className?: string; align?: "left" | "right"; width?: number }) {
  const anchor = useStore((s) => s.popupAnchor);
  const style: React.CSSProperties = { width };
  if (anchor) {
    style.top = anchor.y + anchor.h + 6;
    if (align === "left") style.left = Math.max(8, anchor.x);
    else style.left = Math.max(8, Math.min(window.innerWidth - width - 8, anchor.x + anchor.w - width));
  }
  return (
    <div className={clsx("popup", className)} style={style} onMouseDown={(e) => e.stopPropagation()}>
      {children}
    </div>
  );
}

function MenuItem({ icon, label, shortcut, onClick, danger }: { icon?: string; label: string; shortcut?: string; onClick: () => void; danger?: boolean }) {
  return (
    <button
      className={clsx("menu-item", danger && "danger")}
      onClick={() => {
        useStore.getState().openPopup(null);
        onClick();
      }}
    >
      {icon ? <Icon name={icon} size={15} /> : <span className="menu-icon-space" />}
      <span className="grow">{label}</span>
      {shortcut && <kbd>{shortcut}</kbd>}
    </button>
  );
}

export function Popups() {
  const popup = useStore((s) => s.popup);
  switch (popup) {
    case "main-menu":
      return <MainMenu />;
    case "workspaces":
      return <WorkspacesPopup />;
    case "downloads":
      return <DownloadsPopup />;
    case "extensions":
      return <ExtensionsPopup />;
    case "passwords":
      return <PasswordsPopup />;
    case "profile":
      return <ProfilePopup />;
    case "site-info":
      return <SiteInfoPopup />;
    case "blocked":
      return <BlockedPopup />;
    default:
      return null;
  }
}

function MainMenu() {
  const s = useStore.getState();
  return (
    <PopupFrame align="left" width={290} className="menu">
      <MenuItem icon="plus" label="New tab" shortcut="Ctrl+T" onClick={() => send({ type: "new-tab", url: null, background: false, relativeTo: null })} />
      <MenuItem icon="window" label="New window" shortcut="Ctrl+N" onClick={() => send({ type: "window", action: "new-window" })} />
      <MenuItem icon="ghost" label="New private window" shortcut="Ctrl+Shift+N" onClick={() => send({ type: "window", action: "new-private-window" })} />
      <div className="menu-sep" />
      <MenuItem icon="clock" label="History" shortcut="Ctrl+H" onClick={() => openInternal("haku://history/")} />
      <MenuItem icon="download" label="Downloads" shortcut="Ctrl+J" onClick={() => openInternal("haku://downloads/")} />
      <MenuItem icon="bookmark" label="Bookmarks" shortcut="Ctrl+Shift+O" onClick={() => openInternal("haku://bookmarks/")} />
      <MenuItem icon="puzzle" label="Extensions" shortcut="Ctrl+Shift+E" onClick={() => openInternal("haku://extensions/")} />
      <MenuItem icon="box" label="Mods" onClick={() => openInternal("haku://mods/")} />
      <MenuItem icon="gamepad-2" label="GX Corner" onClick={() => openInternal("haku://corner/")} />
      <div className="menu-sep" />
      <MenuItem icon="search" label="Find in page" shortcut="Ctrl+F" onClick={() => s.setFindOpen(true)} />
      <MenuItem icon="crop" label="Snapshot" shortcut="Ctrl+Shift+5" onClick={() => send({ type: "page", action: "screenshot", tab: null, data: null })} />
      <MenuItem icon="maximize" label="Full screen" shortcut="F11" onClick={() => send({ type: "window", action: "toggle-fullscreen" })} />
      <MenuItem icon="trash" label="Clear browsing data" shortcut="Ctrl+Shift+Del" onClick={() => s.openModal({ kind: "clear-data" })} />
      <div className="menu-sep" />
      <MenuItem icon="sliders" label="Easy Setup" onClick={() => s.setDrawer("easy-setup")} />
      <MenuItem icon="settings" label="Settings" shortcut="Alt+P" onClick={() => openInternal("haku://settings/")} />
      <MenuItem icon="info" label="Help" onClick={() => openInternal("haku://help/")} />
      <MenuItem icon="star" label="About HakuCX" onClick={() => openInternal("haku://about/")} />
      <div className="menu-sep" />
      <MenuItem icon="x" label="Exit" shortcut="Ctrl+Shift+Q" danger onClick={() => (useStore.getState().settings?.tabs.warnQuit && (useStore.getState().session?.tabs.length ?? 0) > 1 ? s.openModal({ kind: "quit" }) : send({ type: "window", action: "quit" }))} />
    </PopupFrame>
  );
}

function WorkspacesPopup() {
  const session = useStore((s) => s.session);
  const openModal = useStore((s) => s.openModal);
  if (!session) return null;
  return (
    <PopupFrame align="left" width={260} className="menu">
      <div className="popup-title display">Workspaces</div>
      {session.workspaces.map((w) => (
        <div key={w.id} className={clsx("menu-item", w.id === session.activeWorkspace && "current")}>
          <button className="row grow" onClick={() => { send({ type: "switch-workspace", workspace: w.id }); useStore.getState().openPopup(null); }}>
            <Icon name={w.icon} size={15} />
            <span className="grow ellipsis">{w.name}</span>
            <span className="dim">{session.tabs.filter((t) => t.workspaceId === w.id).length}</span>
          </button>
          <button className="icon-btn" title="Edit" onClick={() => openModal({ kind: "workspace-edit", workspace: w.id })}>
            <Icon name="pencil" size={13} />
          </button>
        </div>
      ))}
      <div className="menu-sep" />
      <MenuItem icon="plus" label="Add workspace" onClick={() => openModal({ kind: "workspace-edit", workspace: null })} />
    </PopupFrame>
  );
}

function DownloadsPopup() {
  const downloads = useStore((s) => s.downloads);
  return (
    <PopupFrame width={380} className="downloads-popup">
      <div className="popup-title display">Downloads</div>
      {downloads.length === 0 && <div className="panel-empty">Files you download appear here.</div>}
      <div className="popup-list">
        {downloads.slice(0, 8).map((d, i) => (
          <div key={d.id} className={clsx(i === 0 && "latest")}>
            <DownloadRow d={d} compact />
          </div>
        ))}
      </div>
      <div className="row popup-footer">
        <button className="btn" onClick={() => send({ type: "clear-downloads" })}>
          Clear
        </button>
        <span className="grow" />
        <button className="btn" onClick={() => { openInternal("haku://downloads/"); useStore.getState().openPopup(null); }}>
          Show more
        </button>
      </div>
    </PopupFrame>
  );
}

function PasswordsPopup() {
  const session = useStore((s) => s.session);
  const logins = useStore((s) => s.vault?.logins);
  const activeId = session?.activeTabs[session.activeWorkspace];
  const url = useStore((s) => (activeId ? (s.meta[activeId]?.url ?? "") : ""));
  if (!activeId || !logins) return null;
  const matching = loginsFor(logins, url);
  const close = () => useStore.getState().openPopup(null);
  return (
    <PopupFrame width={320}>
      <div className="row popup-title">
        <span className="display grow">Saved passwords</span>
        <button className="icon-btn" onClick={close}>
          <Icon name="x" size={14} />
        </button>
      </div>
      {matching.map((l) => (
        <button
          key={l.id}
          className="menu-item"
          title="Fill this account into the page"
          onClick={() => {
            send({ type: "autofill-pick", tab: activeId, id: l.id });
            close();
          }}
        >
          <Icon name="key" size={15} />
          <span className="grow">{l.username || "(no username)"}</span>
          <small className="muted">Fill</small>
        </button>
      ))}
      <button className="btn wide" onClick={() => { openInternal("haku://settings/passwords"); close(); }}>
        <Icon name="settings" size={14} /> Manage passwords
      </button>
    </PopupFrame>
  );
}

function ExtensionsPopup() {
  const extensions = useStore((s) => s.extensions);
  const anchor = useStore((s) => s.popupAnchor);
  const usable = extensions.filter((e) => e.enabled && e.version);
  return (
    <PopupFrame width={340}>
      <div className="row popup-title">
        <span className="display grow">Extensions</span>
        <button className="icon-btn" onClick={() => useStore.getState().openPopup(null)}>
          <Icon name="x" size={14} />
        </button>
      </div>
      {usable.length === 0 ? (
        <p className="muted">Extensions you install appear here, with buttons to pin them to the toolbar.</p>
      ) : (
        <div className="ext-popup-list">
          {usable.map((ext) => (
            <div key={ext.id} className="ext-popup-row">
              <button
                className="menu-item grow"
                title={ext.actionTitle}
                disabled={!ext.actionEnabled}
                onClick={() => {
                  useStore.getState().openPopup(null);
                  if (anchor) send({ type: "ext-action-click", id: ext.id, anchor });
                }}
              >
                <ExtIcon src={ext.actionIcon ?? ext.icon} size={18} />
                <span className="grow">{ext.name}</span>
                {ext.badgeText && (
                  <span className="ext-badge inline" style={{ background: ext.badgeColor }}>
                    {ext.badgeText.slice(0, 4)}
                  </span>
                )}
              </button>
              <button
                className={clsx("icon-btn", ext.pinned && "active")}
                title={ext.pinned ? "Unpin from toolbar" : "Pin to toolbar"}
                onClick={() => send({ type: "ext-set-pinned", id: ext.id, pinned: !ext.pinned })}
              >
                <Icon name="pin" size={14} />
              </button>
            </div>
          ))}
        </div>
      )}
      <button className="btn wide" onClick={() => { openInternal("haku://extensions/"); useStore.getState().openPopup(null); }}>
        <Icon name="external" size={14} /> Manage extensions
      </button>
    </PopupFrame>
  );
}

function ProfilePopup() {
  const engine = useStore((s) => s.engine);
  const s = useStore.getState();
  if (!engine) return null;
  return (
    <PopupFrame width={320}>
      <div className="profile-head">
        <span className="avatar" style={{ background: engine.profile.color }}>
          {engine.profile.name[0]?.toUpperCase()}
        </span>
        <div>
          <div className="display">{engine.profile.name}</div>
          <small className="muted">{engine.private ? "Private window" : `${engine.profile.kind} profile`}</small>
        </div>
      </div>
      <div className="menu-sep" />
      <MenuItem icon="users" label="Manage profiles" onClick={() => s.openModal({ kind: "profiles" })} />
      <MenuItem icon="ghost" label="New private window" onClick={() => send({ type: "window", action: "new-private-window" })} />
      <MenuItem icon="refresh" label="Synchronization" onClick={() => openInternal("haku://settings/sync")} />
      <MenuItem icon="pin" label="Pinboards" onClick={() => s.setPanel("pinboards")} />
    </PopupFrame>
  );
}

function SiteInfoPopup() {
  const session = useStore((s) => s.session);
  const settings = useStore((s) => s.settings);
  const activeId = session?.activeTabs[session.activeWorkspace];
  const meta = useStore((st) => (activeId ? st.meta[activeId] : undefined));
  if (!meta || !settings) return null;
  let origin = "";
  try {
    origin = new URL(meta.url).origin;
  } catch {
    origin = "";
  }
  const site = settings.permissions.sites[origin] ?? {};
  const security = meta.security;
  return (
    <PopupFrame align="left" width={340}>
      <div className="popup-title display">{hostOf(meta.url)}</div>
      <div className={clsx("site-security", security)}>
        <Icon name={security === "secure" ? "lock" : security === "internal" ? "shield-check" : "lock-open"} size={16} />
        <span>
          {security === "secure"
            ? "Connection is secure"
            : security === "insecure"
              ? "Connection is not secure. Don't enter passwords or card details."
              : security === "internal"
                ? "This is a HakuCX page"
                : security === "local"
                  ? "You are viewing a local or embedded file"
                  : "Site information"}
        </span>
      </div>
      {Object.keys(site).length > 0 && (
        <>
          <div className="section-label display">Permissions</div>
          {Object.entries(site).map(([kind, value]) => (
            <div key={kind} className="row">
              <span className="grow">{PERMISSION_LABELS[kind] ?? kind}</span>
              <span className={clsx("chip", value === "allow" && "accent")}>{value}</span>
            </div>
          ))}
          <button
            className="btn"
            onClick={() => {
              const sites = { ...settings.permissions.sites };
              delete sites[origin];
              send({ type: "patch-settings", patch: { permissions: { sites: sites as never } } });
            }}
          >
            Reset permissions
          </button>
        </>
      )}
      <div className="menu-sep" />
      <MenuItem icon="settings" label="Site settings" onClick={() => openInternal(`haku://settings/site?origin=${encodeURIComponent(origin)}`)} />
      <MenuItem icon="qr" label="Create QR code" onClick={() => useStore.getState().openModal({ kind: "qr", url: meta.url })} />
    </PopupFrame>
  );
}

function BlockedPopup() {
  const session = useStore((s) => s.session);
  const privacy = useStore((s) => s.settings?.privacy);
  const total = useStore((s) => s.blockedTotal);
  const activeId = session?.activeTabs[session.activeWorkspace];
  const meta = useStore((st) => (activeId ? st.meta[activeId] : undefined));
  if (!meta || !privacy) return null;
  const host = hostOf(meta.url);
  const excepted = privacy.adExceptions.includes(host);
  return (
    <PopupFrame width={320}>
      <div className="popup-title display">Ad and tracker blocker</div>
      <div className="blocked-stats">
        <div>
          <strong>{meta.blocked}</strong>
          <small className="muted">on this page</small>
        </div>
        <div>
          <strong>{total}</strong>
          <small className="muted">this session</small>
        </div>
      </div>
      <button
        className="btn wide"
        onClick={() => {
          const list = excepted ? privacy.adExceptions.filter((h) => h !== host) : [...privacy.adExceptions, host];
          const trackers = excepted ? privacy.trackerExceptions.filter((h) => h !== host) : [...privacy.trackerExceptions, host];
          send({ type: "patch-settings", patch: { privacy: { adExceptions: list, trackerExceptions: trackers } } });
          send({ type: "reload", tab: null, bypassCache: false });
        }}
      >
        {excepted ? `Block ads on ${host}` : `Turn off blocking on ${host}`}
      </button>
    </PopupFrame>
  );
}

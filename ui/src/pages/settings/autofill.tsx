/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Password manager, payment methods, addresses and folder sync settings. Passwords and card
// numbers never reach the UI unless the user asks to see one (after Windows Hello).

import { useEffect, useState } from "react";
import { Button, Toggle } from "../../components/controls";
import { Icon } from "../../components/Icon";
import { Input } from "../../components/Field";
import { copyText, onEvent, send } from "../../ipc/bridge";
import type { Address } from "../../ipc/generated/Address";
import type { CardInfo } from "../../ipc/generated/CardInfo";
import type { LoginInfo } from "../../ipc/generated/LoginInfo";
import { clsx, patch } from "../../lib/util";
import { openInternal, useStore } from "../../state/store";

let revealId = 1;

/** Ask the core for a saved password or card number (Windows Hello may ask first). */
export function reveal(id: string): Promise<string | null> {
  const request = revealId++;
  return new Promise((resolve) => {
    const off = onEvent((e) => {
      if (e.type === "revealed" && e.request === request) {
        off();
        resolve(e.value);
      }
    });
    send({ type: "vault-reveal", request, id });
  });
}

const hostOf = (origin: string) => {
  try {
    return new URL(origin).host;
  } catch {
    return origin;
  }
};

/** Saved logins usable on `url` (same rules as the core: host with or without www, http logins on https). */
export function loginsFor(logins: LoginInfo[], url: string): LoginInfo[] {
  let page: URL;
  try {
    page = new URL(url);
  } catch {
    return [];
  }
  if (page.protocol !== "https:" && page.protocol !== "http:") return [];
  const host = (u: URL) => u.hostname.replace(/^www\./, "");
  return logins.filter((l) => {
    try {
      const saved = new URL(l.origin);
      const sameScheme = saved.protocol === page.protocol;
      const upgrade = saved.protocol === "http:" && page.protocol === "https:";
      const portOk = sameScheme ? saved.port === page.port : !saved.port && !page.port;
      return (sameScheme || upgrade) && portOk && host(saved) === host(page);
    } catch {
      return false;
    }
  });
}

function VaultNotice() {
  const vault = useStore((s) => s.vault);
  const [password, setPassword] = useState("");
  if (!vault) return null;
  const { status } = vault;
  if (status.state === "needs-password") {
    return (
      <div className="callout warn">
        <Icon name="key" size={16} />
        <div className="grow">
          <p>{status.message}</p>
          <div className="row">
            <Input className="text-input" type="password" placeholder="Recovery password" value={password} onChange={(e) => setPassword(e.target.value)} />
            <Button kind="accent" disabled={!password} onClick={() => send({ type: "vault-unlock", password })}>
              Open
            </Button>
          </div>
        </div>
      </div>
    );
  }
  if (status.state === "error" || status.state === "unavailable") {
    return (
      <div className="callout warn">
        <Icon name="alert" size={16} />
        <span>{status.message}</span>
      </div>
    );
  }
  return (
    <>
      {!status.writable && (
        <div className="callout">
          <Icon name="ghost" size={16} />
          <span>This is a private window: saved passwords can be filled in, but nothing new is saved.</span>
        </div>
      )}
      {!status.hello && (
        <div className="callout warn">
          <Icon name="shield" size={16} />
          <span>Windows Hello is not set up, so anyone using this Windows account can show your saved passwords here. Set up a PIN, fingerprint or face in Windows Settings → Accounts → Sign-in options.</span>
        </div>
      )}
    </>
  );
}

// ------------------------------------------------------------------ passwords

type LoginDraft = { id: string | null; site: string; username: string; password: string; note: string; passwordChanged: boolean };

export function PasswordsPage() {
  const vault = useStore((s) => s.vault);
  const offer = useStore((s) => s.settings?.autofill.offerToSave ?? true);
  const suggest = useStore((s) => s.settings?.autofill.suggestPasswords ?? true);
  const [filter, setFilter] = useState("");
  const [draft, setDraft] = useState<LoginDraft | null>(null);
  const [shown, setShown] = useState<Record<string, string>>({});
  if (!vault) return null;
  const ready = vault.status.state === "ready";
  const writable = ready && vault.status.writable;
  const q = filter.trim().toLowerCase();
  const logins = vault.logins.filter((l) => !q || l.origin.toLowerCase().includes(q) || l.username.toLowerCase().includes(q));
  const show = async (id: string) => {
    if (shown[id] !== undefined) {
      setShown(({ [id]: _, ...rest }) => rest);
      return;
    }
    const value = await reveal(id);
    if (value !== null) setShown((s) => ({ ...s, [id]: value }));
  };
  const copy = async (id: string) => {
    const value = shown[id] ?? (await reveal(id));
    if (value !== null) {
      copyText(value);
      useStore.getState().toast("Password copied", "info");
    }
  };
  const edit = async (l: LoginInfo) => {
    const value = await reveal(l.id);
    if (value === null) return;
    setDraft({ id: l.id, site: l.origin, username: l.username, password: value, note: l.note, passwordChanged: false });
  };

  return (
    <div className="vault-page">
      <VaultNotice />
      <div className="settings-card card">
        <div className="setting-line">
          <div className="setting-line-text">
            <span>Offer to save passwords</span>
            <small className="muted">After you sign in, HakuCX asks whether to keep the password in its encrypted vault on this device.</small>
          </div>
          <Toggle on={offer} onChange={(v) => patch({ autofill: { offerToSave: v } })} label="Offer to save passwords" />
        </div>
        <div className="setting-line">
          <div className="setting-line-text">
            <span>Suggest saved accounts on sign-in forms</span>
            <small className="muted">HakuCX fills a password only when you pick an account; it never fills one on its own.</small>
          </div>
          <Toggle on={suggest} onChange={(v) => patch({ autofill: { suggestPasswords: v } })} label="Suggest saved accounts" />
        </div>
      </div>

      <div className="settings-card card">
        <div className="row vault-toolbar">
          <div className="search-box grow">
            <Icon name="search" size={15} />
            <Input placeholder="Search passwords" value={filter} onChange={(e) => setFilter(e.target.value)} />
          </div>
          <Button icon="plus" disabled={!writable} onClick={() => setDraft({ id: null, site: "", username: "", password: "", note: "", passwordChanged: true })}>
            Add
          </Button>
          <Button icon="upload" disabled={!writable} title="Import a CSV export from Chrome, Edge, Opera, Brave, Firefox or a password manager" onClick={() => send({ type: "passwords-import" })}>
            Import
          </Button>
          <Button icon="download" disabled={!ready || vault.logins.length === 0} onClick={() => send({ type: "passwords-export" })}>
            Export
          </Button>
        </div>

        {draft && <LoginEditor draft={draft} onChange={setDraft} onDone={() => setDraft(null)} />}

        {logins.length === 0 ? (
          <p className="muted vault-empty">{vault.logins.length === 0 ? "No saved passwords yet." : "No passwords match."}</p>
        ) : (
          <div className="vault-list">
            {logins.map((l) => (
              <div key={l.id} className="vault-row">
                <span className="vault-site" title={l.origin}>
                  {hostOf(l.origin)}
                </span>
                <span className="vault-user" title={l.username}>
                  {l.username || <i className="muted">no username</i>}
                </span>
                <span className={clsx("vault-secret", shown[l.id] !== undefined && "shown")}>{shown[l.id] ?? "••••••••••"}</span>
                <span className="vault-actions">
                  <button className="icon-btn" title={shown[l.id] !== undefined ? "Hide password" : "Show password"} onClick={() => show(l.id)}>
                    <Icon name={shown[l.id] !== undefined ? "eye-off" : "eye"} size={15} />
                  </button>
                  <button className="icon-btn" title="Copy password" onClick={() => copy(l.id)}>
                    <Icon name="copy" size={15} />
                  </button>
                  <button className="icon-btn" title="Copy username" onClick={() => copyText(l.username)}>
                    <Icon name="user" size={15} />
                  </button>
                  <button className="icon-btn" title="Edit" disabled={!writable} onClick={() => edit(l)}>
                    <Icon name="pencil" size={15} />
                  </button>
                  <button className="icon-btn danger" title="Delete" disabled={!writable} onClick={() => send({ type: "vault-delete", id: l.id })}>
                    <Icon name="trash" size={15} />
                  </button>
                </span>
              </div>
            ))}
          </div>
        )}
      </div>

      {vault.neverSave.length > 0 && (
        <div className="settings-card card">
          <h4 className="vault-heading">Never saved</h4>
          {vault.neverSave.map((origin) => (
            <div key={origin} className="setting-line">
              <div className="setting-line-text">
                <span>{hostOf(origin)}</span>
              </div>
              <Button kind="ghost" icon="x" disabled={!writable} onClick={() => send({ type: "never-save-remove", origin })}>
                Remove
              </Button>
            </div>
          ))}
        </div>
      )}

      <RecoveryPassword />
    </div>
  );
}

function LoginEditor({ draft, onChange, onDone }: { draft: LoginDraft; onChange: (d: LoginDraft) => void; onDone: () => void }) {
  const [visible, setVisible] = useState(false);
  const save = () => {
    send({
      type: "save-login",
      login: { id: draft.id, site: draft.site, username: draft.username, password: draft.passwordChanged ? draft.password : null, note: draft.note },
    });
    onDone();
  };
  return (
    <div className="vault-editor">
      <h4 className="vault-heading">{draft.id ? "Edit password" : "Add a password"}</h4>
      <label className="vault-field">
        <span>Site</span>
        <Input className="text-input" placeholder="example.com" value={draft.site} onChange={(e) => onChange({ ...draft, site: e.target.value })} />
      </label>
      <label className="vault-field">
        <span>Username</span>
        <Input className="text-input" value={draft.username} onChange={(e) => onChange({ ...draft, username: e.target.value })} />
      </label>
      <label className="vault-field">
        <span>Password</span>
        <span className="row">
          <Input className="text-input grow" type={visible ? "text" : "password"} value={draft.password} onChange={(e) => onChange({ ...draft, password: e.target.value, passwordChanged: true })} />
          <button className="icon-btn" title={visible ? "Hide" : "Show"} onClick={() => setVisible(!visible)}>
            <Icon name={visible ? "eye-off" : "eye"} size={15} />
          </button>
        </span>
      </label>
      <label className="vault-field">
        <span>Note</span>
        <Input className="text-input" value={draft.note} onChange={(e) => onChange({ ...draft, note: e.target.value })} />
      </label>
      <div className="row">
        <span className="grow" />
        <Button kind="ghost" onClick={onDone}>
          Cancel
        </Button>
        <Button kind="accent" disabled={!draft.site.trim() || !draft.password} onClick={save}>
          Save
        </Button>
      </div>
    </div>
  );
}

function RecoveryPassword() {
  const status = useStore((s) => s.vault?.status);
  const [open, setOpen] = useState(false);
  const [a, setA] = useState("");
  const [b, setB] = useState("");
  if (!status || status.state !== "ready" || !status.writable) return null;
  return (
    <div className="settings-card card">
      <div className="setting-line">
        <div className="setting-line-text">
          <span>Recovery password</span>
          <small className="muted">
            Windows protects your vault with your account's keys. A recovery password also opens it if those keys are lost (a Windows reinstall, a profile copied to another PC).{" "}
            {status.recoveryPassword ? "A recovery password is set." : "No recovery password is set."}
          </small>
        </div>
        <span className="row">
          {status.recoveryPassword && (
            <Button kind="ghost" onClick={() => send({ type: "vault-set-recovery", password: null })}>
              Remove
            </Button>
          )}
          <Button onClick={() => setOpen(!open)}>{status.recoveryPassword ? "Change" : "Set"}</Button>
        </span>
      </div>
      {open && (
        <div className="vault-editor">
          <Input className="text-input" type="password" placeholder="New recovery password (8+ characters)" value={a} onChange={(e) => setA(e.target.value)} />
          <Input className="text-input" type="password" placeholder="Repeat it" value={b} onChange={(e) => setB(e.target.value)} />
          <div className="row">
            <small className="muted grow">HakuCX cannot recover this password for you. Keep it somewhere safe.</small>
            <Button
              kind="accent"
              disabled={a.length < 8 || a !== b}
              onClick={() => {
                send({ type: "vault-set-recovery", password: a });
                setA("");
                setB("");
                setOpen(false);
              }}
            >
              Save
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

// ------------------------------------------------------------------ payment methods

type CardDraft = { id: string | null; nickname: string; holder: string; number: string; expMonth: string; expYear: string; numberChanged: boolean };

export function PaymentMethodsPage() {
  const vault = useStore((s) => s.vault);
  const on = useStore((s) => s.settings?.autofill.paymentMethods ?? true);
  const verify = useStore((s) => s.settings?.autofill.verifyCards ?? true);
  const [draft, setDraft] = useState<CardDraft | null>(null);
  if (!vault) return null;
  const writable = vault.status.state === "ready" && vault.status.writable;
  const edit = (c: CardInfo) =>
    setDraft({ id: c.id, nickname: c.nickname, holder: c.holder, number: "", expMonth: String(c.expMonth), expYear: String(c.expYear), numberChanged: false });
  return (
    <div className="vault-page">
      <VaultNotice />
      <div className="settings-card card">
        <div className="setting-line">
          <div className="setting-line-text">
            <span>Save and fill payment methods</span>
            <small className="muted">Cards are offered on secure (https) checkout forms. The security code is never stored.</small>
          </div>
          <Toggle on={on} onChange={(v) => patch({ autofill: { paymentMethods: v } })} label="Payment methods" />
        </div>
        <div className="setting-line">
          <div className="setting-line-text">
            <span>Verify it's you before filling a card</span>
            <small className="muted">Uses Windows Hello.</small>
          </div>
          <Toggle on={verify} onChange={(v) => patch({ autofill: { verifyCards: v } })} label="Verify before filling cards" />
        </div>
      </div>
      <div className="settings-card card">
        <div className="row vault-toolbar">
          <h4 className="vault-heading grow">Cards</h4>
          <Button icon="plus" disabled={!writable} onClick={() => setDraft({ id: null, nickname: "", holder: "", number: "", expMonth: "", expYear: "", numberChanged: true })}>
            Add card
          </Button>
        </div>
        {draft && <CardEditor draft={draft} onChange={setDraft} onDone={() => setDraft(null)} />}
        {vault.cards.length === 0 ? (
          <p className="muted vault-empty">No saved cards.</p>
        ) : (
          <div className="vault-list">
            {vault.cards.map((c) => (
              <div key={c.id} className="vault-row">
                <span className="vault-site">{c.nickname || c.brand}</span>
                <span className="vault-user">•••• {c.last4}</span>
                <span className="vault-secret">
                  {String(c.expMonth).padStart(2, "0")}/{c.expYear} {c.holder && `· ${c.holder}`}
                </span>
                <span className="vault-actions">
                  <button className="icon-btn" title="Edit" disabled={!writable} onClick={() => edit(c)}>
                    <Icon name="pencil" size={15} />
                  </button>
                  <button className="icon-btn danger" title="Delete" disabled={!writable} onClick={() => send({ type: "vault-delete", id: c.id })}>
                    <Icon name="trash" size={15} />
                  </button>
                </span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function CardEditor({ draft, onChange, onDone }: { draft: CardDraft; onChange: (d: CardDraft) => void; onDone: () => void }) {
  const save = () => {
    send({
      type: "save-card",
      card: {
        id: draft.id,
        nickname: draft.nickname.trim(),
        holder: draft.holder.trim(),
        number: draft.numberChanged ? draft.number : null,
        expMonth: Number(draft.expMonth) || 0,
        expYear: Number(draft.expYear) || 0,
      },
    });
    onDone();
  };
  return (
    <div className="vault-editor">
      <h4 className="vault-heading">{draft.id ? "Edit card" : "Add a card"}</h4>
      <label className="vault-field">
        <span>Card number</span>
        <Input
          className="text-input"
          inputMode="numeric"
          autoComplete="off"
          placeholder={draft.id && !draft.numberChanged ? "Unchanged" : "1234 5678 9012 3456"}
          value={draft.number}
          onChange={(e) => onChange({ ...draft, number: e.target.value, numberChanged: true })}
        />
      </label>
      <label className="vault-field">
        <span>Name on card</span>
        <Input className="text-input" value={draft.holder} onChange={(e) => onChange({ ...draft, holder: e.target.value })} />
      </label>
      <label className="vault-field">
        <span>Expires</span>
        <span className="row">
          <Input className="text-input" style={{ width: 70 }} placeholder="MM" value={draft.expMonth} onChange={(e) => onChange({ ...draft, expMonth: e.target.value.replace(/\D/g, "").slice(0, 2) })} />
          <Input className="text-input" style={{ width: 90 }} placeholder="YYYY" value={draft.expYear} onChange={(e) => onChange({ ...draft, expYear: e.target.value.replace(/\D/g, "").slice(0, 4) })} />
        </span>
      </label>
      <label className="vault-field">
        <span>Nickname</span>
        <Input className="text-input" placeholder="Optional" value={draft.nickname} onChange={(e) => onChange({ ...draft, nickname: e.target.value })} />
      </label>
      <div className="row">
        <span className="grow" />
        <Button kind="ghost" onClick={onDone}>
          Cancel
        </Button>
        <Button kind="accent" disabled={(draft.numberChanged && draft.number.replace(/\D/g, "").length < 12) || !draft.expMonth || draft.expYear.length !== 4} onClick={save}>
          Save
        </Button>
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ addresses

const EMPTY_ADDRESS: Address = {
  id: "",
  fullName: "",
  organization: "",
  street: "",
  city: "",
  region: "",
  postalCode: "",
  country: "",
  phone: "",
  email: "",
  created: 0,
  modified: 0,
};

const ADDRESS_FIELDS: [keyof Address, string][] = [
  ["fullName", "Name"],
  ["organization", "Organization"],
  ["street", "Street address"],
  ["city", "City"],
  ["region", "State / province / county"],
  ["postalCode", "Postal code"],
  ["country", "Country"],
  ["phone", "Phone"],
  ["email", "Email"],
];

export function AddressesPage() {
  const vault = useStore((s) => s.vault);
  const on = useStore((s) => s.settings?.autofill.addresses ?? true);
  const [draft, setDraft] = useState<Address | null>(null);
  if (!vault) return null;
  const writable = vault.status.state === "ready" && vault.status.writable;
  return (
    <div className="vault-page">
      <VaultNotice />
      <div className="settings-card card">
        <div className="setting-line">
          <div className="setting-line-text">
            <span>Save and fill addresses</span>
            <small className="muted">Offered when you click into a name, address, phone or email field on a form.</small>
          </div>
          <Toggle on={on} onChange={(v) => patch({ autofill: { addresses: v } })} label="Addresses" />
        </div>
      </div>
      <div className="settings-card card">
        <div className="row vault-toolbar">
          <h4 className="vault-heading grow">Addresses</h4>
          <Button icon="plus" disabled={!writable} onClick={() => setDraft({ ...EMPTY_ADDRESS })}>
            Add address
          </Button>
        </div>
        {draft && (
          <div className="vault-editor">
            {ADDRESS_FIELDS.map(([key, label]) => (
              <label key={key} className="vault-field">
                <span>{label}</span>
                {key === "street" ? (
                  <textarea className="text-input" rows={2} defaultValue={draft.street} onBlur={(e) => setDraft({ ...draft, street: e.target.value })} />
                ) : (
                  <Input className="text-input" value={String(draft[key])} onChange={(e) => setDraft({ ...draft, [key]: e.target.value })} />
                )}
              </label>
            ))}
            <div className="row">
              <span className="grow" />
              <Button kind="ghost" onClick={() => setDraft(null)}>
                Cancel
              </Button>
              <Button
                kind="accent"
                onClick={() => {
                  send({ type: "save-address", address: draft });
                  setDraft(null);
                }}
              >
                Save
              </Button>
            </div>
          </div>
        )}
        {vault.addresses.length === 0 ? (
          <p className="muted vault-empty">No saved addresses.</p>
        ) : (
          <div className="vault-list">
            {vault.addresses.map((a) => (
              <div key={a.id} className="vault-row">
                <span className="vault-site">{a.fullName || a.organization || "Address"}</span>
                <span className="vault-user">{a.street.split("\n")[0]}</span>
                <span className="vault-secret">{[a.city, a.postalCode, a.country].filter(Boolean).join(", ")}</span>
                <span className="vault-actions">
                  <button className="icon-btn" title="Edit" disabled={!writable} onClick={() => setDraft({ ...a })}>
                    <Icon name="pencil" size={15} />
                  </button>
                  <button className="icon-btn danger" title="Delete" disabled={!writable} onClick={() => send({ type: "vault-delete", id: a.id })}>
                    <Icon name="trash" size={15} />
                  </button>
                </span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ sync

const ago = (ms: number) => {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return new Date(ms).toLocaleDateString();
};

/** Folder sync setup, status, and tabs from other devices. */
export function SyncPanel() {
  const status = useStore((s) => s.sync);
  const settings = useStore((s) => s.settings?.sync);
  const [folder, setFolder] = useState(settings?.folder ?? "");
  const [passphrase, setPassphrase] = useState("");
  const [, tick] = useState(0);
  useEffect(() => {
    const t = window.setInterval(() => tick((n) => n + 1), 30_000);
    return () => window.clearInterval(t);
  }, []);
  if (!status || !settings) return null;
  const connected = settings.folderSync && status.state !== "needs-passphrase" && status.state !== "off";

  if (!connected) {
    return (
      <div className="sync-setup">
        <p className="muted">
          HakuCX has no sync servers. Pick a folder your devices already share (OneDrive, Dropbox, Google Drive, Syncthing, a network drive) and a passphrase. Every device that
          uses the same folder and passphrase shares what you choose below. Everything HakuCX writes there is encrypted with the passphrase.
        </p>
        {status.message && (
          <div className="callout warn">
            <Icon name="alert" size={16} />
            <span>{status.message}</span>
          </div>
        )}
        <label className="vault-field">
          <span>Shared folder</span>
          <Input className="text-input" placeholder="C:\Users\you\OneDrive" value={folder} onChange={(e) => setFolder(e.target.value)} />
        </label>
        <label className="vault-field">
          <span>Sync passphrase</span>
          <Input className="text-input" type="password" placeholder="At least 10 characters; the same on every device" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} />
        </label>
        <div className="row">
          <small className="muted grow">If you forget the passphrase, start over with a new folder: nobody can recover it.</small>
          <Button
            kind="accent"
            icon="refresh"
            disabled={!folder.trim() || passphrase.length < 10 || status.state === "syncing"}
            onClick={() => {
              send({ type: "sync-setup", folder: folder.trim(), passphrase });
              setPassphrase("");
            }}
          >
            {status.state === "syncing" ? "Checking..." : "Start syncing"}
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="sync-setup">
      <div className="setting-line">
        <div className="setting-line-text">
          <span>
            Syncing through <code>{settings.folder}</code>
          </span>
          <small className={clsx("muted", status.state === "error" && "warn-text")}>
            {status.state === "syncing" ? "Syncing..." : status.state === "error" ? status.message : status.lastSync ? `Last synced ${ago(status.lastSync)}` : "Not synced yet"}
            {status.state !== "error" && status.message ? ` · ${status.message}` : ""}
          </small>
        </div>
        <span className="row">
          <Button icon="refresh" disabled={status.state === "syncing"} onClick={() => send({ type: "sync-now" })}>
            Sync now
          </Button>
          <Button kind="ghost" onClick={() => send({ type: "sync-disconnect" })}>
            Stop syncing
          </Button>
        </span>
      </div>
      {status.devices.length > 0 && (
        <div className="sync-devices">
          <h4 className="vault-heading">Other devices</h4>
          {status.devices.map((d) => (
            <details key={d.id} className="sync-device">
              <summary>
                <Icon name="monitor" size={14} /> {d.name} <span className="muted">· {ago(d.writtenAt)}</span> {d.tabs.length > 0 && <span className="muted">· {d.tabs.length} tabs</span>}
              </summary>
              {d.tabs.map((t, i) => (
                <button key={i} className="sync-tab" title={t.url} onClick={() => send({ type: "new-tab", url: t.url, background: false, relativeTo: null })}>
                  <span className="grow">{t.title || t.url}</span>
                  {t.workspace && <span className="muted">{t.workspace}</span>}
                </button>
              ))}
            </details>
          ))}
        </div>
      )}
    </div>
  );
}

/** Link from other pages to the password manager. */
export function openPasswordManager() {
  openInternal("haku://settings/passwords");
}

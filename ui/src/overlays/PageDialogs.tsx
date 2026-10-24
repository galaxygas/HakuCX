/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Prompts that come from web pages. They are drawn inside the page area and always show the
// site's origin, so a page cannot pass them off as browser UI.

import { useEffect, useRef, useState } from "react";
import { Button, Checkbox } from "../components/controls";
import { Icon } from "../components/Icon";
import { send } from "../ipc/bridge";
import { PERMISSION_LABELS } from "../lib/catalog";
import { clsx } from "../lib/util";
import { useStore } from "../state/store";
import { HexInput } from "./Modals";
import { Input } from "../components/Field";

export function PageDialogs() {
  return (
    <>
      <DialogHost />
      <SelectHost />
      <ColorHost />
      <PermissionHost />
      <AuthHost />
      <EasyFilesHost />
    </>
  );
}

function DialogHost() {
  const dialog = useStore((s) => s.dialogs[0]);
  const shift = useStore((s) => s.shiftDialog);
  const [value, setValue] = useState("");
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    setValue(dialog?.defaultValue ?? "");
    window.setTimeout(() => input.current?.focus(), 0);
  }, [dialog?.id, dialog?.defaultValue]);
  if (!dialog) return null;
  const answer = (confirmed: boolean) => {
    send({ type: "dialog-response", id: dialog.id, confirmed, value: dialog.kind === "prompt" ? value : null });
    shift();
  };
  const leave = dialog.kind === "before-unload";
  return (
    <div className="page-dialog-backdrop">
      <div className="page-dialog" role="dialog">
        <div className="page-dialog-origin">
          <Icon name="globe" size={14} /> {dialog.origin || "This page"} says
        </div>
        <p className="page-dialog-message">{dialog.message}</p>
        {dialog.kind === "prompt" && (
          <Input ref={input} className="text-input" value={value} onChange={(e) => setValue(e.target.value)} onKeyDown={(e) => e.key === "Enter" && answer(true)} />
        )}
        <div className="row page-dialog-buttons">
          <span className="grow" />
          {dialog.kind !== "alert" && <Button onClick={() => answer(false)}>{leave ? "Stay" : "Cancel"}</Button>}
          <Button kind="accent" onClick={() => answer(true)}>
            {leave ? "Leave" : "OK"}
          </Button>
        </div>
      </div>
    </div>
  );
}

function SelectHost() {
  const select = useStore((s) => s.select);
  const clear = useStore((s) => s.clearSelect);
  const [chosen, setChosen] = useState<number[]>([]);
  useEffect(() => setChosen(select?.selected ?? []), [select]);
  if (!select) return null;
  const respond = (selected: number[]) => {
    send({ type: "select-response", id: select.id, selected });
    clear();
  };
  const top = Math.min(select.rect.y + select.rect.h, window.innerHeight - 260);
  let group: string | null = null;
  return (
    <>
      <div className="scrim" onMouseDown={() => respond(select.selected)} />
      <div className="popup select-popup" style={{ left: select.rect.x, top, minWidth: Math.max(160, select.rect.w) }} onMouseDown={(e) => e.stopPropagation()}>
        {select.options.map((o) => {
          const header = o.group !== group ? (group = o.group) : null;
          const on = chosen.includes(o.index);
          return (
            <div key={o.index}>
              {header && <div className="select-group">{header}</div>}
              <button
                className={clsx("menu-item", on && "selected")}
                disabled={o.disabled}
                onClick={() => {
                  if (select.multiple) setChosen((c) => (on ? c.filter((x) => x !== o.index) : [...c, o.index]));
                  else respond([o.index]);
                }}
              >
                {select.multiple && <Icon name={on ? "check" : "square"} size={13} />}
                <span className="grow">{o.label}</span>
              </button>
            </div>
          );
        })}
        {select.multiple && (
          <div className="row popup-footer">
            <span className="grow" />
            <Button kind="accent" onClick={() => respond(chosen)}>
              Done
            </Button>
          </div>
        )}
      </div>
    </>
  );
}

const PICKER_COLORS = ["#000000", "#ffffff", "#ff4458", "#ff9900", "#ffe20f", "#36ff4e", "#00e5c3", "#00a4cd", "#4461ee", "#7d46f2", "#ff0077", "#8d8d8d"];

function ColorHost() {
  const color = useStore((s) => s.color);
  const clear = useStore((s) => s.clearColor);
  const [value, setValue] = useState("#000000");
  useEffect(() => setValue(color?.color ?? "#000000"), [color]);
  if (!color) return null;
  const respond = (c: string | null) => {
    send({ type: "color-response", id: color.id, color: c });
    clear();
  };
  return (
    <>
      <div className="scrim" onMouseDown={() => respond(null)} />
      <div className="popup color-popup" style={{ left: color.rect.x, top: Math.min(color.rect.y + color.rect.h, window.innerHeight - 200) }} onMouseDown={(e) => e.stopPropagation()}>
        <div className="swatches">
          {PICKER_COLORS.map((c) => (
            <button key={c} className={clsx("swatch", value === c && "selected")} style={{ background: c }} onClick={() => setValue(c)} />
          ))}
        </div>
        <HexInput value={value} onChange={setValue} />
        <div className="row">
          <span className="grow" />
          <Button onClick={() => respond(null)}>Cancel</Button>
          <Button kind="accent" onClick={() => respond(value)}>
            Choose
          </Button>
        </div>
      </div>
    </>
  );
}

function PermissionHost() {
  const prompt = useStore((s) => s.permissions[0]);
  const shift = useStore((s) => s.shiftPermission);
  const quiet = useStore((s) => s.settings?.permissions.quietRequests ?? false);
  const open = useStore((s) => s.permissionOpen);
  const [remember, setRemember] = useState(true);
  // Quieter messaging: the request waits behind the address bar icon until clicked.
  if (!prompt || (quiet && !open)) return null;
  const answer = (allow: boolean) => {
    send({ type: "permission-response", id: prompt.id, allow, remember });
    shift();
  };
  const label = PERMISSION_LABELS[prompt.kind] ?? prompt.kind;
  return (
    <div className="permission-prompt" role="dialog">
      <div className="row">
        <Icon name={prompt.kind === "camera" ? "camera" : prompt.kind === "microphone" ? "mic" : prompt.kind === "geolocation" ? "map-pin" : prompt.kind === "notifications" ? "bell" : "shield"} size={18} />
        <div className="grow">
          <strong>{prompt.origin}</strong> wants to use: <strong>{label}</strong>
        </div>
      </div>
      <Checkbox checked={remember} onChange={setRemember} label="Remember my decision for this site" />
      <div className="row">
        <span className="grow" />
        <Button onClick={() => answer(false)}>Block</Button>
        <Button kind="accent" onClick={() => answer(true)}>
          Allow
        </Button>
      </div>
    </div>
  );
}

function AuthHost() {
  const auth = useStore((s) => s.auth);
  const clear = useStore((s) => s.clearAuth);
  const [user, setUser] = useState("");
  const [pass, setPass] = useState("");
  if (!auth) return null;
  const respond = (ok: boolean) => {
    send({ type: "auth-response", id: auth.id, username: ok ? user : null, password: ok ? pass : null });
    setUser("");
    setPass("");
    clear();
  };
  let host = auth.url;
  try {
    host = new URL(auth.url).host;
  } catch {
    // keep the raw URL
  }
  return (
    <div className="page-dialog-backdrop">
      <div className="page-dialog" role="dialog">
        <div className="page-dialog-origin">
          <Icon name="lock" size={14} /> {auth.proxy ? "The proxy" : host} requires a sign-in
        </div>
        <label className="field">
          <span>Username</span>
          <Input className="text-input" value={user} onChange={(e) => setUser(e.target.value)} autoFocus />
        </label>
        <label className="field">
          <span>Password</span>
          <Input className="text-input" type="password" value={pass} onChange={(e) => setPass(e.target.value)} onKeyDown={(e) => e.key === "Enter" && respond(true)} />
        </label>
        <div className="row page-dialog-buttons">
          <span className="grow" />
          <Button onClick={() => respond(false)}>Cancel</Button>
          <Button kind="accent" onClick={() => respond(true)}>
            Sign in
          </Button>
        </div>
      </div>
    </div>
  );
}

function fileSize(n: number) {
  return n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(0)} KB` : `${(n / 1048576).toFixed(1)} MB`;
}

/** Easy Files: a page asked for a file; offer recent downloads before the system dialog. */
function EasyFilesHost() {
  const offer = useStore((s) => s.easyFiles);
  const [picked, setPicked] = useState<string[]>([]);
  useEffect(() => setPicked([]), [offer?.id]);
  if (!offer) return null;
  const answer = (paths: string[], browse: boolean) => {
    send({ type: "file-picker-response", id: offer.id, paths, browse });
    useStore.setState({ easyFiles: null });
  };
  return (
    <div className="page-dialog-backdrop">
      <div className="page-dialog easy-files" role="dialog">
        <div className="display">Choose a file</div>
        <p className="muted small">Recent downloads</p>
        <div className="easy-files-list">
          {offer.files.map((f) => {
            const on = picked.includes(f.path);
            return (
              <button
                key={f.path}
                className={clsx("easy-file", on && "on")}
                title={f.path}
                onClick={() => (offer.multiple ? setPicked(on ? picked.filter((p) => p !== f.path) : [...picked, f.path]) : answer([f.path], false))}
              >
                <Icon name={f.mime.startsWith("image/") ? "image" : f.mime.startsWith("video/") ? "film" : "file"} size={16} />
                <span className="grow ellipsis">{f.name}</span>
                <small className="dim">{fileSize(Number(f.size))}</small>
              </button>
            );
          })}
        </div>
        <div className="row">
          <Button kind="ghost" onClick={() => answer([], true)}>
            Browse your computer...
          </Button>
          <span className="grow" />
          <Button onClick={() => answer([], false)}>Cancel</Button>
          {offer.multiple && (
            <Button kind="accent" disabled={!picked.length} onClick={() => answer(picked, false)}>
              Upload {picked.length || ""}
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}

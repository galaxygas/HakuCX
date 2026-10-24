/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// The "save password?" prompt and the suggestions shown under a focused login, address or
// card field. Both are browser UI drawn over the page, so a page cannot fake or read them.

import { useEffect, useState } from "react";
import { Button } from "../components/controls";
import { Icon } from "../components/Icon";
import { Input } from "../components/Field";
import { send } from "../ipc/bridge";
import { clsx } from "../lib/util";
import { useStore } from "../state/store";

export function PasswordPromptHost() {
  const prompt = useStore((s) => s.passwordPrompt);
  const activeTab = useStore((s) => s.session?.activeTabs[s.session.activeWorkspace]);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    setUsername(prompt?.username ?? "");
    setPassword(prompt?.password ?? "");
    setVisible(false);
  }, [prompt]);
  if (!prompt || prompt.tab !== activeTab) return null;
  const answer = (action: "save" | "never" | "dismiss") => {
    send({ type: "password-prompt-answer", prompt: prompt.prompt, action, username, password });
    useStore.setState({ passwordPrompt: null });
  };
  let host = prompt.origin;
  try {
    host = new URL(prompt.origin).host;
  } catch {
    // Show the origin as is.
  }
  return (
    <div className="popup password-prompt" data-hit onMouseDown={(e) => e.stopPropagation()}>
      <div className="row popup-title">
        <Icon name="key" size={16} />
        <span className="display grow">{prompt.update ? "Update password?" : "Save password?"}</span>
        <button className="icon-btn" title="Not now" onClick={() => answer("dismiss")}>
          <Icon name="x" size={14} />
        </button>
      </div>
      <p className="muted">
        {prompt.update ? "Replace the saved password for " : "HakuCX can remember this sign-in for "}
        <b>{host}</b>, encrypted on this device.
      </p>
      <label className="vault-field">
        <span>Username</span>
        <Input className="text-input" value={username} onChange={(e) => setUsername(e.target.value)} />
      </label>
      <label className="vault-field">
        <span>Password</span>
        <span className="row">
          <Input className="text-input grow" type={visible ? "text" : "password"} value={password} onChange={(e) => setPassword(e.target.value)} />
          <button className="icon-btn" title={visible ? "Hide password" : "Show password"} onClick={() => setVisible(!visible)}>
            <Icon name={visible ? "eye-off" : "eye"} size={15} />
          </button>
        </span>
      </label>
      <div className="row">
        {!prompt.update && (
          <Button kind="ghost" onClick={() => answer("never")}>
            Never for this site
          </Button>
        )}
        <span className="grow" />
        <Button kind="accent" disabled={!password} onClick={() => answer("save")}>
          {prompt.update ? "Update" : "Save"}
        </Button>
      </div>
    </div>
  );
}

export function AutofillHost() {
  const suggest = useStore((s) => s.autofill);
  const activeTab = useStore((s) => s.session?.activeTabs[s.session.activeWorkspace]);
  const [selected, setSelected] = useState(0);
  useEffect(() => setSelected(0), [suggest]);
  if (!suggest || suggest.tab !== activeTab || suggest.items.length === 0) return null;
  const pick = (id: string) => {
    send({ type: "autofill-pick", tab: suggest.tab, id });
    useStore.setState({ autofill: null });
  };
  const width = Math.max(240, Math.min(360, suggest.rect.w));
  const below = suggest.rect.y + suggest.rect.h + 4;
  const top = below + 48 * suggest.items.length + 40 > window.innerHeight ? Math.max(8, suggest.rect.y - 4 - (48 * suggest.items.length + 40)) : below;
  const left = Math.max(8, Math.min(window.innerWidth - width - 8, suggest.rect.x));
  const icon = suggest.kind === "login" ? "key" : suggest.kind === "card" ? "card" : "map-pin";
  return (
    <div className="popup menu autofill-menu" data-hit style={{ left, top, width }} onMouseDown={(e) => e.stopPropagation()}>
      {suggest.items.map((item, i) => (
        <button key={item.id} className={clsx("menu-item autofill-item", i === selected && "selected")} onMouseEnter={() => setSelected(i)} onClick={() => pick(item.id)}>
          <Icon name={icon} size={15} />
          <span className="grow autofill-text">
            <span>{item.label}</span>
            {item.sublabel && <small className="muted">{item.sublabel}</small>}
          </span>
        </button>
      ))}
      <div className="menu-sep" />
      <button
        className="menu-item"
        onClick={() => {
          useStore.setState({ autofill: null });
          const page = suggest.kind === "login" ? "passwords" : suggest.kind === "card" ? "payment-methods" : "addresses";
          send({ type: "new-tab", url: `haku://settings/${page}`, background: false, relativeTo: null });
        }}
      >
        <Icon name="settings" size={15} />
        <span className="grow">Manage {suggest.kind === "login" ? "passwords" : suggest.kind === "card" ? "payment methods" : "addresses"}</span>
      </button>
    </div>
  );
}

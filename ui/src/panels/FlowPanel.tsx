/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// My Flow: notes, links and files between HakuCX and your phone, end-to-end encrypted over the
// local network (see haku-core's flow.rs). The phone connects by scanning a QR code; no account,
// no server.

import QRCode from "qrcode";
import { useEffect, useRef, useState } from "react";
import { Button } from "../components/controls";
import { Input } from "../components/Field";
import { Icon } from "../components/Icon";
import { copyText, send } from "../ipc/bridge";
import type { FlowEntry } from "../ipc/generated/FlowEntry";
import { clsx, patch, timeOfDay } from "../lib/util";
import { useStore } from "../state/store";

function size(n: bigint | number) {
  const v = Number(n);
  return v < 1024 ? `${v} B` : v < 1048576 ? `${(v / 1024).toFixed(0)} KB` : `${(v / 1048576).toFixed(1)} MB`;
}

function Pairing({ url, expires }: { url: string; expires: number }) {
  const [svg, setSvg] = useState("");
  const [left, setLeft] = useState(() => Math.max(0, expires - Date.now()));
  useEffect(() => {
    QRCode.toString(url, { type: "svg", margin: 1, errorCorrectionLevel: "M", color: { dark: "#000000", light: "#ffffff" } }).then(setSvg, () => setSvg(""));
  }, [url]);
  useEffect(() => {
    const t = setInterval(() => setLeft(Math.max(0, expires - Date.now())), 1000);
    return () => clearInterval(t);
  }, [expires]);
  const mins = Math.ceil(left / 60000);
  return (
    <div className="flow-pairing card">
      <div className="qr-box" dangerouslySetInnerHTML={{ __html: svg }} />
      {/* Numbered by hand: Servo draws every <ol> item as "0." */}
      <div className="flow-steps">
        {[
          "Connect your phone to the same Wi-Fi as this computer.",
          "Scan this code with the phone's camera and open the link.",
          "If Windows asks whether HakuCX may use the network, allow it on private networks.",
        ].map((step, i) => (
          <div key={i} className="flow-step">
            <span className="flow-step-n">{i + 1}</span>
            <span>{step}</span>
          </div>
        ))}
      </div>
      <p className="muted small">
        The code works once, for {left > 0 ? `${mins} more minute${mins === 1 ? "" : "s"}` : "no more time: show a new one"}. Everything between your phone and HakuCX is
        encrypted; nothing goes through the internet.
      </p>
      <div className="row">
        <Button kind="ghost" icon="copy" title="For a tablet or computer without a camera: open this link on it" onClick={() => copyText(url)}>
          Copy link
        </Button>
        <Button kind="ghost" onClick={() => send({ type: "flow-cancel-pairing" })}>
          Cancel
        </Button>
      </div>
    </div>
  );
}

function Bubble({ e }: { e: FlowEntry }) {
  const mine = e.from === "pc";
  return (
    <div className={clsx("flow-bubble", mine ? "mine" : "theirs")}>
      {e.kind === "link" ? (
        <button className="pin-link" title="Open in a new tab" onClick={() => send({ type: "flow-open", id: e.id })}>
          <Icon name="link" size={13} />
          <span className="ellipsis">{e.text}</span>
        </button>
      ) : e.kind === "file" && e.file ? (
        <div className="flow-file">
          <Icon name="file" size={16} />
          <span className="grow ellipsis" title={e.file.name}>
            {e.file.name} <small className="dim">{size(e.file.size)}</small>
          </span>
          <button className="icon-btn" title="Open" onClick={() => send({ type: "flow-open", id: e.id })}>
            <Icon name="external" size={14} />
          </button>
          <button className="icon-btn" title="Show in folder" onClick={() => send({ type: "flow-show-file", id: e.id })}>
            <Icon name="folder-open" size={14} />
          </button>
        </div>
      ) : (
        <span className="flow-text">{e.text}</span>
      )}
      <small className="dim">
        {mine ? "This computer" : e.fromName} · {timeOfDay(Number(e.at))}
        <button className="flow-remove" title="Remove" onClick={() => send({ type: "flow-remove", id: e.id })}>
          <Icon name="x" size={11} />
        </button>
      </small>
    </div>
  );
}

export function FlowPanel() {
  const status = useStore((s) => s.flow);
  const [text, setText] = useState("");
  const list = useRef<HTMLDivElement>(null);
  const count = status?.entries.length ?? 0;
  useEffect(() => {
    send({ type: "flow-seen" });
  }, [count]);
  useEffect(() => {
    list.current?.scrollTo({ top: list.current.scrollHeight });
  }, [count]);
  if (!status) return null;
  if (status.state === "off") {
    return (
      <div className="panel-scroll flow-panel">
        <div className="panel-empty">
          <Icon name="waves" size={28} />
          <p>My Flow sends notes, links and files between HakuCX and your phone over your Wi-Fi, encrypted end to end. No account needed.</p>
          <Button kind="accent" onClick={() => patch({ features: { myFlow: true } })}>
            Turn on My Flow
          </Button>
        </div>
      </div>
    );
  }
  const submit = () => {
    if (text.trim()) {
      send({ type: "flow-send", text });
      setText("");
    }
  };
  return (
    <div className="flow-panel">
      {status.message && <p className="note warn">{status.message}</p>}
      {status.state !== "unavailable" && (
        <div className="flow-devices">
          {status.devices.map((d) => (
            <div key={d.id} className="flow-device">
              <Icon name="phone2" size={15} />
              <span className="grow ellipsis">
                {d.name}
                {d.lastSeen != null && <small className="dim"> · seen {timeOfDay(Number(d.lastSeen))}</small>}
              </span>
              <button className="icon-btn" title={`Disconnect ${d.name}`} onClick={() => send({ type: "flow-forget-device", device: d.id })}>
                <Icon name="x" size={13} />
              </button>
            </div>
          ))}
          {!status.pairing && (
            <Button icon="qr" onClick={() => send({ type: "flow-pair" })}>
              {status.devices.length ? "Connect another phone" : "Connect phone"}
            </Button>
          )}
          {status.address && <small className="dim">Listening on {status.address} (your local network only)</small>}
        </div>
      )}
      {status.pairing && <Pairing url={status.pairing.url} expires={Number(status.pairing.expires)} />}
      <div className="flow-list panel-scroll" ref={list}>
        {status.entries.length === 0 && <p className="muted small">Notes, links and files you send appear here and on your phone.</p>}
        {status.entries.map((e) => (
          <Bubble key={e.id} e={e} />
        ))}
      </div>
      {status.state === "ready" && (
        <div className="flow-composer row">
          <button className="icon-btn" title="Send the current page" onClick={() => send({ type: "flow-send-page", tab: null })}>
            <Icon name="link" size={16} />
          </button>
          <button className="icon-btn" title="Send a file (up to 25 MB)" onClick={() => send({ type: "flow-send-file" })}>
            <Icon name="upload" size={16} />
          </button>
          <Input
            className="text-input grow"
            placeholder="Note or link"
            value={text}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") submit();
            }}
          />
          <button className="icon-btn" title="Send" disabled={!text.trim()} onClick={submit}>
            <Icon name="send" size={16} />
          </button>
        </div>
      )}
    </div>
  );
}

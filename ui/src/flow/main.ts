/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// The My Flow page HakuCX serves to phones on the local network. Scanning the QR code in
// HakuCX opens it with a one-time pairing key in the URL fragment; the page trades it for its
// own device key (kept in this browser's storage) and then exchanges notes, links and files
// with HakuCX, all sealed end to end (see crypto.ts and haku-core's flow.rs).

import "./flow.css";
import { aad, fromBase64Url, open, openBytes, seal, stamp, toBase64 } from "./crypto";

type Entry = {
  id: string;
  seq: number;
  at: number;
  mine: boolean;
  from: string;
  kind: "text" | "link" | "file";
  text: string;
  file: { name: string; size: number; mime: string } | null;
};
type Saved = { device: string; key: string; pc: string };

const STORAGE = "hakucx-flow";
// The page's policy is no-referrer (links out must not reveal this computer's address), but
// under that policy browsers send "Origin: null" on its own POSTs, which HakuCX refuses; its
// requests to HakuCX use "same-origin" so they carry their real origin.
const MAX_FILE = 25 * 1024 * 1024;
const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

let saved: Saved | null = null;
let key: Uint8Array | null = null;
let last = 0;
const shown = new Map<string, Entry>();
let polling = false;

function load(): Saved | null {
  try {
    const v = JSON.parse(localStorage.getItem(STORAGE) ?? "null") as Saved | null;
    return v && typeof v.device === "string" && typeof v.key === "string" ? v : null;
  } catch {
    return null;
  }
}

function setStatus(text: string, ok: boolean) {
  const s = $("status");
  s.textContent = text;
  s.className = ok ? "ok" : "";
}

function notice(text: string) {
  const n = $("notice");
  n.textContent = text;
  n.hidden = !text;
}

function phoneName(): string {
  const ua = navigator.userAgent;
  if (/iPhone/.test(ua)) return "iPhone";
  if (/iPad/.test(ua)) return "iPad";
  const android = /Android[^;)]*;\s*([^;)]+?)(?:\s+Build|\))/.exec(ua);
  if (android && android[1] && !/^(K|wv)$/.test(android[1])) return android[1].trim();
  if (/Android/.test(ua)) return "Android phone";
  return "Phone";
}

async function pair(pairing: string, pairingKey: string): Promise<boolean> {
  const k = fromBase64Url(pairingKey);
  const body = { p: pairing, s: seal(k, aad(pairing, "hello"), { ...stamp(), name: phoneName() }) };
  const r = await fetch("/api/hello", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body), referrerPolicy: "same-origin" });
  if (!r.ok) return false;
  const answer = open<{ device: string; key: string; pc: string }>(k, aad(pairing, "welcome"), ((await r.json()) as { s: string }).s);
  if (!answer) return false;
  saved = { device: answer.device, key: answer.key, pc: answer.pc };
  localStorage.setItem(STORAGE, JSON.stringify(saved));
  return true;
}

function forget(message: string) {
  localStorage.removeItem(STORAGE);
  saved = null;
  key = null;
  $("composer").hidden = true;
  setStatus("Not connected", false);
  notice(message);
}

const query = (purpose: string, extra: object) => {
  if (!saved || !key) throw new Error("not paired");
  return `d=${encodeURIComponent(saved.device)}&s=${seal(key, aad(saved.device, purpose), { ...stamp(), ...extra })}`;
};

async function poll() {
  if (!saved || !key || polling) return;
  polling = true;
  try {
    const r = await fetch(`/api/inbox?${query("inbox", { after: last })}`, { cache: "no-store", referrerPolicy: "same-origin" });
    if (r.status === 403) {
      forget("This phone is no longer connected to HakuCX. Open My Flow in HakuCX and scan a new QR code.");
      return;
    }
    if (!r.ok) throw new Error(String(r.status));
    const reply = open<{ entries: Entry[]; pc: string }>(key, aad(saved.device, "inbox-reply"), ((await r.json()) as { s: string }).s);
    if (!reply) throw new Error("unreadable reply");
    setStatus(`Connected to ${reply.pc}`, true);
    notice("");
    for (const e of reply.entries) {
      last = Math.max(last, e.seq);
      if (!shown.has(e.id)) {
        shown.set(e.id, e);
        render(e);
      }
    }
  } catch {
    setStatus(`Can't reach ${saved?.pc ?? "HakuCX"}. Is it open, on the same Wi-Fi?`, false);
  } finally {
    polling = false;
  }
}

function fmtSize(n: number) {
  return n < 1024 ? `${n} B` : n < 1048576 ? `${(n / 1024).toFixed(0)} KB` : `${(n / 1048576).toFixed(1)} MB`;
}

function render(e: Entry) {
  const list = $("list");
  const bubble = document.createElement("article");
  bubble.className = `bubble ${e.mine ? "mine" : "theirs"}`;
  const body = document.createElement("div");
  if (e.kind === "link") {
    const a = document.createElement("a");
    a.href = e.text;
    a.textContent = e.text;
    a.target = "_blank";
    a.rel = "noopener noreferrer";
    body.append(a);
  } else if (e.kind === "file" && e.file) {
    const b = document.createElement("button");
    b.className = "file";
    b.textContent = `${e.file.name} · ${fmtSize(e.file.size)}`;
    b.addEventListener("click", () => download(e, b));
    body.append(b);
  } else {
    body.textContent = e.text;
  }
  const meta = document.createElement("small");
  meta.textContent = `${e.mine ? "This phone" : e.from} · ${new Date(e.at).toLocaleString([], { hour: "2-digit", minute: "2-digit", day: "numeric", month: "short" })}`;
  bubble.append(body, meta);
  list.append(bubble);
  bubble.scrollIntoView({ block: "end" });
}

async function download(e: Entry, button: HTMLButtonElement) {
  if (!saved || !key || !e.file) return;
  button.disabled = true;
  try {
    const r = await fetch(`/api/file?${query("file", { entry: e.id })}`, { cache: "no-store", referrerPolicy: "same-origin" });
    if (!r.ok) throw new Error(String(r.status));
    const plain = openBytes(key, aad(saved.device, `file ${e.id}`), new Uint8Array(await r.arrayBuffer()));
    if (!plain) throw new Error("unreadable file");
    const url = URL.createObjectURL(new Blob([plain.slice().buffer], { type: e.file.mime || "application/octet-stream" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = e.file.name;
    document.body.append(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
  } catch {
    notice("Could not get the file. Try again.");
  } finally {
    button.disabled = false;
  }
}

async function send(message: object): Promise<boolean> {
  if (!saved || !key) return false;
  const body = { d: saved.device, s: seal(key, aad(saved.device, "send"), { ...stamp(), ...message }) };
  const r = await fetch("/api/send", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body), referrerPolicy: "same-origin" });
  if (r.status === 403) {
    forget("This phone is no longer connected to HakuCX. Open My Flow in HakuCX and scan a new QR code.");
    return false;
  }
  return r.ok;
}

function connected() {
  if (!saved) return;
  key = fromBase64Url(saved.key);
  $("composer").hidden = false;
  setStatus(`Connecting to ${saved.pc}...`, false);
  void poll();
}

async function start() {
  const frag = /^#p=([A-Za-z0-9_-]+)\.([A-Za-z0-9_-]+)$/.exec(location.hash);
  // The pairing key must not stay in the address bar or the history.
  if (location.hash) history.replaceState(null, "", location.pathname);
  saved = load();
  if (frag) {
    setStatus("Connecting...", false);
    try {
      if (!(await pair(frag[1]!, frag[2]!))) {
        notice("This QR code has expired or was already used. Show a new one in HakuCX (My Flow > Connect phone).");
        if (!saved) return;
      }
    } catch {
      notice("Can't reach HakuCX. Make sure this phone is on the same Wi-Fi as the computer.");
      return;
    }
  }
  if (!saved) {
    notice("Open My Flow in HakuCX on your computer, choose Connect phone, and scan the QR code with this phone.");
    return;
  }
  connected();
}

$("composer").addEventListener("submit", (ev) => {
  ev.preventDefault();
  const box = $<HTMLTextAreaElement>("text");
  const text = box.value.trim();
  if (!text) return;
  $<HTMLButtonElement>("send").disabled = true;
  send({ kind: "text", text })
    .then((ok) => {
      if (ok) {
        box.value = "";
        void poll();
      } else if (saved) notice("Not sent. Try again.");
    })
    .catch(() => notice("Not sent: can't reach HakuCX."))
    .finally(() => ($<HTMLButtonElement>("send").disabled = false));
});

$<HTMLTextAreaElement>("text").addEventListener("keydown", (ev) => {
  if (ev.key === "Enter" && !ev.shiftKey && window.matchMedia("(pointer: fine)").matches) {
    ev.preventDefault();
    $<HTMLFormElement>("composer").requestSubmit();
  }
});

$<HTMLInputElement>("file").addEventListener("change", async (ev) => {
  const input = ev.target as HTMLInputElement;
  const file = input.files?.[0];
  input.value = "";
  if (!file) return;
  if (file.size > MAX_FILE) {
    notice("Files up to 25 MB can be sent.");
    return;
  }
  notice(`Sending ${file.name}...`);
  try {
    const data = toBase64(new Uint8Array(await file.arrayBuffer()));
    const ok = await send({ kind: "file", name: file.name, mime: file.type, data });
    notice(ok ? "" : "Not sent. Try again.");
    if (ok) void poll();
  } catch {
    notice("Not sent: can't reach HakuCX.");
  }
});

setInterval(() => {
  if (document.visibilityState === "visible") void poll();
}, 3000);
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") void poll();
});

void start();

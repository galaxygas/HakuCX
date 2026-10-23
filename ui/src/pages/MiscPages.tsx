/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useEffect, useRef, useState } from "react";
import { Button } from "../components/controls";
import { Logo } from "../components/Icon";
import { onEvent, send } from "../ipc/bridge";
import { hostOf } from "../lib/util";
import { useStore } from "../state/store";
import { httpGet } from "./StartPage";

export function AboutPage() {
  const engine = useStore((s) => s.engine);
  const clicks = useRef(0);
  const [egg, setEgg] = useState(false);
  return (
    <div className="about-page">
      <button
        className="about-logo"
        onClick={() => {
          clicks.current += 1;
          if (clicks.current >= 7) {
            setEgg(true);
            send({ type: "play-sound", sound: "level" });
          }
        }}
      >
        <Logo size={96} />
      </button>
      <h1 className="display">HakuCX</h1>
      {egg && <p className="accent display">You found it. Thanks for building with us.</p>}
      <dl className="about-list">
        <dt>Version</dt>
        <dd>{engine?.appVersion}</dd>
        <dt>Engine</dt>
        <dd>Servo {engine?.servoVersion}</dd>
        <dt>User agent</dt>
        <dd className="mono">{engine?.userAgent}</dd>
        <dt>System</dt>
        <dd>{engine?.os}</dd>
        <dt>Profile</dt>
        <dd>
          {engine?.profile.name} {engine?.private && "(private window)"}
        </dd>
        <dt>Data folder</dt>
        <dd className="mono">{engine?.dataDir}</dd>
      </dl>
      <p className="muted">
        HakuCX is free and open source under the Mozilla Public License 2.0. It is an independent project, not affiliated with Opera. HakuCX has no servers and
        collects no telemetry.
      </p>
      <div className="row">
        <Button onClick={() => send({ type: "open-external", url: "https://github.com/galaxygas/HakuCX" })}>Source code</Button>
        <Button onClick={() => send({ type: "open-external", url: "https://github.com/galaxygas/HakuCX/issues" })}>Report a problem</Button>
        <Button onClick={() => send({ type: "open-external", url: "https://servo.org/" })}>About Servo</Button>
      </div>
    </div>
  );
}

const HELP_SHORTCUTS: [string, string][] = [
  ["Ctrl+T", "New tab"],
  ["Ctrl+W / Ctrl+F4", "Close tab"],
  ["Ctrl+Shift+T", "Reopen closed tab"],
  ["Ctrl+Tab / Ctrl+Shift+Tab", "Next / previous tab"],
  ["Ctrl+1 ... Ctrl+8, Ctrl+9", "Go to tab 1-8, last tab"],
  ["Ctrl+L / Alt+D / F6", "Focus the address bar"],
  ["Ctrl+F, F3", "Find in page"],
  ["F5 / Ctrl+R", "Reload"],
  ["Ctrl+F5 / Ctrl+Shift+R", "Reload without cache"],
  ["Alt+Left / Alt+Right", "Back / forward"],
  ["Ctrl+D", "Bookmark this page"],
  ["Ctrl+H / Ctrl+J", "History / downloads"],
  ["Ctrl+Shift+O", "Bookmarks"],
  ["Alt+P", "Settings"],
  ["Ctrl+N / Ctrl+Shift+N", "New window / private window"],
  ["Ctrl+= / Ctrl+- / Ctrl+0", "Zoom in / out / reset"],
  ["F11", "Full screen"],
  ["Ctrl+Space", "Search tabs"],
  ["Ctrl+Shift+A", "Command palette"],
  ["Ctrl+S", "Save page"],
  ["Ctrl+U", "Page source"],
  ["Ctrl+Shift+Delete", "Clear browsing data"],
  ["Ctrl+Shift+B", "Show or hide the bookmarks bar"],
  ["Ctrl+Shift+S", "Show or hide the sidebar"],
  ["Ctrl+Shift+5", "Snapshot"],
  ["Ctrl+M", "Mute tab"],
  ["F12 (configurable)", "Panic button"],
];

export function HelpPage() {
  return (
    <div className="help-page">
      <h1 className="display">Help</h1>
      <h2 className="display section-label">Keyboard shortcuts</h2>
      <table className="shortcut-table">
        <tbody>
          {HELP_SHORTCUTS.map(([keys, what]) => (
            <tr key={keys}>
              <td>
                {keys.split(" / ").map((k) => (
                  <kbd key={k}>{k}</kbd>
                ))}
              </td>
              <td>{what}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <p className="muted">You can change shortcuts in Settings &gt; Features &gt; Shortcuts.</p>
    </div>
  );
}

export function SourcePage({ target }: { target: string }) {
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!/^https?:\/\//.test(target)) {
      setError("Page source is available for web pages.");
      return;
    }
    httpGet(target).then(setText, (e) => setError(String(e)));
  }, [target]);
  const lines = text?.split("\n") ?? [];
  return (
    <div className="source-page">
      <div className="row source-head">
        <h1 className="display">Page source</h1>
        <span className="muted ellipsis grow">{target}</span>
      </div>
      <p className="note">Fetched again without your cookies, so pages that need you signed in may differ.</p>
      {error && <p className="danger">{error}</p>}
      {text === null && !error && <p className="muted">Loading...</p>}
      <pre className="source-code">
        {lines.slice(0, 20000).map((l, i) => (
          <div key={i} className="source-line">
            <span className="source-num">{i + 1}</span>
            <span>{l}</span>
          </div>
        ))}
      </pre>
    </div>
  );
}

let readerRequest = 60_000;

export function ReaderPage({ tab }: { tab: string }) {
  const [content, setContent] = useState<{ title: string; url: string; paragraphs: string[] } | null>(null);
  const [size, setSize] = useState(18);
  useEffect(() => {
    const request = readerRequest++;
    const off = onEvent((e) => {
      if (e.type === "page-text" && e.request === request) {
        // Plain text only: the reader never renders page markup inside the browser UI.
        const paragraphs = e.text
          .split(/\n\s*\n|\n/)
          .map((p) => p.trim())
          .filter((p) => p.length > 40 || /[.!?]$/.test(p));
        setContent({ title: e.title, url: e.url, paragraphs });
      }
    });
    send({ type: "get-page-text", request, tab });
    return off;
  }, [tab]);
  return (
    <div className="reader-page" style={{ fontSize: size }}>
      <div className="reader-tools">
        <Button onClick={() => setSize((s) => Math.max(14, s - 2))}>A-</Button>
        <Button onClick={() => setSize((s) => Math.min(30, s + 2))}>A+</Button>
        <Button onClick={() => send({ type: "back", tab })}>Exit reader</Button>
      </div>
      {!content && <p className="muted">Preparing reader view...</p>}
      {content && (
        <article>
          <h1>{content.title}</h1>
          <p className="muted">{hostOf(content.url)}</p>
          {content.paragraphs.length === 0 && <p>This page has no readable article text.</p>}
          {content.paragraphs.map((p, i) => (
            <p key={i}>{p}</p>
          ))}
        </article>
      )}
    </div>
  );
}

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// The search pop-up over text selected on a page: search, copy, conversions computed by the
// core (currency, units, time zones) and AI prompts when the assistant is on.

import { Icon } from "../components/Icon";
import { copyText, send } from "../ipc/bridge";
import { useStore } from "../state/store";

const AI_PROMPTS: { label: string; prompt: (text: string) => string }[] = [
  { label: "Explain", prompt: (t) => `Explain this:\n\n${t}` },
  { label: "Summarize", prompt: (t) => `Summarize this:\n\n${t}` },
  { label: "Translate", prompt: (t) => `Translate this into English (or, if it is English, into Spanish):\n\n${t}` },
];

export function SelectionPopupHost() {
  const popup = useStore((s) => s.selectionPopup);
  const askAi = useStore((s) => s.askAi);
  if (!popup) return null;
  const close = () => useStore.setState({ selectionPopup: null });
  const width = 300;
  const left = Math.max(8, Math.min(window.innerWidth - width - 8, popup.rect.x + popup.rect.w / 2 - width / 2));
  // Above the selection, or below it when there is no room.
  const above = popup.rect.y > 90;
  const style: React.CSSProperties = above ? { left, bottom: window.innerHeight - popup.rect.y + 8, width } : { left, top: popup.rect.y + popup.rect.h + 8, width };
  return (
    <div className="selection-popup" data-hit style={style} onMouseDown={(e) => e.stopPropagation()}>
      <div className="row">
        <button
          className="sel-btn"
          title="Search in a new tab"
          onClick={() => {
            send({ type: "navigate", tab: null, input: popup.text, disposition: "new-tab" });
            close();
          }}
        >
          <Icon name="search" size={14} /> Search
        </button>
        <button
          className="sel-btn"
          onClick={() => {
            copyText(popup.text);
            close();
          }}
        >
          <Icon name="copy" size={14} /> Copy
        </button>
        {popup.ai &&
          AI_PROMPTS.map((p) => (
            <button key={p.label} className="sel-btn" title={`Ask the AI assistant to ${p.label.toLowerCase()} the selection`} onClick={() => askAi(p.prompt(popup.text))}>
              <Icon name="sparkles" size={13} /> {p.label}
            </button>
          ))}
      </div>
      {popup.conversions.map((c) => (
        <button key={c} className="sel-conversion" title="Copy" onClick={() => copyText(c)}>
          = {c}
        </button>
      ))}
    </div>
  );
}

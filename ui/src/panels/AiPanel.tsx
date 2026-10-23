/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useEffect, useRef, useState } from "react";
import { Button } from "../components/controls";
import { Icon } from "../components/Icon";
import { onEvent, send } from "../ipc/bridge";
import type { AiMessage } from "../ipc/generated/AiMessage";
import { clsx } from "../lib/util";
import { openInternal, useStore } from "../state/store";
import { TextArea } from "../components/Field";

let aiRequest = 90_000;

export function AiPanel() {
  const ai = useStore((s) => s.settings?.ai);
  const [messages, setMessages] = useState<AiMessage[]>([]);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);
  const pending = useRef<number | null>(null);
  const pagePending = useRef<{ id: number; prompt: string } | null>(null);
  const list = useRef<HTMLDivElement>(null);

  useEffect(
    () =>
      onEvent((e) => {
        if (e.type === "ai-reply" && e.request === pending.current) {
          pending.current = null;
          setBusy(false);
          setMessages((m) => [...m, { role: "assistant", content: e.error ? `⚠ ${e.error}` : e.text }]);
        }
        if (e.type === "page-text" && pagePending.current && e.request === pagePending.current.id) {
          const { prompt } = pagePending.current;
          pagePending.current = null;
          const content = e.text
            ? `${prompt}\n\nPage: ${e.title} (${e.url})\n\n${e.text}`
            : `${prompt}\n\n(The current tab has no readable page text.)`;
          ask(content, prompt);
        }
      }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [messages],
  );

  useEffect(() => {
    list.current?.scrollTo({ top: list.current.scrollHeight });
  }, [messages, busy]);

  const ask = (content: string, shown?: string) => {
    const next: AiMessage[] = [...messages, { role: "user", content }];
    setMessages([...messages, { role: "user", content: shown ?? content }]);
    setBusy(true);
    const request = aiRequest++;
    pending.current = request;
    send({ type: "ai-chat", request, messages: next });
  };

  const askAboutPage = (prompt: string) => {
    const request = aiRequest++;
    pagePending.current = { id: request, prompt };
    send({ type: "get-page-text", request, tab: null });
  };

  if (!ai) return null;
  if (!ai.enabled || ai.provider === "none") {
    return (
      <div className="panel-empty">
        <Icon name="sparkles" size={28} />
        <p>HakuCX's assistant uses the AI provider you choose: Anthropic, any OpenAI-compatible service, or a local Ollama model. Nothing is sent until you set one up.</p>
        <Button kind="accent" onClick={() => openInternal("haku://settings/ai")}>
          Set up AI
        </Button>
      </div>
    );
  }

  return (
    <div className="ai-panel">
      <div className="ai-messages" ref={list}>
        {messages.length === 0 && (
          <div className="ai-suggestions">
            <button className="chip" onClick={() => askAboutPage("Summarize this page in a few bullet points.")}>
              Summarize this page
            </button>
            <button className="chip" onClick={() => askAboutPage("What are the key facts on this page?")}>
              Key facts
            </button>
            <button className="chip" onClick={() => askAboutPage("Explain this page simply.")}>
              Explain simply
            </button>
          </div>
        )}
        {messages.map((m, i) => (
          <div key={i} className={clsx("ai-msg", m.role)}>
            {m.content}
          </div>
        ))}
        {busy && <div className="ai-msg assistant typing">...</div>}
      </div>
      <div className="ai-input">
        <TextArea
          className="text-input"
          placeholder="Ask anything"
          value={text}
          rows={2}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && text.trim() && !busy) {
              e.preventDefault();
              ask(text.trim());
              setText("");
            }
          }}
        />
        <button className="icon-btn" title="Send" disabled={busy || !text.trim()} onClick={() => { ask(text.trim()); setText(""); }}>
          <Icon name="send" size={16} />
        </button>
      </div>
      {messages.length > 0 && (
        <button className="btn btn-ghost" onClick={() => setMessages([])}>
          New chat
        </button>
      )}
    </div>
  );
}

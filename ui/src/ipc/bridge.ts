/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// The UI <-> core channel. Commands go out as `console.debug` lines with a private prefix
// (the core accepts them only from this webview); events come back through
// `window.__haku.receive`. All types are generated from Rust (see ipc/generated).

import type { Command } from "./generated/Command";
import type { Event } from "./generated/Event";

const PREFIX = "\u0001haku-ipc:";

type Listener = (event: Event) => void;
const listeners = new Set<Listener>();

export function send(command: Command): void {
  console.debug(PREFIX + JSON.stringify(command));
}

export function onEvent(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

declare global {
  interface Window {
    __haku?: {
      receive(events: Event[]): void;
    };
  }
}

window.__haku = {
  receive(events: Event[]) {
    for (const event of events) {
      for (const listener of listeners) {
        try {
          listener(event);
        } catch (error) {
          send({ type: "log", level: "error", message: `event handler failed: ${String(error)}` });
        }
      }
    }
  },
};

/** Copy to the system clipboard (the browser core writes it; Servo has no clipboard API for
 * pages, the UI included). */
export function copyText(text: string): void {
  send({ type: "copy-text", text });
}

window.addEventListener("error", (e) => {
  send({ type: "log", level: "error", message: `${e.message} @ ${e.filename}:${e.lineno}` });
});
window.addEventListener("unhandledrejection", (e) => {
  send({ type: "log", level: "error", message: `unhandled rejection: ${String(e.reason)}` });
});

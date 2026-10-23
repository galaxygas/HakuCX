/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { Icon } from "../components/Icon";
import { send } from "../ipc/bridge";
import type { Rect } from "../ipc/generated/Rect";
import type { Tab } from "../ipc/generated/Tab";
import { clsx, isInternal } from "../lib/util";
import { InternalPage } from "../pages/InternalPage";
import { selectModal, selectOverlay, NONE, useStore } from "../state/store";
import { Input } from "../components/Field";

function rectOf(el: Element | null): Rect | null {
  if (!el) return null;
  const r = el.getBoundingClientRect();
  if (r.width < 1 || r.height < 1) return null;
  return { x: r.left, y: r.top, w: r.width, h: r.height };
}

/** Reports page/panel rectangles and overlay state to the core whenever they change. */
function useLayoutReporter(main: React.RefObject<HTMLDivElement | null>, split: React.RefObject<HTMLDivElement | null>) {
  const overlay = useStore(selectOverlay);
  const modal = useStore(selectModal);
  const last = useRef("");
  useLayoutEffect(() => {
    const report = () => {
      const page = rectOf(main.current) ?? { x: 0, y: 0, w: 1, h: 1 };
      const splitRect = rectOf(split.current);
      const panel = rectOf(document.querySelector(".web-panel-hole"));
      const extPopup = rectOf(document.querySelector(".ext-popup-hole"));
      const regions = Array.from(document.querySelectorAll("[data-hit]"))
        .map(rectOf)
        .filter((r): r is Rect => r !== null);
      const msg = { type: "layout" as const, page, split: splitRect, panel, overlay, modal, blur: [] as Rect[], regions, extPopup };
      const key = JSON.stringify(msg);
      if (key !== last.current) {
        last.current = key;
        send(msg);
      }
    };
    report();
    const ro = new ResizeObserver(report);
    if (main.current) ro.observe(main.current);
    if (split.current) ro.observe(split.current);
    window.addEventListener("resize", report);
    window.addEventListener("hx-layout", report);
    // Layout can shift without a resize (panels opening, bars toggling): poll lightly.
    const timer = window.setInterval(report, 250);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", report);
      window.removeEventListener("hx-layout", report);
      window.clearInterval(timer);
    };
  });
}

export function Viewport() {
  const session = useStore((s) => s.session);
  const panel = useStore((s) => s.panel);
  const pinnedPanels = useStore((s) => s.settings?.sidebar.pinnedPanels ?? NONE);
  const setPanel = useStore((s) => s.setPanel);
  const findOpen = useStore((s) => s.find.open);
  const mainRef = useRef<HTMLDivElement>(null);
  const splitRef = useRef<HTMLDivElement>(null);
  useLayoutReporter(mainRef, splitRef);

  if (!session) return <div className="viewport" />;
  const ws = session.activeWorkspace;
  const activeId = session.activeTabs[ws];
  const split = session.split;
  const left = session.tabs.find((t) => t.id === (split ? split[0] : activeId));
  const right = split ? session.tabs.find((t) => t.id === split[1]) : undefined;
  const floatingPanel = panel && !pinnedPanels.includes(panel);

  return (
    <div
      className={clsx("viewport", split && "split")}
      onMouseDown={() => {
        // With a floating panel open, a click on the page closes it (the core routes that
        // click to the UI while the panel covers the page).
        if (floatingPanel) setPanel(null);
      }}
    >
      <Pane tab={left} paneRef={mainRef} focused={!split || activeId === split[0]} />
      {split && (
        <>
          <div className="split-divider" data-hit>
            <button className="split-exit" title="Exit split screen" onClick={() => send({ type: "clear-split" })}>
              <Icon name="x" size={12} />
            </button>
          </div>
          <Pane tab={right} paneRef={splitRef} focused={activeId === split[1]} />
        </>
      )}
      {findOpen && <FindBar />}
    </div>
  );
}

function Pane({ tab, paneRef, focused }: { tab: Tab | undefined; paneRef: React.RefObject<HTMLDivElement | null>; focused: boolean }) {
  const meta = useStore((s) => (tab ? s.meta[tab.id] : undefined));
  const url = meta?.url ?? tab?.url ?? "";
  const internal = isInternal(url);
  return (
    <div ref={paneRef} className={clsx("pane", internal ? "internal" : "page-hole", focused && "focused")} data-tab={tab?.id}>
      {meta?.loading && !internal && <div className="load-bar" />}
      {internal && tab && <InternalPage url={url} tab={tab.id} />}
      {!internal && meta?.crashed && (
        <div className="crashed" data-hit>
          <Icon name="skull" size={40} />
          <h2 className="display">This page crashed</h2>
          <p className="muted">{meta.crashed}</p>
          <button className="btn btn-accent" onClick={() => send({ type: "reload", tab: tab?.id ?? null, bypassCache: false })}>
            Reload
          </button>
        </div>
      )}
    </div>
  );
}

function FindBar() {
  const find = useStore((s) => s.find);
  const setFindOpen = useStore((s) => s.setFindOpen);
  const [text, setText] = useState("");
  const [matchCase, setMatchCase] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, [find.nonce]);
  const run = (forward: boolean, value = text) => send({ type: "find", text: value, forward, matchCase });
  return (
    <div className="find-bar" data-hit>
      <Icon name="search" size={15} />
      <Input
        ref={input}
        value={text}
        placeholder="Find in page"
        onChange={(e) => {
          setText(e.target.value);
          run(true, e.target.value);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") run(!e.shiftKey);
          if (e.key === "Escape") setFindOpen(false);
        }}
      />
      <span className="find-count">{text ? `${find.active}/${find.matches}` : ""}</span>
      <button className={clsx("icon-btn", matchCase && "active")} title="Match case" onClick={() => setMatchCase((m) => !m)}>
        Aa
      </button>
      <button className="icon-btn" title="Previous (Shift+Enter)" onClick={() => run(false)}>
        <Icon name="chevron-up" size={16} />
      </button>
      <button className="icon-btn" title="Next (Enter)" onClick={() => run(true)}>
        <Icon name="chevron-down" size={16} />
      </button>
      <button className="icon-btn" title="Close (Esc)" onClick={() => setFindOpen(false)}>
        <Icon name="x" size={16} />
      </button>
    </div>
  );
}

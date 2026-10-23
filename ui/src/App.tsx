/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useEffect, useLayoutEffect } from "react";
import { send } from "./ipc/bridge";
import { clsx } from "./lib/util";
import { Overlays } from "./overlays/Overlays";
import { BookmarksBar } from "./shell/BookmarksBar";
import { PanelHost } from "./shell/PanelHost";
import { Sidebar } from "./shell/Sidebar";
import { TabStrip } from "./shell/TabStrip";
import { Toolbar } from "./shell/Toolbar";
import { Viewport } from "./shell/Viewport";
import { useStore } from "./state/store";

const FONT_SIZES: Record<string, string> = {
  "very-small": "11px",
  small: "12px",
  medium: "13px",
  large: "14.5px",
  "very-large": "16px",
};

export function App() {
  const ready = useStore((s) => s.ready);
  const palette = useStore((s) => s.palette);
  const appearance = useStore((s) => s.settings?.appearance);
  const sidebarShown = useStore((s) => s.settings?.sidebar.show ?? true);
  const bookmarksBar = useStore((s) => s.settings?.appearance.showBookmarksBar ?? false);
  const fullscreen = useStore((s) => s.fullscreen);
  const maximized = useStore((s) => s.maximized);
  const focused = useStore((s) => s.focused);

  useEffect(() => {
    send({ type: "ready" });
  }, []);

  useLayoutEffect(() => {
    if (!palette) return;
    const root = document.documentElement;
    for (const [k, v] of Object.entries(palette.vars)) root.style.setProperty(k, v);
    root.classList.toggle("light", !palette.dark);
  }, [palette]);

  useLayoutEffect(() => {
    if (!appearance) return;
    const root = document.documentElement;
    const custom = appearance.interfaceMode === "custom";
    root.style.setProperty("--hx-ui-opacity", String(custom ? appearance.opacity : 1));
    root.style.setProperty("--hx-elements-bg", String(custom ? appearance.elementsBackground / 100 : 0.18));
    root.style.setProperty("--hx-vignette", String(custom ? appearance.vignette : 0.55));
    root.style.setProperty("--hx-radius", `${appearance.cornerStyle === "sharp" ? 2 : appearance.roundness}px`);
    root.style.setProperty("--hx-ui-scale", String(appearance.uiScale / 100));
    root.style.fontSize = FONT_SIZES[appearance.fontSize] ?? "13px";
    root.classList.toggle("reduce-motion", appearance.reduceAnimations);
    root.classList.toggle("crosshair", appearance.crosshairCursor);
    root.classList.toggle("focus-mode", custom && appearance.focusMode);
    root.dataset.corner = appearance.cornerStyle;
    let css = document.getElementById("user-ui-css");
    if (!css) {
      css = document.createElement("style");
      css.id = "user-ui-css";
      document.head.appendChild(css);
    }
    if (css.textContent !== appearance.uiCss) css.textContent = appearance.uiCss;
  }, [appearance]);

  const decoration = appearance?.windowDecoration ?? "outside";
  const showDecoration = !fullscreen && (decoration === "outside" || (decoration === "outside-except-maximized" && !maximized));

  return (
    <div
      className={clsx(
        "shell",
        fullscreen && "fullscreen",
        showDecoration && "decorated",
        decoration === "inside" && "decoration-inside",
        !focused && "unfocused",
        !ready && "booting",
      )}
    >
      {!fullscreen && <TabStrip />}
      {!fullscreen && <Toolbar />}
      {!fullscreen && bookmarksBar && <BookmarksBar />}
      <div className="main">
        {!fullscreen && sidebarShown && <Sidebar />}
        <PanelHost />
        <Viewport />
      </div>
      <Overlays />
    </div>
  );
}

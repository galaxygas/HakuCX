/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { describe, expect, it } from "vitest";
import inputRs from "../../../crates/hakucx/src/browser/input.rs?raw";
import { SHORTCUT_LABELS } from "../lib/catalog";
import { clsx, formatBytes, hostOf, isInternal } from "../lib/util";
import { sanitizeModPatch } from "../pages/ModsPage";
import { levelOf } from "../panels/MiscPanels";
import { useStore } from "../state/store";

describe("util", () => {
  it("formats sizes", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(200 * 1024 * 1024)).toBe("200 MB");
    expect(formatBytes(null)).toBe("");
  });
  it("finds hosts", () => {
    expect(hostOf("https://www.example.com/a?b")).toBe("example.com");
    expect(hostOf("haku://settings/")).toBe("HakuCX");
    expect(hostOf("not a url")).toBe("not a url");
    expect(isInternal("haku://start/")).toBe(true);
    expect(isInternal("https://haku.example")).toBe(false);
  });
  it("joins class names", () => {
    expect(clsx("a", false, null, "b", undefined)).toBe("a b");
  });
});

describe("mods", () => {
  it("keeps only appearance, sound and shader settings", () => {
    const out = sanitizeModPatch({
      appearance: { theme: "sakura", uiCss: "x" },
      privacy: { blockAds: false },
      system: { proxyUrl: "http://evil" },
      features: { shader: "duo-fx", webModding: false },
    }) as Record<string, Record<string, unknown>>;
    expect(out.privacy).toBeUndefined();
    expect(out.system).toBeUndefined();
    expect(out.appearance?.theme).toBe("sakura");
    expect(out.features).toEqual({ shader: "duo-fx" });
  });
});

describe("rewards", () => {
  it("levels up with XP", () => {
    const base = { pages: 0, blocked: 0, downloads: 0, themes: [] as string[] };
    expect(levelOf(base as never).level).toBe(1);
    expect(levelOf({ ...base, pages: 1000 } as never).level).toBeGreaterThan(5);
  });
});

describe("shortcuts", () => {
  it("lists exactly the shortcuts the core binds, with the same defaults", () => {
    const table = inputRs.slice(inputRs.indexOf("pub const SHORTCUTS"), inputRs.indexOf("];", inputRs.indexOf("pub const SHORTCUTS")));
    const core = [...table.matchAll(/\("([a-z0-9-]+)", "([^"]+)"\)/g)].map((m) => [m[1], m[2]]);
    expect(core.length).toBeGreaterThan(30);
    expect(SHORTCUT_LABELS.map((s) => [s.id, s.default])).toEqual(core);
  });
});

describe("store", () => {
  it("tracks panel badges and offers to allow blocked pop-ups", () => {
    useStore.getState().apply({ type: "panel-badge", id: "discord", count: 3 });
    expect(useStore.getState().badges.discord).toBe(3);
    useStore.getState().apply({ type: "popup-blocked", tab: "t1", host: "example.com" });
    const toast = useStore.getState().toasts.at(-1);
    expect(toast?.text).toContain("example.com");
    expect(toast?.action?.label).toBe("Always allow");
  });
  it("normalizes popup anchors from DOM rects", () => {
    useStore.getState().openPopup("downloads", { left: 10, top: 20, width: 30, height: 40 } as DOMRect);
    expect(useStore.getState().popupAnchor).toEqual({ x: 10, y: 20, w: 30, h: 40 });
  });
});

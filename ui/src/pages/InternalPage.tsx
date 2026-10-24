/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { memo, type ReactNode } from "react";
import { send } from "../ipc/bridge";
import { clsx } from "../lib/util";
import { AboutPage, HelpPage, ReaderPage, SourcePage } from "./MiscPages";
import { BookmarksPage } from "./BookmarksPage";
import { CornerPage, GamesPage } from "./CornerPage";
import { DownloadsPage } from "./DownloadsPage";
import { ExtensionsPage } from "./ExtensionsPage";
import { HistoryPage } from "./HistoryPage";
import { ModsPage } from "./ModsPage";
import { PlaygroundPage } from "./PlaygroundPage";
import { SettingsPage } from "./settings/SettingsPage";
import { StartPage } from "./StartPage";

export function parseInternal(url: string): { page: string; path: string; params: URLSearchParams } {
  try {
    const u = new URL(url);
    return { page: u.host || "start", path: u.pathname.replace(/^\/+|\/+$/g, ""), params: u.searchParams };
  } catch {
    return { page: "start", path: "", params: new URLSearchParams() };
  }
}

/** Navigate the current tab to another internal page (keeps tab history in the core). */
export function go(url: string) {
  send({ type: "navigate", tab: null, input: url, disposition: "current-tab" });
}

/**
 * Internal pages re-render only when their URL changes. Servo 0.7 lays out the whole UI again
 * after any DOM change, and React rewrites every input's attributes when it re-renders one, so
 * a page re-rendering on each tab title or session update costs a full layout every time.
 */
export const InternalPage = memo(function InternalPage({ url, tab }: { url: string; tab: string }) {
  const { page, path, params } = parseInternal(url);
  switch (page) {
    case "start":
      return <StartPage />;
    case "settings":
      return (
        <ConfigureHub active="settings">
          <SettingsPage section={path || "appearance"} params={params} />
        </ConfigureHub>
      );
    case "history":
      return (
        <ConfigureHub active="history">
          <HistoryPage />
        </ConfigureHub>
      );
    case "downloads":
      return (
        <ConfigureHub active="downloads">
          <DownloadsPage />
        </ConfigureHub>
      );
    case "bookmarks":
      return (
        <ConfigureHub active="bookmarks">
          <BookmarksPage />
        </ConfigureHub>
      );
    case "extensions":
      return (
        <ConfigureHub active="extensions">
          <ExtensionsPage />
        </ConfigureHub>
      );
    case "mods":
      return (
        <ConfigureHub active="mods">
          <ModsPage />
        </ConfigureHub>
      );
    case "playground":
      return (
        <ConfigureHub active="playground">
          <PlaygroundPage />
        </ConfigureHub>
      );
    case "corner":
      return <CornerPage />;
    case "games":
      return <GamesPage />;
    case "source":
      return <SourcePage target={params.get("url") ?? ""} />;
    case "reader":
      return <ReaderPage tab={params.get("tab") ?? tab} />;
    case "about":
      return <AboutPage />;
    case "help":
      return <HelpPage />;
    default:
      return (
        <div className="internal-missing">
          <h1 className="display">Page not found</h1>
          <p className="muted">{url} is not a HakuCX page.</p>
          <button className="btn btn-accent" onClick={() => go("haku://start/")}>
            Go to Speed Dial
          </button>
        </div>
      );
  }
});

const HUB: { id: string; label: string; url: string }[] = [
  { id: "mods", label: "Mods", url: "haku://mods/" },
  { id: "history", label: "History", url: "haku://history/" },
  { id: "downloads", label: "Downloads", url: "haku://downloads/" },
  { id: "settings", label: "Settings", url: "haku://settings/" },
  { id: "playground", label: "Playground", url: "haku://playground/" },
  { id: "bookmarks", label: "Bookmarks", url: "haku://bookmarks/" },
  { id: "extensions", label: "Extensions", url: "haku://extensions/" },
];

/** Opera GX's "Configure" pages share a top navigation and a perspective grid floor. */
function ConfigureHub({ active, children }: { active: string; children: ReactNode }) {
  return (
    <div className="hub">
      <div className="hub-floor" />
      <nav className="hub-nav">
        {HUB.map((h) => (
          <button key={h.id} className={clsx("display hub-link", h.id === active && "active")} onClick={() => go(h.url)}>
            {h.label}
          </button>
        ))}
      </nav>
      <div className="hub-content">{children}</div>
    </div>
  );
}

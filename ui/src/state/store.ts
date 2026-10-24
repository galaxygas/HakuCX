/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// The UI's mirror of core state, plus purely presentational state (which panel or menu is
// open). The core is the source of truth: the UI never edits session or settings locally,
// it sends a command and waits for the resulting event.

import { create } from "zustand";
import type { Bookmarks } from "../ipc/generated/Bookmarks";
import type { ContextMenuInfo } from "../ipc/generated/ContextMenuInfo";
import type { DialogKind } from "../ipc/generated/DialogKind";
import type { DownloadRecord } from "../ipc/generated/DownloadRecord";
import type { EngineInfo } from "../ipc/generated/EngineInfo";
import type { Address } from "../ipc/generated/Address";
import type { AutofillItem } from "../ipc/generated/AutofillItem";
import type { CardInfo } from "../ipc/generated/CardInfo";
import type { ExtensionInfo } from "../ipc/generated/ExtensionInfo";
import type { LoginInfo } from "../ipc/generated/LoginInfo";
import type { SyncStatus } from "../ipc/generated/SyncStatus";
import type { SystemInfo } from "../ipc/generated/SystemInfo";
import type { UpdateStatus } from "../ipc/generated/UpdateStatus";
import type { VaultStatus } from "../ipc/generated/VaultStatus";
import type { Event } from "../ipc/generated/Event";
import type { Metrics } from "../ipc/generated/Metrics";
import type { Palette } from "../ipc/generated/Palette";
import type { Pinboards } from "../ipc/generated/Pinboards";
import type { Rect } from "../ipc/generated/Rect";
import type { SelectOption } from "../ipc/generated/SelectOption";
import type { Session } from "../ipc/generated/Session";
import type { Settings } from "../ipc/generated/Settings";
import type { SiteMods } from "../ipc/generated/SiteMods";
import type { Suggestion } from "../ipc/generated/Suggestion";
import type { TabMeta } from "../ipc/generated/TabMeta";
import type { ThemeInfo } from "../ipc/generated/ThemeInfo";
import type { Visit } from "../ipc/generated/Visit";
import { onEvent, send } from "../ipc/bridge";

export type Popup = "downloads" | "extensions" | "profile" | "site-info" | "blocked" | "main-menu" | "workspaces" | "passwords" | null;
export type Drawer = "easy-setup" | "sidebar-setup" | null;

export type Menu =
  | { kind: "tab"; x: number; y: number; tab: string }
  | { kind: "page"; x: number; y: number; info: ContextMenuInfo }
  | { kind: "bookmark"; x: number; y: number; id: string }
  | { kind: "dial"; x: number; y: number; id: string }
  | { kind: "sidebar"; x: number; y: number; item: string }
  | { kind: "island"; x: number; y: number; island: string };

export type Modal =
  | { kind: "quit" }
  | { kind: "qr"; url: string }
  | { kind: "clear-data" }
  | { kind: "dial-edit"; id: string | null; parent: string }
  | { kind: "bookmark-edit"; id: string }
  | { kind: "island-edit"; island: string }
  | { kind: "workspace-edit"; workspace: string | null }
  | { kind: "theme-editor"; themeId: string | null }
  | { kind: "site-mod"; id: string | null }
  | { kind: "devtools" }
  | { kind: "profiles" };

export type PageDialog = {
  id: number;
  tab: string;
  kind: DialogKind;
  message: string;
  defaultValue: string | null;
  origin: string;
};
export type SelectPicker = { id: number; tab: string; rect: Rect; options: SelectOption[]; selected: number[]; multiple: boolean };
export type ColorPicker = { id: number; tab: string; rect: Rect; color: string | null };
export type PermissionPrompt = { id: number; tab: string; origin: string; kind: string };
export type AuthPrompt = { id: number; tab: string; url: string; proxy: boolean };
export type Toast = { id: number; text: string; kind: string; action?: { label: string; run: () => void } };
export type PanelState = { loading: boolean; url: string; canGoBack: boolean };
/** An open extension popup: where its button is and the size its page asked for. */
export type ExtPopup = { id: string; anchor: Rect; width: number; height: number };
export type ExtInstallPrompt = { request: number; name: string; version: string; warnings: string[]; update: boolean };
export type ExtPermissionPrompt = { request: number; name: string; warnings: string[] };
export type VaultView = { status: VaultStatus; logins: LoginInfo[]; neverSave: string[]; cards: CardInfo[]; addresses: Address[] };
export type PasswordPrompt = { prompt: number; tab: string; origin: string; username: string; password: string; update: boolean };
export type AutofillSuggest = { tab: string; rect: Rect; kind: string; items: AutofillItem[] };

const HISTORY_POINTS = 60;

/**
 * A stable empty list for selector fallbacks. Zustand compares snapshots by identity, so a
 * selector returning a fresh `[]` on every call re-renders forever.
 */
export const NONE: never[] = [];

type State = {
  ready: boolean;
  engine: EngineInfo | null;
  session: Session | null;
  settings: Settings | null;
  palette: Palette | null;
  themes: ThemeInfo[];
  bookmarks: Bookmarks;
  downloads: DownloadRecord[];
  downloadSpeed: Record<string, number>;
  pinboards: Pinboards;
  siteMods: SiteMods;
  meta: Record<string, TabMeta>;
  metrics: Metrics | null;
  cpuHistory: number[];
  memHistory: number[];
  netHistory: number[];
  blockedTotal: number;
  systemDark: boolean;
  maximized: boolean;
  fullscreen: boolean;
  focused: boolean;
  panic: boolean;
  find: { open: boolean; matches: number; active: number; nonce: number };
  suggestions: { text: string; items: Suggestion[]; remote: Suggestion[] };
  historyResults: Record<number, Visit[]>;
  /** Unread counts of sidebar web panels (messengers), from their page titles. */
  badges: Record<string, number>;
  panelStates: Record<string, PanelState>;
  extensions: ExtensionInfo[];
  extDeveloperMode: boolean;
  extPopup: ExtPopup | null;
  extInstall: ExtInstallPrompt | null;
  extPermissions: ExtPermissionPrompt[];
  vault: VaultView | null;
  passwordPrompt: PasswordPrompt | null;
  autofill: AutofillSuggest | null;
  sync: SyncStatus | null;
  system: SystemInfo | null;
  update: UpdateStatus | null;

  panel: string | null;
  popup: Popup;
  popupAnchor: Rect | null;
  drawer: Drawer;
  menu: Menu | null;
  modal: Modal | null;
  dialogs: PageDialog[];
  select: SelectPicker | null;
  color: ColorPicker | null;
  permissions: PermissionPrompt[];
  auth: AuthPrompt | null;
  toasts: Toast[];
  palette_open: "tabs" | "commands" | null;
  addressFocus: number;
  addressOpen: boolean;
  splash: boolean;
};

type Actions = {
  apply(event: Event): void;
  setPanel(panel: string | null): void;
  togglePanel(panel: string): void;
  openPopup(popup: Popup, anchor?: Rect | DOMRect | null): void;
  setDrawer(drawer: Drawer): void;
  openMenu(menu: Menu | null): void;
  openModal(modal: Modal | null): void;
  setFindOpen(open: boolean): void;
  setPalette(open: "tabs" | "commands" | null): void;
  setAddressOpen(open: boolean): void;
  focusAddress(): void;
  dismissToast(id: number): void;
  toast(text: string, kind?: string, action?: Toast["action"]): void;
  shiftDialog(): void;
  shiftPermission(): void;
  clearSelect(): void;
  clearColor(): void;
  clearAuth(): void;
  patchSettings(patch: unknown): void;
  endSplash(): void;
};

let toastId = 1;

function push<T>(list: T[], value: T): T[] {
  const next = [...list, value];
  return next.length > HISTORY_POINTS ? next.slice(next.length - HISTORY_POINTS) : next;
}

export const useStore = create<State & Actions>((set, get) => ({
  ready: false,
  engine: null,
  session: null,
  settings: null,
  palette: null,
  themes: [],
  bookmarks: { nodes: [] },
  downloads: [],
  downloadSpeed: {},
  pinboards: { boards: [] },
  siteMods: { mods: [] },
  meta: {},
  metrics: null,
  cpuHistory: [],
  memHistory: [],
  netHistory: [],
  blockedTotal: 0,
  systemDark: true,
  maximized: false,
  fullscreen: false,
  focused: true,
  panic: false,
  find: { open: false, matches: 0, active: 0, nonce: 0 },
  suggestions: { text: "", items: [], remote: [] },
  historyResults: {},
  badges: {},
  panelStates: {},
  extensions: [],
  extDeveloperMode: false,
  extPopup: null,
  extInstall: null,
  extPermissions: [],
  vault: null,
  passwordPrompt: null,
  autofill: null,
  sync: null,
  system: null,
  update: null,

  panel: null,
  popup: null,
  popupAnchor: null,
  drawer: null,
  menu: null,
  modal: null,
  dialogs: [],
  select: null,
  color: null,
  permissions: [],
  auth: null,
  toasts: [],
  palette_open: null,
  addressFocus: 0,
  addressOpen: false,
  splash: true,

  apply(event) {
    switch (event.type) {
      case "init":
        set({
          ready: true,
          engine: event.engine,
          session: event.session,
          settings: event.settings,
          palette: event.palette,
          themes: event.themes,
          bookmarks: event.bookmarks,
          downloads: event.downloads,
          pinboards: event.pinboards,
          siteMods: event.siteMods,
          systemDark: event.systemDark,
          splash: event.settings.appearance.splashScreen,
        });
        break;
      case "session":
        set({ session: event.session });
        break;
      case "settings":
        set({ settings: event.settings, palette: event.palette, themes: event.themes });
        break;
      case "tab-meta":
        set((s) => ({ meta: { ...s.meta, [event.meta.tab]: event.meta } }));
        break;
      case "tab-closed":
        set((s) => {
          const meta = { ...s.meta };
          delete meta[event.tab];
          return { meta };
        });
        break;
      case "bookmarks":
        set({ bookmarks: event.bookmarks });
        break;
      case "history-result":
        set((s) => ({ historyResults: { ...s.historyResults, [event.request]: event.visits } }));
        break;
      case "suggestions":
        set((s) => {
          if (event.request === 0) {
            // Remote suggestions arrive later; keep them only if the text still matches.
            return s.suggestions.text === event.text ? { suggestions: { ...s.suggestions, remote: event.items } } : {};
          }
          return { suggestions: { text: event.text, items: event.items, remote: [] } };
        });
        break;
      case "top-sites":
        break;
      case "downloads":
        set({ downloads: event.items });
        break;
      case "download-progress":
        set((s) => ({
          downloads: s.downloads.map((d) =>
            d.id === event.id ? { ...d, receivedBytes: event.received, totalBytes: event.total ?? d.totalBytes } : d,
          ),
          downloadSpeed: { ...s.downloadSpeed, [event.id]: event.bps },
        }));
        break;
      case "find-result":
        set((s) => ({ find: { ...s.find, matches: event.matches, active: event.active } }));
        break;
      case "dialog":
        set((s) => ({
          dialogs: [...s.dialogs, { id: event.id, tab: event.tab, kind: event.kind, message: event.message, defaultValue: event.defaultValue, origin: event.origin }],
        }));
        break;
      case "select":
        set({ select: { id: event.id, tab: event.tab, rect: event.rect, options: event.options, selected: event.selected, multiple: event.multiple } });
        break;
      case "color-picker":
        set({ color: { id: event.id, tab: event.tab, rect: event.rect, color: event.color } });
        break;
      case "context-menu":
        set({ menu: { kind: "page", x: event.menu.x, y: event.menu.y, info: event.menu } });
        break;
      case "permission":
        set((s) => ({ permissions: [...s.permissions, { id: event.id, tab: event.tab, origin: event.origin, kind: event.kind }] }));
        break;
      case "auth":
        set({ auth: { id: event.id, tab: event.tab, url: event.url, proxy: event.proxy } });
        break;
      case "hide-control":
        set((s) => ({
          dialogs: s.dialogs.filter((d) => d.id !== event.id),
          select: s.select?.id === event.id ? null : s.select,
          color: s.color?.id === event.id ? null : s.color,
          menu: s.menu?.kind === "page" && s.menu.info.id === event.id ? null : s.menu,
        }));
        break;
      case "toast":
        get().toast(event.text, event.kind);
        break;
      case "fullscreen":
        set({ fullscreen: event.on });
        break;
      case "window-state":
        set({ maximized: event.maximized, fullscreen: event.fullscreen, focused: event.focused });
        break;
      case "metrics":
        set((s) => ({
          metrics: event.metrics,
          cpuHistory: push(s.cpuHistory, event.metrics.cpu),
          memHistory: push(s.memHistory, event.metrics.memoryMb),
          netHistory: push(s.netHistory, event.metrics.downloadBps),
        }));
        break;
      case "blocked":
        set({ blockedTotal: event.total });
        break;
      case "panic":
        set({ panic: event.active });
        break;
      case "pinboards":
        set({ pinboards: event.pinboards });
        break;
      case "site-mods":
        set({ siteMods: event.mods });
        break;
      case "open":
        handleOpen(event.target);
        break;
      case "panel-badge":
        set((s) => ({ badges: { ...s.badges, [event.id]: event.count } }));
        break;
      case "panel-state":
        set((s) => ({ panelStates: { ...s.panelStates, [event.id]: { loading: event.loading, url: event.url, canGoBack: event.canGoBack } } }));
        break;
      case "popup-blocked":
        get().toast(`Blocked a pop-up from ${event.host || "this page"}`, "info", {
          label: "Always allow",
          run: () => {
            const list = get().settings?.privacy.popupAllowlist ?? [];
            if (event.host && !list.includes(event.host)) send({ type: "patch-settings", patch: { privacy: { popupAllowlist: [...list, event.host] } } });
          },
        });
        break;
      case "cleaner-done":
        get().toast(event.summary, "info");
        break;
      case "extensions":
        set({ extensions: event.items, extDeveloperMode: event.developerMode });
        break;
      case "ext-popup":
        set({ extPopup: { id: event.id, anchor: event.anchor, width: event.width, height: event.height }, popup: null, menu: null });
        break;
      case "ext-popup-closed":
        set({ extPopup: null });
        break;
      case "ext-install-prompt":
        set({ extInstall: { request: event.request, name: event.name, version: event.version, warnings: event.warnings, update: event.update } });
        break;
      case "vault":
        set({ vault: { status: event.status, logins: event.logins, neverSave: event.neverSave, cards: event.cards, addresses: event.addresses } });
        break;
      case "password-prompt":
        set({ passwordPrompt: { prompt: event.prompt, tab: event.tab, origin: event.origin, username: event.username, password: event.password, update: event.update } });
        break;
      case "autofill-suggest":
        set({ autofill: { tab: event.tab, rect: event.rect, kind: event.kind, items: event.items } });
        break;
      case "autofill-hide":
        set({ autofill: null });
        break;
      case "sync":
        set({ sync: event.status });
        break;
      case "system":
        set({ system: event.info });
        break;
      case "update": {
        const before = get().update?.state;
        set({ update: event.status });
        if (event.status.state === "ready" && before !== "ready") {
          get().toast(`HakuCX ${event.status.version} is ready to install`, "info", { label: "Restart to update", run: () => send({ type: "install-update" }) });
        }
        break;
      }
      case "revealed":
        // Answered to the component that asked (it listens with onEvent).
        break;
      case "ext-permission-prompt":
        set((s) => ({ extPermissions: [...s.extPermissions, { request: event.request, name: event.name, warnings: event.warnings }] }));
        break;
      case "ai-key":
      case "cleaner-stats":
      case "http-result":
      case "page-text":
      case "ai-reply":
      case "text-file":
        // Answers to one component's request; that component listens with onEvent.
        break;
    }
  },

  setPanel: (panel) => set({ panel, popup: null, drawer: null }),
  togglePanel: (panel) => set((s) => ({ panel: s.panel === panel ? null : panel, popup: null, drawer: null })),
  openPopup: (popup, anchor = null) =>
    set((s) => ({
      popup: s.popup === popup ? null : popup,
      popupAnchor: anchor && "width" in anchor ? { x: anchor.left, y: anchor.top, w: anchor.width, h: anchor.height } : anchor,
      menu: null,
    })),
  setDrawer: (drawer) => set({ drawer, popup: null, menu: null }),
  openMenu: (menu) => set({ menu, popup: null }),
  openModal: (modal) => set({ modal, popup: null, menu: null }),
  setFindOpen: (open) => {
    set((s) => ({ find: { ...s.find, open, nonce: s.find.nonce + 1, matches: open ? s.find.matches : 0, active: open ? s.find.active : 0 } }));
    if (!open) send({ type: "find-stop" });
  },
  setPalette: (open) => set({ palette_open: open, popup: null, menu: null }),
  setAddressOpen: (open) => set({ addressOpen: open }),
  focusAddress: () => set((s) => ({ addressFocus: s.addressFocus + 1 })),
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
  toast: (text, kind = "info", action) => {
    const id = toastId++;
    set((s) => ({ toasts: [...s.toasts.slice(-3), { id, text, kind, action }] }));
    window.setTimeout(() => get().dismissToast(id), kind === "error" || action ? 6000 : 3500);
  },
  shiftDialog: () => set((s) => ({ dialogs: s.dialogs.slice(1) })),
  shiftPermission: () => set((s) => ({ permissions: s.permissions.slice(1) })),
  clearSelect: () => set({ select: null }),
  clearColor: () => set({ color: null }),
  clearAuth: () => set({ auth: null }),
  patchSettings: (patch) => send({ type: "patch-settings", patch: patch as never }),
  endSplash: () => set({ splash: false }),
}));

/** Targets the core asks the UI to open (keyboard shortcuts, downloads popup, ...). */
function handleOpen(target: string) {
  const s = useStore.getState();
  if (target.startsWith("page:")) {
    openInternal(target.slice(5));
    return;
  }
  if (target.startsWith("qr:")) {
    s.openModal({ kind: "qr", url: target.slice(3) });
    return;
  }
  switch (target) {
    case "address":
      s.focusAddress();
      break;
    case "find":
      s.setFindOpen(true);
      break;
    case "tab-search":
      s.setPalette("tabs");
      break;
    case "command-palette":
      s.setPalette("commands");
      break;
    case "downloads-popup":
      useStore.setState({ popup: "downloads" });
      break;
    case "easy-setup":
      s.setDrawer(s.drawer === "easy-setup" ? null : "easy-setup");
      break;
    case "quit":
      s.openModal({ kind: "quit" });
      break;
    case "clear-data":
      s.openModal({ kind: "clear-data" });
      break;
    case "devtools":
      s.openModal({ kind: "devtools" });
      break;
    case "page-clicked":
      // A click on the page closes transient UI, like clicking outside a popup.
      useStore.setState((st) => ({
        popup: null,
        autofill: null,
        menu: st.menu?.kind === "page" ? st.menu : null,
        drawer: null,
        palette_open: null,
        panel: st.panel && st.settings?.sidebar.pinnedPanels.includes(st.panel) ? st.panel : null,
      }));
      break;
  }
}

/** Open an internal page: reuse an existing tab showing it, else a new tab. */
export function openInternal(url: string) {
  const s = useStore.getState();
  const existing = s.session?.tabs.find((t) => t.workspaceId === s.session?.activeWorkspace && t.url.startsWith(url.replace(/[?#].*$/, "")));
  if (existing) {
    send({ type: "activate-tab", tab: existing.id });
    if (existing.url !== url) send({ type: "navigate", tab: existing.id, input: url, disposition: "current-tab" });
  } else {
    send({ type: "new-tab", url, background: false, relativeTo: null });
  }
}

/** A menu, popup or dialog is open: all input goes to the UI, web panels included. */
export function selectModal(s: State): boolean {
  return Boolean(
    s.popup ||
      s.drawer ||
      s.menu ||
      s.modal ||
      s.select ||
      s.color ||
      s.auth ||
      s.palette_open ||
      s.addressOpen ||
      s.dialogs.length ||
      s.permissions.length ||
      s.extInstall ||
      s.extPermissions.length ||
      s.splash,
  );
}

/** UI is drawn over the page area (a modal, or a floating sidebar panel). */
export function selectOverlay(s: State): boolean {
  return selectModal(s) || Boolean(s.panel && s.settings && !s.settings.sidebar.pinnedPanels.includes(s.panel));
}

onEvent((e) => useStore.getState().apply(e));

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { Button, Radio, Segmented, Select } from "../../components/controls";
import { Icon } from "../../components/Icon";
import { send } from "../../ipc/bridge";
import { EARLY_BIRD, WALLPAPERS } from "../../lib/catalog";
import { patch } from "../../lib/util";
import { ThemeCarousel } from "../../overlays/EasySetup";
import { useStore } from "../../state/store";
import { go } from "../InternalPage";
import { SyncPanel } from "./autofill";
import { action, custom, link, select, slider, toggle, unsupported, type SectionDef } from "./rows";

const ENGINE_LIMIT = "Not supported by the Servo engine yet";

function ThemeRow() {
  const appearance = useStore((s) => s.settings?.appearance);
  if (!appearance) return null;
  return (
    <div className="col">
      <div className="row">
        <span className="grow" />
        <button className="link-btn" onClick={() => go("haku://settings/themes")}>
          All themes <Icon name="forward" size={14} />
        </button>
      </div>
      <ThemeCarousel perPage={5} />
      <Segmented wide value={appearance.colorMode} options={[{ value: "light", label: "Light" }, { value: "auto", label: "Auto" }, { value: "dark", label: "Dark" }]} onChange={(v) => patch({ appearance: { colorMode: v } })} />
    </div>
  );
}

function StartupPages() {
  const startup = useStore((s) => s.settings?.startup);
  if (!startup) return null;
  return (
    <div className="col">
      <Radio checked={startup.mode === "start-page"} onChange={() => patch({ startup: { mode: "start-page" } })} label="Begin fresh with a start page" />
      <Radio checked={startup.mode === "restore-session"} onChange={() => patch({ startup: { mode: "restore-session" } })} label="Retain tabs from previous session" />
      <Radio checked={startup.mode === "specific-pages"} onChange={() => patch({ startup: { mode: "specific-pages" } })} label="Open a specific page or set of pages" />
      {startup.mode === "specific-pages" && (
        <textarea
          className="text-input"
          rows={4}
          defaultValue={startup.pages.join("\n")}
          placeholder={"https://example.com\nhttps://another.example"}
          onBlur={(e) => patch({ startup: { pages: e.target.value.split("\n").map((l) => l.trim()).filter((l) => /^https?:\/\//.test(l)) } })}
        />
      )}
    </div>
  );
}

function WeatherCity() {
  const widgets = useStore((s) => s.settings?.startPage.widgets);
  if (!widgets) return null;
  return (
    <input
      className="text-input"
      style={{ width: 200 }}
      defaultValue={widgets.weatherCity}
      placeholder="City"
      onBlur={(e) => {
        const city = e.target.value.trim();
        if (city !== widgets.weatherCity) patch({ startPage: { widgets: { ...widgets, weatherCity: city, weatherLat: null, weatherLon: null } } });
      }}
    />
  );
}

function TextSetting({ value, placeholder, onSave, width = 260 }: { value: string; placeholder?: string; onSave: (v: string) => void; width?: number }) {
  return <input className="text-input" style={{ width }} defaultValue={value} placeholder={placeholder} onBlur={(e) => e.target.value !== value && onSave(e.target.value)} />;
}

function AiKey() {
  const stored = useStore((s) => s.settings?.ai.keyInVault);
  return (
    <div className="row">
      {stored ? <span className="chip accent">Key stored in Windows Credential Manager</span> : <input id="ai-key" className="text-input" type="password" placeholder="Paste API key" style={{ width: 220 }} />}
      {stored ? (
        <Button onClick={() => send({ type: "set-ai-key", key: null })}>Remove</Button>
      ) : (
        <Button
          kind="accent"
          onClick={() => {
            const el = document.getElementById("ai-key") as HTMLInputElement | null;
            if (el?.value.trim()) send({ type: "set-ai-key", key: el.value.trim() });
          }}
        >
          Save key
        </Button>
      )}
    </div>
  );
}

export const SECTIONS: SectionDef[] = [
  {
    id: "appearance",
    label: "Appearance",
    icon: "palette",
    group: 0,
    cards: [
      {
        title: "Themes",
        rows: [custom("", () => <ThemeRow />, undefined, true), action("Custom theme", "Create", () => useStore.getState().openModal({ kind: "theme-editor", themeId: null }), "Your own primary and secondary colors")],
      },
      {
        title: "Wallpapers",
        rows: [
          toggle("appearance", "showWallpaper", "Show wallpaper"),
          custom("Wallpaper", (s) => (
            <Select value={s.appearance.wallpaper} options={[...WALLPAPERS.map((w) => ({ value: w.id, label: w.name })), { value: "custom", label: "Custom image" }]} onChange={(v) => (v === "custom" ? send({ type: "pick-wallpaper" }) : patch({ appearance: { wallpaper: v } }))} width={210} />
          )),
          toggle("appearance", "animatedWallpaper", "Animated wallpaper"),
          toggle("appearance", "dailyWallpapers", "Daily wallpapers", "A new arrangement every day"),
        ],
      },
      {
        title: "Appearance",
        rows: [
          select("appearance", "interfaceMode", "Interface", [["auto", "Auto"], ["custom", "Custom"]], "Custom unlocks opacity, wallpaper blur, element backgrounds, vignette and focus mode"),
          slider("appearance", "opacity", "Opacity", 20, 100, 1, (v) => `${v}%`, 100),
          slider("appearance", "blur", "Wallpaper blur", 0, 40),
          slider("appearance", "elementsBackground", "Elements background", 0, 100),
          slider("appearance", "vignette", "Vignette", 0, 100, 1, (v) => `${v}%`, 100),
          toggle("appearance", "focusMode", "Focus mode", "Dims the browser frame so the page stands out"),
          select("appearance", "windowDecoration", "Show window decoration", [["outside", "Outside"], ["outside-except-maximized", "Outside except maximized"], ["inside", "Inside"]]),
          toggle("appearance", "nativeTitleBar", "Use the Windows title bar", "Takes effect after a restart"),
          toggle("appearance", "showBookmarksBar", "Show bookmarks bar"),
          toggle("appearance", "splitScreen", "Enable split screen", "Show two tabs side by side"),
          toggle("appearance", "splitScreenDrag", "Enable creating a split screen by drag and drop"),
          toggle("appearance", "reduceAnimations", "Reduce animations"),
          select("appearance", "fontSize", "Font size", [["very-small", "Very small"], ["small", "Small"], ["medium", "Medium (recommended)"], ["large", "Large"], ["very-large", "Very large"]]),
          custom("Standard font", (s) => <TextSetting value={s.appearance.fontStandard} onSave={(v) => patch({ appearance: { fontStandard: v } })} width={200} />),
          custom("Serif font", (s) => <TextSetting value={s.appearance.fontSerif} onSave={(v) => patch({ appearance: { fontSerif: v } })} width={200} />),
          custom("Sans-serif font", (s) => <TextSetting value={s.appearance.fontSans} onSave={(v) => patch({ appearance: { fontSans: v } })} width={200} />),
          custom("Fixed-width font", (s) => <TextSetting value={s.appearance.fontFixed} onSave={(v) => patch({ appearance: { fontFixed: v } })} width={200} />),
          slider("appearance", "pageZoom", "Default page zoom", 50, 200, 10, (v) => `${v}%`),
          slider("appearance", "uiScale", "Browser interface size", 75, 150, 5, (v) => `${v}%`),
          select("appearance", "cornerStyle", "Corner style", [["cut", "Cut (GX)"], ["round", "Round"], ["sharp", "Sharp"]]),
          slider("appearance", "roundness", "Roundness", 0, 24),
          toggle("appearance", "crosshairCursor", "Crosshair cursor"),
          custom("Interface CSS", (s) => <textarea className="text-input" rows={5} defaultValue={s.appearance.uiCss} placeholder="/* Restyle the browser itself */" onBlur={(e) => patch({ appearance: { uiCss: e.target.value } })} />, "Advanced: your CSS is applied to the browser interface", true),
        ],
      },
      {
        title: "User interface",
        rows: [
          toggle("tabs", "autoIslands", "Automatically create tab islands", "Tabs opened from a page are grouped with it"),
          select("tabs", "traces", "Show traces on most recently used tabs", [["off", "Off"], ["more-than10", "More than 10 tabs"], ["more-than15", "More than 15 tabs"], ["more-than30", "More than 30 tabs"]]),
          toggle("tabs", "snoozeInactive", "Snooze inactive tabs to save memory"),
          slider("tabs", "snoozeAfterMinutes", "Snooze after (minutes)", 5, 240, 5),
          toggle("tabs", "snoozeOnMemoryPressure", "Snooze inactive tabs when system memory usage is high"),
          toggle("tabs", "addressSuggestions", "Address bar search suggestions", "Your default search engine suggests searches as you type"),
          toggle("tabs", "tabPreviews", "Show tab previews"),
          toggle("tabs", "enhancedTooltips", "Show enhanced tab tooltips"),
          toggle("tabs", "searchInTabs", "Enable search in tabs"),
          toggle("tabs", "showAccountButton", "Show profile button"),
          toggle("tabs", "mruCycling", "Cycle tabs in most recently used order"),
          toggle("tabs", "showFullUrl", "Show full URL in combined search and address bar"),
          toggle("tabs", "scrollTopOnActiveClick", "Scroll to the top of the page by clicking the active tab"),
          toggle("tabs", "closeExternalAppTabs", "Automatically close tabs used to open external applications"),
          toggle("tabs", "warnCloseWindow", "Warn on closing window with multiple tabs"),
          toggle("tabs", "warnQuit", "Warn on quitting HakuCX with multiple tabs open"),
          toggle("tabs", "tabEmojis", "Tab emojis"),
        ],
      },
      {
        title: "Sidebar",
        rows: [
          toggle("appearance", "gxMessengerIcons", "Use GX messenger icons"),
          toggle("sidebar", "centerIcons", "Center icons"),
          toggle("sidebar", "show", "Show sidebar"),
          toggle("sidebar", "notificationBadges", "Enable notification badges for messengers"),
          toggle("appearance", "colorizeIcons", "Colorize icons using primary color"),
          action("Sidebar elements", "Open sidebar setup", () => useStore.getState().setDrawer("sidebar-setup")),
        ],
      },
      {
        title: "Start page",
        rows: [
          custom("Weather", (s) => (
            <div className="row">
              <WeatherCity />
              <Segmented value={s.startPage.widgets.fahrenheit ? "f" : "c"} options={[{ value: "c", label: "°C" }, { value: "f", label: "°F" }]} onChange={(v) => patch({ startPage: { widgets: { ...s.startPage.widgets, fahrenheit: v === "f" } } })} />
            </div>
          ), "Forecasts from Open-Meteo"),
          custom("Weather widget", (s) => <ToggleInline on={s.startPage.widgets.weather} set={(v) => patch({ startPage: { widgets: { ...s.startPage.widgets, weather: v } } })} />),
          custom("Clock", (s) => <ToggleInline on={s.startPage.widgets.clock} set={(v) => patch({ startPage: { widgets: { ...s.startPage.widgets, clock: v } } })} />),
          toggle("startPage", "hideSearch", "Hide search box"),
          toggle("startPage", "hideSpeedDial", "Hide Speed Dial"),
          toggle("startPage", "hideAddSite", "Hide \"Add a site\" button"),
          select("startPage", "dialPosition", "Speed Dial position", [["auto", "Auto"], ["top", "Top"], ["center", "Center"], ["bottom", "Bottom"], ["left", "Left"]]),
          select("startPage", "dialEffect", "Speed Dial effect", [["none", "None"], ["skew", "Skew"], ["glow", "Glow"], ["tilt", "Tilt"]]),
          toggle("startPage", "showDialNames", "Show Speed Dial names"),
          toggle("startPage", "biggerTiles", "Use bigger Speed Dial tiles"),
          toggle("startPage", "dialSuggestions", "Show Speed Dial suggestions"),
          toggle("startPage", "gameStrips", "Game strips", "Free games from the Epic Games Store"),
          toggle("startPage", "showNews", "Show news on start page"),
          toggle("startPage", "showContinue", "Show Continue On section"),
          toggle("startPage", "autoColumns", "Automatically adjust the maximum number of columns"),
          slider("startPage", "dialColumns", "Columns", 2, 12),
        ],
      },
    ],
  },
  {
    id: "features",
    label: "Features",
    icon: "gamepad-2",
    group: 0,
    cards: [
      {
        title: "GX basics",
        rows: [
          link("GX Lights", "haku://settings/lights", "Razer Chroma and OpenRGB devices"),
          link("Browser sounds", "haku://settings/browser-sounds"),
          link("Keyboard sounds", "haku://settings/keyboard-sounds"),
          link("Background music", "haku://settings/music"),
          link("Shaders", "haku://settings/shaders"),
          link("Web modding", "haku://settings/web-modding"),
          link("Panic button", "haku://settings/panic"),
          toggle("sidebar", "show", "Show sidebar"),
          toggle("features", "forceDarkPages", "Force dark pages"),
        ],
      },
      { title: "Profiles", rows: [action("Profiles", "Manage", () => useStore.getState().openModal({ kind: "profiles" }), "Standard, Streaming and Roguelike profiles")] },
      {
        title: "Battery saver",
        rows: [
          toggle("battery", "saver", "Enable battery saver", "Pauses animations and music and snoozes background tabs sooner"),
          toggle("battery", "autoWhenUnplugged", "Save battery automatically when unplugged"),
          slider("battery", "autoBelowPercent", "Or when the battery is below", 0, 80, 5, (v) => (v ? `${v}%` : "never")),
          toggle("battery", "showToolbarIcon", "Always show battery icon on toolbar"),
        ],
      },
      { title: "Easy files", rows: [toggle("features", "easyFiles", "Show recent downloads when a site asks you to upload a file")] },
      {
        title: "Player",
        rows: [
          toggle("player", "enabled", "Enable Player"),
          select("player", "miniplayer", "Show miniplayer when audio starts", [["off", "Never"], ["in-toolbar", "In toolbar"], ["in-sidebar", "In sidebar"]]),
          toggle("player", "miniplayerBackground", "Show miniplayer background"),
          toggle("player", "autoPause", "Automatically pause playback when other media starts playing"),
          slider("player", "resumeAfterSecs", "Resume playback after", 0, 30, 1, (v) => `${v} s`),
        ],
      },
      { title: "Pinboards", rows: [toggle("features", "pinboards", "Enable Pinboards")] },
      { title: "GX Rewards", rows: [toggle("features", "rewards", "Count XP and achievements on this device")] },
      {
        title: "Shortcuts",
        rows: [
          toggle("shortcuts", "mouseGestures", "Enable mouse gestures", "Hold the right button and draw: left = back, right = forward, up-down = reload, down-right = close tab"),
          toggle("shortcuts", "rockerGestures", "Enable rocker gestures", "Hold right and click left to go back; hold left and click right to go forward"),
          toggle("shortcuts", "advancedShortcuts", "Enable advanced keyboard shortcuts", "Single-key shortcuts without Ctrl or Alt"),
          link("Configure shortcuts", "haku://settings/shortcuts"),
        ],
      },
    ],
  },
  {
    id: "sync",
    label: "Synchronization",
    icon: "refresh",
    group: 0,
    cards: [
      {
        title: "Folder sync",
        rows: [{ kind: "custom", label: "Sync through a shared folder", render: () => <SyncPanel />, full: true }],
      },
      {
        title: "What to sync",
        rows: [
          toggle("sync", "bookmarks", "Bookmarks"),
          toggle("sync", "speedDial", "Speed Dial"),
          toggle("sync", "history", "History", "The last 30 days"),
          toggle("sync", "settings", "Settings", "Appearance, tabs, sidebar, start page, sounds, search, languages and shortcuts. Downloads, privacy, limits and this computer's paths stay local."),
          toggle("sync", "openTabs", "Open tabs", "See the tabs open on your other devices"),
          toggle("sync", "passwords", "Passwords, cards and addresses", "Encrypted with your sync passphrase in the shared folder"),
        ],
      },
      { title: "My Flow", rows: [toggle("features", "myFlow", "Enable My Flow")] },
    ],
  },
  {
    id: "early-bird",
    label: "Early Bird",
    icon: "rocket",
    group: 0,
    cards: [
      {
        title: "Early Bird",
        rows: [
          toggle("earlyBird", "enabled", "Get experimental features early", "Some experiments change or disappear between versions"),
          ...EARLY_BIRD.map((f) =>
            custom(f.name, (s) => (
              <ToggleInline
                on={!!s.earlyBird.features[f.id]}
                disabled={!s.earlyBird.enabled}
                set={(v) => patch({ earlyBird: { features: { ...s.earlyBird.features, [f.id]: v } } })}
              />
            ), f.description),
          ),
          action("Restart HakuCX now", "Relaunch", () => send({ type: "relaunch" }), "Some experiments need a restart"),
        ],
      },
    ],
  },
  {
    id: "video",
    label: "Video",
    icon: "film",
    group: 0,
    cards: [
      { title: "RGX", rows: [toggle("video", "rgxEnhanceVideos", "Enhance videos", "Sharper contrast and richer color for video"), toggle("video", "rgxEnhanceImages", "Enhance images")] },
      {
        title: "Video pop out",
        rows: [
          toggle("video", "popout", "Enable video pop out", "Watch a page's video in a small window that stays on top"),
          toggle("video", "autoPopout", "Automatically pop out videos when you switch tabs"),
          toggle("video", "popoutTransparent", "Make pop out transparent"),
        ],
      },
      { title: "Video pickup", rows: [toggle("video", "videoPickup", "Video pickup", "Show a button on videos to pop them out")] },
    ],
  },
  {
    id: "ai",
    label: "AI",
    icon: "sparkles",
    group: 0,
    cards: [
      {
        title: "AI assistant",
        rows: [
          toggle("ai", "enabled", "Enable the AI assistant"),
          select("ai", "provider", "Provider", [["none", "None"], ["anthropic", "Anthropic (Claude)"], ["open-ai-compatible", "OpenAI-compatible"], ["ollama", "Ollama (local)"]], "HakuCX sends your prompts only to the provider you choose"),
          custom("Model", (s) => <TextSetting value={s.ai.model} placeholder={s.ai.provider === "anthropic" ? "claude-sonnet-5-5" : s.ai.provider === "ollama" ? "llama3.2" : "model name"} onSave={(v) => patch({ ai: { model: v.trim() } })} width={220} />),
          custom("Endpoint", (s) => <TextSetting value={s.ai.endpoint} placeholder={s.ai.provider === "ollama" ? "http://127.0.0.1:11434" : "https://api.example.com/v1"} onSave={(v) => patch({ ai: { endpoint: v.trim() } })} width={260} />, "OpenAI-compatible and Ollama only"),
          custom("API key", () => <AiKey />),
          toggle("ai", "toolbarButton", "AI button in the toolbar"),
          toggle("ai", "highlightPrompts", "AI prompts in text highlight popup"),
        ],
      },
    ],
  },
  {
    id: "smart-home",
    label: "Smart home",
    icon: "lightbulb",
    group: 0,
    cards: [
      {
        title: "MQTT",
        rows: [
          toggle("smartHome", "mqtt", "MQTT support", "Publish your theme color and media state to an MQTT broker (Home Assistant, etc.)"),
          custom("Broker", (s) => <TextSetting value={s.smartHome.brokerUrl} onSave={(v) => patch({ smartHome: { brokerUrl: v.trim() } })} />),
          custom("Topic prefix", (s) => <TextSetting value={s.smartHome.topicPrefix} onSave={(v) => patch({ smartHome: { topicPrefix: v.trim() } })} width={160} />),
        ],
      },
    ],
  },
  {
    id: "autofill",
    label: "Autofill and passwords",
    icon: "key",
    group: 1,
    cards: [
      {
        title: "Autofill",
        rows: [
          link("Password manager", "haku://settings/passwords"),
          link("Payment methods", "haku://settings/payment-methods"),
          link("Addresses and more", "haku://settings/addresses"),
        ],
      },
    ],
  },
  {
    id: "vpn",
    label: "VPN",
    icon: "shield",
    group: 1,
    cards: [
      {
        title: "Proxy and VPN",
        rows: [
          custom("", () => (
            <p className="note">
              HakuCX has no VPN servers of its own and never pretends to. Route the browser through any proxy or VPN you trust: enter its HTTP proxy address here,
              or use your VPN app's system-wide connection.
            </p>
          ), undefined, true),
          select("system", "proxyMode", "Connection", [["system", "Use system proxy settings"], ["direct", "Direct (no proxy)"], ["manual", "Manual proxy"]]),
          custom("Proxy address", (s) => <TextSetting value={s.system.proxyUrl} placeholder="http://127.0.0.1:8080" onSave={(v) => patch({ system: { proxyUrl: v.trim() } })} />, "Used for HTTP and HTTPS in manual mode"),
          custom("Bypass for", (s) => <TextSetting value={s.system.proxyBypass} onSave={(v) => patch({ system: { proxyBypass: v } })} />, "Comma-separated hosts"),
        ],
      },
    ],
  },
  {
    id: "privacy",
    label: "Privacy & security",
    icon: "shield-check",
    group: 1,
    cards: [
      {
        title: "Privacy protection",
        rows: [
          toggle("privacy", "blockAds", "Block ads and surf the web faster"),
          toggle("privacy", "blockTrackers", "Block trackers"),
          toggle("privacy", "cosmeticFiltering", "Hide blocked ad placeholders", "Element hiding from the filter lists"),
          toggle("privacy", "showBlockedStats", "Show blocked count in the address bar"),
          link("Manage lists", "haku://settings/filter-lists", "EasyList, EasyPrivacy, uBlock filters, your own rules"),
          custom("Exceptions", (s) => <span className="dim">{s.privacy.adExceptions.length} sites</span>, "Turn blocking off for a site from the shield in the address bar"),
        ],
      },
      {
        title: "Privacy and security",
        rows: [
          action("Delete browsing data", "Delete...", () => useStore.getState().openModal({ kind: "clear-data" })),
          unsupported("Block third-party cookies", `${ENGINE_LIMIT}: the engine does not let the browser filter cookies`),
          toggle("privacy", "doNotTrack", "Send a \"Do Not Track\" signal", "Exposed to pages as navigator.doNotTrack (the engine does not let the browser add the DNT header)"),
          toggle("privacy", "globalPrivacyControl", "Send Global Privacy Control", "Exposed to pages as navigator.globalPrivacyControl"),
          toggle("privacy", "httpsOnly", "Always use secure connections", "Refuse plain HTTP sites"),
          toggle("privacy", "maliciousSiteProtection", "Protect me from malicious sites", "Blocks sites on the URLhaus malware list"),
          toggle("privacy", "javascriptOptimization", "JavaScript optimization", "Off disables the JIT compilers: slower, but a smaller attack surface"),
          toggle("privacy", "blockPopups", "Block pop-ups", "Allows new windows only right after you click or type on the page"),
          link("Site settings", "haku://settings/site-settings", "Permissions and content per site"),
          toggle("privacy", "pasteProtectHijack", "Paste Protect: warn when a site changes your clipboard"),
          toggle("privacy", "autoRemoveUnusedPermissions", "Automatically remove permissions from unused sites"),
          unsupported("Send crash reports", "HakuCX never sends anything about you or your browsing"),
          unsupported("Send feature usage statistics", "HakuCX has no telemetry"),
        ],
      },
      {
        title: "Clear on exit",
        rows: [
          custom("History", (s) => <ToggleInline on={s.privacy.clearOnExit.history} set={(v) => patch({ privacy: { clearOnExit: { ...s.privacy.clearOnExit, history: v } } })} />),
          custom("Cookies", (s) => <ToggleInline on={s.privacy.clearOnExit.cookies} set={(v) => patch({ privacy: { clearOnExit: { ...s.privacy.clearOnExit, cookies: v } } })} />),
          custom("Cache", (s) => <ToggleInline on={s.privacy.clearOnExit.cache} set={(v) => patch({ privacy: { clearOnExit: { ...s.privacy.clearOnExit, cache: v } } })} />),
          custom("Download history", (s) => <ToggleInline on={s.privacy.clearOnExit.downloads} set={(v) => patch({ privacy: { clearOnExit: { ...s.privacy.clearOnExit, downloads: v } } })} />),
        ],
      },
    ],
  },
  {
    id: "search",
    label: "Search engine",
    icon: "search",
    group: 1,
    cards: [
      {
        title: "Search engine",
        rows: [
          custom("Search engine used in the address bar", (s) => (
            <Select value={s.search.defaultEngine} options={s.search.engines.filter((e) => e.active).map((e) => ({ value: e.id, label: e.name }))} onChange={(v) => patch({ search: { defaultEngine: v } })} width={210} />
          )),
          custom("Search engine in private windows", (s) => (
            <Select value={s.search.privateEngine ?? ""} options={[{ value: "", label: "Same as normal" }, ...s.search.engines.filter((e) => e.active).map((e) => ({ value: e.id, label: e.name }))]} onChange={(v) => patch({ search: { privateEngine: v || null } })} width={210} />
          )),
          link("Manage search engines and site search", "haku://settings/search-engines"),
          toggle("search", "keywordShortcuts", "Enable search engine shortcuts in address bar", "Type a keyword and a space, like \"w rust\""),
        ],
      },
      {
        title: "Search pop-up",
        rows: [
          toggle("search", "searchPopup", "Show a pop-up when you select text on a page"),
          custom("Convert currencies to", (s) => (
            <Select value={s.search.popupCurrency} options={["USD", "EUR", "GBP", "JPY", "CAD", "AUD", "NOK", "SEK", "PLN", "BRL", "INR", "CNY"].map((c) => ({ value: c, label: c }))} onChange={(v) => patch({ search: { popupCurrency: v } })} width={120} />
          )),
          toggle("search", "popupUnits", "Convert units"),
          toggle("search", "popupTimeZones", "Convert time zones"),
        ],
      },
    ],
  },
  {
    id: "default-browser",
    label: "Default browser",
    icon: "globe",
    group: 1,
    cards: [{ title: "Default browser", rows: [unsupported("Make HakuCX your default browser", "Comes with the Windows installer, which registers HakuCX with Windows")] }],
  },
  {
    id: "startup",
    label: "On startup",
    icon: "play",
    group: 1,
    cards: [
      {
        title: "On startup",
        rows: [
          custom("", () => <StartupPages />, undefined, true),
          toggle("startup", "showStartPageFirst", "Display the start page first"),
          toggle("appearance", "splashScreen", "Splash screen on startup"),
          toggle("control", "keepAfterRestart", "Keep limiters enabled after browser restart"),
          unsupported("Run when my computer starts", "Comes with the Windows installer"),
        ],
      },
    ],
  },
  {
    id: "languages",
    label: "Languages",
    icon: "languages",
    group: 2,
    cards: [
      {
        title: "Languages",
        rows: [
          custom("Browser language", () => <span className="dim">English</span>, "More languages come from contributors (see CONTRIBUTING.md)"),
          custom("Preferred languages for websites", (s) => <TextSetting value={s.languages.acceptLanguages.join(", ")} onSave={(v) => patch({ languages: { acceptLanguages: v.split(",").map((x) => x.trim()).filter(Boolean) } })} />, "Comma-separated, most preferred first (for example en-US, nb)"),
          unsupported("Spell check", `${ENGINE_LIMIT}: no spell-checking in text fields`),
          custom("Translate into", (s) => <TextSetting value={s.languages.translateTarget} onSave={(v) => patch({ languages: { translateTarget: v.trim() || "en" } })} width={100} />, "Used by \"Translate page\" in the page menu"),
        ],
      },
    ],
  },
  {
    id: "downloads",
    label: "Downloads",
    icon: "download",
    group: 2,
    cards: [
      {
        title: "Downloads",
        rows: [
          custom("Location", (s) => (
            <div className="row">
              <span className="dim ellipsis" style={{ maxWidth: 260 }}>
                {s.downloads.directory || "Downloads folder"}
              </span>
              <Button onClick={() => send({ type: "pick-download-dir" })}>Change</Button>
            </div>
          )),
          toggle("downloads", "askWhere", "Ask where to save each file before downloading"),
          toggle("downloads", "openWhenDone", "Open files when they finish downloading"),
          toggle("downloads", "showPopup", "Show the downloads popup when a download starts"),
        ],
      },
    ],
  },
  {
    id: "accessibility",
    label: "Accessibility",
    icon: "eye",
    group: 2,
    cards: [
      {
        title: "Accessibility",
        rows: [
          toggle("accessibility", "focusHighlight", "Show a quick highlight on the focused object"),
          toggle("accessibility", "swipeNavigation", "Swipe between pages", "Two-finger horizontal swipe on a touchpad goes back and forward"),
          toggle("accessibility", "copyConfirmations", "Copied to clipboard confirmations"),
          toggle("accessibility", "highContrastUi", "High-contrast browser interface"),
          unsupported("Screen reader support", `${ENGINE_LIMIT} on Windows (accessibility tree is experimental)`),
        ],
      },
    ],
  },
  {
    id: "system",
    label: "System",
    icon: "cpu",
    group: 2,
    cards: [
      {
        title: "System",
        rows: [
          toggle("system", "multiprocess", "Multi-process mode (experimental)", "Pages run in separate processes; needed for the RAM hard limit. Restart required."),
          toggle("system", "diskCache", "Keep the web cache on disk", "Pages load faster on later visits. Takes effect after a restart."),
          toggle("system", "webgpu", "WebGPU (experimental)"),
          toggle("system", "textAntialiasing", "Smooth text"),
          select("system", "webrtcPolicy", "WebRTC", [["default", "Allowed"], ["disable-non-proxied-udp", "Disabled (no IP leaks, no video calls)"]], "The engine cannot limit WebRTC to specific network interfaces"),
          unsupported("Use graphics acceleration when available", "The engine always renders with the GPU (through ANGLE)"),
          toggle("system", "devtoolsServer", "Developer tools server", "Firefox DevTools can connect to localhost"),
        ],
      },
    ],
  },
  {
    id: "reset",
    label: "Reset settings",
    icon: "undo",
    group: 2,
    cards: [{ title: "Reset settings", rows: [action("Restore settings to their original defaults", "Reset", () => window.confirm("Reset every setting to its default? Bookmarks, history and tabs are kept.") && send({ type: "reset-settings" }), undefined, true)] }],
  },
  {
    id: "update",
    label: "Update & recovery",
    icon: "refresh",
    group: 3,
    cards: [{ title: "Update", rows: [link("Releases", "haku://about/", "HakuCX has no updater yet: new versions are published as GitHub releases"), unsupported("Automatic updates", "Comes with the signed updater")] }],
  },
];

function ToggleInline({ on, set, disabled }: { on: boolean; set: (v: boolean) => void; disabled?: boolean }) {
  return (
    <button type="button" role="switch" aria-checked={on} disabled={disabled} className={`toggle${on ? " on" : ""}`} onClick={() => set(!on)}>
      <span className="knob" />
    </button>
  );
}

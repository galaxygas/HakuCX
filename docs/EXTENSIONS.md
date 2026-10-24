# Extensions in HakuCX

HakuCX runs Chrome extensions (Manifest V2 and V3) and Firefox extensions. Servo has no extension support of its own, so the whole WebExtensions runtime is HakuCX code:

| Piece | Where |
|---|---|
| Manifest parsing (MV2, MV3, Firefox `browser_specific_settings`), match patterns, globs, `_locales`, packages (`.crx` 2/3 with signature checks, `.xpi`, `.zip`), ids, storage areas and quotas, declarativeNetRequest rules | `crates/haku-core/src/extensions/` (engine-independent, unit-tested) |
| The `chrome.*` / `browser.*` API objects that run inside extension pages and content scripts | `crates/hakucx/src/extensions/api.js` |
| Runtime state, content-script generation, serving extension files | `crates/hakucx/src/extensions/mod.rs` |
| Lifecycle, messaging, ports, toolbar, popups, context menus, shortcuts, request rules | `crates/hakucx/src/browser/extensions.rs` |
| The browser side of every API method | `crates/hakucx/src/browser/ext_api.rs` |
| Extensions page, toolbar buttons, popup frame, prompts | `ui/src/pages/ExtensionsPage.tsx`, `ui/src/shell/ExtensionButtons.tsx`, `ui/src/overlays/ExtPrompts.tsx` |
| Sample extensions and a test site | `tests/extensions/` |

## How it works

- **Extension pages** (background pages, popups, options pages, offscreen documents, pages opened in tabs) are served from `https://<id>.haku-extension.invalid/`. The `.invalid` host never resolves; HakuCX answers those requests itself before anything reaches the network. A real `https` origin is used because Servo gives custom schemes opaque origins, where `localStorage`, IndexedDB and other storage throw. `chrome.runtime.getURL()` returns these URLs.
- Every HTML page of an extension gets `/__haku/api.js` injected before its own scripts, which defines `chrome` and `browser` (the same promise-capable object; a trailing callback works too, with `runtime.lastError`).
- **Background pages** and **MV3 service workers** run in hidden webviews for as long as the extension is enabled. A service worker script runs in a generated page with the worker globals extensions use (`importScripts`, `self.registration`, `clients`, `skipWaiting`). It is never suspended, so `runtime.onSuspend` never fires.
- **Content scripts** are generated into one user script per extension and added to every tab's and web panel's `UserContentManager`; it checks `matches`, `exclude_matches`, globs, `all_frames`, `match_about_blank` and `run_at` in the page, and injects the CSS. Changes (enable, disable, `scripting.registerContentScripts`) apply to pages loaded afterwards, because Servo runs user scripts when a document starts.
- **Calls** from extension code go to the browser as console lines with a private prefix and a per-session token; replies and events come back through `evaluate_javascript`. Messages for content scripts are handed to the top frame, which relays them to child frames with `postMessage`.
- **Requests**: declarativeNetRequest rules and blocking `webRequest.onBeforeRequest` listeners run in `load_web_resource`. Servo's request interceptor handles one request at a time while a decision is pending, so a blocking listener gets at most 400 ms before the request continues unchanged. Redirects of a page navigate the tab; redirects of a subresource are answered with the target's content (extension files, `data:` URLs, or an `http(s)` fetch made by HakuCX).

## Installing

- **Install from file** on the Extensions page accepts `.crx`, `.xpi` and `.zip`. HakuCX shows what the extension can do (its permission warnings) and installs only after you confirm. Packages are copied into the profile's `extensions` folder.
- `.crx` signatures are checked the way Chrome checks packages from outside its store: every signature must match the archive, and one must come from the key the extension id is derived from, so an update can only come from whoever holds that key. `.xpi` signatures (Mozilla's) are not checked; `.zip` archives are unsigned.
- **Developer mode** adds **Load unpacked**, which runs an extension from its folder, and **Reload**.
- `hakucx --load-extension <folder>` runs an unpacked extension for one session (used by the tests).
- There is no extension store in HakuCX, and installed extensions are not updated automatically yet (`runtime.requestUpdateCheck` answers `no_update`). Firefox Add-ons offers `.xpi` downloads directly.

## API support

Legend: **yes** works as in Chrome; **partial** works with the differences listed; **no** is not available (calls reject with a "not supported by HakuCX yet" error, or the namespace is missing).

| Namespace | Support | Notes |
|---|---|---|
| `runtime` | yes | `sendMessage`/`onMessage`, `connect`/`onConnect` ports, `onInstalled` (`install`, `update`), `onStartup`, `getURL`, `getManifest`, `openOptionsPage`, `reload`, `setUninstallURL` (http/https), `getPlatformInfo`, `getBrowserInfo`, `getContexts`, `onMessageExternal` between installed extensions (`externally_connectable` is not enforced; web pages cannot message extensions). No `getBackgroundPage` (pages cannot share JavaScript objects), `connectNative`, `sendNativeMessage`. |
| `storage` | yes | `local` (10 MB, or unlimited with `unlimitedStorage`), `sync` (Chrome's quotas, stored locally, not synced), `session` (memory; content scripts only after `setAccessLevel`), `onChanged`. `managed` is always empty. |
| `i18n` | partial | `getMessage` (placeholders, `@@` messages), `getUILanguage`, `getAcceptLanguages`. `detectLanguage` finds nothing (no language detector). |
| `tabs` | partial | `query`, `get`, `getCurrent`, `create`, `update`, `remove`, `reload`, `duplicate`, `sendMessage`, `connect`, `captureVisibleTab`, `executeScript`/`insertCSS`/`removeCSS` (MV2), `highlight`, `move`, `getZoom`, `setZoom`, `discard` (snooze), `group`/`ungroup` (tab islands). Events: `onCreated`, `onUpdated`, `onRemoved`, `onActivated`. `url`, `title` and `favIconUrl` need the `tabs` permission or host access, as in Chrome. All workspaces' tabs are in one window. `onMoved`, `onReplaced`, `onHighlighted`, `onAttached`, `onDetached`, `onZoomChange` never fire. |
| `windows` | partial | One window (id 1): `get`, `getCurrent`, `getLastFocused`, `getAll`, `update` (state, position, size, focus), `remove`. Every HakuCX window is its own process, so `create` opens its URLs as tabs in this window, or a new private window for `incognito: true` (which extensions cannot address). Window events never fire. |
| `action`, `browserAction`, `pageAction` | yes | Title, badge text and colors, icon (paths or `ImageData`), popup, enable/disable, global or per tab, `openPopup`, `onClicked`, `getUserSettings`. Popups size themselves to their content (up to 800×600) and stay inside the page area, under the toolbar. |
| `contextMenus`, `menus` | partial | `create`, `update`, `remove`, `removeAll`, `onClicked`, normal/checkbox/radio/separator items, `documentUrlPatterns`, `targetUrlPatterns`. Contexts `all`, `page`, `link`, `image`, `selection`, `editable` (no `frame`, `video`, `audio`, `action`). `onShown`/`onHidden` never fire. |
| `alarms` | yes | Checked once a second. |
| `notifications` | partial | Shown as HakuCX notifications. `onClicked`, `onClosed` and `onButtonClicked` never fire. |
| `scripting` | partial | `executeScript` (`func` + `args`, or `files`), `insertCSS`, `removeCSS`, `registerContentScripts` (persisted unless `persistAcrossSessions: false`), `getRegisteredContentScripts`, `updateContentScripts`, `unregisterContentScripts`. Injection reaches the top frame only; `frameIds` other than 0 and `documentIds` are refused. `world` is always the page's world (see Security). |
| `cookies` | partial | `get`, `getAll`, `set`, `remove`, `getAllCookieStores`, using the engine's cookie store and needing host permissions. Servo looks cookies up by URL: `getAll` without `url` checks the root path of every site that has cookies, so cookies scoped to a sub-path are found only when `url` names that path. `onChanged` never fires. |
| `declarativeNetRequest` | partial | Static rulesets (toggle in the Extensions page, `updateEnabledRulesets`, `updateStaticRules`), dynamic and session rules, `block`, `allow`, `allowAllRequests`, `upgradeScheme`, `redirect` (all forms), priorities, `getMatchedRules` (needs `declarativeNetRequestFeedback` or `activeTab`), `testMatchOutcome`, `isRegexSupported`, `setExtensionActionOptions` (badge count). `modifyHeaders` rules are accepted but not applied (Servo does not let the browser change request or response headers). `onRuleMatchedDebug` never fires. |
| `webRequest` | partial | `onBeforeRequest`, observing or blocking (`cancel`, `redirectUrl`) from the background context, MV2 with `webRequestBlocking`. Every other event (`onBeforeSendHeaders`, `onHeadersReceived`, `onCompleted`, `onErrorOccurred`, ...) never fires. |
| `commands` | yes | Manifest shortcuts, editable on the Extensions page; `_execute_action` opens the popup. |
| `permissions` | yes | `contains`, `getAll`, `request` (asks you), `remove`, `onAdded`, `onRemoved`. |
| `history` | partial | `search`, `getVisits`, `addUrl`, `deleteUrl`, `deleteRange`, `deleteAll`, `onVisited`. `onVisitRemoved` never fires. |
| `downloads` | partial | `download` (GET only, no extra headers or body), `search`, `open`, `show`, `showDefaultFolder`, `cancel`, `pause`, `resume`, `erase`, `onCreated`, `onChanged`, `onErased`. |
| `bookmarks` | partial | `getTree`, `get`, `getChildren`, `getRecent`, `search`, `create`, `update`, `remove`/`removeTree` (to HakuCX's bookmark trash), `move`. Ids are HakuCX's. Events never fire. |
| `management` | partial | `getSelf`; `getAll`/`get` for other extensions with the `management` permission. Events never fire. |
| `idle` | partial | `queryState`, `setDetectionInterval`, `onStateChanged` (Windows input idle time). `locked` is never reported. |
| `offscreen` | yes | `createDocument`, `closeDocument`, `hasDocument`. |
| `extension` | partial | `getURL`, `inIncognitoContext`, `isAllowedIncognitoAccess`, `isAllowedFileSchemeAccess` (always false), `getViews` (empty), `getBackgroundPage` (null). |
| `webNavigation`, `omnibox`, `sidePanel`/`sidebarAction`, `devtools`, `tabCapture`, `desktopCapture`, `tts`, `proxy`, `privacy`, `identity`, `debugger`, `nativeMessaging`, `sessions`, `topSites`, `contentSettings`, `fontSettings`, `declarativeContent`, `pageCapture`, `search`, `tabGroups`, `userScripts`, `chrome_url_overrides`, `chrome_settings_overrides` | no | The Extensions page lists what an installed extension uses that HakuCX does not have yet. |

## Known differences

- **Content scripts share the page's JavaScript world** (Servo has no isolated worlds). See [SECURITY.md](../SECURITY.md).
- **Extension pages are `https` pages.** Servo blocks plain `http://` subresources and `fetch`es from them as mixed content (`localhost` and `127.0.0.1` are allowed). Extensions that talk to `http://` servers from their pages fail where Chrome would allow it.
- **MV3 service workers run in a page**, so `window` and `document` exist in them; code that checks for their absence to detect a worker will think it runs in a page.
- `importScripts` in a service worker evaluates the fetched script with `eval`, so the generated page that hosts the worker allows `'unsafe-eval'`. Other extension pages keep Chrome's default CSP (MV3: `script-src 'self' 'wasm-unsafe-eval'; object-src 'self'`) or the manifest's.
- **Private windows** run only extensions allowed in private, with the storage they had when the window opened; their changes are not saved. Extensions cannot be installed from a private window.
- Servo's speculative parser can fetch a script twice; `webRequest` listeners then see both requests (the badge count counts each URL once per page load).
- Web pages cannot navigate to extension pages unless the extension lists them in `web_accessible_resources`, as in Chrome.

## Testing

`tests/extensions/` has three sample extensions that exercise most of the runtime (static, dynamic and redirect rules; a content script with CSS, messages and ports; a context-menu item, toolbar button, popup, options page, shortcut, storage and alarms; a Firefox-style `browser.*` extension with a blocking `webRequest` listener) and a test site that checks the results:

```bash
python tests/extensions/server.py
```

```bash
cargo run -p hakucx -- --load-extension tests/extensions/blocker-mv3 --load-extension tests/extensions/content-mv2 --load-extension tests/extensions/firefox-mv2 http://127.0.0.1:8765/
```

The page shows PASS/FAIL for each check, and every extension and the page report what they saw to `tests/extensions/reports.jsonl`.

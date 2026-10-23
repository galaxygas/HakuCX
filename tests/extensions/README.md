# Sample extensions

Three small extensions that exercise HakuCX's WebExtensions runtime, and a local test site that checks what they did. See [docs/EXTENSIONS.md](../../docs/EXTENSIONS.md#testing) for how to run them.

| Folder | Kind | Exercises |
|---|---|---|
| `blocker-mv3` | Chrome MV3, service worker | Static, dynamic and redirect declarativeNetRequest rules, the matched-rule badge, `getMatchedRules`, popup, options page, `storage`, `alarms`, `_locales` |
| `content-mv2` | Chrome MV2, persistent background page | Content script with CSS, `runtime.sendMessage` with an async reply, ports, `tabs.sendMessage`, `browserAction.onClicked`, `contextMenus`, `commands`, `tabs.onUpdated`, `i18n` placeholders |
| `firefox-mv2` | Firefox (gecko id), `browser.*` promises | Blocking `webRequest.onBeforeRequest`, `runtime.getPlatformInfo` |

`site/` is served by `server.py` on `http://127.0.0.1:8765/`; results POSTed to `/report` are appended to `reports.jsonl`.

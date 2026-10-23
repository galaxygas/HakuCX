# Network requests HakuCX makes on its own

HakuCX has no servers and no telemetry. This file lists every request the browser makes that is not a page you asked to load. Anything added to the browser that contacts the network on its own must be listed here, be visible in the UI, and have an off switch.

Requests made by HakuCX code go out through `reqwest` (filter lists, downloads, `http-get` for the UI) or from the UI webview itself (images on the start page and in GX Corner). None of them carry a HakuCX identifier. Private windows never send address bar suggestions.

| Feature | Host(s) | When | Off switch |
|---|---|---|---|
| Ad and tracker filter lists | `easylist.to` (EasyList, EasyPrivacy); `ublockorigin.github.io` (uBlock filters) and `secure.fanboy.co.nz` (annoyances) only if you turn them on; any list URL you add | At startup, for enabled lists older than 4 days | Settings → Privacy & security → Manage lists (turn a list off), or turn off ad and tracker blocking |
| Malicious site protection | `malware-filter.gitlab.io` (URLhaus filter) | Same schedule as the filter lists | Settings → Privacy & security → Protect me from malicious sites |
| Address bar suggestions | The search engine you chose, e.g. `duckduckgo.com/ac/`, `suggestqueries.google.com` | While you type in the address bar (never in private windows) | Settings → Appearance → User interface → Address bar search suggestions |
| Weather widget | `geocoding-api.open-meteo.com`, `api.open-meteo.com` | When the start page shows and a city is set | Settings → Appearance → Start page → Weather widget |
| Start page game strips | `store-site-backend-static.ak.epicgames.com` and the image hosts it returns | When the start page shows | Settings → Appearance → Start page → Game strips |
| Start page news | `www.pcgamer.com`, `www.rockpapershotgun.com` (RSS) | When the start page shows | Settings → Appearance → Start page → Show news on start page |
| GX Corner and My Games | Epic Games Store (as above), `www.cheapshark.com`, `store.steampowered.com`, RSS from PC Gamer, Rock Paper Shotgun and GameSpot, and the image hosts they return (Steam and Epic CDNs) | Only while you have GX Corner or My Games open | Don't open the page; remove GX Corner and My Games from the sidebar |
| Personal news panel | The RSS feeds listed in the panel (defaults: PC Gamer, Rock Paper Shotgun, Ars Technica gaming, The Verge) | When the panel is open | Close the panel or remove the feeds |
| AI assistant | The provider you configure: `api.anthropic.com`, your OpenAI-compatible endpoint, or your local Ollama | Only when you send a message | Settings → AI (provider "None" or the assistant off) |
| Downloads | The site you download from (with that site's cookies) | When you download a file | — |
| Page source view | The page's own host, fetched again without your cookies | When you open View page source | — |

Sidebar web panels (Twitch, Discord, WhatsApp, ...) are ordinary pages: they load only when you open them, and stay loaded until you close the panel.

Extensions you install make their own requests, like any page; HakuCX does not add any for them, with one exception: when an extension redirects a page's subresource to another `http(s)` address (declarativeNetRequest or `webRequest`), HakuCX fetches that address itself, without cookies, and hands the result to the page, because Servo does not follow redirects from intercepted requests. Extension pages themselves are served from the profile, never from the network (`*.haku-extension.invalid` does not resolve). HakuCX does not check for extension updates.

Not built yet, and listed here when they land: update checks (GitHub Releases), folder sync (no network; your sync app moves the files), GX Lights (local only: Razer Chroma SDK on `localhost:54235`, OpenRGB on `127.0.0.1:6742`), MQTT smart home (your broker).

# HakuCX compared with Opera GX: what is done, partial and missing

Status as of 2026-10-23 (milestone M9), HakuCX on Servo 0.7.0.

**Sources:**
- Opera's official feature page (opera.com/gx/features), which lists 56 features.
- Opera's June 2026 GX update post (Playground, GX Rewards, Easy Files, mini player, browser connector, GX Calendar).
- Opera's feature pages for Fake My History, Custom Cursors, Live Wallpapers and RGX.
- The research screenshots of Opera GX's settings and GX surfaces.
- Live checks of HakuCX itself, recorded in [COMPAT.md](../COMPAT.md).

**Status values:**
- **Done:** works in HakuCX and was checked.
- **Partial:** part of the feature works; the note says which part and why the rest doesn't.
- **Missing:** not in HakuCX. The blocker is one of these:
  - **engine:** Servo can't do it yet.
  - **service:** it needs Opera's servers or accounts.
  - **licensing:** it needs content or a brand HakuCX can't ship.
  - **not built:** nothing stops it; it hasn't been written yet.

HakuCX never shows a feature that does nothing. A setting that can't work is shown disabled, with the reason.

## Opera's 56 listed features

| # | Opera GX feature | HakuCX | Notes and blocker |
| --- | --- | --- | --- |
| 1 | GX Control | Partial | The CPU limiter is a Windows job object rate cap, measured at 25.8% under a 25% cap (see COMPAT.md). The RAM soft limit snoozes background tabs. The RAM hard limit needs experimental multi-process mode. Hot Tabs Killer lists tabs by how long they have been idle, because Servo reports no per-tab CPU or memory (**engine**). |
| 2 | GX Mods | Partial | HakuCX mods (`.hakumod`): theme, wallpaper, sounds, music, shaders and UI CSS, with a Mods hub. Opera's GX Store mods can't be installed. The store is Opera's (**service**) and its mod payload format is not publicly documented. |
| 3 | Integrated Twitch | Partial | Twitch opens as a sidebar web panel and browsing works. Live streams don't play, because Twitch needs Media Source Extensions (**engine**, servo#22416). There is no followed-channels list with live alerts, which would need Twitch API keys (**service**). |
| 4 | Integrated Discord | Partial | Discord opens as a sidebar web panel, but the Discord web app doesn't start in Servo (**engine**, see COMPAT.md). |
| 5 | Ad Blocker | Done | Brave's `adblock` engine with EasyList, EasyPrivacy and optional uBlock lists, cosmetic filters, per-site exceptions and blocked counts. |
| 6 | X in browser | Done | Sidebar web panel. The signed-out page renders; signing in was not tested. |
| 7 | Instagram | Partial | Sidebar web panel; not tested against the live site. |
| 8 | Telegram | Partial | Sidebar web panel; not tested against the live site. |
| 9 | WhatsApp | Partial | Sidebar web panel; not tested against the live site. |
| 10 | Messenger | Partial | Sidebar web panel; not tested against the live site. |
| 11 | Live Wallpapers | Partial | Animated browser wallpapers and daily wallpapers. Opera's interactive scenes, which show system metrics, are GX Store content (**licensing**). A HakuCX version is **not built**. |
| 12 | Smart Home | Partial | HakuCX publishes the theme color and now-playing state to your MQTT broker, so Home Assistant or Node-RED can drive the lights. Opera's direct Philips Hue control is **not built**; the Hue bridge has a local API, so it is feasible. |
| 13 | ChatGPT | Done | Sidebar web panel. It works as far as chatgpt.com works in Servo. |
| 14 | Browser AI in GX | Partial | The AI assistant with highlight prompts uses your own provider: Anthropic, any OpenAI-compatible endpoint, or local Ollama. There is no free hosted AI, because that would need servers (**service**). |
| 15 | Music Player | Partial | The Player panel, mini player and auto-pause work. Spotify, Apple Music, Deezer and Tidal need DRM (**engine**, servo#27623). YouTube Music needs MSE (**engine**). |
| 16 | Custom Cursors | Missing | **Not built.** It would need `.cur`/`.ani` parsing, a custom winit cursor for the UI, and CSS `cursor` for pages. |
| 17 | Shaders | Done | ShiftFX, MidnightFX, FadeFX, BoostFX, MonoCore, DuoFX and user shaders. |
| 18 | Opera AI | Partial | Same as #14. Page context works, because the assistant can read the open page's text. Opera's AI "control" of the browser is Missing (**not built**). |
| 19 | Tab Islands | Done | Manual islands and islands made automatically when you open links; collapse, rename and color. |
| 20 | Tabs | Done | Pinning, muting, emojis, tab traces, hover previews, search in tabs, and snoozing inactive tabs. |
| 21 | Split Screen | Done | Drag a tab onto the page to split; checked live. |
| 22 | Messengers | Partial | See #4 and #7–10. |
| 23 | Snapshot | Done | Captures the visible area or a cropped area. |
| 24 | Keyboard sounds | Done | Keyboard sound packs with their own volume. |
| 25 | Deezer | Missing | Needs DRM (**engine**). |
| 26 | Tidal | Missing | Needs DRM (**engine**). |
| 27 | YouTube Music | Missing | Needs MSE (**engine**). |
| 28 | Spotify | Missing | Needs DRM (**engine**). |
| 29 | Apple Music | Missing | Needs DRM (**engine**). |
| 30 | Free VPN for Windows | Partial | You bring your own proxy or VPN in Settings → VPN. HakuCX has no VPN servers (**service**) and doesn't pretend to. |
| 31 | Free VPN for Mac | Missing | HakuCX is Windows-only for now. Same **service** blocker as #30. |
| 32 | Browser Games | Missing | GX.games is Opera's platform (**service**). Ordinary web games run as far as Servo's WebGL and WebAssembly allow. |
| 33 | Game Trailers | Missing | The trailers are on YouTube, which can't play without MSE (**engine**). A trailer feed is **not built**. |
| 34 | Game Deal Aggregator | Done | GX Corner and My Games use CheapShark deals, Epic free games and Steam specials (keyless public APIs). |
| 35 | Release Calendar | Partial | GX Corner shows Steam's "coming soon" list. A date calendar view is **not built**. |
| 36 | Browser Profiles | Done | Standard, Streaming and Roguelike profiles, each running as its own process. |
| 37 | Opera Translate | Partial | Translate page opens the page in Google Translate's web proxy. Built-in translation is **not built**; Bergamot (local, open source) is the candidate. |
| 38 | Tab Emojis | Done | |
| 39 | Bluesky | Done | Sidebar web panel. |
| 40 | Slack | Partial | Sidebar web panel; not tested against the live site. |
| 41 | Personal News | Done | RSS panel with your own feeds. |
| 42 | GX Network Limiter | Partial | Caps HakuCX's own downloads, and the UI says so. Page traffic goes through Servo's network stack, which has no rate limit hook (**engine**). |
| 43 | Background Music | Done | Pauses while a page plays sound and while battery saver is on. |
| 44 | Force Dark Pages | Done | Applies globally and per site. |
| 45 | Fake My History | Partial | A Playground experiment: "Redeem my soul" in History replaces your history with wholesome pages (checked live). The automatic replacement after 14 days of inactivity is **not built**. |
| 46 | RGX Mode | Partial | Raises contrast and saturation of videos and images with CSS filters. Opera's real-time enhancement and A/B slider are **not built**, and Servo has no hook to process video frames (**engine**). |
| 47 | Panic Button | Done | Configurable key and safe pages, and the session comes back afterwards. |
| 48 | Battery Saver | Done | Manual, or automatic when unplugged or below a charge level. It pauses music, snoozes background tabs and stops animations. |
| 49 | Video Popout | Done | Pop-out window, mini player and auto pop-out, for video Servo can play. |
| 50 | Bookmarks | Done | Folders, trash, drag and drop, and HTML import/export. |
| 51 | Workspaces | Done | |
| 52 | Pinboards | Done | |
| 53 | Flow | Done | My Flow pairs with a phone on your own network with end-to-end encryption, with no Opera account. |
| 54 | GX Cleaner | Done | MIN, MED and MAX presets, plus age filters per category. |
| 55 | Free VPN | Partial | See #30. |
| 56 | GX Corner | Partial | Free games, deals, upcoming games, Steam top sellers, news and wishlist work. Daily quizzes, custom YouTube feeds and Opera's curated Top 10 lists are **not built**, and their videos would need MSE. |

**Summary:** 24 Done, 23 Partial, 9 Missing.
- Of the 9 Missing, five are music services blocked by DRM or MSE, and Game Trailers is blocked by MSE. GX.games and Mac VPN are blocked by Opera's servers or by platform. Custom Cursors can be built.
- Most Partial entries are web panels whose sites can't run fully in Servo, or Opera-hosted services that HakuCX replaces with something you bring yourself.

## Other Opera GX features (not on the 56-item page)

| Feature | HakuCX | Notes and blocker |
| --- | --- | --- |
| Playground (opt-in experiments) | Done | Two experiments that each gate real code: Fake My History, and "Touch grass after panic". Up to three in the loadout. |
| GX Rewards (Roblox rewards) | Partial | Local XP and levels that unlock themes and sounds. The Roblox rewards come from Opera's partnership (**service**, **licensing**). |
| Easy Files | Done | When a page asks for a file, recent downloads are offered first, or you can browse. Checked live. |
| Mini player | Done | |
| Gemini in the sidebar | Done | Web panel; also Claude. |
| Browser connector (lets ChatGPT or Claude Code use the browser) | Missing | **Not built.** An MCP server over Servo's WebDriver support is feasible. |
| GX Calendar | Missing | **Not built.** It would sit on the GX Corner wishlist and the Steam "coming soon" data. |
| Search pop-up (search, copy, currency, units, time zones) | Done | Checked live: currency, units and time zones. |
| Mouse and rocker gestures | Done | |
| Paste Protect | Done | Covers IBANs, card numbers and crypto addresses. |
| GX Lights (Razer Chroma, other RGB through OpenRGB) | Done | Includes a music visualizer. |
| Tab hover previews | Done | |
| Quieter permission prompts | Done | |
| Remove permissions of unused sites | Done | After 90 days. |
| Swipe between pages | Missing | **engine**: Servo doesn't report whether a page used a sideways scroll, so a swipe can't be told apart from scrolling a carousel. |
| Spell check | Missing | **engine**: Servo has no spell checking in text fields. |
| Block third-party cookies | Missing | **engine**: Servo gives the embedder no cookie filtering. |
| Screen reader support | Missing | **engine**: Servo's accessibility tree is experimental on Windows. |
| Opera account sync | Partial | Encrypted export/import and folder sync (OneDrive, Dropbox, Syncthing). There is no Opera account (**service**). |

## Engine gaps (Servo 0.7.0)

These limit HakuCX itself, not only single features:

1. **No Media Source Extensions** (servo#22416): YouTube, Twitch live, YouTube Music and most streaming video. This is the biggest gap for a gaming browser.
2. **No EME/DRM** (servo#27623): Netflix, Disney+, Spotify, Apple Music, Deezer, Tidal. Widevine also needs a license from Google, so this may stay out of reach for an independent engine.
3. **Large single-page apps fail** on missing or incomplete web APIs: Discord, GitHub's file browser and BBC News all fail (COMPAT.md). Servo's overall WPT pass rate is around 68%.
4. **No content-process sandbox on Windows.** Multi-process mode exists but is experimental and off by default. See SECURITY.md.
5. **No isolated worlds:** extension content scripts share the page's JavaScript world. See SECURITY.md.
6. **No per-tab resource accounting:** Hot Tabs Killer can't rank tabs by real CPU or memory use.
7. **No network rate-limit hook:** the network limiter covers only HakuCX's own downloads.
8. **Passkeys/WebAuthn:** not available to pages.
9. **Spell check, third-party cookie blocking, screen readers, swipe navigation:** see the table above.
10. **DevTools:** Servo's Firefox remote-debugging server is exposed as a setting. The experience is limited compared with Chrome's or Firefox's own tools.
11. **AV1 and other codecs** depend on the installed GStreamer plugins. See MEDIA.md.
12. **CSS gaps** seen on real sites: icons drawn with `mask-image` show as solid boxes (MDN, Wikipedia).

## Prioritized roadmap

Ordered by what most improves real use for Opera GX's audience, then by effort.

1. **Track Servo's MSE work** (servo#22416) and adopt it as soon as it lands. It unblocks YouTube, Twitch live, YouTube Music, game trailers and much of the Corner. Contributing upstream is the most valuable thing this project can do.
2. **Site compatibility triage:** reduce the Discord, GitHub and BBC failures to minimal test cases and file them upstream, then re-run COMPAT.md with each Servo release (monthly).
3. **Multi-process mode on by default**, once Servo's is stable: it enables the RAM hard limit and real per-tab metrics for Hot Tabs Killer.
4. **Custom Cursors:** `.cur`/`.ani` support for the UI and pages, shipped in mods.
5. **Local translation with Bergamot**, replacing the Google Translate hand-off.
6. **Direct Philips Hue control** through the bridge's local API, next to MQTT.
7. **GX Calendar** and a date view for the release calendar, built on the existing Steam and wishlist data.
8. **Browser connector:** an MCP server so local AI tools can drive HakuCX, with logins staying on the computer.
9. **Fake My History auto-replace** after 14 days of inactivity.
10. **Spell check** in HakuCX's own text fields (Hunspell dictionaries), until Servo has its own.
11. **Twitch followed-channels list:** only if it can work without HakuCX holding API secrets. If it can't, leave it out.
12. **macOS and Linux builds** (Servo supports both). These would also make the "VPN for Mac" row moot.

Out of reach without a third party: DRM playback, GX.games, Opera-hosted AI and VPN, GX Store content, and Roblox rewards.

# Site compatibility

What real sites do in HakuCX today. HakuCX renders pages with Servo 0.7.0, so a page that breaks here
usually breaks in Servo itself; the cause column says when it is a known engine gap.

Tested 2026-10-23 on Windows 11 with a debug build. Each site was opened in a fresh profile
(default settings, ad and tracker blocking on) and checked after 18 seconds. Nobody was signed in.

| Site | Page | Result | Notes |
| --- | --- | --- | --- |
| Wikipedia | article (`/wiki/Rust_(programming_language)`) | Works | Layout, infobox, contents and the appearance panel all render. A few icons show as solid squares (CSS `mask-image` icons). |
| DuckDuckGo | search results | Works | Results, the knowledge panel and the filters render and the search box works. |
| Hacker News | front page | Works | Renders exactly as expected. |
| Twitch | `/directory` | Works | Browsing, images and the live channel list render. Live video is untested; see the YouTube note. |
| Reddit | `/r/rust` | Works | Passes Reddit's JavaScript challenge and renders posts, code blocks and the sidebar. |
| Steam store | front page | Works | The hero, menus and search render. |
| X | `x.com` (signed out) | Works | The sign-in page renders; the blocker counted one tracker. Signing in was not tested. |
| MDN | `/docs/Web/CSS/grid` | Mostly works | Text, code and navigation render, but icons drawn with CSS `mask-image` show as white squares. The ads shown are MDN's own first-party ads, which the default lists allow. |
| GitHub | repository page (`servo/servo`) | Partial | The header and tabs render, but the file list shows GitHub's "Unable to load page" error from a script failure in the engine. |
| YouTube | watch page | Partial | The page and player chrome render, but videos do not play ("Your browser can't play this video"). YouTube streams through Media Source Extensions, which Servo does not have (servo#22416). |
| BBC News | `/news` | Broken | The consent dialog shows, but behind it the page fails with Next.js's "Application error: a client-side exception". |
| Discord | `/login` | Broken | Stays blank: the app never starts in Servo. |

## Known engine gaps behind these results

- **No Media Source Extensions** (servo#22416): YouTube, Twitch live, and most streaming sites cannot
  play. Plain `<video>` files (MP4/WebM/Ogg) do play through GStreamer; see [MEDIA.md](MEDIA.md).
- **No DRM (EME/Widevine)** (servo#27623): Netflix, Disney+, Spotify Web and other DRM services cannot play.
- **Script compatibility:** large single-page apps (GitHub's file browser, Discord, BBC's Next.js
  front end) hit APIs Servo does not implement yet and fail on the client.
- **CSS `mask-image` icons** render as solid boxes on some sites (MDN, Wikipedia).

HakuCX does not hide or fake any of this: these are Servo's limits, tracked in
[research/MISSING_FEATURES.md](research/MISSING_FEATURES.md) along with the HakuCX features they affect.

## GX Control limiter check

Measured on 2026-10-23 on a 16-core machine. The test page ran 16 Web Workers in busy loops, and
CPU use was summed over every `hakucx` process with `Get-Process` across 10 seconds:

| CPU limiter | Browser CPU use (share of all cores) |
| --- | --- |
| Off | 98.7% |
| On, capped at 25% | 25.8% |

The cap is a Windows job object CPU rate limit, enforced by the kernel. The RAM hard limit needs
multi-process mode (Settings → System, experimental and off by default). Without it, GX Control
shows the hard limit as unavailable and says why, and the soft limit (snoozing background tabs)
applies instead. The RAM hard limit was not measured in this pass.

## Re-running the check

Open each URL above in a fresh profile (`hakucx --data-dir <empty folder> <url>`), wait for it to
settle, and update the table with the date and the Servo version.

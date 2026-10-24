# Audio, video, pop out and the miniplayer

## Playback

Servo plays `<audio>` and `<video>` through GStreamer. HakuCX builds Servo with its GStreamer
backend (the `media` feature of the `hakucx` crate, on by default) and ships the 36 GStreamer
plugins Servo loads, plus the libraries they need, next to `hakucx.exe` (see
[THIRD-PARTY.md](THIRD-PARTY.md) for licenses).

What plays (checked against the shipped plugins):

| Kind | Formats |
|---|---|
| Containers | MP4/MOV, WebM/Matroska, Ogg, WAV, MP3 |
| Video | H.264, H.265, VP8, VP9, Theora |
| Audio | AAC, MP3, Opus, Vorbis, FLAC/WAV |

What does not, because Servo does not support it yet (this cannot be fixed inside HakuCX):

- **Media Source Extensions** (`MediaSource`, used by YouTube, Twitch, most streaming sites and
  most web music players). Pages that build their stream in script do not play.
- **DRM** (EME / Widevine): Netflix, Disney+, Spotify, Apple Music, Deezer, Tidal.
- **AV1**: Servo's plugin list does not include an AV1 decoder.

A plain `<video src="https://...">` (news sites, many embeds, direct links to video files)
plays.

## Building with media

Building needs GStreamer's MSVC x86_64 release **with development files**. `build.rs` finds it
through `GSTREAMER_1_0_ROOT_MSVC_X86_64` or the installer's default folders, and links the
plugins and the DLLs they import (found by reading their import tables) into `target/<profile>/`
and `target/<profile>/deps/`, and lists them in `gstreamer-files.txt` for packaging.

```powershell
packaging\windows\gstreamer.ps1 -Install
```

That downloads the official installer (version and SHA-256 pinned in the script), installs it
for your Windows account, and sets `PATH` and `PKG_CONFIG_PATH` in the current PowerShell. In a
new PowerShell, run it again (without `-Install`) before `cargo build`:

```powershell
packaging\windows\gstreamer.ps1
```

Building without media (`cargo build -p hakucx --no-default-features`) needs no GStreamer, and
pages then cannot play audio or video.

Servo fires `timeupdate` on a media element only once the element's text track list exists,
which Servo creates when something reads `textTracks` (its own media controls do). HakuCX's
page script reads it for every media element, so custom players see their position move.

## Video pop out

Servo draws every webview of a painter into one surface and cannot move a running page into
another window, so the pop out does not move the tab's `<video>` element:

1. The page (the Pop out button over a video, the miniplayer, or auto pop out) reports the
   video's address, position, volume and size; HakuCX pauses the tab's copy.
2. HakuCX opens a borderless always-on-top window showing `haku://popout/` (its own page,
   [popout](../crates/hakucx/src/browser/popout/)), which plays the same address from the same
   position. The address travels in the URL fragment of that internal page.
3. **Back to tab** (or double-click) closes the window and resumes the tab's video at the pop
   out's position; closing the window leaves it paused there.

This works for videos with an `http(s)` address. A video the page builds from a stream
(`blob:` Media Source, WebRTC) cannot be handed to another window; HakuCX says so instead of
opening an empty one. Conference pop out (video calls) is therefore not offered.

Settings → Video: **Enable video pop out**, **Automatically pop out videos when you switch tabs**
(and bring them back when you return to the tab), **Make pop out transparent** (the window is
see-through while the pointer is elsewhere), and **Video pickup** (the Pop out button over
videos). Drag the video to move the window; drag its edges to resize it. Keys: Space play/pause,
arrows seek and volume, M mute, Enter back to tab, Esc close.

## Miniplayer

Settings → Features → Player → **Show miniplayer**: in the toolbar (a headphones button with a
popup) or at the bottom of the sidebar. It lists every tab with audio or video, with the title,
artist and artwork the page gives its Media Session (Servo passes no artwork to the browser and
reports only the most recent session, so HakuCX reads them in the page), play/pause,
previous/next when the page handles them, pop out for videos, and Go to tab.

**Automatically pause playback when other media starts playing** pauses the sidebar Player
while a tab plays sound and resumes it the set number of seconds after the tab stops. The
Player's music services are DRM or Media Source sites, which Servo cannot play yet, so in
practice this has nothing to pause today.

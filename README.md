# HakuCX

HakuCX is an independent, open-source desktop browser with a GX-style interface: themes, wallpapers, sounds, a sidebar, resource limiters and deep customization. It is built in **Rust** on the **[Servo](https://servo.org) web engine**. It is not Chromium, not Electron, and not affiliated with Opera.

> **Status: early development.** Browsing, tabs, workspaces, islands, split screen, history, bookmarks, Speed Dial, downloads, ad and tracker blocking, the full GX-style interface and settings, GX Control, GX Cleaner, shaders, web mods, sounds, GX Corner, a bring-your-own-key AI assistant, Chrome/Firefox extensions, an encrypted password manager with autofill, and folder sync work today. GX Lights and the installer are still being built (see [the roadmap](#roadmap)). Expect sites that break: Servo is younger than Blink and Gecko.

## Why

- **Not Chromium.** Every page is rendered by Servo, the memory-safe, parallel engine written in Rust.
- **Customizable to the core.** The whole browser UI is HTML/CSS rendered by Servo itself, so themes and mods can restyle everything, not just a few colors.
- **Honest features.** Nothing is simulated. GX Control's limits are real Windows kernel limits; ad blocking uses real filter lists; if something can't work yet, the UI says so instead of faking it.
- **Private by default.** HakuCX has no servers and sends no telemetry. Online features (news, deals, weather) call public APIs directly and can be turned off.

## Engine limits you should know about

These come from Servo itself and cannot be fixed inside HakuCX:

| Limit | Effect |
|---|---|
| No EME / Widevine DRM | Netflix, Disney+, Spotify Web and other DRM video/audio will not play. |
| No Media Source Extensions | Adaptive-streaming players (including parts of YouTube) may not play. |
| No content-process sandbox on Windows | A Servo bug that leads to code execution is not contained by an OS sandbox. See [SECURITY.md](SECURITY.md). |
| No isolated worlds | Extension content scripts share the page's JavaScript world. See [SECURITY.md](SECURITY.md) and [EXTENSIONS.md](docs/EXTENSIONS.md). |
| ~68% Web Platform Tests pass rate | Some sites render incorrectly. Reports help: please file them. |

## Building (Windows)

Requirements:

- Rust (stable, MSVC toolchain), 1.97 or newer
- Visual Studio 2022 Build Tools with **Desktop development with C++** and **ATL**
- LLVM/Clang 19+ (set `LIBCLANG_PATH`, e.g. `C:\Program Files\LLVM\bin`)
- CMake
- Node.js 22+ and pnpm (for the UI)

```bash
winget install Microsoft.VisualStudio.2022.BuildTools LLVM.LLVM Kitware.CMake
```

```bash
cargo build -p hakucx
```

```bash
cargo run -p hakucx
```

The first build compiles all of Servo (about 600 crates) and takes a while on a typical desktop; later builds are incremental and take seconds. A debug build uses about 5 GB in `target/` with the workspace's default profile settings.

## Repository layout

| Path | What it is |
|---|---|
| `crates/hakucx` | The browser binary: window, Servo embedding, compositor, input routing, IPC |
| `crates/haku-core` | Engine-independent state: settings, themes, tabs/workspaces/islands, persistence, the IPC protocol |
| `ui/` | The browser interface (React + TypeScript), rendered by Servo in a transparent layer over the pages |
| `docs/` | Architecture, research, agent guidance |

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how the pieces fit together.

## Roadmap

| Milestone | Scope | State |
|---|---|---|
| M0 | Servo embedding spike: transparent HTML UI composited over pages, IPC, interception | done |
| M1 | Repository foundation, CI, contributor and agent docs | done |
| M2 | Core browser: sessions, workspaces, islands, history, bookmarks, Speed Dial, downloads | done |
| M3 | GX-fidelity UI: tab strip, sidebar, Easy Setup, settings, themes, wallpapers | done |
| M4 | Ad/tracker blocking, privacy, permissions, proxy/VPN, downloads | mostly done (third-party cookie control and DoH need engine support) |
| M5 | GX Control (kernel limits), Cleaner, shaders, web mods, sounds, lights, player, video tools | in progress: GX Lights, video pop-out and the miniplayer remain |
| M6 | WebExtensions runtime (Chrome MV2/MV3, Firefox) | done (see [EXTENSIONS.md](docs/EXTENSIONS.md) for what each API supports) |
| M7 | Mods, Corner, AI, password manager, sync | done (see [PASSWORDS-AND-SYNC.md](docs/PASSWORDS-AND-SYNC.md)); My Flow (phone pairing) remains |
| M8 | Installer, updater, default-browser integration | planned |

## Contributing

Contributions from people and AI agents are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) and, for agents, [AGENTS.md](AGENTS.md).

## License

[Mozilla Public License 2.0](LICENSE), the same license as Servo.

"Opera" and "Opera GX" are trademarks of Opera Norway AS. HakuCX is an independent project and is not affiliated with, endorsed by, or sponsored by Opera.

# HakuCX architecture

HakuCX is a Rust application that embeds the Servo web engine (`servo` crate, pinned exactly) and renders **both** web pages and its own user interface with it.

```
┌──────────────────────────── winit window (WindowRenderingContext, ANGLE/EGL) ───────────────────────────┐
│  compositor (glow): 1. clear  2. page textures at the rects the UI reports  3. UI shell texture on top   │
│                                                                                                         │
│   ┌──────────── UI shell webview (haku://ui/) ─────────────┐     ┌──── page webviews (tabs) ─────┐      │
│   │ TextureRenderingContext (own painter)                  │     │ TextureRenderingContext       │      │
│   │ transparent background; leaves a "viewport hole"       │     │ shared by all tabs; only the  │      │
│   └────────────────────────────────────────────────────────┘     │ active tab is shown           │      │
│                                                                   └───────────────────────────────┘      │
└─────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

## Rendering

- Every webview renders into a `TextureRenderingContext` (`crates/hakucx/src/texture_context.rs`), our implementation of Servo's `RenderingContext` trait that renders into a GL texture we own. Servo's built-in `OffscreenRenderingContext` only offers an opaque framebuffer blit, which cannot layer a transparent UI over a page.
- Servo creates **one painter (WebRender instance) per rendering context**. HakuCX therefore keeps a small fixed set of contexts: the UI shell, the shared page context for tabs (only the active tab is shown), a second page context for split screen, and one for sidebar web panels.
- Servo's clear color (`shell_background_color_rgba`) is fully transparent, so UI areas without a background let the page show through. The compositor paints a white underlay below pages, because pages without a background are expected to be white.
- The compositor (`compositor.rs`) draws premultiplied-alpha textured quads with `glow`, using GLSL ES 3.0 on ANGLE and GLSL 3.30 on desktop GL.
- `backdrop-filter` and `mask-image` do not render in Servo 0.7, and in any case the UI could not blur the page, because the page is a different texture.
- Servo draws inline `<svg>` as a separate image: `currentColor`, CSS variables and web fonts never reach it. UI icons are therefore glyphs from the Lucide icon font, and the few SVG drawings (logo, gauges, sparklines, wallpapers) get concrete colors from the theme palette in the store (`useThemeColor`).
- `WebView::resize` must be called without resizing the rendering context first: Servo skips the resize when the context already has the new size.

## Input

- Pointer events are hit-tested in Rust: inside the page rect they go to the active page (translated to page coordinates) unless the UI reports an open overlay (menus, popovers). Everything else goes to the UI shell.
- Keyboard input goes to whichever view has focus; focus follows clicks. Browser-level shortcuts are matched in Rust before forwarding (paused while the UI records a shortcut, `key-capture`).
- After handling every window event or wake-up, the app pumps its queues, calls `Servo::spin_event_loop()` so the input reaches Servo immediately (as servoshell does), and pumps again. Without the second spin, typed text waits for the next unrelated event before it is shown.

## UI ↔ core IPC

- **UI → Rust.** The UI writes `console.debug("\u0001haku-ipc:" + JSON)`. Servo reports console messages per webview through `WebViewDelegate::show_console_message`, so Rust knows exactly which webview sent a message. **Only the UI shell's webview id is accepted**; IPC lines from any other webview are rejected and logged. (A custom-protocol `fetch` endpoint was considered; Servo streams request bodies to protocol handlers through an internal IPC channel that embedders cannot easily read, and the request's webview id is the same identity we already get here.)
- **Rust → UI.** Events are delivered in batches with `WebView::evaluate_javascript` calling `window.__haku.receive(event)`.
- **Re-entrancy.** Servo invokes delegate methods and evaluation callbacks while holding internal `RefCell` borrows, so calling back into Servo from them panics. All IPC messages and UI events are queued (`inbox` / `outbox`) and processed by `State::pump()` after `spin_event_loop()` returns.

## Internal pages

`haku://ui/` (the UI shell) is served by `HakuProtocolHandler` from `ui/dist`, compiled into the binary, with a strict Content-Security-Policy. Other internal pages (`haku://start/`, `haku://settings/...`, history, bookmarks, downloads, mods, GX Corner, ...) are not separate webviews: the UI shell draws them inside the page area while the tab shows an internal URL, so they share the UI's IPC channel and nothing else does. Web content is never allowed to navigate to `haku:`.

## Extensions

Servo has no WebExtensions support, so HakuCX implements the runtime (details and the API support matrix: [EXTENSIONS.md](EXTENSIONS.md)):

- Extension pages are served by HakuCX from `https://<id>.haku-extension.invalid/` through `load_web_resource` (a custom scheme would get an opaque origin, where web storage throws). Background pages and service workers run in hidden webviews on their own painter; a popup runs in a fourth pane context and is composited under the UI, which leaves a hole for it (so UI menus and dialogs can still cover it) and keeps it inside the page area.
- Extension code calls the browser with console lines carrying a per-session token, like the UI's IPC but on a separate, token-checked channel with a per-context method allow-list; the UI's own IPC channel stays restricted to the UI webview.
- Content scripts are one user script per extension in every page's `UserContentManager`; they run in the page's world.
- declarativeNetRequest and blocking `webRequest` run in `load_web_resource`. A blocked decision holds the request (and, because Servo's interceptor is serialized, every other request) for at most 400 ms.

## UI state

The UI keeps a mirror of the core's state in a Zustand store (`ui/src/state/store.ts`), updated only by events from Rust. Selectors must return stable values: a selector that returns a fresh `[]` or object on every call makes React re-render forever (use the shared `NONE` constant).

## State

`haku-core` holds everything that does not need the engine: the settings schema (one JSON document per profile with JSON-merge-patch updates and range clamping), the theme catalog and palette derivation, and the session model (tabs, workspaces, tab islands, split screen, recently closed tabs, MRU order). The browser applies an operation to the session and then reconciles Servo webviews against it, which keeps every tab rule unit-testable.

## Verified behaviour (engine spike, Servo 0.7.0, Windows 11)

- A standalone `cargo build` against `servo = "=0.7.0"` with the `no-wgl` (ANGLE) feature works; the ANGLE DLLs are copied next to the binary by `crates/hakucx/build.rs`.
- The transparent UI shell composites over live pages (Wikipedia), including menus drawn over page content, in windowed and maximized states.
- Real mouse, wheel and keyboard input route correctly between the UI and the page.
- A page forging an IPC line is rejected; the ad-network request test is intercepted and cancelled; user scripts run in pages; `take_screenshot` returns the page image.

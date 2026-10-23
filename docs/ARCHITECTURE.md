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
- Servo creates **one painter (WebRender instance) per rendering context**. HakuCX therefore keeps a small fixed set of contexts: the UI shell, the shared page context for tabs (only the active tab is shown), and later a second page context for split screen and contexts for sidebar web panels and the video pop-out.
- Servo's clear color (`shell_background_color_rgba`) is fully transparent, so UI areas without a background let the page show through. The compositor paints a white underlay below pages, because pages without a background are expected to be white.
- The compositor (`compositor.rs`) draws premultiplied-alpha textured quads with `glow`, using GLSL ES 3.0 on ANGLE and GLSL 3.30 on desktop GL.
- `backdrop-filter` in the UI cannot blur the page, because the page is a different texture. Glass effects over pages are done by the compositor (blur regions reported by the UI).

## Input

- Pointer events are hit-tested in Rust: inside the page rect they go to the active page (translated to page coordinates) unless the UI reports an open overlay (menus, popovers). Everything else goes to the UI shell.
- Keyboard input goes to whichever view has focus; focus follows clicks. Browser-level shortcuts are matched in Rust before forwarding.

## UI ↔ core IPC

- **UI → Rust.** The UI writes `console.debug("\u0001haku-ipc:" + JSON)`. Servo reports console messages per webview through `WebViewDelegate::show_console_message`, so Rust knows exactly which webview sent a message. **Only the UI shell's webview id is accepted**; IPC lines from any other webview are rejected and logged. (A custom-protocol `fetch` endpoint was considered; Servo streams request bodies to protocol handlers through an internal IPC channel that embedders cannot easily read, and the request's webview id is the same identity we already get here.)
- **Rust → UI.** Events are delivered in batches with `WebView::evaluate_javascript` calling `window.__haku.receive(event)`.
- **Re-entrancy.** Servo invokes delegate methods and evaluation callbacks while holding internal `RefCell` borrows, so calling back into Servo from them panics. All IPC messages and UI events are queued (`inbox` / `outbox`) and processed by `State::pump()` after `spin_event_loop()` returns.

## Internal pages

`haku://<host>/<path>` is served by `HakuProtocolHandler` from assets compiled into the binary. Every response carries a strict Content-Security-Policy. Internal pages never get IPC access unless they are the UI shell.

## State

`haku-core` holds everything that does not need the engine: the settings schema (one JSON document per profile with JSON-merge-patch updates and range clamping), the theme catalog and palette derivation, and the session model (tabs, workspaces, tab islands, split screen, recently closed tabs, MRU order). The browser applies an operation to the session and then reconciles Servo webviews against it, which keeps every tab rule unit-testable.

## Verified behaviour (engine spike, Servo 0.7.0, Windows 11)

- A standalone `cargo build` against `servo = "=0.7.0"` with the `no-wgl` (ANGLE) feature works; the ANGLE DLLs are copied next to the binary by `crates/hakucx/build.rs`.
- The transparent UI shell composites over live pages (Wikipedia), including menus drawn over page content, in windowed and maximized states.
- Real mouse, wheel and keyboard input route correctly between the UI and the page.
- A page forging an IPC line is rejected; the ad-network request test is intercepted and cancelled; user scripts run in pages; `take_screenshot` returns the page image.

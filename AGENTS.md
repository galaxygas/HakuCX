# Guidance for AI agents (and humans in a hurry)

This file is the contract for anyone, human or AI, changing HakuCX. Read it before editing.

## Ground rules

1. **Never fake a feature.** If something cannot work (engine limit, no server, missing API), show it disabled with the reason, or leave it out. No simulated VPNs, fake stats, placeholder data presented as real, or toggles that do nothing.
2. **Never re-enter Servo from a Servo callback.** Delegate methods and `evaluate_javascript` callbacks run while Servo holds internal borrows. Queue work (`State::inbox` / `State::outbox`) and handle it in `State::pump()` after `spin_event_loop()` returns. Violations panic with `RefCell already borrowed`.
3. **Only the UI shell webview may use IPC.** IPC messages from any other webview must be rejected (see `show_console_message`). Never expose the IPC channel, a token, or a privileged API to web content or content scripts.
4. **Resize webviews, not rendering contexts.** Call `WebView::resize`; it resizes the shared context. Resizing the context first makes Servo skip the viewport update.
5. **Pin Servo exactly.** `servo = "=x.y.z"`. Upgrades are their own PR with the API changes from Servo's monthly release notes.
6. **No telemetry, no tracking, no remote code.** Network calls the browser makes on its own (filter lists, feeds, weather) must be user-visible, documented in `docs/NETWORK.md`, and switchable off.
7. **Settings changes go through `haku_core::settings`.** Add fields with defaults; never rename or remove a persisted key without a migration in `Settings::migrate`.
8. **Tests come with logic.** Anything in `haku-core` (or any engine-independent crate) needs unit tests. Run `cargo test -p haku-core` before you commit.

## Where things live

| Task | Start here |
|---|---|
| Tabs, workspaces, islands, split screen rules | `crates/haku-core/src/session.rs` |
| A new setting | `crates/haku-core/src/settings.rs` (+ UI) |
| Theme colors / palette tokens | `crates/haku-core/src/themes.rs` |
| Window, compositor, input routing | `crates/hakucx/src/main.rs`, `compositor.rs` |
| `haku://` internal pages | `crates/hakucx/src/protocol.rs` |

## Build and check

```bash
cargo fmt --all
```

```bash
cargo clippy --workspace -- -D warnings
```

```bash
cargo test --workspace
```

Building `hakucx` compiles Servo; see the README for Windows prerequisites.

## Style

- Match the surrounding code: comment density, naming and idiom.
- Every source file starts with the MPL-2.0 header.
- Explain *why* in comments, not *what*.
- Keep UI strings in sentence case. Do not use Opera trademarks in user-facing text except to say HakuCX is not affiliated.

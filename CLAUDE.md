# CLAUDE.md

Follow [AGENTS.md](AGENTS.md). It is the single source of rules for agents working on HakuCX.

Quick reminders:

- Never fake a feature; disable it with a reason instead.
- Never call Servo APIs from inside a Servo callback; queue and handle in `State::pump()`.
- Only the UI shell webview may speak IPC.
- Run `cargo test -p haku-core` and `cargo clippy --workspace -- -D warnings` before committing.

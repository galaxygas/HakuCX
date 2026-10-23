# Security policy

## Reporting a vulnerability

Please report security issues privately through GitHub's **[Report a vulnerability](https://github.com/galaxygas/HakuCX/security/advisories/new)** form. Do not open a public issue. Include steps to reproduce, the affected version or commit, and the impact you expect.

You will get an acknowledgement as soon as a maintainer sees it. Fixes for confirmed issues are prioritized over feature work, and reporters are credited unless they ask not to be.

Bugs in the Servo engine itself should also be reported to the Servo project (see Servo's own security policy); we will help coordinate.

## Threat model and known limits

HakuCX is early-stage software. Be aware of these properties before relying on it:

- **No OS sandbox for web content on Windows.** Servo does not currently sandbox content processes on Windows. A memory-safety bug in Servo's C/C++ dependencies (SpiderMonkey, ANGLE, media stack) that leads to code execution would run with your user's privileges.
- **Content scripts share the page's JavaScript world.** Servo has no isolated worlds, so HakuCX's WebExtensions runtime injects content scripts into the page's own context. A hostile page can observe or tamper with an extension's content script. Extensions' privileged APIs are only reachable from their background/popup/options pages, never from content scripts directly.
- **Privileged UI.** The browser UI is HTML rendered by Servo from the `haku://` scheme. Only the UI shell webview may send IPC to the Rust core; messages from any other webview are rejected and logged. Internal pages carry a strict Content-Security-Policy.
- **No telemetry.** HakuCX does not phone home. Network requests the browser makes on its own behalf (filter list updates, optional feeds, weather) are listed in `docs/NETWORK.md`.
- **Secrets.** Settings files never contain secrets. The password/card/API-key vault (milestone M7) is designed around Argon2id + XChaCha20-Poly1305 with the key protected by Windows DPAPI; until it ships, HakuCX does not store passwords at all.

## Supported versions

Only the latest release and `main` receive security fixes during early development.

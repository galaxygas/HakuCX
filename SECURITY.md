# Security policy

## Reporting a vulnerability

Please report security issues privately through GitHub's **[Report a vulnerability](https://github.com/galaxygas/HakuCX/security/advisories/new)** form. Do not open a public issue. Include steps to reproduce, the affected version or commit, and the impact you expect.

You will get an acknowledgement as soon as a maintainer sees it. Fixes for confirmed issues are prioritized over feature work, and reporters are credited unless they ask not to be.

Bugs in the Servo engine itself should also be reported to the Servo project (see Servo's own security policy); we will help coordinate.

## Threat model and known limits

HakuCX is early-stage software. Be aware of these properties before relying on it:

- **No OS sandbox for web content on Windows.** Servo does not currently sandbox content processes on Windows. A memory-safety bug in Servo's C/C++ dependencies (SpiderMonkey, ANGLE, media stack) that leads to code execution would run with your user's privileges.
- **Content scripts share the page's JavaScript world.** Servo has no isolated worlds, so HakuCX's WebExtensions runtime injects content scripts into the page's own context. A hostile page on a site where an extension's content scripts run can observe and tamper with them, and can impersonate them: it can read and write that extension's `storage.local`/`storage.sync`, and send messages and open ports that arrive as if they came from the content script (the `tab` in `sender` is still the real tab; `sender.url` is not trustworthy). Content scripts only get the APIs Chrome gives them (messaging, storage, i18n); tabs, cookies, history, downloads, scripting and request rules are reachable only from the extension's own pages, which run on their own origin and are checked against the extension's per-session token. Extensions you install should be treated as trusted with every site they declare in their permissions.
- **Extension packages.** `.crx` signatures are verified (every proof must match the archive, and one must come from the key the extension id derives from). `.xpi` signatures are not verified. HakuCX shows an extension's permissions and asks before installing or granting more. The generated page that hosts an MV3 service worker allows `'unsafe-eval'` so `importScripts` can work; other extension pages keep Chrome's default policy or the extension's own. Web pages cannot navigate to an extension's pages unless the extension lists them as web-accessible.
- **Privileged UI.** The browser UI is HTML rendered by Servo from the `haku://` scheme. Only the UI shell webview may send IPC to the Rust core; messages from any other webview are rejected and logged. Internal pages carry a strict Content-Security-Policy.
- **Updates are signed.** The updater installs only an installer whose minisign signature verifies with the public key built into HakuCX and whose signed comment names that version and file (no downgrades to old signed installers). It downloads only from GitHub. Crash reports stay on your computer.
- **Default browser.** Links from other programs start HakuCX with `-- "%1"`, so a link can never be read as a command-line option. A second launch hands its addresses to the running HakuCX through a loopback connection that needs a random token from the profile folder, so other users' programs cannot open tabs in your browser.
- **My Flow** (phone pairing) runs a small HTTP server on your local network only while it is on and a phone is paired or a QR code is showing. Everything is end-to-end encrypted with a key the QR code hands over once; the page itself is plain HTTP, so pair on a network you trust. Details and limits: `docs/MY-FLOW.md`.
- **Video pop out and media.** The pop-out window is a separate webview on an internal page with its own strict policy; it plays the video's web address and talks to the browser only through four pop-out messages. Audio and video are decoded by GStreamer and FFmpeg code (C), which, like the rest of Servo's C/C++ dependencies, runs without an OS sandbox.
- **Clipboard.** Web pages cannot read or write the clipboard through the asynchronous Clipboard API (Servo keeps it off and HakuCX does not turn it on).
  - Ordinary copy and paste in a page goes through HakuCX's clipboard delegate (`browser/paste_protect.rs`). The browser's own Copy buttons also write the clipboard from the core.
  - **Paste Protect.** When you copy what looks like an IBAN, card number or crypto address from a page, Paste Protect watches it for two minutes and warns if what you paste has changed. Only that value is kept in memory, and it is never written to disk.
- **Smart home.** It connects only to the MQTT broker you enter. It uses plain MQTT, so use a broker on your own network; TLS brokers are refused rather than connected to insecurely. It publishes and never subscribes, so nothing on the broker can control the browser.
- **No telemetry.** HakuCX does not phone home. Network requests the browser makes on its own behalf (filter list updates, optional feeds, weather) are listed in `docs/NETWORK.md`.
- **Secrets.** Settings files never contain secrets. Saved passwords, cards and addresses live in an encrypted vault (XChaCha20-Poly1305; the key wrapped by Windows DPAPI and optionally by an Argon2id recovery password). Windows Hello is required before passwords are shown, exported or edited and before cards are filled. HakuCX never fills a password without you picking it. The AI provider key is kept in the Windows Credential Manager. Folder sync encrypts everything it writes with a key derived from your sync passphrase. Details: `docs/PASSWORDS-AND-SYNC.md`.
- **Anything running as your Windows user can unwrap DPAPI-protected keys** (that is how DPAPI works). Malware on your account can therefore read your vault, as with every browser's password store. A recovery password does not change this; it only adds a second way to open the vault.

## Supported versions

Only the latest release and `main` receive security fixes during early development.

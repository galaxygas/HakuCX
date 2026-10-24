# Passwords, autofill and sync

## The vault

HakuCX keeps saved passwords, payment cards and addresses in one encrypted file per profile, `vault.json` (`crates/haku-core/src/vault.rs`).

- **Encryption.** The contents are encrypted with XChaCha20-Poly1305 under a random 256-bit data key; a fresh random nonce is used on every save, and any change to the file is detected.
- **The data key** is never stored in the clear. It is wrapped:
  - by Windows DPAPI for the current Windows user, with an application-specific entropy value, so the vault opens without a prompt when you use HakuCX;
  - optionally, by a **recovery password** you choose (Argon2id, 64 MiB, 3 passes). It opens the vault if Windows can no longer unwrap the key, for example after a Windows reinstall or when the profile was copied to another PC. HakuCX cannot recover a forgotten recovery password.
- **A vault that cannot be opened is never overwritten.** HakuCX shows the reason and leaves the file alone.
- **Several windows of one profile** (each is its own process) merge each other's changes before saving: item by item, the most recent change wins, and deletions are kept as tombstones.

### Windows Hello

Windows Hello confirms it is you, and stays valid for one minute, before HakuCX:
- shows or copies a saved password or card number;
- edits a saved password;
- exports passwords;
- changes the recovery password;
- fills a payment card (unless you turn that off under Payment methods).

If Windows Hello is not set up, HakuCX says so on the Passwords page and shows passwords without a prompt. Set up a PIN, fingerprint or face in Windows Settings → Accounts → Sign-in options.

### Saving and filling

- **Saving.** A small page script (`browser/autofill.js`, top frame only) reports a submitted login form. HakuCX offers to save or update it in a prompt under the toolbar, where you can correct the username or password. It never saves without asking. **Never for this site** stops the offers, and you can undo that on the Passwords page.
- **Filling is always your action.** HakuCX never fills a password into a page by itself. When you click into a sign-in, address or card field, HakuCX shows what it has saved for that site in its own UI, under the field. When saved logins exist for the site, a key appears in the address bar. Only when you pick an entry does HakuCX fill it, and only into the page that is still at the same site.
- **Which logins a site gets.** A login is offered on its own site: the same host with or without `www.`. A login saved on `http` is also offered on the `https` version, never the reverse.
- **Payment cards** are offered only on `https` pages (and on this computer: `localhost`, `127.0.0.1`). The security code is never stored.
- **Import and export.**
  - **Import** reads the CSV exports of Chrome, Edge, Opera, Brave, Firefox and most password managers.
  - **Export** writes Chrome's format (`name,url,username,password,note`). The file is not encrypted; HakuCX warns you to delete it.
- **Private windows** fill from the profile's vault but never save to it.

## Folder sync

HakuCX has no sync servers. Devices share a folder that something else already syncs (OneDrive, Dropbox, Google Drive, Syncthing, a network drive). Each device writes one encrypted file there and merges the others' (`crates/haku-core/src/sync.rs`, `crates/hakucx/src/browser/sync.rs`):

```text
<your folder>/HakuCX Sync/sync.json         Argon2id salt and a check value for the passphrase
<your folder>/HakuCX Sync/<device>.hxsync   one device's data, encrypted with the passphrase key
```

- **The passphrase.** You choose a sync passphrase (10+ characters) and enter it once on each device. The key derived from it is stored on each device wrapped by DPAPI. Nothing in the folder can be read without the passphrase, and nobody can recover a forgotten one: start over with a new folder.
- **What syncs** (each can be switched off):

| Data | How |
|---|---|
| Bookmarks and Speed Dial | Merged: added, renamed, moved and deleted bookmarks travel. Reordering within a folder does not. The starter Speed Dial each device created is not duplicated. |
| History | The last 30 days of visits, added to each device's history. Deleting history on one device does not delete it on the others. |
| Settings | Appearance, tabs, sidebar, start page, features, sounds, search, languages, shortcuts, player, video and accessibility. The most recent change wins. Downloads, privacy, limits, the cleaner, AI, sync settings and paths on this computer (custom wallpaper files) stay local. |
| Open tabs | Each device's open web tabs, listed under Settings → Synchronization → Other devices; click one to open it. |
| Passwords, cards and addresses | Merged item by item, including deletions. |

- **When it runs.** HakuCX syncs a few seconds after it starts, about 20 seconds after you change something that syncs, every 10 minutes, and when you click **Sync now**. Reading and writing the folder happens off the UI thread, because cloud folders can pause while a file downloads.
- **Stop syncing** forgets the passphrase on this device and removes this device's file from the folder.

## Tests

- `cargo test -p haku-core` covers:
  - Encryption round trips, tampering, the recovery password and bounded Argon2 parameters.
  - Merging (both directions, idempotent, with deletions).
  - Login matching, CSV import/export (including quoting and spreadsheet-formula neutralizing), and card checks.
  - Bookmark and history merging, and sync bundles and passphrases.
- `cargo test -p hakucx` round-trips a key through the real DPAPI.
- `tests/autofill/` has sign-in and checkout pages for trying saving and filling by hand (`python tests/extensions/server.py 8766 tests/autofill`).

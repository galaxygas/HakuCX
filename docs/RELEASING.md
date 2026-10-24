# Releasing HakuCX

## What a release contains

| File | What it is |
|---|---|
| `HakuCX-Setup-<version>-x64.exe` | The installer (NSIS). Installs for the current user only, without administrator rights, into `%LOCALAPPDATA%\Programs\HakuCX`. Adds a Start menu shortcut, an optional desktop shortcut and an entry in Settings → Apps. Registers HakuCX as a browser Windows can make the default. |
| `HakuCX-<version>-x64-portable.zip` | A portable copy. The `portable` file next to `hakucx.exe` keeps all data in a `Data` folder beside it. |
| `*.minisig` | A minisign signature for each file, made with the HakuCX update key. |
| `SHA256SUMS.txt` | Checksums of the installer and the zip. |

Both packages include the Visual C++ runtime DLLs next to `hakucx.exe` (app-local deployment, which Microsoft allows), so HakuCX runs on a fresh Windows without separate installs.

## The update key (once)

The updater only installs an installer whose signature verifies with the public key built into HakuCX. The key's signed comment must also name that exact version and file, so an old signed installer cannot be passed off as an update. Without `packaging/update-key.pub`, a build has no updater and says so in Settings.

1. Create the key on a trusted machine. The password is asked for, or taken from `HAKU_SIGNING_PASSWORD`:

   ```bash
   cargo run -p haku-release -- keygen hakucx-update.secret packaging/update-key.pub
   ```

2. Commit `packaging/update-key.pub`. **Never commit the secret file.** Keep it, and its password, somewhere safe and backed up: losing it means existing installs can no longer update.
3. In the repository's Settings → Secrets and variables → Actions, add:
   - `HAKU_SIGNING_KEY`: the contents of `hakucx-update.secret`;
   - `HAKU_SIGNING_PASSWORD`: its password.

## Making a release

1. Set the version in the workspace `Cargo.toml` (`[workspace.package] version`) and merge it to `main`.
2. Tag the merge commit and push the tag:

   ```bash
   git tag v0.2.0
   ```

   ```bash
   git push origin v0.2.0
   ```

3. The **Release** workflow (`.github/workflows/release.yml`) then:
   - checks the tag matches the version and the update key is committed;
   - runs the tests, builds, and packages;
   - signs the installer and zip, and publishes the GitHub release with generated notes.

   It refuses to publish unsigned packages.

Running the workflow by hand (Actions → Release → Run workflow) builds the packages as a downloadable artifact without publishing anything.

### Building packages locally

You need Visual Studio 2022 with the C++ tools, GStreamer (`packaging/windows/gstreamer.ps1 -Install`; the packager finds it and ships its DLLs and licenses), and NSIS 3 for the installer (`winget install NSIS.NSIS`):

```bash
powershell -ExecutionPolicy Bypass -File packaging/windows/package.ps1
```

`-SkipInstaller` builds only the portable zip; `-SkipBuild` reuses an existing release build. The output goes to `dist/`.

## How installed copies update

- **When it checks.** About a minute after starting, and then daily, installed copies ask GitHub for the latest release (`api.github.com/repos/galaxygas/HakuCX/releases/latest`). The check is off when "Update automatically" is off.
- **Download and verification.** When the release is newer, HakuCX downloads the installer and its `.minisig` from GitHub and verifies both against the built-in key. It then shows **Restart to update**.
- **Installing.** Restarting runs the installer silently (`/S /RELAUNCH`). It waits for HakuCX to close, replaces the files, and starts HakuCX again.
- **Portable copies** are told about new versions but are updated by hand.

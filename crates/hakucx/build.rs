/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Build steps for the browser binary:
//!
//! 1. Build the UI (`ui/`, Vite) into `ui/dist`, which `include_dir!` embeds into the binary.
//!    Set `HAKU_SKIP_UI_BUILD=1` to use an existing `ui/dist` as-is.
//! 2. On Windows, Servo renders through ANGLE (`libEGL.dll` / `libGLESv2.dll`), which the
//!    `mozangle` crate builds into its own OUT_DIR. The DLLs must sit next to `hakucx.exe`,
//!    so copy the most recently built pair into the profile output directory.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;
use std::{env, fs};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    build_ui();
    copy_angle_dlls();
    embed_update_key();
    embed_resources();
}

/// The program icon and version information shown by Explorer, the taskbar and installers.
fn embed_resources() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let ico = out.join("hakucx.ico");
    fs::write(&ico, haku_brand::ico(haku_brand::ICO_SIZES)).expect("write icon");
    let version = env::var("CARGO_PKG_VERSION").expect("version");
    let numbers: Vec<u32> = version
        .split(['.', '-'])
        .take(3)
        .map(|p| p.parse().unwrap_or(0))
        .collect();
    let (major, minor, patch) = (numbers[0], numbers[1], numbers[2]);
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "HakuCX contributors"
      VALUE "FileDescription", "HakuCX"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "hakucx"
      VALUE "LegalCopyright", "Mozilla Public License 2.0"
      VALUE "OriginalFilename", "hakucx.exe"
      VALUE "ProductName", "HakuCX"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = ico.display().to_string().replace('\\', "/"),
    );
    let rc_path = out.join("hakucx.rc");
    fs::write(&rc_path, rc).expect("write resource script");
    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_optional()
        .expect("compile the Windows resources (rc.exe from the Windows SDK)");
}

/// The updater's public key: the key line of `packaging/update-key.pub` (minisign format).
/// Without that file the build has no updater (it says so in Settings).
fn embed_update_key() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let file = manifest.join("../../packaging/update-key.pub");
    println!("cargo::rerun-if-changed={}", file.display());
    let Ok(text) = fs::read_to_string(&file) else {
        return;
    };
    if let Some(key) = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("untrusted comment:"))
    {
        println!("cargo::rustc-env=HAKU_UPDATE_KEY={key}");
    }
}

fn build_ui() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let ui = manifest.join("../../ui");
    for watched in [
        "src",
        "public",
        "index.html",
        "package.json",
        "pnpm-lock.yaml",
        "vite.config.ts",
    ] {
        println!("cargo::rerun-if-changed={}", ui.join(watched).display());
    }
    println!("cargo::rerun-if-env-changed=HAKU_SKIP_UI_BUILD");
    let dist = ui.join("dist").join("index.html");
    if env::var_os("HAKU_SKIP_UI_BUILD").is_some() {
        assert!(
            dist.is_file(),
            "HAKU_SKIP_UI_BUILD is set but ui/dist is missing"
        );
        return;
    }
    let pnpm = |args: &[&str]| {
        // pnpm is a .cmd shim on Windows, which Command cannot spawn directly.
        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.arg("/C").arg("pnpm");
            c
        } else {
            Command::new("pnpm")
        };
        cmd.args(args).current_dir(&ui);
        match cmd.status() {
            Ok(status) if status.success() => {}
            Ok(status) => panic!("`pnpm {}` failed with {status}", args.join(" ")),
            Err(error) => panic!(
                "could not run pnpm ({error}). Install Node.js and pnpm, or set HAKU_SKIP_UI_BUILD=1 with a prebuilt ui/dist"
            ),
        }
    };
    if !ui.join("node_modules").is_dir() {
        pnpm(&["install", "--frozen-lockfile"]);
    }
    pnpm(&["run", "build"]);
}

fn copy_angle_dlls() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    // OUT_DIR = <target>/<profile>/build/hakucx-<hash>/out
    let Some(build_dir) = out_dir.ancestors().nth(2) else {
        return;
    };
    let Some(profile_dir) = build_dir.parent() else {
        return;
    };

    let Some(source) = newest_mozangle_out(build_dir) else {
        println!(
            "cargo::warning=ANGLE DLLs not found yet; run the build again if hakucx.exe fails with 'egl function was not loaded'"
        );
        return;
    };
    for dll in ["libEGL.dll", "libGLESv2.dll"] {
        let from = source.join(dll);
        println!("cargo::rerun-if-changed={}", from.display());
        if let Err(error) = fs::copy(&from, profile_dir.join(dll)) {
            println!("cargo::warning=could not copy {dll}: {error}");
        }
    }
}

fn newest_mozangle_out(build_dir: &Path) -> Option<PathBuf> {
    fs::read_dir(build_dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("mozangle-"))
        .map(|e| e.path().join("out"))
        .filter(|p| p.join("libEGL.dll").is_file() && p.join("libGLESv2.dll").is_file())
        .max_by_key(|p| {
            fs::metadata(p.join("libGLESv2.dll"))
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        })
}

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

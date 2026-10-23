/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! On Windows, Servo renders through ANGLE (`libEGL.dll` / `libGLESv2.dll`), which the
//! `mozangle` crate builds into its own OUT_DIR. The DLLs must sit next to `hakucx.exe`,
//! so copy the most recently built pair into the profile output directory.

use std::path::{Path, PathBuf};
use std::time::SystemTime;
use std::{env, fs};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
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

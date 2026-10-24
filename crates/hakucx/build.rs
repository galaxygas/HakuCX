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
//! 3. With the `media` feature (Windows), Servo loads its GStreamer plugins from the folder of
//!    `hakucx.exe` and exits if one is missing, so put the plugins and every GStreamer DLL they
//!    and the GStreamer libraries need next to it (and next to test binaries in `deps/`), and
//!    list them in `gstreamer-files.txt` for the packager.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;
use std::{env, fs};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    build_ui();
    copy_angle_dlls();
    copy_gstreamer();
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

/// The plugins Servo 0.7 loads on Windows (`servo/gstreamer_plugin_lists/common.rs.in` and
/// `windows.rs.in`).
const GST_PLUGINS: &[&str] = &[
    "gstcoreelements",
    "gstnice",
    "gstapp",
    "gstaudioconvert",
    "gstaudioresample",
    "gstgio",
    "gstogg",
    "gstopengl",
    "gstopus",
    "gstplayback",
    "gsttheora",
    "gsttypefindfunctions",
    "gstvideoconvertscale",
    "gstvolume",
    "gstvorbis",
    "gstaudiofx",
    "gstaudioparsers",
    "gstautodetect",
    "gstdeinterlace",
    "gstid3demux",
    "gstinterleave",
    "gstisomp4",
    "gstmatroska",
    "gstrtp",
    "gstrtpmanager",
    "gstvideofilter",
    "gstvpx",
    "gstwavparse",
    "gstaudiobuffersplit",
    "gstdtls",
    "gstid3tag",
    "gstproxy",
    "gstvideoparsersbad",
    "gstwebrtc",
    "gstlibav",
    "gstwasapi",
];

/// GStreamer's MSVC install: `GSTREAMER_1_0_ROOT_MSVC_X86_64`, else the installer's default
/// folders (per user, all users, and the pre-1.26 `C:\gstreamer`).
fn gstreamer_root() -> Option<PathBuf> {
    println!("cargo::rerun-if-env-changed=GSTREAMER_1_0_ROOT_MSVC_X86_64");
    let mut candidates = Vec::new();
    if let Some(root) = env::var_os("GSTREAMER_1_0_ROOT_MSVC_X86_64") {
        candidates.push(PathBuf::from(root));
    }
    if let Some(local) = env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(local).join(r"Programs\gstreamer\1.0\msvc_x86_64"));
    }
    if let Some(programs) = env::var_os("ProgramFiles") {
        candidates.push(PathBuf::from(programs).join(r"gstreamer\1.0\msvc_x86_64"));
    }
    candidates.push(PathBuf::from(r"C:\gstreamer\1.0\msvc_x86_64"));
    candidates
        .into_iter()
        .find(|root| root.join(r"bin\gstreamer-1.0-0.dll").is_file())
}

fn copy_gstreamer() {
    if env::var_os("CARGO_FEATURE_MEDIA").is_none()
        || env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
    {
        return;
    }
    let Some(root) = gstreamer_root() else {
        println!(
            "cargo::warning=GStreamer (MSVC x86_64) not found; hakucx.exe will not start without its DLLs. See docs/MEDIA.md."
        );
        return;
    };
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    // OUT_DIR = <target>/<profile>/build/hakucx-<hash>/out
    let Some(profile_dir) = out_dir.ancestors().nth(3) else {
        return;
    };
    let bin = root.join("bin");
    let plugin_dir = root.join(r"lib\gstreamer-1.0");

    // Start from the plugins and every GStreamer library (the bindings link several), then
    // follow their imports to the DLLs they need from GStreamer's bin folder.
    let mut files: Vec<PathBuf> = GST_PLUGINS
        .iter()
        .map(|p| plugin_dir.join(format!("{p}.dll")))
        .collect();
    if let Ok(entries) = fs::read_dir(&bin) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.starts_with("gst") && name.ends_with("-1.0-0.dll") {
                files.push(entry.path());
            }
        }
    }
    let mut seen: BTreeSet<String> = files
        .iter()
        .filter_map(|f| f.file_name())
        .map(|n| n.to_string_lossy().to_lowercase())
        .collect();
    let mut queue = files.clone();
    while let Some(file) = queue.pop() {
        let Ok(data) = fs::read(&file) else {
            println!("cargo::warning=missing GStreamer file {}", file.display());
            continue;
        };
        for dll in pe_imports(&data).unwrap_or_default() {
            if seen.insert(dll.to_lowercase()) {
                let path = bin.join(&dll);
                if path.is_file() {
                    files.push(path.clone());
                    queue.push(path);
                }
            }
        }
    }

    for dir in [profile_dir.to_path_buf(), profile_dir.join("deps")] {
        let _ = fs::create_dir_all(&dir);
        for file in &files {
            if let Some(name) = file.file_name() {
                link_or_copy(file, &dir.join(name));
            }
        }
    }
    let listed: String = files
        .iter()
        .filter_map(|f| f.file_name())
        .map(|n| format!("{}\n", n.to_string_lossy()))
        .collect();
    let _ = fs::write(profile_dir.join("gstreamer-files.txt"), listed);
    println!(
        "cargo::rerun-if-changed={}",
        bin.join("gstreamer-1.0-0.dll").display()
    );
}

/// Hard-link (same drive) or copy `from` to `to`, unless `to` already has the same size and is
/// not older.
fn link_or_copy(from: &Path, to: &Path) {
    let Ok(source) = fs::metadata(from) else {
        return;
    };
    if let Ok(target) = fs::metadata(to)
        && source.len() == target.len()
        && target.modified().ok() >= source.modified().ok()
    {
        return;
    }
    let _ = fs::remove_file(to);
    if fs::hard_link(from, to).is_err()
        && let Err(error) = fs::copy(from, to)
    {
        println!("cargo::warning=could not copy {}: {error}", from.display());
    }
}

/// The DLLs a PE file imports (normal and delay-loaded imports).
fn pe_imports(d: &[u8]) -> Option<Vec<String>> {
    let u16_at = |o: usize| d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |o: usize| {
        d.get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let pe = u32_at(0x3c)? as usize;
    if d.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let coff = pe + 4;
    let sections = u16_at(coff + 2)? as usize;
    let optional_size = u16_at(coff + 16)? as usize;
    let optional = coff + 20;
    let directories = match u16_at(optional)? {
        0x20b => optional + 112,
        0x10b => optional + 96,
        _ => return None,
    };
    let section_table = optional + optional_size;
    let offset = |rva: u32| -> Option<usize> {
        (0..sections).find_map(|i| {
            let s = section_table + i * 40;
            let (size, va) = (u32_at(s + 8)?, u32_at(s + 12)?);
            let (raw_size, raw) = (u32_at(s + 16)?, u32_at(s + 20)?);
            (rva >= va && rva < va + size.max(raw_size)).then(|| (rva - va + raw) as usize)
        })
    };
    let name_at = |rva: u32| -> Option<String> {
        let start = offset(rva)?;
        let len = d.get(start..)?.iter().position(|b| *b == 0)?;
        Some(String::from_utf8_lossy(&d[start..start + len]).into_owned())
    };
    let mut names = Vec::new();
    // (data directory, descriptor size, where the DLL name's RVA sits in a descriptor)
    for (index, size, name_field) in [(1usize, 20usize, 12usize), (13, 32, 4)] {
        let rva = u32_at(directories + index * 8)?;
        if rva == 0 {
            continue;
        }
        let Some(mut at) = offset(rva) else {
            continue;
        };
        loop {
            let name_rva = u32_at(at + name_field)?;
            if name_rva == 0 {
                break;
            }
            if let Some(name) = name_at(name_rva) {
                names.push(name);
            }
            at += size;
        }
    }
    Some(names)
}

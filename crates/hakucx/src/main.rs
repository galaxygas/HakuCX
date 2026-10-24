/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! HakuCX: a GX-style browser built on Rust and the Servo web engine.

// Release builds are GUI apps: no console window on Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod adblock;
mod app;
mod audio;
mod browser;
mod compositor;
mod crash;
mod downloads;
mod extensions;
mod favicon;
mod instance;
mod keyutils;
mod lights;
mod limits;
mod os_integration;
mod os_secrets;
mod protocol;
mod texture_context;

use browser::LaunchOptions;
use winit::event_loop::EventLoop;

const HELP: &str = "HakuCX - a GX-style browser built on Servo

Usage: hakucx [options] [URL...]

Options:
  --private            open a private window (nothing is saved)
  --profile <id>       use the given profile
  --data-dir <path>    store profiles in <path> instead of the user data folder
  --load-extension <folder>
                       run an unpacked extension for this session (development)
  --help               show this help
";

fn parse_args() -> Result<LaunchOptions, String> {
    let mut options = LaunchOptions::default();
    let mut args = std::env::args().skip(1);
    let mut only_urls = false;
    while let Some(arg) = args.next() {
        if only_urls {
            options.urls.push(arg);
            continue;
        }
        match arg.as_str() {
            "--" => only_urls = true,
            "--private" => options.private = true,
            "--secondary-window" => options.secondary = true,
            "--profile" => options.profile = Some(args.next().ok_or("--profile needs an id")?),
            "--load-extension" => options
                .load_extensions
                .push(args.next().ok_or("--load-extension needs a folder")?.into()),
            "--data-dir" => {
                let dir = args.next().ok_or("--data-dir needs a path")?;
                // SAFETY: set before any thread is spawned.
                unsafe { std::env::set_var("HAKU_DATA_DIR", dir) };
            }
            "--help" | "-h" => return Err(HELP.to_owned()),
            other if other.starts_with("--") => {
                return Err(format!("unknown option {other}\n\n{HELP}"));
            }
            _ => options.urls.push(arg),
        }
    }
    // Windows passes files ("Open with HakuCX") as paths: open them as file:// addresses.
    for url in options.urls.iter_mut() {
        let path = std::path::Path::new(url.as_str());
        if path.is_absolute()
            && path.exists()
            && let Ok(file) = url::Url::from_file_path(path)
        {
            *url = file.to_string();
        }
    }
    Ok(options)
}

fn main() {
    // Servo multi-process mode re-runs this executable as a content process.
    let mut raw = std::env::args().skip(1);
    if raw.next().as_deref() == Some("--content-process") {
        if let Some(token) = raw.next() {
            servo::run_content_process(token);
        }
        return;
    }

    let options = match parse_args() {
        Ok(o) => o,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(if message.starts_with("HakuCX") { 0 } else { 2 });
        }
    };

    // A portable copy keeps its data next to itself.
    if std::env::var_os("HAKU_DATA_DIR").is_none()
        && let Some(dir) = os_integration::portable_data_dir()
    {
        // SAFETY: set before any thread is spawned.
        unsafe { std::env::set_var("HAKU_DATA_DIR", dir) };
    }
    if let Ok(root) = haku_core::storage::data_root() {
        crash::install_hook(&root);
    }

    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let startup = app::prepare(options);
    // One process per profile: if this profile is already open, hand it our addresses.
    let instance_dir = startup
        .paths
        .as_ref()
        .filter(|_| !startup.options.private && !startup.options.secondary)
        .map(|p| p.root.clone());
    if let Some(dir) = instance_dir.as_ref()
        && instance::forward(dir, &startup.options.urls)
    {
        return;
    }
    let event_loop = EventLoop::<app::AppEvent>::with_user_event()
        .build()
        .expect("create event loop");
    let waker = app::Waker(event_loop.create_proxy());
    let mut application = app::App::new(waker, startup, instance_dir.clone());
    let result = event_loop.run_app(&mut application);
    if let Some(dir) = instance_dir.as_ref() {
        instance::release(dir);
    }
    if let Err(e) = result {
        eprintln!("HakuCX stopped with an error: {e}");
        std::process::exit(1);
    }
}

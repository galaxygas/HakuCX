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
mod downloads;
mod extensions;
mod favicon;
mod keyutils;
mod limits;
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

    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("install rustls crypto provider");

    let startup = app::prepare(options);
    let event_loop = EventLoop::<app::AppEvent>::with_user_event()
        .build()
        .expect("create event loop");
    let waker = app::Waker(event_loop.create_proxy());
    let mut application = app::App::new(waker, startup);
    if let Err(e) = event_loop.run_app(&mut application) {
        eprintln!("HakuCX stopped with an error: {e}");
        std::process::exit(1);
    }
}

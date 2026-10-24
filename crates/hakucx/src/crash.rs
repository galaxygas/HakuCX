/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Crash reports, kept on this computer only. A panic writes a text report (version, message,
//! where, backtrace) to `<data folder>/Crash Reports`; the next start tells the user and
//! offers to open the folder. HakuCX never uploads them.

use std::path::{Path, PathBuf};

pub const DIR_NAME: &str = "Crash Reports";
const SEEN: &str = ".seen";
const KEEP: usize = 20;

pub fn dir(data_root: &Path) -> PathBuf {
    data_root.join(DIR_NAME)
}

/// Write a report for any panic, then let the default handler run (it prints to stderr).
pub fn install_hook(data_root: &Path) {
    let dir = dir(data_root);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let thread = std::thread::current();
        let report = format!(
            "HakuCX {} crashed\nOS: {} {}\nTime (ms since 1970): {}\nThread: {}\n\n{info}\n\nBacktrace:\n{backtrace}\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            haku_core::now_ms(),
            thread.name().unwrap_or("unnamed"),
        );
        if std::fs::create_dir_all(&dir).is_ok() {
            let _ = std::fs::write(
                dir.join(format!("crash-{}.txt", haku_core::now_ms())),
                report,
            );
            prune(&dir);
        }
        previous(info);
    }));
}

fn reports(dir: &Path) -> Vec<PathBuf> {
    let mut list: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("crash-"))
                })
                .collect()
        })
        .unwrap_or_default();
    list.sort();
    list
}

fn prune(dir: &Path) {
    let list = reports(dir);
    for old in list.iter().take(list.len().saturating_sub(KEEP)) {
        let _ = std::fs::remove_file(old);
    }
}

/// Reports written since the user was last told about crashes (and mark them as told).
pub fn take_new_reports(data_root: &Path) -> usize {
    let dir = dir(data_root);
    let seen = std::fs::read_to_string(dir.join(SEEN)).unwrap_or_default();
    let list = reports(&dir);
    let new = list
        .iter()
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().as_ref() > seen.as_str())
        })
        .count();
    if let Some(last) = list.last().and_then(|p| p.file_name()) {
        let _ = std::fs::write(dir.join(SEEN), last.to_string_lossy().as_bytes());
    }
    new
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_reports_are_counted_once_and_old_ones_pruned() {
        let root = tempfile::tempdir().unwrap();
        let d = dir(root.path());
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(take_new_reports(root.path()), 0);
        for i in 0..25 {
            std::fs::write(d.join(format!("crash-{:013}.txt", 1_000 + i)), "x").unwrap();
        }
        prune(&d);
        assert_eq!(reports(&d).len(), KEEP);
        assert_eq!(take_new_reports(root.path()), KEEP);
        assert_eq!(take_new_reports(root.path()), 0);
        std::fs::write(d.join(format!("crash-{:013}.txt", 5_000)), "x").unwrap();
        assert_eq!(take_new_reports(root.path()), 1);
    }
}

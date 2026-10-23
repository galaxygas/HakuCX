/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Smaller persisted records: downloads list, pinboards, web-modding styles, user themes.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

// ---------------------------------------------------------------------------------------------
// Downloads
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case", tag = "state", content = "reason")]
#[ts(export)]
pub enum DownloadState {
    InProgress,
    Paused,
    Completed,
    Cancelled,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DownloadRecord {
    pub id: String,
    pub url: String,
    pub file_name: String,
    /// Absolute path of the file (final path once complete, `.part` path while running).
    pub path: String,
    pub mime: String,
    /// `None` when the server did not say.
    pub total_bytes: Option<u64>,
    pub received_bytes: u64,
    pub state: DownloadState,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub referrer: Option<String>,
}

/// Opera GX's download page filters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum FileCategory {
    Documents,
    Images,
    Videos,
    Music,
    Archives,
    Programs,
    Others,
}

impl DownloadRecord {
    pub fn new(url: &str, file_name: &str, path: &str, mime: &str, total: Option<u64>) -> Self {
        Self {
            id: new_id(),
            url: url.to_owned(),
            file_name: file_name.to_owned(),
            path: path.to_owned(),
            mime: mime.to_owned(),
            total_bytes: total,
            received_bytes: 0,
            state: DownloadState::InProgress,
            started_at: crate::now_ms(),
            ended_at: None,
            referrer: None,
        }
    }

    pub fn category(&self) -> FileCategory {
        let ext = std::path::Path::new(&self.file_name)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let mime = self.mime.as_str();
        match ext.as_str() {
            "pdf" | "doc" | "docx" | "odt" | "rtf" | "txt" | "md" | "xls" | "xlsx" | "ods"
            | "csv" | "ppt" | "pptx" | "odp" | "epub" => FileCategory::Documents,
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "bmp" | "svg" | "ico" | "tif"
            | "tiff" | "heic" => FileCategory::Images,
            "mp4" | "mkv" | "webm" | "mov" | "avi" | "m4v" | "wmv" | "flv" => FileCategory::Videos,
            "mp3" | "wav" | "flac" | "ogg" | "opus" | "m4a" | "aac" | "wma" => FileCategory::Music,
            "zip" | "7z" | "rar" | "tar" | "gz" | "xz" | "bz2" | "zst" => FileCategory::Archives,
            "exe" | "msi" | "msix" | "appx" | "bat" | "cmd" | "ps1" | "jar" => {
                FileCategory::Programs
            }
            _ if mime.starts_with("image/") => FileCategory::Images,
            _ if mime.starts_with("video/") => FileCategory::Videos,
            _ if mime.starts_with("audio/") => FileCategory::Music,
            _ if mime.starts_with("text/") => FileCategory::Documents,
            _ => FileCategory::Others,
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self.state,
            DownloadState::InProgress | DownloadState::Paused
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct DownloadList {
    pub items: Vec<DownloadRecord>,
}

const MAX_DOWNLOAD_RECORDS: usize = 500;

impl DownloadList {
    pub fn push(&mut self, record: DownloadRecord) {
        self.items.insert(0, record);
        // Keep running ones; drop the oldest finished ones beyond the cap.
        while self.items.len() > MAX_DOWNLOAD_RECORDS {
            match self.items.iter().rposition(|d| !d.is_active()) {
                Some(i) => {
                    self.items.remove(i);
                }
                None => break,
            }
        }
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut DownloadRecord> {
        self.items.iter_mut().find(|d| d.id == id)
    }

    /// Remove finished records (optionally only those older than `before`).
    pub fn clear_finished(&mut self, before: Option<i64>) -> Vec<DownloadRecord> {
        let (gone, keep): (Vec<_>, Vec<_>) = self.items.drain(..).partition(|d| {
            !d.is_active() && before.is_none_or(|b| d.ended_at.unwrap_or(d.started_at) < b)
        });
        self.items = keep;
        gone
    }

    /// After a crash or quit, anything still "in progress" is paused (it can be resumed).
    pub fn settle_after_restart(&mut self) {
        for d in &mut self.items {
            if d.state == DownloadState::InProgress {
                d.state = DownloadState::Paused;
            }
        }
    }
}

/// A file name that is safe on Windows: no reserved characters or device names, no trailing
/// dots/spaces, at most 200 characters, never empty.
pub fn sanitize_file_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    out = out.trim().trim_end_matches(['.', ' ']).to_owned();
    if out.chars().count() > 200 {
        let ext = std::path::Path::new(&out)
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .filter(|e| e.len() <= 16)
            .unwrap_or_default();
        let stem: String = out.chars().take(200 - ext.chars().count()).collect();
        out = format!("{stem}{ext}");
    }
    let stem = out
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem.as_str()) {
        out = format!("_{out}");
    }
    if out.is_empty() {
        "download".into()
    } else {
        out
    }
}

/// `name.ext`, `name (1).ext`, `name (2).ext`, ... the first that `exists` says is free.
pub fn unique_file_name(
    dir: &std::path::Path,
    name: &str,
    exists: impl Fn(&std::path::Path) -> bool,
) -> std::path::PathBuf {
    let candidate = dir.join(name);
    if !exists(&candidate) {
        return candidate;
    }
    let path = std::path::Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for n in 1..10_000 {
        let candidate = dir.join(format!("{stem} ({n}){ext}"));
        if !exists(&candidate) {
            return candidate;
        }
    }
    dir.join(format!("{stem} ({}){ext}", crate::now_ms()))
}

// ---------------------------------------------------------------------------------------------
// Pinboards
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum PinKind {
    Link,
    Note,
    Image,
    Todo,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct PinItem {
    pub id: String,
    pub kind: PinKind,
    pub title: String,
    pub url: Option<String>,
    pub text: String,
    pub color: Option<String>,
    pub done: bool,
    pub created: i64,
}

impl Default for PinItem {
    fn default() -> Self {
        Self {
            id: new_id(),
            kind: PinKind::Note,
            title: String::new(),
            url: None,
            text: String::new(),
            color: None,
            done: false,
            created: crate::now_ms(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Pinboard {
    pub id: String,
    pub name: String,
    pub emoji: Option<String>,
    pub color: String,
    pub items: Vec<PinItem>,
    pub created: i64,
}

impl Default for Pinboard {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: "My board".into(),
            emoji: None,
            color: "#7d46f2".into(),
            items: Vec::new(),
            created: crate::now_ms(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Pinboards {
    pub boards: Vec<Pinboard>,
}

// ---------------------------------------------------------------------------------------------
// Web modding (per-domain CSS) and user themes
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct SiteMod {
    pub id: String,
    /// Host the CSS applies to; subdomains match too (`youtube.com` covers `www.youtube.com`).
    pub domain: String,
    pub css: String,
    pub enabled: bool,
}

impl Default for SiteMod {
    fn default() -> Self {
        Self {
            id: new_id(),
            domain: String::new(),
            css: String::new(),
            enabled: true,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct SiteMods {
    pub mods: Vec<SiteMod>,
}

impl SiteMods {
    /// CSS for a host: every enabled mod whose domain equals the host or is a parent domain.
    pub fn css_for_host(&self, host: &str) -> String {
        let host = host.to_ascii_lowercase();
        self.mods
            .iter()
            .filter(|m| m.enabled)
            .filter(|m| {
                let d = m
                    .domain
                    .trim()
                    .trim_start_matches("*.")
                    .to_ascii_lowercase();
                !d.is_empty() && (host == d || host.ends_with(&format!(".{d}")))
            })
            .map(|m| m.css.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct UserThemes {
    pub themes: Vec<crate::themes::UserTheme>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories() {
        let d = |n: &str, m: &str| DownloadRecord::new("https://x/", n, n, m, None).category();
        assert_eq!(d("a.PDF", ""), FileCategory::Documents);
        assert_eq!(d("clip", "video/mp4"), FileCategory::Videos);
        assert_eq!(d("setup.exe", ""), FileCategory::Programs);
        assert_eq!(d("x.bin", "application/octet-stream"), FileCategory::Others);
    }

    #[test]
    fn sanitize_names() {
        assert_eq!(sanitize_file_name("a/b:c?.txt"), "a_b_c_.txt");
        assert_eq!(sanitize_file_name("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_file_name(" ..."), "download");
        assert_eq!(sanitize_file_name("name. "), "name");
        let long = format!("{}.zip", "x".repeat(300));
        let s = sanitize_file_name(&long);
        assert!(s.ends_with(".zip") && s.chars().count() == 200);
    }

    #[test]
    fn unique_names() {
        let taken = ["/d/a.txt", "/d/a (1).txt"];
        let p = unique_file_name(std::path::Path::new("/d"), "a.txt", |p| {
            taken.iter().any(|t| std::path::Path::new(t) == p)
        });
        assert_eq!(p, std::path::Path::new("/d/a (2).txt"));
    }

    #[test]
    fn download_list_cap_keeps_running() {
        let mut l = DownloadList::default();
        for i in 0..(MAX_DOWNLOAD_RECORDS + 5) {
            let mut r = DownloadRecord::new("https://x/", &format!("{i}"), "", "", None);
            r.state = if i == 0 {
                DownloadState::InProgress
            } else {
                DownloadState::Completed
            };
            l.push(r);
        }
        assert_eq!(l.items.len(), MAX_DOWNLOAD_RECORDS);
        assert!(l.items.iter().any(|d| d.file_name == "0"));
        l.settle_after_restart();
        assert!(l.items.iter().all(|d| d.state != DownloadState::InProgress));
        let gone = l.clear_finished(None);
        assert_eq!(gone.len(), MAX_DOWNLOAD_RECORDS - 1);
        assert_eq!(l.items.len(), 1);
    }

    #[test]
    fn site_mods_match_subdomains_only() {
        let mods = SiteMods {
            mods: vec![
                SiteMod {
                    domain: "youtube.com".into(),
                    css: "a{}".into(),
                    ..Default::default()
                },
                SiteMod {
                    domain: "tube.com".into(),
                    css: "b{}".into(),
                    ..Default::default()
                },
                SiteMod {
                    domain: "youtube.com".into(),
                    css: "c{}".into(),
                    enabled: false,
                    ..Default::default()
                },
            ],
        };
        assert_eq!(mods.css_for_host("www.youtube.com"), "a{}");
        assert_eq!(mods.css_for_host("tube.com"), "b{}");
        assert_eq!(mods.css_for_host("evil-youtube.com"), "");
    }

    #[test]
    fn download_state_serializes_tagged() {
        let s = serde_json::to_string(&DownloadState::Failed("disk full".into())).unwrap();
        assert_eq!(s, r#"{"state":"failed","reason":"disk full"}"#);
    }
}

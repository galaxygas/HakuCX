/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Profile directories and crash-safe JSON persistence.
//!
//! Layout (Windows: `%APPDATA%\HakuCX`):
//!
//! ```text
//! HakuCX/
//!   profiles.json            list of profiles + last used
//!   Profiles/<id>/
//!     engine/                Servo's own storage (cookies, local storage, HSTS, cache)
//!     settings.json
//!     session.json
//!     bookmarks.json         bookmarks bar, Speed Dial, other bookmarks, trash
//!     history.sqlite
//!     downloads.json
//!     pinboards.json
//!     themes.json            user-made themes
//!     site-mods.json         web modding CSS per domain
//!     wallpapers/  mods/  extensions/  filters/
//! ```

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const APP_DIR_NAME: &str = "HakuCX";

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("I/O error on {path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("could not encode {path}: {source}")]
    Encode {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("no per-user data directory is available on this system")]
    NoDataDir,
}

/// Root data directory. `HAKU_DATA_DIR` overrides it (portable installs, tests).
pub fn data_root() -> Result<PathBuf, StorageError> {
    if let Some(dir) = std::env::var_os("HAKU_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    dirs::data_dir()
        .map(|d| d.join(APP_DIR_NAME))
        .ok_or(StorageError::NoDataDir)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ProfileKind {
    /// Everything persists.
    Standard,
    /// Mutes tabs on start and can turn off GPU acceleration; for streaming setups.
    Streaming,
    /// Nothing is saved between launches (temporary engine storage, no history).
    Roguelike,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProfileInfo {
    pub id: String,
    pub name: String,
    pub color: String,
    pub kind: ProfileKind,
    pub created: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct ProfileList {
    pub profiles: Vec<ProfileInfo>,
    pub last_used: Option<String>,
}

impl ProfileList {
    pub fn load(root: &Path) -> ProfileList {
        let mut list: ProfileList = read_json_or_default(&root.join("profiles.json"));
        if list.profiles.is_empty() {
            list.profiles.push(ProfileInfo {
                id: "default".into(),
                name: "Player".into(),
                color: "#7d46f2".into(),
                kind: ProfileKind::Standard,
                created: crate::now_ms(),
            });
        }
        list
    }

    pub fn save(&self, root: &Path) -> Result<(), StorageError> {
        write_json_atomic(&root.join("profiles.json"), self)
    }

    pub fn get(&self, id: &str) -> Option<&ProfileInfo> {
        self.profiles.iter().find(|p| p.id == id)
    }

    /// Add a profile with a filesystem-safe id derived from a UUID.
    pub fn add(&mut self, name: &str, color: &str, kind: ProfileKind) -> ProfileInfo {
        let info = ProfileInfo {
            id: uuid::Uuid::new_v4().simple().to_string()[..12].to_owned(),
            name: name.trim().chars().take(40).collect(),
            color: color.to_owned(),
            kind,
            created: crate::now_ms(),
        };
        self.profiles.push(info.clone());
        info
    }
}

/// All paths of one profile. Creating it makes sure the directories exist.
#[derive(Clone, Debug)]
pub struct ProfilePaths {
    pub root: PathBuf,
}

impl ProfilePaths {
    pub fn new(data_root: &Path, profile_id: &str) -> Result<Self, StorageError> {
        // Profile ids come from our own list, but never let one escape the profiles folder.
        let safe: String = profile_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        let safe = if safe.is_empty() {
            "default".to_owned()
        } else {
            safe
        };
        let paths = Self {
            root: data_root.join("Profiles").join(safe),
        };
        for dir in [
            paths.root.clone(),
            paths.engine(),
            paths.wallpapers(),
            paths.mods(),
            paths.extensions(),
            paths.filters(),
        ] {
            fs::create_dir_all(&dir).map_err(|source| StorageError::Io { path: dir, source })?;
        }
        Ok(paths)
    }

    pub fn engine(&self) -> PathBuf {
        self.root.join("engine")
    }
    pub fn settings(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    pub fn session(&self) -> PathBuf {
        self.root.join("session.json")
    }
    pub fn bookmarks(&self) -> PathBuf {
        self.root.join("bookmarks.json")
    }
    pub fn history(&self) -> PathBuf {
        self.root.join("history.sqlite")
    }
    pub fn downloads(&self) -> PathBuf {
        self.root.join("downloads.json")
    }
    pub fn pinboards(&self) -> PathBuf {
        self.root.join("pinboards.json")
    }
    pub fn user_themes(&self) -> PathBuf {
        self.root.join("themes.json")
    }
    pub fn site_mods(&self) -> PathBuf {
        self.root.join("site-mods.json")
    }
    pub fn wallpapers(&self) -> PathBuf {
        self.root.join("wallpapers")
    }
    pub fn mods(&self) -> PathBuf {
        self.root.join("mods")
    }
    pub fn extensions(&self) -> PathBuf {
        self.root.join("extensions")
    }
    /// The installed-extension list.
    pub fn extension_list(&self) -> PathBuf {
        self.root.join("extensions.json")
    }
    /// `chrome.storage.local` / `sync` data, one file per extension.
    pub fn extension_storage(&self) -> PathBuf {
        self.root.join("extension-storage")
    }
    pub fn filters(&self) -> PathBuf {
        self.root.join("filters")
    }
}

/// Write JSON to a temporary file, flush it to disk, then rename it over the target, so a
/// crash mid-write can never leave a truncated file behind.
pub fn write_json_atomic<T: Serialize + ?Sized>(
    path: &Path,
    value: &T,
) -> Result<(), StorageError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|source| StorageError::Encode {
        path: path.to_owned(),
        source,
    })?;
    write_bytes_atomic(path, &bytes)
}

pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    let io_err = |source| StorageError::Io {
        path: path.to_owned(),
        source,
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_err)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp).map_err(io_err)?;
        file.write_all(bytes).map_err(io_err)?;
        file.sync_all().map_err(io_err)?;
    }
    // `fs::rename` replaces an existing file on Windows (MoveFileExW REPLACE_EXISTING).
    fs::rename(&tmp, path).map_err(io_err)
}

/// Read a JSON file. Missing file -> default. Unreadable or corrupt file -> the file is kept
/// as `<name>.corrupt-<timestamp>` for recovery, and the default is returned.
pub fn read_json_or_default<T: DeserializeOwned + Default>(path: &Path) -> T {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return T::default(),
        Err(error) => {
            log::error!("could not read {}: {error}", path.display());
            return T::default();
        }
    };
    match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(error) => {
            let backup = path.with_extension(format!("corrupt-{}", crate::now_ms()));
            log::error!(
                "{} is not valid ({error}); keeping it as {}",
                path.display(),
                backup.display()
            );
            let _ = fs::rename(path, &backup);
            T::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("value.json");
        write_json_atomic(&path, &vec![1, 2, 3]).unwrap();
        write_json_atomic(&path, &vec![4]).unwrap();
        let back: Vec<i32> = read_json_or_default(&path);
        assert_eq!(back, vec![4]);
        assert!(!path.with_extension("tmp").exists());
    }

    #[test]
    fn corrupt_file_is_preserved_and_default_returned() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{ not json").unwrap();
        let value: Vec<i32> = read_json_or_default(&path);
        assert!(value.is_empty());
        assert!(!path.exists());
        let kept = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains("corrupt"));
        assert!(kept);
    }

    #[test]
    fn profile_ids_cannot_escape() {
        let dir = tempfile::tempdir().unwrap();
        let paths = ProfilePaths::new(dir.path(), "../../evil").unwrap();
        assert!(paths.root.starts_with(dir.path().join("Profiles")));
        assert!(paths.engine().is_dir());
    }

    #[test]
    fn default_profile_list() {
        let dir = tempfile::tempdir().unwrap();
        let mut list = ProfileList::load(dir.path());
        assert_eq!(list.profiles.len(), 1);
        let p = list.add("Stream setup", "#ff0077", ProfileKind::Streaming);
        list.save(dir.path()).unwrap();
        let again = ProfileList::load(dir.path());
        assert!(again.get(&p.id).is_some());
    }
}

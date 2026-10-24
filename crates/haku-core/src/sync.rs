/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Folder sync: devices share a folder (OneDrive, Dropbox, Syncthing, a network drive) and
//! each writes one encrypted bundle there. HakuCX has no sync server; the folder is moved by
//! whatever program already syncs it.
//!
//! ```text
//! <folder>/HakuCX Sync/sync.json          Argon2id salt and a check value for the passphrase
//! <folder>/HakuCX Sync/<device>.hxsync    one device's data, sealed with the passphrase key
//! ```
//!
//! Each device reads the other bundles and merges them into its own data (bookmarks, history,
//! settings, passwords); open tabs are only shown. Nothing in the folder is readable without
//! the sync passphrase.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bookmarks::Bookmarks;
use crate::history::SyncVisit;
use crate::vault::{self, KdfParams, Key, Sealed, VaultData};

pub const FORMAT: u32 = 1;
pub const DIR_NAME: &str = "HakuCX Sync";
pub const INFO_FILE: &str = "sync.json";
pub const BUNDLE_EXT: &str = "hxsync";
const CHECK_TEXT: &[u8] = b"HakuCX sync passphrase check";
const AAD_CHECK: &[u8] = b"hakucx-sync-check-v1";
const AAD_BUNDLE: &[u8] = b"hakucx-sync-bundle-v1";

/// Settings sections that sync. Everything about this machine (downloads folder, proxy,
/// limits, cleaner, sync itself, AI keys, lights, battery) and privacy choices stay local.
pub const SYNCED_SETTINGS: &[&str] = &[
    "appearance",
    "tabs",
    "sidebar",
    "startPage",
    "features",
    "sounds",
    "search",
    "languages",
    "shortcuts",
    "player",
    "video",
    "accessibility",
];

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SyncError {
    #[error("this passphrase does not match the one your other devices use")]
    WrongPassphrase,
    #[error("the sync folder holds data from a newer HakuCX")]
    TooNew,
    #[error("a file in the sync folder is damaged")]
    Damaged,
    #[error("{0}")]
    Vault(#[from] vault::VaultError),
}

/// `sync.json`: how to turn the passphrase into the folder key, and a value that proves a
/// passphrase is the right one.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FolderInfo {
    pub format: u32,
    pub kdf: KdfParams,
    pub check: Sealed,
}

impl FolderInfo {
    /// A new folder: fresh salt, and the key for `passphrase`.
    pub fn create(passphrase: &str) -> Result<(FolderInfo, Key), SyncError> {
        Self::create_with(passphrase, KdfParams::new())
    }

    fn create_with(passphrase: &str, kdf: KdfParams) -> Result<(FolderInfo, Key), SyncError> {
        let key = vault::derive_key(passphrase, &kdf)?;
        let check = vault::seal(&key, CHECK_TEXT, AAD_CHECK)?;
        Ok((
            FolderInfo {
                format: FORMAT,
                kdf,
                check,
            },
            key,
        ))
    }

    pub fn parse(bytes: &[u8]) -> Result<FolderInfo, SyncError> {
        let info: FolderInfo = serde_json::from_slice(bytes).map_err(|_| SyncError::Damaged)?;
        if info.format > FORMAT {
            return Err(SyncError::TooNew);
        }
        Ok(info)
    }

    /// The folder key, if `passphrase` is the folder's passphrase.
    pub fn unlock(&self, passphrase: &str) -> Result<Key, SyncError> {
        let key = vault::derive_key(passphrase, &self.kdf)?;
        self.verify(&key)?;
        Ok(key)
    }

    /// Check a remembered key against the folder (the passphrase may have been changed by
    /// another device starting over).
    pub fn verify(&self, key: &Key) -> Result<(), SyncError> {
        match vault::open(key, &self.check, AAD_CHECK) {
            Some(text) if text.as_slice() == CHECK_TEXT => Ok(()),
            _ => Err(SyncError::WrongPassphrase),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self).expect("serializable")
    }
}

/// A tab open on another device.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SyncedTab {
    pub title: String,
    pub url: String,
    pub workspace: String,
}

/// Settings as one device last changed them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyncedSettings {
    /// When the device last changed one of [`SYNCED_SETTINGS`] (ms).
    pub modified: i64,
    pub value: serde_json::Value,
}

/// One device's bundle. Each part is present only if that device syncs it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Bundle {
    pub format: u32,
    pub device_id: String,
    pub device_name: String,
    pub written_at: i64,
    pub bookmarks: Option<Bookmarks>,
    /// `Some("speed-dial")` when only the Speed Dial folder is shared.
    pub bookmark_scope: Option<String>,
    pub history: Option<Vec<SyncVisit>>,
    pub settings: Option<SyncedSettings>,
    pub tabs: Option<Vec<SyncedTab>>,
    pub vault: Option<VaultData>,
}

#[derive(Serialize, Deserialize)]
struct BundleFile {
    format: u32,
    data: Sealed,
}

impl Bundle {
    pub fn seal(&self, key: &Key) -> Result<Vec<u8>, SyncError> {
        let plain =
            zeroize::Zeroizing::new(serde_json::to_vec(self).map_err(|_| SyncError::Damaged)?);
        let file = BundleFile {
            format: FORMAT,
            data: vault::seal(key, &plain, AAD_BUNDLE)?,
        };
        Ok(serde_json::to_vec(&file).expect("serializable"))
    }

    pub fn open(bytes: &[u8], key: &Key) -> Result<Bundle, SyncError> {
        let file: BundleFile = serde_json::from_slice(bytes).map_err(|_| SyncError::Damaged)?;
        if file.format > FORMAT {
            return Err(SyncError::TooNew);
        }
        let plain = vault::open(key, &file.data, AAD_BUNDLE).ok_or(SyncError::WrongPassphrase)?;
        let bundle: Bundle = serde_json::from_slice(&plain).map_err(|_| SyncError::Damaged)?;
        if bundle.format > FORMAT {
            return Err(SyncError::TooNew);
        }
        Ok(bundle)
    }
}

/// The synced part of a settings document (JSON, camelCase keys), without values that are
/// paths on this machine.
pub fn settings_subset(settings: &serde_json::Value) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for key in SYNCED_SETTINGS {
        if let Some(v) = settings.get(*key) {
            out.insert((*key).to_owned(), v.clone());
        }
    }
    if let Some(appearance) = out.get_mut("appearance").and_then(|a| a.as_object_mut()) {
        appearance.remove("customWallpaper");
    }
    serde_json::Value::Object(out)
}

/// A file name for a device id (letters and digits only).
pub fn bundle_name(device_id: &str) -> String {
    let safe: String = device_id
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .take(64)
        .collect();
    format!("{safe}.{BUNDLE_EXT}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passphrase_and_bundles() {
        let (info, key) = FolderInfo::create_with("tiger lily", KdfParams::fast()).unwrap();
        let info = FolderInfo::parse(&info.to_bytes()).unwrap();
        assert!(info.unlock("tiger lily").is_ok());
        assert_eq!(
            info.unlock("tiger lilly").err(),
            Some(SyncError::WrongPassphrase)
        );

        let mut vault = VaultData::default();
        vault.upsert_login(
            vault::Login {
                origin: "https://a.test".into(),
                username: "kim".into(),
                password: "secret-pw".into(),
                ..vault::Login::default()
            },
            1,
        );
        let bundle = Bundle {
            format: FORMAT,
            device_id: "abc".into(),
            device_name: "Desk".into(),
            written_at: 5,
            tabs: Some(vec![SyncedTab {
                title: "T".into(),
                url: "https://t.test/".into(),
                workspace: "Home".into(),
            }]),
            vault: Some(vault),
            ..Bundle::default()
        };
        let bytes = bundle.seal(&key).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("secret-pw"));
        assert_eq!(Bundle::open(&bytes, &key).unwrap(), bundle);
        let (_, other_key) = FolderInfo::create_with("tiger lily", KdfParams::fast()).unwrap();
        assert_eq!(
            Bundle::open(&bytes, &other_key).err(),
            Some(SyncError::WrongPassphrase)
        );
        assert_eq!(bundle_name("ab/../c"), "abc.hxsync");
    }

    #[test]
    fn settings_subset_skips_local_values() {
        let s = serde_json::to_value(crate::settings::Settings::default()).unwrap();
        let sub = settings_subset(&s);
        assert!(sub.get("appearance").is_some());
        assert!(sub["appearance"].get("customWallpaper").is_none());
        assert!(
            sub.get("downloads").is_none() && sub.get("sync").is_none() && sub.get("ai").is_none()
        );
        for key in SYNCED_SETTINGS {
            assert!(s.get(*key).is_some(), "{key} is not a settings section");
        }
    }
}

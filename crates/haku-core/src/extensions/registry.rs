/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Installed extensions (`extensions.json` in the profile) and `chrome.storage` areas.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use crate::storage::{StorageError, read_json_or_default, write_json_atomic};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum ExtensionSource {
    /// "Load unpacked": runs from the developer's folder.
    Unpacked,
    /// Installed from a `.crx`, `.xpi` or `.zip` into the profile.
    Package,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExtensionRecord {
    /// 32-character id, also the first label of its pages' host (see `extensions::base_url`).
    pub id: String,
    /// What `runtime.id` returns: the Firefox id for Firefox extensions, else `id`.
    pub runtime_id: String,
    pub dir: PathBuf,
    pub source: ExtensionSource,
    pub enabled: bool,
    pub allow_private: bool,
    pub pinned: bool,
    pub installed_at: i64,
    /// Optional permissions the user granted later.
    pub granted: Vec<String>,
    /// Shortcut overrides: command name -> accelerator ("" unbinds).
    pub shortcuts: BTreeMap<String, String>,
    /// declarativeNetRequest static rulesets toggled from their manifest default.
    pub ruleset_overrides: BTreeMap<String, bool>,
    /// The version that last ran, to tell `runtime.onInstalled` "install" from "update".
    pub last_version: String,
}

impl Default for ExtensionRecord {
    fn default() -> Self {
        Self {
            id: String::new(),
            runtime_id: String::new(),
            dir: PathBuf::new(),
            source: ExtensionSource::Package,
            enabled: true,
            allow_private: false,
            pinned: false,
            installed_at: 0,
            granted: Vec::new(),
            shortcuts: BTreeMap::new(),
            ruleset_overrides: BTreeMap::new(),
            last_version: String::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ExtensionList {
    pub extensions: Vec<ExtensionRecord>,
    pub developer_mode: bool,
}

impl ExtensionList {
    pub fn load(path: &Path) -> ExtensionList {
        read_json_or_default(path)
    }

    pub fn save(&self, path: &Path) -> Result<(), StorageError> {
        write_json_atomic(path, self)
    }

    pub fn get(&self, id: &str) -> Option<&ExtensionRecord> {
        self.extensions.iter().find(|e| e.id == id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut ExtensionRecord> {
        self.extensions.iter_mut().find(|e| e.id == id)
    }

    /// Insert or replace (an update keeps the user's choices).
    pub fn upsert(&mut self, mut record: ExtensionRecord) {
        if let Some(existing) = self.get_mut(&record.id) {
            record.enabled = existing.enabled;
            record.allow_private = existing.allow_private;
            record.pinned = existing.pinned;
            record.granted = std::mem::take(&mut existing.granted);
            record.shortcuts = std::mem::take(&mut existing.shortcuts);
            record.ruleset_overrides = std::mem::take(&mut existing.ruleset_overrides);
            record.installed_at = existing.installed_at;
            record.last_version = std::mem::take(&mut existing.last_version);
            *existing = record;
        } else {
            self.extensions.push(record);
        }
    }

    pub fn remove(&mut self, id: &str) -> Option<ExtensionRecord> {
        let i = self.extensions.iter().position(|e| e.id == id)?;
        Some(self.extensions.remove(i))
    }
}

// ---------------------------------------------------------------------------------------------
// chrome.storage
// ---------------------------------------------------------------------------------------------

/// One `chrome.storage` area (`local`, `sync`, `session`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StorageArea {
    pub items: Map<String, Value>,
}

/// Chrome's limits: `local` 10 MB (unless `unlimitedStorage`), `sync` 100 KB with 8 KB items.
#[derive(Clone, Copy, Debug)]
pub struct Quota {
    pub total_bytes: usize,
    pub item_bytes: usize,
    pub max_items: usize,
}

impl Quota {
    pub const LOCAL: Quota = Quota {
        total_bytes: 10 * 1024 * 1024,
        item_bytes: usize::MAX,
        max_items: usize::MAX,
    };
    pub const UNLIMITED: Quota = Quota {
        total_bytes: usize::MAX,
        item_bytes: usize::MAX,
        max_items: usize::MAX,
    };
    pub const SYNC: Quota = Quota {
        total_bytes: 102_400,
        item_bytes: 8_192,
        max_items: 512,
    };
    pub const SESSION: Quota = Quota {
        total_bytes: 10 * 1024 * 1024,
        item_bytes: usize::MAX,
        max_items: usize::MAX,
    };
}

/// `onChanged` record: key -> { oldValue?, newValue? }.
pub type Changes = Map<String, Value>;

impl StorageArea {
    /// `get(keys)`: `null` = everything; a string; an array of strings; or an object of
    /// defaults.
    pub fn get(&self, keys: &Value) -> Map<String, Value> {
        match keys {
            Value::Null => self.items.clone(),
            Value::String(k) => self
                .items
                .get(k)
                .map(|v| Map::from_iter([(k.clone(), v.clone())]))
                .unwrap_or_default(),
            Value::Array(list) => list
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|k| self.items.get(k).map(|v| (k.to_owned(), v.clone())))
                .collect(),
            Value::Object(defaults) => defaults
                .iter()
                .map(|(k, d)| {
                    (
                        k.clone(),
                        self.items.get(k).cloned().unwrap_or_else(|| d.clone()),
                    )
                })
                .collect(),
            _ => Map::new(),
        }
    }

    pub fn bytes_in_use(&self, keys: &Value) -> usize {
        let selected = if keys.is_null() {
            self.items.clone()
        } else {
            self.get(keys)
        };
        selected
            .iter()
            .map(|(k, v)| k.len() + v.to_string().len())
            .sum()
    }

    pub fn set(&mut self, items: &Map<String, Value>, quota: Quota) -> Result<Changes, String> {
        let mut next = self.items.clone();
        for (k, v) in items {
            if k.len() + v.to_string().len() > quota.item_bytes {
                return Err(format!("QUOTA_BYTES_PER_ITEM quota exceeded for \"{k}\""));
            }
            next.insert(k.clone(), v.clone());
        }
        if next.len() > quota.max_items {
            return Err("MAX_ITEMS quota exceeded".into());
        }
        let total: usize = next
            .iter()
            .map(|(k, v)| k.len() + v.to_string().len())
            .sum();
        if total > quota.total_bytes {
            return Err("QUOTA_BYTES quota exceeded".into());
        }
        let mut changes = Changes::new();
        for (k, v) in items {
            let old = self.items.get(k);
            if old != Some(v) {
                let mut change = Map::new();
                if let Some(old) = old {
                    change.insert("oldValue".into(), old.clone());
                }
                change.insert("newValue".into(), v.clone());
                changes.insert(k.clone(), Value::Object(change));
            }
        }
        self.items = next;
        Ok(changes)
    }

    pub fn remove(&mut self, keys: &[String]) -> Changes {
        let mut changes = Changes::new();
        for k in keys {
            if let Some(old) = self.items.remove(k) {
                changes.insert(k.clone(), serde_json::json!({ "oldValue": old }));
            }
        }
        changes
    }

    pub fn clear(&mut self) -> Changes {
        let keys: Vec<String> = self.items.keys().cloned().collect();
        self.remove(&keys)
    }
}

/// `local` and `sync` areas of one extension, persisted together.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtensionStorage {
    pub local: StorageArea,
    pub sync: StorageArea,
    /// `storage.managed` is policy-only; always empty here.
    #[serde(skip)]
    pub session: StorageArea,
}

impl ExtensionStorage {
    pub fn path(storage_dir: &Path, id: &str) -> PathBuf {
        storage_dir.join(format!("{id}.json"))
    }
    pub fn load(storage_dir: &Path, id: &str) -> ExtensionStorage {
        read_json_or_default(&Self::path(storage_dir, id))
    }
    pub fn save(&self, storage_dir: &Path, id: &str) -> Result<(), StorageError> {
        write_json_atomic(&Self::path(storage_dir, id), self)
    }
    pub fn area_mut(&mut self, name: &str) -> Option<&mut StorageArea> {
        match name {
            "local" => Some(&mut self.local),
            "sync" => Some(&mut self.sync),
            "session" => Some(&mut self.session),
            _ => None,
        }
    }
    pub fn area(&self, name: &str) -> Option<&StorageArea> {
        match name {
            "local" => Some(&self.local),
            "sync" => Some(&self.sync),
            "session" => Some(&self.session),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn storage_get_set_remove_with_changes() {
        let mut a = StorageArea::default();
        let changes = a
            .set(
                json!({"a": 1, "b": {"x": true}}).as_object().unwrap(),
                Quota::LOCAL,
            )
            .unwrap();
        assert_eq!(changes["a"], json!({"newValue": 1}));
        let changes = a
            .set(
                json!({"a": 2, "b": {"x": true}}).as_object().unwrap(),
                Quota::LOCAL,
            )
            .unwrap();
        assert_eq!(changes.len(), 1, "unchanged values produce no change");
        assert_eq!(changes["a"], json!({"oldValue": 1, "newValue": 2}));
        assert_eq!(Value::Object(a.get(&json!("a"))), json!({"a": 2}));
        assert_eq!(Value::Object(a.get(&json!(["a", "zz"]))), json!({"a": 2}));
        assert_eq!(
            Value::Object(a.get(&json!({"zz": 5, "a": 0}))),
            json!({"zz": 5, "a": 2})
        );
        assert_eq!(a.get(&Value::Null).len(), 2);
        let removed = a.remove(&["a".into()]);
        assert_eq!(removed["a"], json!({"oldValue": 2}));
        assert_eq!(a.clear().len(), 1);
        assert!(a.items.is_empty());
    }

    #[test]
    fn sync_quota() {
        let mut a = StorageArea::default();
        let big = "x".repeat(9000);
        assert!(
            a.set(json!({ "k": big }).as_object().unwrap(), Quota::SYNC)
                .is_err()
        );
        assert!(a.items.is_empty(), "a refused write changes nothing");
    }

    #[test]
    fn upsert_keeps_user_choices() {
        let mut list = ExtensionList::default();
        list.upsert(ExtensionRecord {
            id: "a".into(),
            enabled: false,
            pinned: true,
            ..Default::default()
        });
        list.upsert(ExtensionRecord {
            id: "a".into(),
            runtime_id: "new".into(),
            ..Default::default()
        });
        let e = list.get("a").unwrap();
        assert!(!e.enabled && e.pinned);
        assert_eq!(e.runtime_id, "new");
        assert!(list.remove("a").is_some() && list.extensions.is_empty());
    }
}

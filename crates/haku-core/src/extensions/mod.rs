/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! WebExtensions (Chrome MV2/MV3 and Firefox): the engine-independent parts.
//!
//! The browser crate runs extensions on Servo (background pages, content scripts, the
//! `chrome.*` API); everything here is plain data and can be unit-tested without an engine.

pub mod dnr;
pub mod i18n;
pub mod manifest;
pub mod matching;
pub mod package;
pub mod registry;

use std::path::{Path, PathBuf};

pub use manifest::Manifest;
pub use registry::{ExtensionList, ExtensionRecord, ExtensionSource};

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("{0}")]
    Package(#[from] package::PackageError),
    #[error("{0}")]
    Manifest(#[from] manifest::ManifestError),
    #[error("the extension has no manifest.json")]
    NoManifest,
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// An installed extension, ready to run.
#[derive(Clone, Debug)]
pub struct Extension {
    pub record: ExtensionRecord,
    pub manifest: Manifest,
    pub messages: i18n::Messages,
    /// Display name and description with `__MSG_` replaced.
    pub name: String,
    pub description: String,
}

impl Extension {
    pub fn load(record: ExtensionRecord, ui_locale: &str) -> Result<Extension, InstallError> {
        let text = std::fs::read_to_string(record.dir.join("manifest.json"))
            .map_err(|_| InstallError::NoManifest)?;
        let manifest = Manifest::parse(&text)?;
        let messages =
            i18n::Messages::load(&record.dir, manifest.default_locale.as_deref(), ui_locale);
        let name = messages.localize(&manifest.name, &record.runtime_id);
        let description = messages.localize(&manifest.description, &record.runtime_id);
        Ok(Extension {
            record,
            manifest,
            messages,
            name,
            description,
        })
    }

    pub fn id(&self) -> &str {
        &self.record.id
    }

    /// Where the extension's pages are served: `https://<id>.haku-extension.invalid/`.
    pub fn base_url(&self) -> String {
        base_url(&self.record.id)
    }

    /// A path inside the extension, refusing anything that would leave its folder.
    pub fn file(&self, relative: &str) -> Option<PathBuf> {
        resolve_inside(&self.record.dir, relative)
    }

    /// Permissions in effect (manifest plus granted optional ones).
    pub fn has_permission(&self, name: &str) -> bool {
        self.manifest.has_permission(name) || self.record.granted.iter().any(|p| p == name)
    }

    /// Host patterns in effect.
    pub fn host_patterns(&self) -> Vec<String> {
        let mut out = self.manifest.host_permissions.clone();
        out.extend(
            self.record
                .granted
                .iter()
                .filter(|p| manifest::is_host_pattern(p))
                .cloned(),
        );
        out
    }

    pub fn can_access_url(&self, url: &str) -> bool {
        self.host_patterns()
            .iter()
            .filter_map(|p| matching::MatchPattern::parse(p))
            .any(|p| p.matches(url))
    }

    /// Is `path` exposed to web pages (`web_accessible_resources`) for a page at `page_url`?
    pub fn is_web_accessible(&self, path: &str, page_url: Option<&str>) -> bool {
        let path = path.trim_start_matches('/');
        self.manifest.web_accessible_resources.iter().any(|w| {
            let page_ok = w.matches.is_empty()
                || page_url.is_some_and(|u| {
                    w.matches
                        .iter()
                        .filter_map(|p| matching::MatchPattern::parse(p))
                        .any(|p| p.matches(u))
                });
            page_ok
                && w.resources.iter().any(|r| {
                    matching::Glob::parse(r.trim_start_matches('/'))
                        .is_some_and(|g| g.matches(path))
                })
        })
    }
}

/// Extension pages live at `https://<id>` + this suffix. The `.invalid` name never resolves;
/// the browser answers these requests itself. A real `https` origin gives extension pages
/// working storage and a secure context (custom schemes get opaque origins in Servo).
pub const HOST_SUFFIX: &str = ".haku-extension.invalid";

pub fn base_url(id: &str) -> String {
    format!("https://{id}{HOST_SUFFIX}/")
}

/// Join `relative` onto `root` only if the result stays inside `root`.
pub fn resolve_inside(root: &Path, relative: &str) -> Option<PathBuf> {
    let relative = relative
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .trim_start_matches('/');
    let decoded = percent_decode(relative);
    let mut out = root.to_path_buf();
    for part in decoded.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => return None,
            p if p.contains(':') => return None,
            p => out.push(p),
        }
    }
    Some(out)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(h), Some(l)) = (hex(bytes.get(i + 1)), hex(bytes.get(i + 2)))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: Option<&u8>) -> Option<u8> {
    let b = *b?;
    (b as char).to_digit(16).map(|d| d as u8)
}

/// Install a package (`.crx`, `.xpi`, `.zip`) into `extensions_dir/<id>/`. An existing install
/// with the same id is replaced (an update).
pub fn install_package(
    bytes: &[u8],
    extensions_dir: &Path,
    now_ms: i64,
) -> Result<(ExtensionRecord, Manifest), InstallError> {
    std::fs::create_dir_all(extensions_dir)?;
    let staging = extensions_dir.join(format!(".staging-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let info = package::extract(bytes, &staging)?;
        let text = std::fs::read_to_string(staging.join("manifest.json"))
            .map_err(|_| InstallError::NoManifest)?;
        let manifest = Manifest::parse(&text)?;
        let id = manifest
            .key
            .as_deref()
            .and_then(package::id_from_manifest_key)
            .or(info.crx_id)
            .or_else(|| manifest.gecko_id.as_deref().map(package::id_from_name))
            .unwrap_or_else(|| {
                package::id_from_name(&format!("{}@{}", manifest.name, uuid::Uuid::new_v4()))
            });
        let dest = extensions_dir.join(&id);
        if dest.exists() {
            std::fs::remove_dir_all(&dest)?;
        }
        std::fs::rename(&staging, &dest)?;
        let record = ExtensionRecord {
            runtime_id: manifest.gecko_id.clone().unwrap_or_else(|| id.clone()),
            id,
            dir: dest,
            source: ExtensionSource::Package,
            installed_at: now_ms,
            ..Default::default()
        };
        Ok((record, manifest))
    })();
    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

/// "Load unpacked": the extension runs from its own folder.
pub fn load_unpacked(dir: &Path, now_ms: i64) -> Result<(ExtensionRecord, Manifest), InstallError> {
    let text =
        std::fs::read_to_string(dir.join("manifest.json")).map_err(|_| InstallError::NoManifest)?;
    let manifest = Manifest::parse(&text)?;
    let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let id = manifest
        .key
        .as_deref()
        .and_then(package::id_from_manifest_key)
        .unwrap_or_else(|| package::id_from_path(&canonical));
    let record = ExtensionRecord {
        runtime_id: manifest.gecko_id.clone().unwrap_or_else(|| id.clone()),
        id,
        dir: canonical,
        source: ExtensionSource::Unpacked,
        installed_at: now_ms,
        ..Default::default()
    };
    Ok((record, manifest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn package(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            for (name, text) in files {
                w.start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                w.write_all(text.as_bytes()).unwrap();
            }
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn install_and_load_with_locales() {
        let dir = tempfile::tempdir().unwrap();
        let xpi = package(&[
            (
                "manifest.json",
                r#"{"manifest_version": 2, "name": "__MSG_name__", "version": "1", "default_locale": "en",
              "browser_specific_settings": {"gecko": {"id": "demo@example.org"}},
              "web_accessible_resources": ["img/*.png"]}"#,
            ),
            (
                "_locales/en/messages.json",
                r#"{"name": {"message": "Demo"}}"#,
            ),
            ("img/a.png", "png"),
        ]);
        let (record, manifest) = install_package(&xpi, dir.path(), 5).unwrap();
        assert_eq!(record.runtime_id, "demo@example.org");
        assert_eq!(record.id, package::id_from_name("demo@example.org"));
        assert_eq!(manifest.version, "1");
        let ext = Extension::load(record, "en-US").unwrap();
        assert_eq!(ext.name, "Demo");
        assert!(ext.file("img/a.png").unwrap().exists());
        assert!(ext.file("../../etc/passwd").is_none());
        assert!(ext.file("img/%2e%2e/%2e%2e/x").is_none());
        assert!(ext.is_web_accessible("/img/a.png", Some("https://x.test/")));
        assert!(!ext.is_web_accessible("secret.js", Some("https://x.test/")));
        // Reinstall replaces in place.
        let (again, _) = install_package(&xpi, dir.path(), 6).unwrap();
        assert_eq!(again.id, ext.record.id);
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            1,
            "no staging leftovers"
        );
    }

    #[test]
    fn unpacked_ids_are_stable() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("manifest.json"),
            r#"{"manifest_version": 3, "name": "U", "version": "0.1"}"#,
        )
        .unwrap();
        let (a, _) = load_unpacked(dir.path(), 1).unwrap();
        let (b, _) = load_unpacked(dir.path(), 2).unwrap();
        assert_eq!(a.id, b.id);
        assert!(package::is_valid_id(&a.id));
        assert_eq!(a.source, ExtensionSource::Unpacked);
    }
}

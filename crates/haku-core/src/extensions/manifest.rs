/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! WebExtension manifests (Chrome MV2/MV3 and Firefox), normalized into one model.
//!
//! Differences between the formats are resolved here so the runtime sees one shape:
//! MV2 `browser_action`/`page_action` and MV3 `action` become [`Manifest::action`]; MV2 host
//! patterns inside `permissions` move to `host_permissions`; `background.scripts`,
//! `background.page` and `background.service_worker` become one [`Background`].

use std::collections::BTreeMap;

use serde_json::Value;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ManifestError {
    #[error("manifest.json is not valid JSON: {0}")]
    Json(String),
    #[error("manifest.json is missing \"{0}\"")]
    Missing(&'static str),
    #[error("manifest_version {0} is not supported (2 and 3 are)")]
    Version(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunAt {
    DocumentStart,
    DocumentEnd,
    DocumentIdle,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ContentScript {
    pub matches: Vec<String>,
    pub exclude_matches: Vec<String>,
    pub include_globs: Vec<String>,
    pub exclude_globs: Vec<String>,
    pub js: Vec<String>,
    pub css: Vec<String>,
    pub run_at: RunAt,
    pub all_frames: bool,
    pub match_about_blank: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Background {
    /// MV2 `background.scripts` (loaded in order into a generated page).
    Scripts(Vec<String>),
    /// MV2 `background.page`.
    Page(String),
    /// MV3 `background.service_worker`, run in a hidden page (Servo has no extension
    /// service workers). `module` follows `"type": "module"`.
    ServiceWorker { path: String, module: bool },
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Action {
    pub default_popup: Option<String>,
    pub default_title: Option<String>,
    /// Size -> path (a single string becomes size 0).
    pub default_icon: BTreeMap<u32, String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WebAccessible {
    pub resources: Vec<String>,
    /// Empty means any page (MV2 lists only resources).
    pub matches: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CommandSpec {
    pub suggested_key: Option<String>,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Ruleset {
    pub id: String,
    pub enabled: bool,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Manifest {
    pub manifest_version: u8,
    pub name: String,
    pub short_name: Option<String>,
    pub version: String,
    pub description: String,
    pub default_locale: Option<String>,
    pub icons: BTreeMap<u32, String>,
    pub permissions: Vec<String>,
    pub optional_permissions: Vec<String>,
    pub host_permissions: Vec<String>,
    pub content_scripts: Vec<ContentScript>,
    pub background: Option<Background>,
    pub action: Option<Action>,
    pub options_page: Option<String>,
    pub options_open_in_tab: bool,
    pub web_accessible_resources: Vec<WebAccessible>,
    pub commands: BTreeMap<String, CommandSpec>,
    pub rulesets: Vec<Ruleset>,
    pub gecko_id: Option<String>,
    pub key: Option<String>,
    pub homepage_url: Option<String>,
    pub author: Option<String>,
    /// `spanning`, `split` or `not_allowed`.
    pub incognito: String,
    pub content_security_policy: Option<String>,
    /// The manifest as written, for `runtime.getManifest()`.
    pub raw: Value,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest, ManifestError> {
        let cleaned = strip_json_comments(text.trim_start_matches('\u{feff}'));
        let raw: Value =
            serde_json::from_str(&cleaned).map_err(|e| ManifestError::Json(e.to_string()))?;
        Self::from_value(raw)
    }

    pub fn from_value(raw: Value) -> Result<Manifest, ManifestError> {
        let version = raw
            .get("manifest_version")
            .and_then(Value::as_u64)
            .ok_or(ManifestError::Missing("manifest_version"))?;
        if version != 2 && version != 3 {
            return Err(ManifestError::Version(version));
        }
        let name = str_of(&raw, "name").ok_or(ManifestError::Missing("name"))?;
        let ext_version = str_of(&raw, "version").ok_or(ManifestError::Missing("version"))?;

        let mut permissions = Vec::new();
        let mut host_permissions = strings(raw.get("host_permissions"));
        for p in strings(raw.get("permissions")) {
            if is_host_pattern(&p) {
                host_permissions.push(p);
            } else {
                permissions.push(p);
            }
        }
        let mut optional_permissions = Vec::new();
        for p in strings(raw.get("optional_permissions"))
            .into_iter()
            .chain(strings(raw.get("optional_host_permissions")))
        {
            optional_permissions.push(p);
        }

        let content_scripts = raw
            .get("content_scripts")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(content_script).collect())
            .unwrap_or_default();

        let background = raw.get("background").and_then(|b| {
            if let Some(sw) = str_of(b, "service_worker") {
                let module = b.get("type").and_then(Value::as_str) == Some("module");
                Some(Background::ServiceWorker { path: sw, module })
            } else if let Some(page) = str_of(b, "page") {
                Some(Background::Page(page))
            } else {
                let scripts = strings(b.get("scripts"));
                (!scripts.is_empty()).then_some(Background::Scripts(scripts))
            }
        });

        let action = ["action", "browser_action", "page_action"]
            .iter()
            .find_map(|k| raw.get(*k))
            .map(|a| Action {
                default_popup: str_of(a, "default_popup").filter(|p| !p.is_empty()),
                default_title: str_of(a, "default_title"),
                default_icon: icon_map(a.get("default_icon")),
            });

        let (options_page, options_open_in_tab) = match raw.get("options_ui") {
            Some(ui) => (
                str_of(ui, "page"),
                ui.get("open_in_tab")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
            None => (str_of(&raw, "options_page"), true),
        };

        let web_accessible_resources = match raw.get("web_accessible_resources") {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|item| match item {
                    Value::String(s) => Some(WebAccessible {
                        resources: vec![s.clone()],
                        matches: Vec::new(),
                    }),
                    Value::Object(_) => Some(WebAccessible {
                        resources: strings(item.get("resources")),
                        matches: strings(item.get("matches")),
                    }),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };

        let commands = raw
            .get("commands")
            .and_then(Value::as_object)
            .map(|map| {
                map.iter()
                    .map(|(name, spec)| {
                        let key = spec.get("suggested_key").and_then(|k| match k {
                            Value::String(s) => Some(s.clone()),
                            Value::Object(o) => o
                                .get("windows")
                                .or_else(|| o.get("default"))
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                            _ => None,
                        });
                        (
                            name.clone(),
                            CommandSpec {
                                suggested_key: key,
                                description: str_of(spec, "description").unwrap_or_default(),
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        let rulesets = raw
            .get("declarative_net_request")
            .and_then(|d| d.get("rule_resources"))
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|r| {
                        Some(Ruleset {
                            id: str_of(r, "id")?,
                            enabled: r.get("enabled").and_then(Value::as_bool).unwrap_or(false),
                            path: str_of(r, "path")?,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let gecko_id = raw
            .get("browser_specific_settings")
            .or_else(|| raw.get("applications"))
            .and_then(|b| b.get("gecko"))
            .and_then(|g| str_of(g, "id"));

        let csp = match raw.get("content_security_policy") {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Object(o)) => o
                .get("extension_pages")
                .and_then(Value::as_str)
                .map(str::to_owned),
            _ => None,
        };

        Ok(Manifest {
            manifest_version: version as u8,
            name,
            short_name: str_of(&raw, "short_name"),
            version: ext_version,
            description: str_of(&raw, "description").unwrap_or_default(),
            default_locale: str_of(&raw, "default_locale"),
            icons: icon_map(raw.get("icons")),
            permissions,
            optional_permissions,
            host_permissions,
            content_scripts,
            background,
            action,
            options_page,
            options_open_in_tab,
            web_accessible_resources,
            commands,
            rulesets,
            gecko_id,
            key: str_of(&raw, "key"),
            homepage_url: str_of(&raw, "homepage_url"),
            author: match raw.get("author") {
                Some(Value::String(s)) => Some(s.clone()),
                Some(Value::Object(o)) => o.get("email").and_then(Value::as_str).map(str::to_owned),
                _ => None,
            },
            incognito: str_of(&raw, "incognito").unwrap_or_else(|| "spanning".into()),
            content_security_policy: csp,
            raw,
        })
    }

    pub fn has_permission(&self, name: &str) -> bool {
        self.permissions.iter().any(|p| p == name)
    }

    /// The icon closest to `size` (at least that size if available).
    pub fn best_icon(&self, size: u32) -> Option<&str> {
        best_icon(&self.icons, size).or_else(|| {
            self.action
                .as_ref()
                .and_then(|a| best_icon(&a.default_icon, size))
        })
    }

    /// Human-readable permission warnings, as shown before installing.
    pub fn permission_warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        let all_hosts = self
            .host_permissions
            .iter()
            .chain(self.content_scripts.iter().flat_map(|c| c.matches.iter()))
            .any(|p| {
                p == "<all_urls>"
                    || p.starts_with("*://*/")
                    || p.starts_with("http://*/")
                    || p.starts_with("https://*/")
            });
        if all_hosts {
            out.push("Read and change all your data on all websites".to_owned());
        } else {
            let mut hosts: Vec<String> = self
                .host_permissions
                .iter()
                .chain(self.content_scripts.iter().flat_map(|c| c.matches.iter()))
                .filter_map(|p| {
                    p.split("://")
                        .nth(1)
                        .map(|rest| rest.split('/').next().unwrap_or("").to_owned())
                })
                .filter(|h| !h.is_empty())
                .collect();
            hosts.sort();
            hosts.dedup();
            if !hosts.is_empty() {
                out.push(format!("Read and change your data on {}", hosts.join(", ")));
            }
        }
        for p in &self.permissions {
            let text = match p.as_str() {
                "tabs" => "Read your browsing history (tab addresses and titles)",
                "history" => "Read and change your browsing history",
                "bookmarks" => "Read and change your bookmarks",
                "cookies" => "Read and change cookies",
                "downloads" => "Manage your downloads",
                "notifications" => "Display notifications",
                "clipboardRead" => "Read data you copy and paste",
                "clipboardWrite" => "Modify data you copy and paste",
                "declarativeNetRequest" | "declarativeNetRequestWithHostAccess" => {
                    "Block content on any page"
                }
                "webRequestBlocking" => "Block and change network requests",
                "management" => "Manage your apps, extensions and themes",
                "privacy" => "Change your privacy-related settings",
                "proxy" => "Control the browser's proxy settings",
                "geolocation" => "Detect your physical location",
                "nativeMessaging" => "Communicate with cooperating native applications",
                _ => continue,
            };
            out.push(text.to_owned());
        }
        out
    }
}

fn best_icon(icons: &BTreeMap<u32, String>, size: u32) -> Option<&str> {
    icons
        .iter()
        .find(|(s, _)| **s >= size)
        .or_else(|| icons.iter().next_back())
        .map(|(_, p)| p.as_str())
}

fn content_script(v: &Value) -> Option<ContentScript> {
    let matches = strings(v.get("matches"));
    if matches.is_empty() {
        return None;
    }
    Some(ContentScript {
        matches,
        exclude_matches: strings(v.get("exclude_matches")),
        include_globs: strings(v.get("include_globs")),
        exclude_globs: strings(v.get("exclude_globs")),
        js: strings(v.get("js")),
        css: strings(v.get("css")),
        run_at: match v.get("run_at").and_then(Value::as_str) {
            Some("document_start") => RunAt::DocumentStart,
            Some("document_end") => RunAt::DocumentEnd,
            _ => RunAt::DocumentIdle,
        },
        all_frames: v
            .get("all_frames")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        match_about_blank: v
            .get("match_about_blank")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn icon_map(v: Option<&Value>) -> BTreeMap<u32, String> {
    match v {
        Some(Value::String(s)) => BTreeMap::from([(0, s.clone())]),
        Some(Value::Object(o)) => o
            .iter()
            .filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?.to_owned())))
            .collect(),
        _ => BTreeMap::new(),
    }
}

fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

/// A permission entry that is a host pattern rather than an API name.
pub fn is_host_pattern(p: &str) -> bool {
    p == "<all_urls>" || p.contains("://")
}

/// Chrome accepts `//` and `/* */` comments in manifest.json; strip them outside strings.
pub fn strip_json_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mv2_normalizes_actions_and_host_permissions() {
        let m = Manifest::parse(
            r#"{
              // comment
              "manifest_version": 2, "name": "Dark", "version": "1.2",
              "permissions": ["storage", "tabs", "<all_urls>", "https://a.test/*"],
              "browser_action": {"default_popup": "popup.html", "default_icon": "i.png"},
              "background": {"scripts": ["a.js", "b.js"]},
              "content_scripts": [{"matches": ["<all_urls>"], "js": ["c.js"], "run_at": "document_start", "all_frames": true}],
              "web_accessible_resources": ["img/*.png"],
              "applications": {"gecko": {"id": "dark@example.org"}}
            }"#,
        )
        .unwrap();
        assert_eq!(m.permissions, vec!["storage", "tabs"]);
        assert_eq!(m.host_permissions, vec!["<all_urls>", "https://a.test/*"]);
        assert_eq!(
            m.action.as_ref().unwrap().default_popup.as_deref(),
            Some("popup.html")
        );
        assert_eq!(
            m.action
                .as_ref()
                .unwrap()
                .default_icon
                .get(&0)
                .map(String::as_str),
            Some("i.png")
        );
        assert_eq!(
            m.background,
            Some(Background::Scripts(vec!["a.js".into(), "b.js".into()]))
        );
        assert_eq!(m.content_scripts[0].run_at, RunAt::DocumentStart);
        assert!(m.content_scripts[0].all_frames);
        assert_eq!(m.web_accessible_resources[0].resources, vec!["img/*.png"]);
        assert_eq!(m.gecko_id.as_deref(), Some("dark@example.org"));
        assert!(m.permission_warnings()[0].contains("all websites"));
    }

    #[test]
    fn mv3_service_worker_rulesets_and_commands() {
        let m = Manifest::parse(
            r#"{"manifest_version": 3, "name": "Blocker", "version": "2",
              "background": {"service_worker": "sw.js", "type": "module"},
              "action": {"default_title": "Block"},
              "options_ui": {"page": "opts.html", "open_in_tab": false},
              "declarative_net_request": {"rule_resources": [{"id": "ads", "enabled": true, "path": "ads.json"}]},
              "commands": {"_execute_action": {"suggested_key": {"default": "Alt+Shift+B"}, "description": "Open"}},
              "web_accessible_resources": [{"resources": ["a.js"], "matches": ["https://*/*"]}]}"#,
        )
        .unwrap();
        assert_eq!(
            m.background,
            Some(Background::ServiceWorker {
                path: "sw.js".into(),
                module: true
            })
        );
        assert_eq!(m.options_page.as_deref(), Some("opts.html"));
        assert!(!m.options_open_in_tab);
        assert_eq!(m.rulesets[0].id, "ads");
        assert_eq!(
            m.commands["_execute_action"].suggested_key.as_deref(),
            Some("Alt+Shift+B")
        );
        assert_eq!(m.web_accessible_resources[0].matches, vec!["https://*/*"]);
    }

    #[test]
    fn rejects_bad_manifests() {
        assert!(matches!(Manifest::parse("{"), Err(ManifestError::Json(_))));
        assert_eq!(
            Manifest::parse(r#"{"manifest_version": 2, "version": "1"}"#),
            Err(ManifestError::Missing("name"))
        );
        assert_eq!(
            Manifest::parse(r#"{"manifest_version": 4, "name": "x", "version": "1"}"#),
            Err(ManifestError::Version(4))
        );
    }

    #[test]
    fn comments_inside_strings_survive() {
        assert_eq!(
            strip_json_comments(r#"{"a": "http://x/*y*/" /* c */}"#),
            r#"{"a": "http://x/*y*/" }"#
        );
    }
}

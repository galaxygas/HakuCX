/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! WebExtensions on Servo: runtime state, script generation and extension file serving.
//!
//! Servo has no extension support, so HakuCX provides it:
//! - Extension pages (background, popup, options) are served from
//!   `https://<id>.haku-extension.invalid/`. The `.invalid` name never resolves; HakuCX answers
//!   those requests itself before any network access. A real `https` origin (custom schemes get
//!   opaque origins in Servo) gives extension pages working `localStorage`, IndexedDB and a
//!   secure context, which many extensions depend on.
//! - Every extension page gets `/__haku/api.js` (the `chrome.*`/`browser.*` API, see `api.js`)
//!   injected before its own scripts.
//! - Content scripts are one user script per extension, added to every page's
//!   `UserContentManager`. Servo has no isolated worlds: content scripts share the page's
//!   JavaScript world (see SECURITY.md). Their API object is kept in a closure, not on `window`.
//! - The browser side of the API is in `browser::extensions`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use haku_core::extensions::dnr::RuleSet;
use haku_core::extensions::manifest::{Background, ContentScript, RunAt};
use haku_core::extensions::matching::UrlFilter;
use haku_core::extensions::registry::ExtensionStorage;
use haku_core::extensions::{Extension, i18n};
use haku_core::session::TabId;
use serde_json::{Value, json};
use servo::{UserScript, WebView, WebViewId};

pub const API_JS: &str = include_str!("api.js");

/// `api.js` as an expression (without its trailing semicolon) so it can be called inline.
pub fn api_expr() -> &'static str {
    API_JS.trim_end().trim_end_matches(';')
}
pub use haku_core::extensions::{HOST_SUFFIX, base_url};

/// The extension id of an extension page URL.
pub fn id_of_url(url: &url::Url) -> Option<String> {
    let host = url.host_str()?;
    let id = host.strip_suffix(HOST_SUFFIX)?;
    (url.scheme() == "https" && haku_core::extensions::package::is_valid_id(id))
        .then(|| id.to_owned())
}

/// Default extension-page CSPs (Chrome's).
const CSP_MV2: &str = "script-src 'self' blob: filesystem: 'unsafe-eval'; object-src 'self'";
const CSP_MV3: &str = "script-src 'self' 'wasm-unsafe-eval'; object-src 'self'";

/// Which kind of extension context sent a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextKind {
    Background,
    Popup,
    Offscreen,
    /// An extension page open in a tab (options, a page from `tabs.create`).
    Tab,
    Content,
}

impl ContextKind {
    pub fn name(self) -> &'static str {
        match self {
            ContextKind::Background => "BACKGROUND",
            ContextKind::Popup => "POPUP",
            ContextKind::Offscreen => "OFFSCREEN_DOCUMENT",
            ContextKind::Tab => "TAB",
            ContextKind::Content => "CONTENT_SCRIPT",
        }
    }
}

/// Toolbar button properties (global, or overridden per tab).
#[derive(Clone, Debug, Default)]
pub struct ActionProps {
    pub title: Option<String>,
    pub badge_text: Option<String>,
    pub badge_color: Option<String>,
    pub badge_text_color: Option<String>,
    pub icon: Option<String>,
    pub popup: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct MenuItem {
    pub id: String,
    pub parent: Option<String>,
    pub title: String,
    pub kind: String,
    pub checked: bool,
    pub enabled: bool,
    pub visible: bool,
    pub contexts: Vec<String>,
    pub document_patterns: Option<UrlFilter>,
    pub target_patterns: Option<UrlFilter>,
}

#[derive(Clone, Debug)]
pub struct Alarm {
    pub when_ms: f64,
    pub period_minutes: Option<f64>,
}

/// One extension that is enabled and loaded.
pub struct Running {
    pub ext: Extension,
    /// Per-session secret that authenticates this extension's API calls.
    pub token: String,
    /// The `window` property extension pages receive messages on.
    pub dispatch: String,
    pub storage: ExtensionStorage,
    pub static_rules: BTreeMap<String, RuleSet>,
    pub dynamic_rules: Vec<Value>,
    pub session_rules: Vec<Value>,
    /// All enabled rules, compiled.
    pub rules: RuleSet,
    pub badge_from_rule_count: bool,
    pub menus: Vec<MenuItem>,
    pub action: ActionProps,
    pub tab_action: HashMap<TabId, ActionProps>,
    pub alarms: BTreeMap<String, Alarm>,
    /// `scripting.registerContentScripts` entries by id.
    pub registered_scripts: BTreeMap<String, ContentScript>,
    /// Registered scripts that persist across sessions.
    pub persisted_scripts: BTreeSet<String>,
    /// Static rules turned off with `updateStaticRules`, per ruleset.
    pub disabled_static: BTreeMap<String, BTreeSet<i64>>,
    /// Recent rule matches for `getMatchedRules`: (tab, rule id, ruleset id, time ms).
    pub matched_rules: Vec<(i64, i64, String, f64)>,
    /// `storage.setAccessLevel`: content scripts may use `storage.session`.
    pub session_for_content: bool,
    /// `idle.setDetectionInterval`, in seconds, and the last state reported.
    pub idle_interval: f64,
    pub idle_state: &'static str,
    /// `runtime.onInstalled` or `onStartup`, fired when the background page has loaded.
    pub lifecycle: Option<(&'static str, Vec<Value>)>,
    pub content_script: Option<Rc<UserScript>>,
    pub background: Option<WebView>,
    pub offscreen: Option<WebView>,
    pub errors: Vec<String>,
    pub uninstall_url: Option<String>,
    /// Events each extension page context listens to.
    pub listening: HashMap<WebViewId, HashSet<String>>,
    /// Content scripts listening, per tab: (frame id, event).
    pub content_listening: HashMap<TabId, HashSet<(i64, String)>>,
    /// webRequest.onBeforeRequest listeners (filter, blocking).
    pub web_request: Vec<(UrlFilter, Vec<String>, bool)>,
    /// `allowAllRequests` in effect per tab (rule priority).
    pub allow_all: HashMap<TabId, i64>,
    /// Blocked or redirected requests per tab (for the rule-count badge).
    pub matched: HashMap<TabId, u32>,
    /// URLs already counted in `matched` since the tab's last navigation. Servo's speculative
    /// parser fetches some resources twice; each is counted once.
    pub matched_seen: HashMap<TabId, HashSet<String>>,
    pub active_tab_grants: HashSet<TabId>,
}

impl Running {
    pub fn new(ext: Extension, storage: ExtensionStorage) -> Running {
        let token = uuid::Uuid::new_v4().simple().to_string();
        Running {
            dispatch: format!("__hakuExt_{}", &token[..12]),
            token,
            ext,
            storage,
            static_rules: BTreeMap::new(),
            dynamic_rules: Vec::new(),
            session_rules: Vec::new(),
            rules: RuleSet::default(),
            badge_from_rule_count: false,
            menus: Vec::new(),
            action: ActionProps::default(),
            tab_action: HashMap::new(),
            alarms: BTreeMap::new(),
            registered_scripts: BTreeMap::new(),
            persisted_scripts: BTreeSet::new(),
            disabled_static: BTreeMap::new(),
            matched_rules: Vec::new(),
            session_for_content: false,
            idle_interval: 60.0,
            idle_state: "active",
            lifecycle: None,
            content_script: None,
            background: None,
            offscreen: None,
            errors: Vec::new(),
            uninstall_url: None,
            listening: HashMap::new(),
            content_listening: HashMap::new(),
            web_request: Vec::new(),
            allow_all: HashMap::new(),
            matched: HashMap::new(),
            matched_seen: HashMap::new(),
            active_tab_grants: HashSet::new(),
        }
    }

    pub fn id(&self) -> &str {
        &self.ext.record.id
    }

    pub fn base(&self) -> String {
        base_url(self.id())
    }

    /// Recompile the rules in effect after any change.
    pub fn rebuild_rules(&mut self) {
        let mut rules = Vec::new();
        for (id, set) in &self.static_rules {
            let default_on = self
                .ext
                .manifest
                .rulesets
                .iter()
                .any(|r| &r.id == id && r.enabled);
            let on = self
                .ext
                .record
                .ruleset_overrides
                .get(id)
                .copied()
                .unwrap_or(default_on);
            if on {
                let disabled = self.disabled_static.get(id);
                rules.extend(
                    set.rules
                        .iter()
                        .filter(|r| !disabled.is_some_and(|d| d.contains(&r.id)))
                        .cloned(),
                );
            }
        }
        for list in [&self.dynamic_rules, &self.session_rules] {
            let (parsed, _) = RuleSet::parse_list(&Value::Array(list.clone()));
            rules.extend(parsed);
        }
        self.rules = RuleSet { rules };
    }

    /// The ruleset a matched rule came from (rule ids are unique within a ruleset; session
    /// and dynamic rules are checked first, as Chrome reports them).
    pub fn ruleset_of(&self, rule_id: i64) -> String {
        if self
            .session_rules
            .iter()
            .any(|r| r["id"].as_i64() == Some(rule_id))
        {
            return "_session".into();
        }
        if self
            .dynamic_rules
            .iter()
            .any(|r| r["id"].as_i64() == Some(rule_id))
        {
            return "_dynamic".into();
        }
        self.static_rules
            .iter()
            .find(|(_, set)| set.rules.iter().any(|r| r.id == rule_id))
            .map(|(id, _)| id.clone())
            .unwrap_or_default()
    }

    /// A request in `tab` matched a blocking or redirecting rule.
    pub fn count_match(&mut self, tab: &TabId, url: &str, tab_int: i64, rule_id: i64) {
        if self
            .matched_seen
            .entry(tab.clone())
            .or_default()
            .insert(url.to_owned())
        {
            *self.matched.entry(tab.clone()).or_default() += 1;
            self.note_match(tab_int, rule_id);
        }
    }

    /// Remember a rule match for `getMatchedRules` (only extensions that may read them).
    pub fn note_match(&mut self, tab: i64, rule_id: i64) {
        if !(self.ext.has_permission("declarativeNetRequestFeedback")
            || self.ext.has_permission("activeTab"))
        {
            return;
        }
        let ruleset = self.ruleset_of(rule_id);
        self.matched_rules
            .push((tab, rule_id, ruleset, haku_core::now_ms() as f64));
        // Chrome keeps matches for five minutes.
        let cutoff = haku_core::now_ms() as f64 - 300_000.0;
        self.matched_rules.retain(|m| m.3 >= cutoff);
        if self.matched_rules.len() > 10_000 {
            self.matched_rules.drain(..5_000);
        }
    }

    /// The effective toolbar props for a tab.
    pub fn action_for(&self, tab: Option<&TabId>) -> ActionProps {
        let mut out = self.action.clone();
        if let Some(t) = tab.and_then(|t| self.tab_action.get(t)) {
            macro_rules! over {
                ($f:ident) => {
                    if t.$f.is_some() {
                        out.$f = t.$f.clone();
                    }
                };
            }
            over!(title);
            over!(badge_text);
            over!(badge_color);
            over!(badge_text_color);
            over!(icon);
            over!(popup);
            over!(enabled);
        }
        out
    }

    /// The in-page config for an extension context.
    pub fn config(&self, kind: ContextKind, ui_locale: &str, incognito: bool) -> Value {
        let manifest = i18n::localize_value(
            &self.ext.manifest.raw,
            &self.ext.messages,
            &self.ext.record.runtime_id,
        );
        let mut permissions = self.ext.manifest.permissions.clone();
        permissions.extend(self.ext.record.granted.iter().cloned());
        json!({
            "id": self.id(),
            "base": self.base(),
            "runtimeId": self.ext.record.runtime_id,
            "context": match kind { ContextKind::Content => "content", ContextKind::Background => "background", _ => "page" },
            "token": self.token,
            "dispatch": self.dispatch,
            "manifest": manifest,
            "messages": self.ext.messages.to_json(),
            "uiLocale": ui_locale,
            "incognito": incognito,
            "mv": self.ext.manifest.manifest_version,
            "permissions": permissions,
            "serviceWorker": matches!(self.ext.manifest.background, Some(Background::ServiceWorker { .. })),
        })
    }

    /// `/__haku/api.js` for this extension's pages.
    pub fn page_api_script(&self, ui_locale: &str, incognito: bool, kind: ContextKind) -> String {
        format!(
            "(() => {{ const api = ({})({}); \
             try {{ Object.defineProperty(window, 'chrome', {{ value: api, configurable: true, writable: true }}); }} catch (_) {{ window.chrome = api; }} \
             window.browser = api; }})();",
            api_expr(),
            self.config(kind, ui_locale, incognito)
        )
    }

    /// The generated background page for `background.scripts` / `service_worker`.
    pub fn background_html(&self) -> Option<String> {
        let scripts = match self.ext.manifest.background.as_ref()? {
            Background::Page(_) => return None,
            Background::Scripts(list) => list
                .iter()
                .map(|s| {
                    format!(
                        "<script src=\"/{}\"></script>",
                        html_escape::encode_double_quoted_attribute(s.trim_start_matches('/'))
                    )
                })
                .collect::<String>(),
            Background::ServiceWorker { path, module } => format!(
                "<script{} src=\"/{}\"></script>",
                if *module { " type=\"module\"" } else { "" },
                html_escape::encode_double_quoted_attribute(path.trim_start_matches('/'))
            ),
        };
        Some(format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><script src=\"/__haku/api.js?ctx=background\"></script>{scripts}</head><body></body></html>"
        ))
    }

    /// The URL to load as this extension's background context.
    pub fn background_url(&self) -> Option<String> {
        match self.ext.manifest.background.as_ref()? {
            Background::Page(page) => {
                Some(format!("{}{}", self.base(), page.trim_start_matches('/')))
            }
            _ => Some(format!("{}_generated_background_page.html", self.base())),
        }
    }

    pub fn csp(&self) -> String {
        self.ext
            .manifest
            .content_security_policy
            .clone()
            .unwrap_or_else(|| {
                if self.ext.manifest.manifest_version == 2 {
                    CSP_MV2.to_owned()
                } else {
                    CSP_MV3.to_owned()
                }
            })
    }

    /// All content script entries: the manifest's and registered ones.
    pub fn content_entries(&self) -> Vec<ContentScript> {
        let mut out = self.ext.manifest.content_scripts.clone();
        out.extend(self.registered_scripts.values().cloned());
        out
    }

    /// One user script with every content script of this extension, or `None` if it has none.
    pub fn content_user_script(&self, ui_locale: &str, incognito: bool) -> Option<String> {
        let entries = self.content_entries();
        if entries.is_empty() {
            return None;
        }
        let mut parts = Vec::new();
        for entry in &entries {
            let filter = UrlFilter::new(
                &entry.matches,
                &entry.exclude_matches,
                &entry.include_globs,
                &entry.exclude_globs,
            );
            let mut js = String::new();
            for file in &entry.js {
                match self
                    .ext
                    .file(file)
                    .and_then(|p| std::fs::read_to_string(p).ok())
                {
                    Some(text) => {
                        js.push_str(&text);
                        js.push_str("\n;\n");
                    }
                    None => log::warn!(
                        "extension {}: content script {file} is missing",
                        self.ext.name
                    ),
                }
            }
            let mut css = String::new();
            for file in &entry.css {
                if let Some(text) = self
                    .ext
                    .file(file)
                    .and_then(|p| std::fs::read_to_string(p).ok())
                {
                    css.push_str(
                        &self
                            .ext
                            .messages
                            .localize(&text, &self.ext.record.runtime_id),
                    );
                    css.push('\n');
                }
            }
            let run_at = match entry.run_at {
                RunAt::DocumentStart => "start",
                RunAt::DocumentEnd => "end",
                RunAt::DocumentIdle => "idle",
            };
            parts.push(format!(
                "{{ test: {}, allFrames: {}, aboutBlank: {}, runAt: {:?}, css: {}, js: function () {{ (function (chrome, browser) {{ {}\n }}).call(window, api(), api()); }} }}",
                filter.to_js(),
                entry.all_frames,
                entry.match_about_blank,
                run_at,
                serde_json::to_string(&css).unwrap_or_else(|_| "\"\"".into()),
                js
            ));
        }
        Some(format!(
            r#"(() => {{
  const href = location.href;
  // Never inside extension pages (Chrome does not inject into chrome-extension:// either).
  if (location.hostname.endsWith("{suffix}")) return;
  const top = window.top === window;
  const API = ({api});
  let instance = null;
  const api = () => instance || (instance = API({config}));
  const entries = [{entries}];
  for (const e of entries) {{
    if (!e.allFrames && !top) continue;
    if (!e.test(href) && !(e.aboutBlank && (href === "about:blank" || href === "about:srcdoc"))) continue;
    if (e.css) {{
      const style = document.createElement("style");
      style.textContent = e.css;
      (document.head || document.documentElement).appendChild(style);
    }}
    const run = () => {{ try {{ e.js(); }} catch (err) {{ console.error(err); }} }};
    if (e.runAt === "start") run();
    else if (e.runAt === "end") {{
      if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", run, {{ once: true }}); else run();
    }} else {{
      if (document.readyState === "complete") run();
      else if (document.readyState === "interactive") setTimeout(run, 0);
      else document.addEventListener("DOMContentLoaded", () => setTimeout(run, 0), {{ once: true }});
    }}
  }}
}})();"#,
            api = api_expr(),
            suffix = HOST_SUFFIX,
            config = self.config(ContextKind::Content, ui_locale, incognito),
            entries = parts.join(",\n")
        ))
    }
}

/// Insert the API script into an extension HTML page, before any of its own scripts.
pub fn inject_api(html: &str) -> String {
    const TAG: &str = "<script src=\"/__haku/api.js\"></script>";
    let lower = html.to_ascii_lowercase();
    for marker in ["<head>", "<head "] {
        if let Some(i) = lower.find(marker) {
            let end = i + lower[i..].find('>').map(|e| e + 1).unwrap_or(marker.len());
            return format!("{}{TAG}{}", &html[..end], &html[end..]);
        }
    }
    if let Some(i) = lower.find("<html")
        && let Some(e) = lower[i..].find('>')
    {
        let end = i + e + 1;
        return format!("{}{TAG}{}", &html[..end], &html[end..]);
    }
    // No <html>/<head>: a doctype or nothing. Put it first (after a doctype if present).
    if let Some(i) = lower.find("<!doctype")
        && let Some(e) = lower[i..].find('>')
    {
        let end = i + e + 1;
        return format!("{}{TAG}{}", &html[..end], &html[end..]);
    }
    format!("{TAG}{html}")
}

/// Content type for a file served from an extension.
pub fn mime_for(path: &str) -> String {
    match path
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("js" | "mjs") => "text/javascript; charset=utf-8".into(),
        Some("json") => "application/json; charset=utf-8".into(),
        Some("html" | "htm") => "text/html; charset=utf-8".into(),
        Some("css") => "text/css; charset=utf-8".into(),
        Some("wasm") => "application/wasm".into(),
        _ => mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_injection_goes_before_page_scripts() {
        assert_eq!(
            inject_api("<html><head><script src=a.js></script></head>"),
            "<html><head><script src=\"/__haku/api.js\"></script><script src=a.js></script></head>"
        );
        assert_eq!(
            inject_api("<!DOCTYPE html><HEAD lang=x>x"),
            "<!DOCTYPE html><HEAD lang=x><script src=\"/__haku/api.js\"></script>x"
        );
        assert_eq!(
            inject_api("<!doctype html><script src=a.js></script>"),
            "<!doctype html><script src=\"/__haku/api.js\"></script><script src=a.js></script>"
        );
        assert!(inject_api("<p>hi").starts_with("<script"));
    }

    #[test]
    fn extension_urls() {
        let id = "abcdefghijklmnopabcdefghijklmnop";
        let u = url::Url::parse(&format!("{}popup.html", base_url(id))).unwrap();
        assert_eq!(id_of_url(&u).as_deref(), Some(id));
        assert_eq!(
            id_of_url(&url::Url::parse("https://example.com/").unwrap()),
            None
        );
        assert_eq!(
            id_of_url(
                &url::Url::parse("http://abcdefghijklmnopabcdefghijklmnop.haku-extension.invalid/")
                    .unwrap()
            ),
            None
        );
    }

    #[test]
    fn api_js_is_an_expression() {
        assert!(api_expr().ends_with("})"));
        assert!(API_JS.contains("(function (config) {"));
    }
}

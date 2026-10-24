/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! `declarativeNetRequest` rules: parsing, matching and the winning action.
//!
//! Supported actions: `block`, `allow`, `allowAllRequests`, `upgradeScheme` and `redirect`
//! (url, extensionPath, regexSubstitution, transform). `modifyHeaders` parses but cannot be
//! applied: Servo does not let the browser change request or response headers.

use regex::{Regex, RegexBuilder};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Block,
    Allow,
    AllowAllRequests,
    UpgradeScheme,
    Redirect(String),
    ModifyHeaders,
}

impl Action {
    /// Chrome's tie-break order at equal priority (lower wins).
    fn rank(&self) -> u8 {
        match self {
            Action::Allow => 0,
            Action::AllowAllRequests => 1,
            Action::Block => 2,
            Action::UpgradeScheme => 3,
            Action::Redirect(_) => 4,
            Action::ModifyHeaders => 5,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum RedirectSpec {
    Url(String),
    ExtensionPath(String),
    RegexSubstitution(String),
    Transform(Value),
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Block,
    Allow,
    AllowAllRequests,
    UpgradeScheme,
    Redirect(RedirectSpec),
    ModifyHeaders,
}

#[derive(Clone, Debug)]
pub struct Rule {
    pub id: i64,
    pub priority: i64,
    kind: Kind,
    url: Option<Regex>,
    request_domains: Vec<String>,
    excluded_request_domains: Vec<String>,
    initiator_domains: Vec<String>,
    excluded_initiator_domains: Vec<String>,
    resource_types: Vec<String>,
    excluded_resource_types: Vec<String>,
    methods: Vec<String>,
    excluded_methods: Vec<String>,
    domain_type: Option<String>,
    tab_ids: Vec<i64>,
    excluded_tab_ids: Vec<i64>,
}

/// The request being decided.
pub struct Request<'a> {
    pub url: &'a str,
    /// Host of the page (or frame) that made the request; `None` for top-level navigations.
    pub initiator_host: Option<&'a str>,
    /// `main_frame`, `sub_frame`, `script`, `image`, `stylesheet`, `xmlhttprequest`, ...
    pub resource_type: &'a str,
    pub method: &'a str,
    pub tab_id: i64,
}

/// A winning rule.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub rule_id: i64,
    pub priority: i64,
    pub action: Action,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum RuleError {
    #[error("rule {0}: {1}")]
    Invalid(i64, String),
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(|s| s.to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_default()
}

fn ints(v: Option<&Value>) -> Vec<i64> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default()
}

impl Rule {
    pub fn parse(v: &Value) -> Result<Rule, RuleError> {
        let id = v
            .get("id")
            .and_then(Value::as_i64)
            .ok_or(RuleError::Invalid(0, "missing id".into()))?;
        let bad = |msg: &str| RuleError::Invalid(id, msg.to_owned());
        let priority = v.get("priority").and_then(Value::as_i64).unwrap_or(1);
        let action = v.get("action").ok_or_else(|| bad("missing action"))?;
        let kind = match action.get("type").and_then(Value::as_str) {
            Some("block") => Kind::Block,
            Some("allow") => Kind::Allow,
            Some("allowAllRequests") => Kind::AllowAllRequests,
            Some("upgradeScheme") => Kind::UpgradeScheme,
            Some("modifyHeaders") => Kind::ModifyHeaders,
            Some("redirect") => {
                let r = action
                    .get("redirect")
                    .ok_or_else(|| bad("redirect without target"))?;
                Kind::Redirect(if let Some(u) = r.get("url").and_then(Value::as_str) {
                    RedirectSpec::Url(u.to_owned())
                } else if let Some(p) = r.get("extensionPath").and_then(Value::as_str) {
                    RedirectSpec::ExtensionPath(p.to_owned())
                } else if let Some(s) = r.get("regexSubstitution").and_then(Value::as_str) {
                    RedirectSpec::RegexSubstitution(s.to_owned())
                } else if let Some(t) = r.get("transform") {
                    RedirectSpec::Transform(t.clone())
                } else {
                    return Err(bad("redirect without target"));
                })
            }
            _ => return Err(bad("unknown action type")),
        };
        let c = v.get("condition").cloned().unwrap_or(Value::Null);
        let case_sensitive = c
            .get("isUrlFilterCaseSensitive")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let (url, regex_filter) = if let Some(re) = c.get("regexFilter").and_then(Value::as_str) {
            let r = RegexBuilder::new(re)
                .case_insensitive(!case_sensitive)
                .size_limit(1 << 20)
                .build()
                .map_err(|e| bad(&format!("bad regexFilter: {e}")))?;
            (Some(r), true)
        } else if let Some(filter) = c.get("urlFilter").and_then(Value::as_str) {
            let source = url_filter_regex(filter);
            let r = RegexBuilder::new(&source)
                .case_insensitive(!case_sensitive)
                .build()
                .map_err(|e| bad(&e.to_string()))?;
            (Some(r), false)
        } else {
            (None, false)
        };
        if matches!(kind, Kind::Redirect(RedirectSpec::RegexSubstitution(_))) && !regex_filter {
            return Err(bad("regexSubstitution needs regexFilter"));
        }
        // `domains`/`excludedDomains` are the older names of the initiator lists.
        let mut initiator_domains = strings(c.get("initiatorDomains"));
        initiator_domains.extend(strings(c.get("domains")));
        let mut excluded_initiator_domains = strings(c.get("excludedInitiatorDomains"));
        excluded_initiator_domains.extend(strings(c.get("excludedDomains")));
        Ok(Rule {
            id,
            priority,
            kind,
            url,
            request_domains: strings(c.get("requestDomains")),
            excluded_request_domains: strings(c.get("excludedRequestDomains")),
            initiator_domains,
            excluded_initiator_domains,
            resource_types: strings(c.get("resourceTypes")),
            excluded_resource_types: strings(c.get("excludedResourceTypes")),
            methods: strings(c.get("requestMethods")),
            excluded_methods: strings(c.get("excludedRequestMethods")),
            domain_type: c
                .get("domainType")
                .and_then(Value::as_str)
                .map(str::to_owned),
            tab_ids: ints(c.get("tabIds")),
            excluded_tab_ids: ints(c.get("excludedTabIds")),
        })
    }

    fn matches(&self, r: &Request) -> bool {
        let host = url::Url::parse(r.url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
            .unwrap_or_default();
        let initiator = r.initiator_host.map(str::to_ascii_lowercase);
        // Default resource types: everything except main_frame (Chrome's rule).
        if self.resource_types.is_empty() {
            if r.resource_type == "main_frame" && !matches!(self.kind, Kind::AllowAllRequests) {
                return false;
            }
        } else if !self.resource_types.iter().any(|t| t == r.resource_type) {
            return false;
        }
        if self
            .excluded_resource_types
            .iter()
            .any(|t| t == r.resource_type)
        {
            return false;
        }
        let method = r.method.to_ascii_lowercase();
        if !self.methods.is_empty() && !self.methods.contains(&method) {
            return false;
        }
        if self.excluded_methods.contains(&method) {
            return false;
        }
        if !self.tab_ids.is_empty() && !self.tab_ids.contains(&r.tab_id) {
            return false;
        }
        if self.excluded_tab_ids.contains(&r.tab_id) {
            return false;
        }
        if !self.request_domains.is_empty()
            && !self
                .request_domains
                .iter()
                .any(|d| domain_matches(&host, d))
        {
            return false;
        }
        if self
            .excluded_request_domains
            .iter()
            .any(|d| domain_matches(&host, d))
        {
            return false;
        }
        match &initiator {
            Some(i) => {
                if !self.initiator_domains.is_empty()
                    && !self.initiator_domains.iter().any(|d| domain_matches(i, d))
                {
                    return false;
                }
                if self
                    .excluded_initiator_domains
                    .iter()
                    .any(|d| domain_matches(i, d))
                {
                    return false;
                }
            }
            None => {
                if !self.initiator_domains.is_empty() {
                    return false;
                }
            }
        }
        if let Some(kind) = &self.domain_type {
            let third = initiator.as_deref().is_some_and(|i| !same_site(i, &host));
            if (kind == "thirdParty") != third {
                return false;
            }
        }
        self.url.as_ref().is_none_or(|re| re.is_match(r.url))
    }

    fn action(&self, r: &Request, extension_base: &str) -> Action {
        match &self.kind {
            Kind::Block => Action::Block,
            Kind::Allow => Action::Allow,
            Kind::AllowAllRequests => Action::AllowAllRequests,
            Kind::UpgradeScheme => Action::UpgradeScheme,
            Kind::ModifyHeaders => Action::ModifyHeaders,
            Kind::Redirect(spec) => Action::Redirect(match spec {
                RedirectSpec::Url(u) => u.clone(),
                RedirectSpec::ExtensionPath(p) => {
                    format!("{extension_base}{}", p.trim_start_matches('/'))
                }
                RedirectSpec::RegexSubstitution(sub) => {
                    let re = self.url.as_ref().expect("checked at parse");
                    match re.captures(r.url) {
                        Some(caps) => {
                            let mut out = String::new();
                            let mut chars = sub.chars().peekable();
                            while let Some(c) = chars.next() {
                                if c == '\\' {
                                    match chars.peek().copied() {
                                        Some(d @ '0'..='9') => {
                                            chars.next();
                                            out.push_str(
                                                caps.get(d as usize - '0' as usize)
                                                    .map_or("", |m| m.as_str()),
                                            );
                                        }
                                        Some('\\') => {
                                            chars.next();
                                            out.push('\\');
                                        }
                                        _ => out.push('\\'),
                                    }
                                } else {
                                    out.push(c);
                                }
                            }
                            out
                        }
                        None => r.url.to_owned(),
                    }
                }
                RedirectSpec::Transform(t) => transform_url(r.url, t),
            }),
        }
    }
}

/// `host` is `domain` or a subdomain of it.
fn domain_matches(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{domain}"))
}

/// Approximate "same site" without a public-suffix list: one host is the other or a
/// subdomain of the other's last two labels.
fn same_site(a: &str, b: &str) -> bool {
    let base = |h: &str| -> String {
        let labels: Vec<&str> = h.split('.').collect();
        labels[labels.len().saturating_sub(2)..].join(".")
    };
    base(a) == base(b)
}

/// `urlFilter` syntax: `||` domain anchor, `|` start or end anchor, `^` separator, `*` any.
pub fn url_filter_regex(filter: &str) -> String {
    let mut out = String::new();
    let mut rest = filter;
    if let Some(r) = rest.strip_prefix("||") {
        out.push_str("^[a-z][a-z0-9+.-]*://(?:[^/?#@]*@)?(?:[^/?#:]*\\.)?");
        rest = r;
    } else if let Some(r) = rest.strip_prefix('|') {
        out.push('^');
        rest = r;
    }
    let end_anchor = rest.ends_with('|');
    if end_anchor {
        rest = &rest[..rest.len() - 1];
    }
    for c in rest.chars() {
        match c {
            '*' => out.push_str(".*"),
            '^' => out.push_str("(?:[^a-zA-Z0-9_.%-]|$)"),
            c if "\\.+?()|[]{}$".contains(c) => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    if end_anchor {
        out.push('$');
    }
    out
}

fn transform_url(original: &str, t: &Value) -> String {
    let Ok(mut u) = url::Url::parse(original) else {
        return original.to_owned();
    };
    if let Some(s) = t.get("scheme").and_then(Value::as_str) {
        let _ = u.set_scheme(s);
    }
    if let Some(h) = t.get("host").and_then(Value::as_str) {
        let _ = u.set_host(Some(h));
    }
    if let Some(p) = t.get("port").and_then(Value::as_str) {
        let _ = u.set_port(if p.is_empty() { None } else { p.parse().ok() });
    }
    if let Some(p) = t.get("path").and_then(Value::as_str) {
        u.set_path(p);
    }
    if let Some(q) = t.get("query").and_then(Value::as_str) {
        u.set_query(if q.is_empty() || q == "?" {
            None
        } else {
            Some(q.trim_start_matches('?'))
        });
    }
    if let Some(qt) = t.get("queryTransform") {
        let remove = strings(qt.get("removeParams"));
        let mut pairs: Vec<(String, String)> = u
            .query_pairs()
            .filter(|(k, _)| !remove.contains(&k.to_ascii_lowercase()))
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        if let Some(add) = qt.get("addOrReplaceParams").and_then(Value::as_array) {
            for p in add {
                let (Some(k), Some(v)) = (
                    p.get("key").and_then(Value::as_str),
                    p.get("value").and_then(Value::as_str),
                ) else {
                    continue;
                };
                match pairs.iter_mut().find(|(pk, _)| pk == k) {
                    Some(pair) => pair.1 = v.to_owned(),
                    None => pairs.push((k.to_owned(), v.to_owned())),
                }
            }
        }
        if pairs.is_empty() {
            u.set_query(None);
        } else {
            u.query_pairs_mut().clear().extend_pairs(pairs);
        }
    }
    if let Some(f) = t.get("fragment").and_then(Value::as_str) {
        u.set_fragment(if f.is_empty() || f == "#" {
            None
        } else {
            Some(f.trim_start_matches('#'))
        });
    }
    u.to_string()
}

/// All rules of one extension (enabled static rulesets plus dynamic and session rules).
#[derive(Clone, Debug, Default)]
pub struct RuleSet {
    pub rules: Vec<Rule>,
}

impl RuleSet {
    /// Parse a rules file / dynamic rules array. Invalid rules are skipped and reported.
    pub fn parse_list(v: &Value) -> (Vec<Rule>, Vec<RuleError>) {
        let mut rules = Vec::new();
        let mut errors = Vec::new();
        for r in v.as_array().map(Vec::as_slice).unwrap_or(&[]) {
            match Rule::parse(r) {
                Ok(rule) => rules.push(rule),
                Err(e) => errors.push(e),
            }
        }
        (rules, errors)
    }

    /// The winning rule for this request, by priority, then Chrome's action order.
    pub fn evaluate(&self, r: &Request, extension_base: &str) -> Option<Outcome> {
        self.rules
            .iter()
            .filter(|rule| rule.matches(r))
            .map(|rule| (rule, rule.action(r, extension_base)))
            .min_by(|(a, aa), (b, ba)| b.priority.cmp(&a.priority).then(aa.rank().cmp(&ba.rank())))
            .map(|(rule, action)| Outcome {
                rule_id: rule.id,
                priority: rule.priority,
                action,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req<'a>(url: &'a str, initiator: Option<&'a str>, ty: &'a str) -> Request<'a> {
        Request {
            url,
            initiator_host: initiator,
            resource_type: ty,
            method: "GET",
            tab_id: 1,
        }
    }

    fn set(rules: Value) -> RuleSet {
        let (rules, errors) = RuleSet::parse_list(&rules);
        assert!(errors.is_empty(), "{errors:?}");
        RuleSet { rules }
    }

    #[test]
    fn url_filter_syntax() {
        let re = |f: &str| Regex::new(&format!("(?i){}", url_filter_regex(f))).unwrap();
        assert!(re("||ads.example^").is_match("https://ads.example/x.js"));
        assert!(re("||ads.example^").is_match("https://cdn.ads.example:8080/"));
        assert!(!re("||ads.example^").is_match("https://notads.example/"));
        assert!(!re("||ads.example^").is_match("https://ads.example.com/"));
        assert!(re("|https://a.test/").is_match("https://a.test/x"));
        assert!(!re("|https://a.test/").is_match("http://b/?u=https://a.test/"));
        assert!(re("banner*.gif|").is_match("https://x/banner_1.gif"));
        assert!(!re("banner*.gif|").is_match("https://x/banner_1.gif?x"));
    }

    #[test]
    fn priority_and_tie_break() {
        let s = set(json!([
            {"id": 1, "action": {"type": "block"}, "condition": {"urlFilter": "||ads.test^"}},
            {"id": 2, "action": {"type": "allow"}, "condition": {"urlFilter": "||ads.test/ok"}},
            {"id": 3, "priority": 5, "action": {"type": "block"}, "condition": {"urlFilter": "||ads.test/ok/forced"}}
        ]));
        let o = |u| {
            s.evaluate(&req(u, Some("site.test"), "script"), "haku-ext://x/")
                .map(|o| o.action)
        };
        assert_eq!(o("https://ads.test/a.js"), Some(Action::Block));
        assert_eq!(
            o("https://ads.test/ok.js"),
            Some(Action::Allow),
            "allow wins a tie"
        );
        assert_eq!(
            o("https://ads.test/ok/forced.js"),
            Some(Action::Block),
            "higher priority wins"
        );
        assert_eq!(o("https://other.test/"), None);
    }

    #[test]
    fn conditions() {
        let s = set(json!([
            {"id": 1, "action": {"type": "block"}, "condition": {"requestDomains": ["tracker.test"], "resourceTypes": ["image"], "domainType": "thirdParty", "excludedInitiatorDomains": ["friend.test"]}}
        ]));
        let ev = |u, i, t| s.evaluate(&req(u, i, t), "").is_some();
        assert!(ev(
            "https://px.tracker.test/p.gif",
            Some("news.test"),
            "image"
        ));
        assert!(!ev(
            "https://px.tracker.test/p.gif",
            Some("news.test"),
            "script"
        ));
        assert!(
            !ev(
                "https://px.tracker.test/p.gif",
                Some("www.tracker.test"),
                "image"
            ),
            "first party"
        );
        assert!(!ev(
            "https://px.tracker.test/p.gif",
            Some("friend.test"),
            "image"
        ));
        // Rules without resourceTypes never apply to main_frame.
        let s2 = set(
            json!([{"id": 1, "action": {"type": "block"}, "condition": {"urlFilter": "x.test"}}]),
        );
        assert!(
            s2.evaluate(&req("https://x.test/", None, "main_frame"), "")
                .is_none()
        );
    }

    #[test]
    fn redirects() {
        let s = set(json!([
            {"id": 1, "action": {"type": "redirect", "redirect": {"extensionPath": "/blank.js"}}, "condition": {"urlFilter": "||ads.test/a.js"}},
            {"id": 2, "action": {"type": "redirect", "redirect": {"regexSubstitution": "https://m.\\1/\\2"}}, "condition": {"regexFilter": "^https://www\\.([^/]+)/(.*)$", "resourceTypes": ["main_frame"]}},
            {"id": 3, "action": {"type": "redirect", "redirect": {"transform": {"queryTransform": {"removeParams": ["utm_source"]}}}}, "condition": {"urlFilter": "utm_source=", "resourceTypes": ["main_frame"]}}
        ]));
        let ev = |u, t| {
            s.evaluate(&req(u, None, t), "haku-ext://abc/")
                .map(|o| o.action)
        };
        assert_eq!(
            ev("https://ads.test/a.js", "script"),
            Some(Action::Redirect("haku-ext://abc/blank.js".into()))
        );
        assert_eq!(
            ev("https://www.site.test/page", "main_frame"),
            Some(Action::Redirect("https://m.site.test/page".into()))
        );
        assert_eq!(
            ev("https://n.test/a?utm_source=x&id=2", "main_frame"),
            Some(Action::Redirect("https://n.test/a?id=2".into()))
        );
    }

    #[test]
    fn invalid_rules_are_reported() {
        let (rules, errors) = RuleSet::parse_list(&json!([
            {"id": 1, "action": {"type": "explode"}},
            {"id": 2, "action": {"type": "block"}, "condition": {"regexFilter": "("}},
            {"id": 3, "action": {"type": "block"}}
        ]));
        assert_eq!(rules.len(), 1);
        assert_eq!(errors.len(), 2);
    }
}

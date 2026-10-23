/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Match patterns (`*://*.example.com/*`, `<all_urls>`) and globs, as used by content scripts,
//! host permissions and `tabs.query`.
//!
//! Each pattern compiles to one regular expression source that is used both here and in the
//! JavaScript that gates content scripts, so Rust and the page agree on what matches. Only
//! syntax common to Rust's `regex` and JavaScript is generated.

use regex::Regex;

/// A compiled match pattern.
#[derive(Clone, Debug)]
pub struct MatchPattern {
    pub source: String,
    regex: Regex,
}

impl MatchPattern {
    pub fn parse(pattern: &str) -> Option<MatchPattern> {
        let source = pattern_regex(pattern)?;
        let regex = Regex::new(&source).ok()?;
        Some(MatchPattern { source, regex })
    }

    pub fn matches(&self, url: &str) -> bool {
        self.regex.is_match(strip_fragment(url))
    }
}

/// A compiled glob (`include_globs`/`exclude_globs`): `*` is any run of characters, `?` is
/// one character, and the whole URL must match.
#[derive(Clone, Debug)]
pub struct Glob {
    pub source: String,
    regex: Regex,
}

impl Glob {
    pub fn parse(glob: &str) -> Option<Glob> {
        let mut source = String::from("^");
        for c in glob.chars() {
            match c {
                '*' => source.push_str(".*"),
                '?' => source.push('.'),
                c => push_escaped(&mut source, c),
            }
        }
        source.push('$');
        let regex = Regex::new(&source).ok()?;
        Some(Glob { source, regex })
    }

    pub fn matches(&self, url: &str) -> bool {
        self.regex.is_match(strip_fragment(url))
    }
}

/// The URL test for one content script entry (or `scripting.registerContentScripts` entry).
#[derive(Clone, Debug, Default)]
pub struct UrlFilter {
    pub matches: Vec<MatchPattern>,
    pub exclude_matches: Vec<MatchPattern>,
    pub include_globs: Vec<Glob>,
    pub exclude_globs: Vec<Glob>,
}

impl UrlFilter {
    pub fn new(
        matches: &[String],
        exclude_matches: &[String],
        include_globs: &[String],
        exclude_globs: &[String],
    ) -> UrlFilter {
        UrlFilter {
            matches: matches
                .iter()
                .filter_map(|p| MatchPattern::parse(p))
                .collect(),
            exclude_matches: exclude_matches
                .iter()
                .filter_map(|p| MatchPattern::parse(p))
                .collect(),
            include_globs: include_globs
                .iter()
                .filter_map(|g| Glob::parse(g))
                .collect(),
            exclude_globs: exclude_globs
                .iter()
                .filter_map(|g| Glob::parse(g))
                .collect(),
        }
    }

    pub fn matches(&self, url: &str) -> bool {
        self.matches.iter().any(|p| p.matches(url))
            && !self.exclude_matches.iter().any(|p| p.matches(url))
            && (self.include_globs.is_empty() || self.include_globs.iter().any(|g| g.matches(url)))
            && !self.exclude_globs.iter().any(|g| g.matches(url))
    }

    /// A JavaScript expression `(href) => boolean` with the same logic.
    pub fn to_js(&self) -> String {
        let list =
            |sources: Vec<&str>| serde_json::to_string(&sources).unwrap_or_else(|_| "[]".into());
        format!(
            "((m, xm, ig, xg) => {{ const t = (l, u) => l.some((s) => new RegExp(s).test(u)); \
             return (href) => {{ const u = href.split('#')[0]; return t(m, u) && !t(xm, u) && (ig.length === 0 || t(ig, u)) && !t(xg, u); }}; }})({}, {}, {}, {})",
            list(self.matches.iter().map(|p| p.source.as_str()).collect()),
            list(
                self.exclude_matches
                    .iter()
                    .map(|p| p.source.as_str())
                    .collect()
            ),
            list(
                self.include_globs
                    .iter()
                    .map(|g| g.source.as_str())
                    .collect()
            ),
            list(
                self.exclude_globs
                    .iter()
                    .map(|g| g.source.as_str())
                    .collect()
            ),
        )
    }
}

fn strip_fragment(url: &str) -> &str {
    url.split('#').next().unwrap_or(url)
}

fn push_escaped(out: &mut String, c: char) {
    if "\\.+*?()|[]{}^$/-".contains(c) {
        out.push('\\');
    }
    out.push(c);
}

/// The regular expression for a match pattern, or `None` if the pattern is invalid.
pub fn pattern_regex(pattern: &str) -> Option<String> {
    if pattern == "<all_urls>" {
        return Some("^(?:https?|wss?|ftp|file)://.*$".to_owned());
    }
    let (scheme, rest) = pattern.split_once("://")?;
    let scheme_re = match scheme {
        "*" => "(?:https?|wss?)".to_owned(),
        "http" | "https" | "ws" | "wss" | "ftp" | "file" | "urn" => scheme.to_owned(),
        _ => return None,
    };
    let i = rest.find('/')?;
    let (host, path) = (&rest[..i], &rest[i..]);
    let host_re = if scheme == "file" {
        if !host.is_empty() {
            return None;
        }
        String::new()
    } else if host == "*" {
        "[^/]*".to_owned()
    } else {
        let (any_sub, domain) = match host.strip_prefix("*.") {
            Some(d) => (true, d),
            None => (false, host),
        };
        if domain.is_empty() || domain.contains('*') || domain.contains(':') {
            return None;
        }
        let mut escaped = String::new();
        for c in domain.to_ascii_lowercase().chars() {
            push_escaped(&mut escaped, c);
        }
        let prefix = if any_sub { "(?:[^/:@]*\\.)?" } else { "" };
        format!("{prefix}{escaped}(?::[0-9]+)?")
    };
    let mut path_re = String::new();
    for c in path.chars() {
        if c == '*' {
            path_re.push_str(".*");
        } else {
            push_escaped(&mut path_re, c);
        }
    }
    Some(format!("^{scheme_re}://{host_re}{path_re}$"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str, u: &str) -> bool {
        MatchPattern::parse(p).unwrap().matches(u)
    }

    #[test]
    fn patterns() {
        assert!(m("<all_urls>", "https://example.com/x"));
        assert!(m("<all_urls>", "file:///C:/a.html"));
        assert!(!m("<all_urls>", "haku://settings/"));
        assert!(m("*://*/*", "http://a.test/"));
        assert!(!m("*://*/*", "file:///a"));
        assert!(m("https://*.example.com/*", "https://example.com/a"));
        assert!(m(
            "https://*.example.com/*",
            "https://www.example.com:8443/a?b"
        ));
        assert!(!m("https://*.example.com/*", "https://notexample.com/"));
        assert!(!m("https://*.example.com/*", "http://example.com/"));
        assert!(m(
            "https://example.com/foo*",
            "https://example.com/foobar?x=1"
        ));
        assert!(!m("https://example.com/foo*", "https://example.com/bar"));
        assert!(m("https://example.com/*", "https://example.com/#frag"));
        assert!(m("file:///*", "file:///home/a.txt"));
    }

    #[test]
    fn invalid_patterns() {
        for p in [
            "example.com",
            "https://*foo.com/*",
            "chrome://x/*",
            "https://a.com",
            "file://host/*",
        ] {
            assert!(MatchPattern::parse(p).is_none(), "{p}");
        }
    }

    #[test]
    fn filters_combine() {
        let f = UrlFilter::new(
            &["*://*/*".into()],
            &["*://*.bank.test/*".into()],
            &["*news*".into()],
            &["*?print=1".into()],
        );
        assert!(f.matches("https://news.site/a"));
        assert!(!f.matches("https://www.bank.test/news"));
        assert!(!f.matches("https://site/a"));
        assert!(!f.matches("https://news.site/a?print=1"));
        assert!(f.to_js().starts_with("((m, xm, ig, xg)"));
    }
}

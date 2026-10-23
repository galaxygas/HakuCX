/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Address bar suggestions: calculator, unit conversion, open tabs, bookmarks, history, the
//! search itself, and (when enabled) the search engine's own suggestions fetched off-thread.

use haku_core::ipc::{Event, Suggestion, SuggestionKind};
use haku_core::omnibox::{self, Target};

use super::Browser;

const MAX_LOCAL: usize = 8;

impl Browser {
    pub(crate) fn suggestions(&self, text: &str) -> Vec<Suggestion> {
        let text = text.trim();
        let mut out: Vec<Suggestion> = Vec::new();
        if text.is_empty() {
            return out;
        }
        let settings = self.settings.borrow().clone();
        let lower = text.to_lowercase();

        if let Some(value) = omnibox::calculate(text) {
            out.push(Suggestion {
                kind: SuggestionKind::Calculator,
                title: format!("= {}", omnibox::format_number(value)),
                url: String::new(),
                detail: Some(text.to_owned()),
                tab: None,
            });
        }
        if let Some(result) = omnibox::convert(text) {
            out.push(Suggestion {
                kind: SuggestionKind::Conversion,
                title: format!("= {result}"),
                url: String::new(),
                detail: Some(text.to_owned()),
                tab: None,
            });
        }

        // What pressing Enter would do comes first among navigations.
        if let Some(target) = omnibox::resolve(text, &settings.search, self.private) {
            match target {
                Target::Url(url) => out.push(Suggestion {
                    kind: SuggestionKind::Url,
                    title: url.clone(),
                    url,
                    detail: None,
                    tab: None,
                }),
                Target::Search { engine, query, url } => {
                    let name = settings
                        .search
                        .engines
                        .iter()
                        .find(|e| e.id == engine)
                        .map(|e| e.name.clone())
                        .unwrap_or(engine);
                    out.push(Suggestion {
                        kind: SuggestionKind::Search,
                        title: query,
                        url,
                        detail: Some(format!("Search {name}")),
                        tab: None,
                    });
                }
            }
        }

        if settings.tabs.search_in_tabs {
            let session = self.session.borrow();
            for tab in session
                .tabs
                .iter()
                .filter(|t| {
                    t.title.to_lowercase().contains(&lower) || t.url.to_lowercase().contains(&lower)
                })
                .take(3)
            {
                out.push(Suggestion {
                    kind: SuggestionKind::OpenTab,
                    title: if tab.title.is_empty() {
                        tab.url.clone()
                    } else {
                        tab.title.clone()
                    },
                    url: tab.url.clone(),
                    detail: Some("Switch to tab".into()),
                    tab: Some(tab.id.clone()),
                });
            }
        }

        for b in self.bookmarks.borrow().search(text, 3) {
            if let Some(url) = &b.url {
                push_unique(
                    &mut out,
                    Suggestion {
                        kind: SuggestionKind::Bookmark,
                        title: b.title.clone(),
                        url: url.clone(),
                        detail: Some(url.clone()),
                        tab: None,
                    },
                );
            }
        }

        if !self.private {
            match self.history.suggest(text, MAX_LOCAL, haku_core::now_ms()) {
                Ok(rows) => {
                    for row in rows {
                        push_unique(
                            &mut out,
                            Suggestion {
                                kind: SuggestionKind::History,
                                title: if row.title.is_empty() {
                                    row.url.clone()
                                } else {
                                    row.title.clone()
                                },
                                detail: Some(row.url.clone()),
                                url: row.url,
                                tab: None,
                            },
                        );
                    }
                }
                Err(e) => log::warn!("history suggestions: {e}"),
            }
        }
        out.truncate(MAX_LOCAL + 4);

        if settings.tabs.address_suggestions && !self.private {
            self.fetch_remote_suggestions(text, &settings);
        }
        out
    }

    /// Ask the default search engine for suggestions on a worker thread; results arrive as a
    /// second `suggestions` event for the same text (the UI merges them if still relevant).
    fn fetch_remote_suggestions(&self, text: &str, settings: &haku_core::settings::Settings) {
        let Some(engine) = settings
            .search
            .engines
            .iter()
            .find(|e| e.id == settings.search.default_engine)
            .cloned()
        else {
            return;
        };
        let Some(template) = engine.suggest_url.clone() else {
            return;
        };
        if text.len() > 200 || text.contains("://") {
            return;
        }
        let encoded: String = url::form_urlencoded::byte_serialize(text.as_bytes()).collect();
        let url = template.replace("%s", &encoded);
        let text = text.to_owned();
        let sender = self.background_sender();
        let user_agent = servo::prefs::get().user_agent.clone();
        std::thread::spawn(move || {
            let Ok(client) = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(3))
                .user_agent(user_agent)
                .build()
            else {
                return;
            };
            let Ok(response) = client.get(&url).send() else {
                return;
            };
            let Ok(body) = response.text() else { return };
            let phrases = parse_suggest_response(&body);
            if phrases.is_empty() {
                return;
            }
            sender.send(Box::new(move |b: &Browser| {
                let items: Vec<Suggestion> = phrases
                    .into_iter()
                    .take(5)
                    .map(|p| {
                        let target = omnibox::search_with(&engine, &p);
                        Suggestion {
                            kind: SuggestionKind::Search,
                            title: p,
                            url: target.url().to_owned(),
                            detail: Some(format!("{} suggestion", engine.name)),
                            tab: None,
                        }
                    })
                    .collect();
                b.emit(Event::Suggestions {
                    request: 0,
                    text,
                    items,
                });
            }));
        });
    }
}

fn push_unique(out: &mut Vec<Suggestion>, s: Suggestion) {
    if !out.iter().any(|o| o.url == s.url && !s.url.is_empty()) {
        out.push(s);
    }
}

/// Parse the two common suggestion formats: OpenSearch (`["q", ["a", "b"]]`) and DuckDuckGo's
/// list (`[{"phrase": "a"}, ...]`).
pub fn parse_suggest_response(body: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let Some(array) = value.as_array() else {
        return Vec::new();
    };
    if let Some(list) = array.get(1).and_then(|v| v.as_array()) {
        return list
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
    }
    array
        .iter()
        .filter_map(|v| v.get("phrase").and_then(|p| p.as_str()).map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggest_formats() {
        assert_eq!(
            parse_suggest_response(r#"["rust",["rust lang","rust game"]]"#),
            ["rust lang", "rust game"]
        );
        assert_eq!(
            parse_suggest_response(r#"[{"phrase":"servo"},{"phrase":"servo browser"}]"#),
            ["servo", "servo browser"]
        );
        assert!(parse_suggest_response("nope").is_empty());
    }
}

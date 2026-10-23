/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Ad, tracker and malware blocking with Brave's `adblock` engine (ABP/uBO filter syntax).
//!
//! Filter lists are downloaded from their publishers into the profile's `filters/` folder and
//! refreshed every few days. Engines are built on a background thread, one per category, so
//! "Block ads" and "Block trackers" can be toggled independently, like in Opera GX.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use adblock::Engine;
use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use content_security_policy::Destination;
use haku_core::settings::FilterList;
use log::{info, warn};

const REFRESH_AFTER_MS: i64 = 4 * 24 * 60 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Ads,
    Trackers,
    Malware,
}

pub struct Engines {
    ads: Option<Engine>,
    trackers: Option<Engine>,
    malware: Option<Engine>,
}

enum Built {
    Engines(Box<Engines>),
    ListUpdated { id: String, at: i64 },
}

/// Which kinds of blocking are turned on for a request.
#[derive(Clone, Copy, Debug)]
pub struct Enabled {
    pub ads: bool,
    pub trackers: bool,
    pub malware: bool,
}

pub struct Blocker {
    engines: std::cell::RefCell<Engines>,
    tx: Sender<Built>,
    rx: Receiver<Built>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

/// The request type names the filter syntax uses.
fn request_type(destination: Destination, main_frame: bool) -> &'static str {
    use Destination as D;
    match destination {
        D::Document if main_frame => "document",
        D::Document | D::IFrame | D::Frame => "subdocument",
        D::Script
        | D::ServiceWorker
        | D::SharedWorker
        | D::Worker
        | D::AudioWorklet
        | D::PaintWorklet => "script",
        D::Style | D::Xslt => "stylesheet",
        D::Image => "image",
        D::Font => "font",
        D::Audio | D::Video | D::Track => "media",
        D::Object | D::Embed => "object",
        D::Report => "ping",
        D::None | D::Json | D::Text => "xmlhttprequest",
        _ => "other",
    }
}

fn list_path(dir: &Path, list: &FilterList) -> PathBuf {
    let safe: String = list
        .id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    dir.join(format!("{safe}.txt"))
}

impl Blocker {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = channel();
        Self {
            engines: std::cell::RefCell::new(Engines {
                ads: None,
                trackers: None,
                malware: None,
            }),
            tx,
            rx,
            wake,
        }
    }

    /// Build engines from the cached list files (fast; no network), then refresh stale or
    /// missing lists in the background and rebuild once they arrive.
    pub fn load(&self, dir: PathBuf, lists: Vec<FilterList>, custom: String, user_agent: String) {
        let tx = self.tx.clone();
        let wake = self.wake.clone();
        std::thread::Builder::new()
            .name("filter-lists".into())
            .spawn(move || {
                let _ = tx.send(Built::Engines(Box::new(build(&dir, &lists, &custom))));
                wake();
                let now = haku_core::now_ms();
                let stale: Vec<&FilterList> = lists
                    .iter()
                    .filter(|l| l.enabled)
                    .filter(|l| {
                        !list_path(&dir, l).exists()
                            || l.last_updated.is_none_or(|t| now - t > REFRESH_AFTER_MS)
                    })
                    .collect();
                if stale.is_empty() {
                    return;
                }
                let client = match reqwest::blocking::Client::builder()
                    .timeout(Duration::from_secs(60))
                    .user_agent(user_agent)
                    .build()
                {
                    Ok(c) => c,
                    Err(e) => {
                        warn!("filter list client: {e}");
                        return;
                    }
                };
                let mut changed = false;
                for list in stale {
                    match fetch_list(&client, &list.url) {
                        Ok(text) => {
                            if haku_core::storage::write_bytes_atomic(
                                &list_path(&dir, list),
                                text.as_bytes(),
                            )
                            .is_ok()
                            {
                                info!("updated filter list {}", list.id);
                                changed = true;
                                let _ = tx.send(Built::ListUpdated {
                                    id: list.id.clone(),
                                    at: haku_core::now_ms(),
                                });
                            }
                        }
                        Err(e) => warn!("could not update filter list {}: {e}", list.id),
                    }
                }
                if changed {
                    let _ = tx.send(Built::Engines(Box::new(build(&dir, &lists, &custom))));
                }
                wake();
            })
            .expect("spawn filter thread");
    }

    /// Install engines built in the background. Returns lists that were refreshed.
    pub fn poll(&self) -> Vec<(String, i64)> {
        let mut updated = Vec::new();
        for msg in self.rx.try_iter() {
            match msg {
                Built::Engines(engines) => *self.engines.borrow_mut() = *engines,
                Built::ListUpdated { id, at } => updated.push((id, at)),
            }
        }
        updated
    }

    /// Should this request be blocked? `page_url` is the top-level page making it.
    pub fn check(
        &self,
        url: &str,
        page_url: &str,
        destination: Destination,
        main_frame: bool,
        on: Enabled,
    ) -> Option<Category> {
        let Enabled {
            ads,
            trackers,
            malware,
        } = on;
        let kind = request_type(destination, main_frame);
        let Ok(request) = Request::new(url, page_url, kind, "GET") else {
            return None;
        };
        let engines = self.engines.borrow();
        let hit = |e: &Option<Engine>| {
            e.as_ref()
                .is_some_and(|e| e.check_network_request(&request).should_block())
        };
        if malware && hit(&engines.malware) {
            return Some(Category::Malware);
        }
        // Main-frame documents are never blocked by ad/tracker lists (that is what malware
        // protection is for); blocking them would just show an error page.
        if main_frame {
            return None;
        }
        if trackers && hit(&engines.trackers) {
            return Some(Category::Trackers);
        }
        if ads && hit(&engines.ads) {
            return Some(Category::Ads);
        }
        None
    }

    /// Cosmetic filtering: CSS that hides ad elements on this page.
    /// Generic element-hiding rules (`##.ad-banner`) that match class and id names a page uses.
    /// Pages report their names because the engine cannot list every generic rule up front.
    pub fn generic_cosmetic_css(&self, url: &str, classes: &[String], ids: &[String]) -> String {
        let engines = self.engines.borrow();
        let Some(engine) = engines.ads.as_ref() else {
            return String::new();
        };
        let resources = engine.url_cosmetic_resources(url);
        if resources.generichide {
            return String::new();
        }
        let mut selectors = engine.hidden_class_id_selectors(classes, ids, &resources.exceptions);
        selectors.sort();
        hide_rules(selectors.iter())
    }

    pub fn cosmetic_css(&self, url: &str) -> String {
        let engines = self.engines.borrow();
        let Some(engine) = engines.ads.as_ref() else {
            return String::new();
        };
        let resources = engine.url_cosmetic_resources(url);
        if resources.hide_selectors.is_empty() {
            return String::new();
        }
        let mut selectors: Vec<&String> = resources.hide_selectors.iter().collect();
        selectors.sort();
        hide_rules(selectors.into_iter())
    }
}

/// `display: none` rules, chunked so one invalid selector only disables its own rule.
fn hide_rules<'a>(selectors: impl Iterator<Item = &'a String>) -> String {
    let selectors: Vec<&str> = selectors.map(String::as_str).collect();
    selectors
        .chunks(64)
        .map(|chunk| format!("{} {{ display: none !important; }}", chunk.join(", ")))
        .collect::<Vec<_>>()
        .join("\n")
}

fn fetch_list(client: &reqwest::blocking::Client, url: &str) -> Result<String, String> {
    let response = client.get(url).send().map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let text = response.text().map_err(|e| e.to_string())?;
    // A filter list is text; refuse anything that is obviously an HTML error page.
    if text.trim_start().starts_with("<!") || text.trim_start().starts_with("<html") {
        return Err("not a filter list".into());
    }
    Ok(text)
}

fn build(dir: &Path, lists: &[FilterList], custom: &str) -> Engines {
    let started = std::time::Instant::now();
    let mut sets: [(FilterSet, bool); 3] = [
        (FilterSet::new(false), false),
        (FilterSet::new(false), false),
        (FilterSet::new(false), false),
    ];
    for list in lists.iter().filter(|l| l.enabled) {
        let Ok(text) = std::fs::read_to_string(list_path(dir, list)) else {
            continue;
        };
        let slot = match list.category.as_str() {
            "trackers" => 1,
            "malware" => 2,
            _ => 0,
        };
        sets[slot].0.add_filter_list(text, ParseOptions::default());
        sets[slot].1 = true;
    }
    if !custom.trim().is_empty() {
        sets[0]
            .0
            .add_filter_list(custom.to_owned(), ParseOptions::default());
        sets[0].1 = true;
    }
    let [(ads, a), (trackers, t), (malware, m)] = sets;
    let engines = Engines {
        ads: a.then(|| Engine::new_with_filter_set(ads)),
        trackers: t.then(|| Engine::new_with_filter_set(trackers)),
        malware: m.then(|| Engine::new_with_filter_set(malware)),
    };
    info!("filter engines built in {:?}", started.elapsed());
    engines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocker_with(ads: &str, trackers: &str) -> Blocker {
        let b = Blocker::new(Arc::new(|| {}));
        let mut a = FilterSet::new(false);
        a.add_filter_list(ads.to_owned(), ParseOptions::default());
        let mut t = FilterSet::new(false);
        t.add_filter_list(trackers.to_owned(), ParseOptions::default());
        *b.engines.borrow_mut() = Engines {
            ads: Some(Engine::new_with_filter_set(a)),
            trackers: Some(Engine::new_with_filter_set(t)),
            malware: None,
        };
        b
    }

    #[test]
    fn blocks_by_category_and_toggle() {
        let b = blocker_with("||ads.example^\n##.ad-banner", "||track.example^");
        let page = "https://news.test/";
        assert_eq!(
            b.check(
                "https://ads.example/x.js",
                page,
                Destination::Script,
                false,
                Enabled {
                    ads: true,
                    trackers: true,
                    malware: true
                }
            ),
            Some(Category::Ads)
        );
        assert_eq!(
            b.check(
                "https://ads.example/x.js",
                page,
                Destination::Script,
                false,
                Enabled {
                    ads: false,
                    trackers: true,
                    malware: true
                }
            ),
            None
        );
        assert_eq!(
            b.check(
                "https://track.example/p.gif",
                page,
                Destination::Image,
                false,
                Enabled {
                    ads: true,
                    trackers: true,
                    malware: true
                }
            ),
            Some(Category::Trackers)
        );
        assert_eq!(
            b.check(
                "https://ads.example/",
                page,
                Destination::Document,
                true,
                Enabled {
                    ads: true,
                    trackers: true,
                    malware: true
                }
            ),
            None
        );
        // Generic rules apply once the page reports the class names it uses.
        assert!(!b.cosmetic_css(page).contains(".ad-banner"));
        let css = b.generic_cosmetic_css(page, &["ad-banner".into(), "article".into()], &[]);
        assert!(css.contains(".ad-banner"));
        assert!(!css.contains(".article"));
    }

    #[test]
    fn request_types() {
        assert_eq!(request_type(Destination::Document, true), "document");
        assert_eq!(request_type(Destination::IFrame, false), "subdocument");
        assert_eq!(request_type(Destination::None, false), "xmlhttprequest");
    }
}

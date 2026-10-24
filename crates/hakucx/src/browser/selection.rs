/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The search pop-up over selected text (Settings → Search engine → Search pop-up): search,
//! copy, conversions (`haku_core::selection`) and, with the AI assistant on, AI prompts.
//!
//! Currency conversion uses the European Central Bank's reference rates, fetched from
//! frankfurter.app at most once a day and only when a selection is a price (see NETWORK.md).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use haku_core::ipc::{Event, SelectionPopup};
use haku_core::selection;
use haku_core::session::TabId;
use log::warn;

use super::Browser;
use super::vault::CssRect;

const RATES_URL: &str = "https://api.frankfurter.app/latest?from=EUR";
const RATES_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Default)]
pub struct SelectionState {
    rates: Option<(Instant, HashMap<String, f64>)>,
    fetching: bool,
    /// The selection on screen, to update when rates arrive.
    shown: Option<(TabId, String, CssRect)>,
}

/// Rates against EUR from frankfurter.app's JSON (`{"base":"EUR","rates":{"USD":1.08,...}}`).
pub fn parse_rates(json: &str) -> Option<HashMap<String, f64>> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let mut rates: HashMap<String, f64> = v
        .get("rates")?
        .as_object()?
        .iter()
        .filter_map(|(k, v)| Some((k.clone(), v.as_f64().filter(|r| *r > 0.0)?)))
        .collect();
    rates.insert(v.get("base")?.as_str()?.to_owned(), 1.0);
    Some(rates)
}

/// Minutes east of UTC on this computer, now.
fn local_offset_minutes() -> i32 {
    time::UtcOffset::current_local_offset()
        .map(|o| o.whole_minutes() as i32)
        .unwrap_or(0)
}

impl Browser {
    pub(crate) fn selection_changed(&self, tab: &TabId, text: String, rect: CssRect) {
        let text: String = text.trim().chars().take(500).collect();
        if text.is_empty() || !self.settings.borrow().search.search_popup || self.private {
            self.selection.borrow_mut().shown = None;
            self.emit(Event::SelectionPopup { popup: None });
            return;
        }
        self.selection.borrow_mut().shown = Some((tab.clone(), text.clone(), rect));
        self.selection_emit(tab, &text, rect);
    }

    fn selection_emit(&self, tab: &TabId, text: &str, rect: CssRect) {
        let Some(ui_rect) = self.page_css_rect_to_ui(tab, rect) else {
            return;
        };
        let (currency, units, zones, ai) = {
            let s = self.settings.borrow();
            (
                s.search.popup_currency.clone(),
                s.search.popup_units,
                s.search.popup_time_zones,
                s.ai.enabled && s.ai.highlight_prompts,
            )
        };
        let mut conversions = Vec::new();
        if let Some((amount, from)) = selection::parse_money(text) {
            let fresh = self
                .selection
                .borrow()
                .rates
                .as_ref()
                .filter(|(at, _)| at.elapsed() < RATES_MAX_AGE)
                .map(|(_, r)| r.clone());
            match fresh {
                Some(rates) => {
                    conversions.extend(selection::convert_money(amount, from, &currency, &rates))
                }
                None => self.fetch_rates(),
            }
        }
        if units {
            conversions.extend(selection::convert_units(text));
        }
        if zones && let Some((minutes, offset)) = selection::parse_time(text) {
            conversions.push(selection::convert_time(
                minutes,
                offset,
                local_offset_minutes(),
            ));
        }
        self.emit(Event::SelectionPopup {
            popup: Some(SelectionPopup {
                tab: tab.clone(),
                rect: ui_rect,
                text: text.to_owned(),
                conversions,
                ai,
            }),
        });
    }

    fn fetch_rates(&self) {
        if std::mem::replace(&mut self.selection.borrow_mut().fetching, true) {
            return;
        }
        let sender = self.background_sender();
        std::thread::spawn(move || {
            let result = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .and_then(|c| c.get(RATES_URL).send())
                .and_then(|r| r.error_for_status())
                .and_then(|r| r.text());
            let rates = match result {
                Ok(text) => parse_rates(&text),
                Err(e) => {
                    warn!("exchange rates: {e}");
                    None
                }
            };
            sender.send(Box::new(move |b: &Browser| {
                let shown = {
                    let mut s = b.selection.borrow_mut();
                    s.fetching = false;
                    if let Some(rates) = rates {
                        s.rates = Some((Instant::now(), rates));
                    }
                    s.shown.clone()
                };
                // Show the price conversion if the same selection is still up.
                if let Some((tab, text, rect)) = shown {
                    b.selection_emit(&tab, &text, rect);
                }
            }));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_json() {
        let r = parse_rates(r#"{"amount":1.0,"base":"EUR","date":"2026-10-23","rates":{"USD":1.0832,"JPY":161.2,"BAD":-1}}"#).unwrap();
        assert_eq!(r["EUR"], 1.0);
        assert_eq!(r["USD"], 1.0832);
        assert!(!r.contains_key("BAD"));
        assert!(parse_rates("not json").is_none());
    }
}

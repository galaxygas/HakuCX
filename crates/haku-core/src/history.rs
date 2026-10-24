/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Browsing history in SQLite.
//!
//! `urls` holds one row per URL with aggregate counts; `visits` holds every visit. Address-bar
//! suggestions are ranked by a frecency score (visit count and typed count, decayed by age).

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Visit {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub visited_at: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UrlStat {
    pub url: String,
    pub title: String,
    pub visit_count: i64,
    pub typed_count: i64,
    pub last_visit: i64,
}

/// One visit as folder sync carries it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyncVisit {
    pub url: String,
    pub title: String,
    pub at: i64,
    pub transition: u8,
}

/// How a navigation was started; typed URLs weigh more in suggestions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    Link = 0,
    Typed = 1,
    Bookmark = 2,
    Reload = 3,
}

pub struct History {
    conn: Connection,
}

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS urls (
    id INTEGER PRIMARY KEY,
    url TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL DEFAULT '',
    visit_count INTEGER NOT NULL DEFAULT 0,
    typed_count INTEGER NOT NULL DEFAULT 0,
    last_visit INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS visits (
    id INTEGER PRIMARY KEY,
    url_id INTEGER NOT NULL REFERENCES urls(id) ON DELETE CASCADE,
    visited_at INTEGER NOT NULL,
    transition INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS visits_time ON visits(visited_at);
CREATE INDEX IF NOT EXISTS visits_url ON visits(url_id);
CREATE INDEX IF NOT EXISTS urls_last ON urls(last_visit);
";

impl History {
    pub fn open(path: &Path) -> rusqlite::Result<History> {
        Self::init(Connection::open(path)?)
    }

    /// In-memory history for private windows and Roguelike profiles: nothing touches disk.
    pub fn in_memory() -> rusqlite::Result<History> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<History> {
        conn.execute_batch(SCHEMA)?;
        Ok(History { conn })
    }

    /// Whether a URL belongs in history at all (no internal or data pages).
    pub fn recordable(url: &str) -> bool {
        url::Url::parse(url)
            .map(|u| matches!(u.scheme(), "http" | "https" | "file" | "ftp"))
            .unwrap_or(false)
    }

    pub fn record_visit(
        &self,
        url: &str,
        title: &str,
        transition: Transition,
        at: i64,
    ) -> rusqlite::Result<()> {
        if !Self::recordable(url) {
            return Ok(());
        }
        let typed = i64::from(transition == Transition::Typed);
        let counts = i64::from(transition != Transition::Reload);
        self.conn.execute(
            "INSERT INTO urls (url, title, visit_count, typed_count, last_visit) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(url) DO UPDATE SET
               title = CASE WHEN excluded.title <> '' THEN excluded.title ELSE urls.title END,
               visit_count = urls.visit_count + ?3,
               typed_count = urls.typed_count + ?4,
               last_visit = MAX(urls.last_visit, ?5)",
            params![url, title, counts, typed, at],
        )?;
        let url_id: i64 =
            self.conn
                .query_row("SELECT id FROM urls WHERE url = ?1", [url], |r| r.get(0))?;
        self.conn.execute(
            "INSERT INTO visits (url_id, visited_at, transition) VALUES (?1, ?2, ?3)",
            params![url_id, at, transition as i64],
        )?;
        Ok(())
    }

    pub fn update_title(&self, url: &str, title: &str) -> rusqlite::Result<()> {
        if title.is_empty() {
            return Ok(());
        }
        self.conn.execute(
            "UPDATE urls SET title = ?2 WHERE url = ?1",
            params![url, title],
        )?;
        Ok(())
    }

    /// Visits newest first, optionally filtered by text and a time range [from, to).
    pub fn query(
        &self,
        text: &str,
        from: Option<i64>,
        to: Option<i64>,
        limit: usize,
        offset: usize,
    ) -> rusqlite::Result<Vec<Visit>> {
        let like = format!("%{}%", escape_like(text.trim()));
        let mut stmt = self.conn.prepare(
            "SELECT v.id, u.url, u.title, v.visited_at FROM visits v JOIN urls u ON u.id = v.url_id
             WHERE (?1 = '%%' OR u.url LIKE ?1 ESCAPE '\\' OR u.title LIKE ?1 ESCAPE '\\')
               AND v.visited_at >= ?2 AND v.visited_at < ?3
             ORDER BY v.visited_at DESC LIMIT ?4 OFFSET ?5",
        )?;
        let rows = stmt.query_map(
            params![
                like,
                from.unwrap_or(i64::MIN),
                to.unwrap_or(i64::MAX),
                limit as i64,
                offset as i64
            ],
            |r| {
                Ok(Visit {
                    id: r.get(0)?,
                    url: r.get(1)?,
                    title: r.get(2)?,
                    visited_at: r.get(3)?,
                })
            },
        )?;
        rows.collect()
    }

    /// Address-bar suggestions: URLs whose address or title contains every word typed,
    /// ranked by frecency. `now` is the current time in ms.
    pub fn suggest(&self, text: &str, limit: usize, now: i64) -> rusqlite::Result<Vec<UrlStat>> {
        let words: Vec<String> = text
            .split_whitespace()
            .take(6)
            .map(|w| format!("%{}%", escape_like(w)))
            .collect();
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let clause = (1..=words.len())
            .map(|i| format!("(url LIKE ?{i} ESCAPE '\\' OR title LIKE ?{i} ESCAPE '\\')"))
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = format!(
            "SELECT url, title, visit_count, typed_count, last_visit FROM urls WHERE {clause}
             ORDER BY last_visit DESC LIMIT 200"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows: Vec<UrlStat> = stmt
            .query_map(rusqlite::params_from_iter(words.iter()), |r| {
                Ok(UrlStat {
                    url: r.get(0)?,
                    title: r.get(1)?,
                    visit_count: r.get(2)?,
                    typed_count: r.get(3)?,
                    last_visit: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let first = text
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_lowercase();
        rows.sort_by(|a, b| {
            frecency(b, now, &first)
                .partial_cmp(&frecency(a, now, &first))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        rows.truncate(limit);
        Ok(rows)
    }

    /// Most visited sites (for Speed Dial suggestions), one per host.
    pub fn top_sites(&self, limit: usize) -> rusqlite::Result<Vec<UrlStat>> {
        let mut stmt = self.conn.prepare(
            "SELECT url, title, visit_count, typed_count, last_visit FROM urls
             ORDER BY visit_count + 2 * typed_count DESC, last_visit DESC LIMIT 400",
        )?;
        let rows: Vec<UrlStat> = stmt
            .query_map([], |r| {
                Ok(UrlStat {
                    url: r.get(0)?,
                    title: r.get(1)?,
                    visit_count: r.get(2)?,
                    typed_count: r.get(3)?,
                    last_visit: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut seen = std::collections::HashSet::new();
        Ok(rows
            .into_iter()
            .filter(|r| {
                let host = url::Url::parse(&r.url).ok().and_then(|u| {
                    u.host_str()
                        .map(|h| h.trim_start_matches("www.").to_owned())
                });
                host.is_some_and(|h| seen.insert(h))
            })
            .take(limit)
            .collect())
    }

    /// URLs (one row each, with their row id) whose address or title contains `text` and that
    /// were last visited in [from, to), most recent first. For `chrome.history.search`.
    pub fn search_urls(
        &self,
        text: &str,
        from: i64,
        to: i64,
        limit: usize,
    ) -> rusqlite::Result<Vec<(i64, UrlStat)>> {
        let like = format!("%{}%", escape_like(text.trim()));
        let mut stmt = self.conn.prepare(
            "SELECT id, url, title, visit_count, typed_count, last_visit FROM urls
             WHERE (?1 = '%%' OR url LIKE ?1 ESCAPE '\\' OR title LIKE ?1 ESCAPE '\\')
               AND last_visit >= ?2 AND last_visit < ?3
             ORDER BY last_visit DESC LIMIT ?4",
        )?;
        let rows = stmt.query_map(params![like, from, to, limit as i64], |r| {
            Ok((
                r.get(0)?,
                UrlStat {
                    url: r.get(1)?,
                    title: r.get(2)?,
                    visit_count: r.get(3)?,
                    typed_count: r.get(4)?,
                    last_visit: r.get(5)?,
                },
            ))
        })?;
        rows.collect()
    }

    /// Every visit to one URL, oldest first: (url row id, visit id, time, transition).
    pub fn visits_for_url(&self, url: &str) -> rusqlite::Result<Vec<(i64, i64, i64, Transition)>> {
        let mut stmt = self.conn.prepare(
            "SELECT u.id, v.id, v.visited_at, v.transition FROM visits v JOIN urls u ON u.id = v.url_id
             WHERE u.url = ?1 ORDER BY v.visited_at",
        )?;
        let rows = stmt.query_map([url], |r| {
            let transition = match r.get::<_, i64>(3)? {
                1 => Transition::Typed,
                2 => Transition::Bookmark,
                3 => Transition::Reload,
                _ => Transition::Link,
            };
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, transition))
        })?;
        rows.collect()
    }

    /// Visits since `since` (ms), newest first, for folder sync.
    pub fn visits_since(&self, since: i64, limit: usize) -> rusqlite::Result<Vec<SyncVisit>> {
        let mut stmt = self.conn.prepare(
            "SELECT u.url, u.title, v.visited_at, v.transition FROM visits v JOIN urls u ON u.id = v.url_id
             WHERE v.visited_at >= ?1 ORDER BY v.visited_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![since, limit as i64], |r| {
            Ok(SyncVisit {
                url: r.get(0)?,
                title: r.get(1)?,
                at: r.get(2)?,
                transition: r.get::<_, i64>(3)? as u8,
            })
        })?;
        rows.collect()
    }

    /// Add visits from another device, skipping ones already here (same address and time).
    /// Returns how many were added.
    pub fn import_visits(&self, visits: &[SyncVisit]) -> rusqlite::Result<usize> {
        let mut added = 0;
        for v in visits {
            let exists: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM visits v JOIN urls u ON u.id = v.url_id
                 WHERE u.url = ?1 AND v.visited_at = ?2)",
                params![v.url, v.at],
                |r| r.get(0),
            )?;
            if exists {
                continue;
            }
            let transition = match v.transition {
                1 => Transition::Typed,
                2 => Transition::Bookmark,
                3 => Transition::Reload,
                _ => Transition::Link,
            };
            self.record_visit(&v.url, &v.title, transition, v.at)?;
            added += 1;
        }
        Ok(added)
    }

    pub fn delete_visit(&self, id: i64) -> rusqlite::Result<()> {
        let url_id: Option<i64> = self
            .conn
            .query_row("SELECT url_id FROM visits WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        self.conn
            .execute("DELETE FROM visits WHERE id = ?1", [id])?;
        if let Some(url_id) = url_id {
            self.drop_unvisited(Some(url_id))?;
        }
        Ok(())
    }

    pub fn delete_url(&self, url: &str) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM urls WHERE url = ?1", [url])?;
        Ok(())
    }

    /// Delete visits in [from, to) and URLs left with no visits. Returns visits removed.
    pub fn delete_range(&self, from: i64, to: i64) -> rusqlite::Result<usize> {
        let n = self.conn.execute(
            "DELETE FROM visits WHERE visited_at >= ?1 AND visited_at < ?2",
            params![from, to],
        )?;
        self.drop_unvisited(None)?;
        self.recount()?;
        Ok(n)
    }

    /// Fake My History (a Playground experiment): replace all history with the wholesome
    /// pages in [`WHOLESOME`], visited over the last two weeks before `now`.
    pub fn redeem(&self, now: i64) -> rusqlite::Result<usize> {
        self.clear()?;
        const DAY: i64 = 86_400_000;
        let n = WHOLESOME.len() as i64;
        for (i, (url, title)) in WHOLESOME.iter().enumerate() {
            // Spread over 14 days, newest first, at different times of day.
            let i = i as i64;
            let at = now - i * 14 * DAY / n - (i % 5) * 3_600_000;
            self.record_visit(url, title, Transition::Link, at)?;
        }
        Ok(WHOLESOME.len())
    }

    pub fn clear(&self) -> rusqlite::Result<()> {
        self.conn
            .execute_batch("DELETE FROM visits; DELETE FROM urls;")
    }

    pub fn visit_count(&self) -> rusqlite::Result<i64> {
        self.conn
            .query_row("SELECT COUNT(*) FROM visits", [], |r| r.get(0))
    }

    fn drop_unvisited(&self, url_id: Option<i64>) -> rusqlite::Result<()> {
        match url_id {
            Some(id) => self.conn.execute(
                "DELETE FROM urls WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM visits WHERE url_id = ?1)",
                [id],
            )?,
            None => self.conn.execute(
                "DELETE FROM urls WHERE NOT EXISTS (SELECT 1 FROM visits WHERE url_id = urls.id)",
                [],
            )?,
        };
        Ok(())
    }

    fn recount(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
            "UPDATE urls SET
               visit_count = (SELECT COUNT(*) FROM visits WHERE url_id = urls.id AND transition <> 3),
               typed_count = (SELECT COUNT(*) FROM visits WHERE url_id = urls.id AND transition = 1),
               last_visit = COALESCE((SELECT MAX(visited_at) FROM visits WHERE url_id = urls.id), 0);",
        )
    }
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Frecency: typed visits count triple, recency decays with a 14-day half-life, and URLs whose
/// host starts with what the user typed get a strong boost (so "git" ranks github.com first).
fn frecency(s: &UrlStat, now: i64, first_word: &str) -> f64 {
    let age_days = ((now - s.last_visit).max(0) as f64) / 86_400_000.0;
    let decay = 0.5f64.powf(age_days / 14.0);
    let mut score = (s.visit_count as f64 + 3.0 * s.typed_count as f64 + 1.0) * (0.25 + decay);
    if let Ok(u) = url::Url::parse(&s.url)
        && let Some(host) = u.host_str()
    {
        let host = host.trim_start_matches("www.");
        if !first_word.is_empty() && host.starts_with(first_word) {
            score *= 4.0;
            if u.path() == "/" {
                score *= 1.5;
            }
        }
    }
    score
}

/// What Fake My History leaves behind: real pages a model citizen would visit.
pub const WHOLESOME: &[(&str, &str)] = &[
    (
        "https://www.redcross.org/give-blood.html",
        "Give Blood | American Red Cross",
    ),
    ("https://en.wikipedia.org/wiki/Cat", "Cat - Wikipedia"),
    (
        "https://www.khanacademy.org/math/algebra",
        "Algebra 1 | Khan Academy",
    ),
    (
        "https://www.gutenberg.org/ebooks/1342",
        "Pride and Prejudice by Jane Austen | Project Gutenberg",
    ),
    (
        "https://en.wikipedia.org/wiki/Origami",
        "Origami - Wikipedia",
    ),
    ("https://www.nasa.gov/", "NASA"),
    (
        "https://en.wikipedia.org/wiki/Birdwatching",
        "Birdwatching - Wikipedia",
    ),
    ("https://www.duolingo.com/", "Duolingo"),
    (
        "https://en.wikipedia.org/wiki/Vegetable_soup",
        "Vegetable soup - Wikipedia",
    ),
    (
        "https://en.wikipedia.org/wiki/Houseplant",
        "Houseplant - Wikipedia",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400_000;

    #[test]
    fn redeem_replaces_everything() {
        let h = History::in_memory().unwrap();
        h.record_visit(
            "https://example.com/secret",
            "Secret",
            Transition::Typed,
            1_000,
        )
        .unwrap();
        let now = 100 * 86_400_000;
        assert_eq!(h.redeem(now).unwrap(), WHOLESOME.len());
        let all = h.query("", None, None, 100, 0).unwrap();
        assert_eq!(all.len(), WHOLESOME.len());
        assert!(all.iter().all(|v| v.url != "https://example.com/secret"));
        assert!(
            all.iter()
                .all(|v| v.visited_at <= now && v.visited_at > now - 15 * 86_400_000)
        );
    }

    #[test]
    fn records_and_queries() {
        let h = History::in_memory().unwrap();
        h.record_visit("https://servo.org/", "Servo", Transition::Typed, 1_000)
            .unwrap();
        h.record_visit("https://servo.org/", "", Transition::Link, 2_000)
            .unwrap();
        h.record_visit("haku://start/", "Start", Transition::Typed, 3_000)
            .unwrap();
        h.record_visit("https://example.com/", "Example", Transition::Link, 4_000)
            .unwrap();
        let all = h.query("", None, None, 10, 0).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].url, "https://example.com/");
        assert_eq!(all[1].title, "Servo");
        let found = h.query("serv", None, None, 10, 0).unwrap();
        assert_eq!(found.len(), 2);
        let ranged = h.query("", Some(1_500), Some(4_000), 10, 0).unwrap();
        assert_eq!(ranged.len(), 1);
    }

    #[test]
    fn url_search_and_visits_for_extensions() {
        let h = History::in_memory().unwrap();
        h.record_visit("https://servo.org/", "Servo", Transition::Typed, 1_000)
            .unwrap();
        h.record_visit("https://servo.org/", "", Transition::Reload, 2_000)
            .unwrap();
        h.record_visit("https://example.com/", "Example", Transition::Link, 3_000)
            .unwrap();
        let urls = h.search_urls("", 0, i64::MAX, 10).unwrap();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0].1.url, "https://example.com/");
        assert_eq!(urls[1].1.visit_count, 1);
        assert_eq!(h.search_urls("servo", 0, 1_500, 10).unwrap().len(), 0);
        let visits = h.visits_for_url("https://servo.org/").unwrap();
        assert_eq!(visits.len(), 2);
        assert_eq!(visits[0].3, Transition::Typed);
        assert_eq!(visits[1].3, Transition::Reload);
        assert_eq!(visits[0].0, urls[1].0);
    }

    #[test]
    fn sync_export_and_import() {
        let a = History::in_memory().unwrap();
        a.record_visit("https://a.test/", "A", Transition::Typed, 100)
            .unwrap();
        a.record_visit("https://b.test/", "B", Transition::Link, 200)
            .unwrap();
        let visits = a.visits_since(150, 10).unwrap();
        assert_eq!(visits.len(), 1);
        let all = a.visits_since(0, 10).unwrap();
        let b = History::in_memory().unwrap();
        b.record_visit("https://a.test/", "A", Transition::Typed, 100)
            .unwrap();
        assert_eq!(b.import_visits(&all).unwrap(), 1);
        assert_eq!(b.import_visits(&all).unwrap(), 0);
        assert_eq!(b.visit_count().unwrap(), 2);
        assert_eq!(b.suggest("a.test", 5, 1000).unwrap()[0].typed_count, 1);
    }

    #[test]
    fn like_wildcards_are_literal() {
        let h = History::in_memory().unwrap();
        h.record_visit("https://a.test/100%25", "a_b", Transition::Link, 1)
            .unwrap();
        h.record_visit("https://b.test/", "ab", Transition::Link, 2)
            .unwrap();
        assert_eq!(h.query("a_b", None, None, 10, 0).unwrap().len(), 1);
        assert_eq!(h.query("%", None, None, 10, 0).unwrap().len(), 1);
    }

    #[test]
    fn suggestions_prefer_typed_host_prefix() {
        let h = History::in_memory().unwrap();
        let now = 100 * DAY;
        for i in 0..5 {
            h.record_visit(
                "https://blog.example/git-tips",
                "Git tips",
                Transition::Link,
                now - i,
            )
            .unwrap();
        }
        h.record_visit(
            "https://github.com/",
            "GitHub",
            Transition::Typed,
            now - DAY,
        )
        .unwrap();
        let s = h.suggest("git", 5, now).unwrap();
        assert_eq!(s[0].url, "https://github.com/");
        assert_eq!(s.len(), 2);
        assert!(h.suggest("   ", 5, now).unwrap().is_empty());
    }

    #[test]
    fn delete_range_recounts_and_drops_orphans() {
        let h = History::in_memory().unwrap();
        h.record_visit("https://a.test/", "A", Transition::Link, 10)
            .unwrap();
        h.record_visit("https://a.test/", "A", Transition::Link, 20)
            .unwrap();
        h.record_visit("https://b.test/", "B", Transition::Link, 30)
            .unwrap();
        assert_eq!(h.delete_range(15, 40).unwrap(), 2);
        let top = h.top_sites(10).unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].visit_count, 1);
        assert_eq!(top[0].last_visit, 10);
    }

    #[test]
    fn delete_single_visit_and_clear() {
        let h = History::in_memory().unwrap();
        h.record_visit("https://a.test/", "A", Transition::Link, 10)
            .unwrap();
        let v = h.query("", None, None, 1, 0).unwrap()[0].id;
        h.delete_visit(v).unwrap();
        assert!(h.top_sites(5).unwrap().is_empty());
        h.record_visit("https://a.test/", "A", Transition::Link, 10)
            .unwrap();
        h.clear().unwrap();
        assert_eq!(h.visit_count().unwrap(), 0);
    }

    #[test]
    fn top_sites_one_per_host() {
        let h = History::in_memory().unwrap();
        h.record_visit("https://www.a.test/x", "A1", Transition::Link, 1)
            .unwrap();
        h.record_visit("https://a.test/y", "A2", Transition::Link, 2)
            .unwrap();
        h.record_visit("https://b.test/", "B", Transition::Link, 3)
            .unwrap();
        assert_eq!(h.top_sites(10).unwrap().len(), 2);
    }

    #[test]
    fn file_backed_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite");
        {
            let h = History::open(&path).unwrap();
            h.record_visit("https://a.test/", "A", Transition::Link, 1)
                .unwrap();
        }
        let h = History::open(&path).unwrap();
        assert_eq!(h.visit_count().unwrap(), 1);
    }
}

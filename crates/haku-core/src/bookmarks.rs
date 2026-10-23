/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Bookmarks, including the Speed Dial (a bookmark folder, like in Opera).
//!
//! Four fixed roots: the bookmarks bar, Speed Dial, other bookmarks, and trash. Deleting moves
//! a node to the trash; deleting from the trash removes it for good.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const ROOT_BAR: &str = "bar";
pub const ROOT_SPEED_DIAL: &str = "speed-dial";
pub const ROOT_OTHER: &str = "other";
pub const ROOT_TRASH: &str = "trash";
pub const ROOTS: [&str; 4] = [ROOT_BAR, ROOT_SPEED_DIAL, ROOT_OTHER, ROOT_TRASH];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BookmarkNode {
    pub id: String,
    /// `None` only for the four roots.
    pub parent: Option<String>,
    pub title: String,
    /// `None` for folders.
    pub url: Option<String>,
    pub added: i64,
    /// Speed Dial tile accent color (`#rrggbb`), optional.
    #[serde(default)]
    pub color: Option<String>,
    /// Where it lived before going to the trash, so it can be restored.
    #[serde(default)]
    pub trashed_from: Option<String>,
}

impl BookmarkNode {
    pub fn is_folder(&self) -> bool {
        self.url.is_none()
    }
}

/// All nodes in a flat list; sibling order is list order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Bookmarks {
    pub nodes: Vec<BookmarkNode>,
}

impl Default for Bookmarks {
    fn default() -> Self {
        let mut b = Bookmarks { nodes: Vec::new() };
        b.ensure_roots();
        // Starter Speed Dial; the user can edit or delete all of these.
        for (title, url, color) in [
            ("Twitch", "https://www.twitch.tv/", "#9146ff"),
            ("Discord", "https://discord.com/app", "#5865f2"),
            ("YouTube", "https://www.youtube.com/", "#ff0033"),
            ("Reddit", "https://www.reddit.com/", "#ff4500"),
            ("Wikipedia", "https://www.wikipedia.org/", "#636466"),
            ("GitHub", "https://github.com/", "#24292f"),
            ("Steam", "https://store.steampowered.com/", "#1b2838"),
            ("Servo", "https://servo.org/", "#2c9f6f"),
        ] {
            let id = b.add(ROOT_SPEED_DIAL, title, Some(url), None).expect("root exists");
            b.set_color(&id, Some(color.into()));
        }
        b
    }
}

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn root_title(id: &str) -> &'static str {
    match id {
        ROOT_BAR => "Bookmarks bar",
        ROOT_SPEED_DIAL => "Speed Dial",
        ROOT_OTHER => "Other bookmarks",
        _ => "Trash",
    }
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum BookmarkError {
    #[error("no such bookmark or folder")]
    NotFound,
    #[error("the parent is not a folder")]
    NotAFolder,
    #[error("a folder cannot be moved into itself")]
    Cycle,
    #[error("the built-in folders cannot be changed")]
    Root,
    #[error("only http(s), ftp, file and haku URLs can be bookmarked")]
    BadUrl,
}

impl Bookmarks {
    /// Make sure all roots exist (repairs damaged files).
    pub fn ensure_roots(&mut self) {
        for (i, root) in ROOTS.iter().enumerate() {
            if !self.nodes.iter().any(|n| n.id == *root) {
                self.nodes.insert(
                    i.min(self.nodes.len()),
                    BookmarkNode {
                        id: (*root).into(),
                        parent: None,
                        title: root_title(root).into(),
                        url: None,
                        added: 0,
                        color: None,
                        trashed_from: None,
                    },
                );
            }
        }
        // Orphans (parent missing) go to "other bookmarks".
        let ids: HashSet<String> = self.nodes.iter().map(|n| n.id.clone()).collect();
        for node in self.nodes.iter_mut() {
            if let Some(parent) = &node.parent
                && !ids.contains(parent)
            {
                node.parent = Some(ROOT_OTHER.into());
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&BookmarkNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    fn get_mut(&mut self, id: &str) -> Option<&mut BookmarkNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    pub fn children(&self, parent: &str) -> Vec<&BookmarkNode> {
        self.nodes
            .iter()
            .filter(|n| n.parent.as_deref() == Some(parent))
            .collect()
    }

    fn is_descendant(&self, node: &str, ancestor: &str) -> bool {
        let mut cur = self.get(node).and_then(|n| n.parent.clone());
        while let Some(id) = cur {
            if id == ancestor {
                return true;
            }
            cur = self.get(&id).and_then(|n| n.parent.clone());
        }
        false
    }

    fn valid_url(url: &str) -> bool {
        url::Url::parse(url)
            .map(|u| matches!(u.scheme(), "http" | "https" | "ftp" | "file" | "haku"))
            .unwrap_or(false)
    }

    /// Add a bookmark (`url = Some`) or folder (`url = None`) at the end of `parent`, or at
    /// `index` among its siblings.
    pub fn add(
        &mut self,
        parent: &str,
        title: &str,
        url: Option<&str>,
        index: Option<usize>,
    ) -> Result<String, BookmarkError> {
        let p = self.get(parent).ok_or(BookmarkError::NotFound)?;
        if !p.is_folder() {
            return Err(BookmarkError::NotAFolder);
        }
        if let Some(url) = url
            && !Self::valid_url(url)
        {
            return Err(BookmarkError::BadUrl);
        }
        let node = BookmarkNode {
            id: new_id(),
            parent: Some(parent.to_owned()),
            title: title.trim().to_owned(),
            url: url.map(str::to_owned),
            added: crate::now_ms(),
            color: None,
            trashed_from: None,
        };
        let id = node.id.clone();
        self.nodes.push(node);
        if let Some(index) = index {
            self.reorder(&id, index);
        }
        Ok(id)
    }

    pub fn rename(&mut self, id: &str, title: &str, url: Option<&str>) -> Result<(), BookmarkError> {
        if ROOTS.contains(&id) {
            return Err(BookmarkError::Root);
        }
        if let Some(url) = url
            && !Self::valid_url(url)
        {
            return Err(BookmarkError::BadUrl);
        }
        let node = self.get_mut(id).ok_or(BookmarkError::NotFound)?;
        node.title = title.trim().to_owned();
        if node.url.is_some()
            && let Some(url) = url
        {
            node.url = Some(url.to_owned());
        }
        Ok(())
    }

    pub fn set_color(&mut self, id: &str, color: Option<String>) {
        if let Some(n) = self.get_mut(id) {
            n.color = color;
        }
    }

    /// Move a node into `parent` at sibling position `index` (end if `None`).
    pub fn move_to(&mut self, id: &str, parent: &str, index: Option<usize>) -> Result<(), BookmarkError> {
        if ROOTS.contains(&id) {
            return Err(BookmarkError::Root);
        }
        let target = self.get(parent).ok_or(BookmarkError::NotFound)?;
        if !target.is_folder() {
            return Err(BookmarkError::NotAFolder);
        }
        if id == parent || self.is_descendant(parent, id) {
            return Err(BookmarkError::Cycle);
        }
        let node = self.get_mut(id).ok_or(BookmarkError::NotFound)?;
        node.parent = Some(parent.to_owned());
        if parent != ROOT_TRASH {
            node.trashed_from = None;
        }
        let pos = self.nodes.iter().position(|n| n.id == id).expect("exists");
        let node = self.nodes.remove(pos);
        self.nodes.push(node);
        self.reorder(id, index.unwrap_or(usize::MAX));
        Ok(())
    }

    /// Place `id` at sibling position `index` within its current parent.
    fn reorder(&mut self, id: &str, index: usize) {
        let Some(parent) = self.get(id).and_then(|n| n.parent.clone()) else { return };
        let pos = self.nodes.iter().position(|n| n.id == id).expect("exists");
        let node = self.nodes.remove(pos);
        let sibling_positions: Vec<usize> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.parent.as_deref() == Some(parent.as_str()))
            .map(|(i, _)| i)
            .collect();
        let insert_at = match sibling_positions.get(index) {
            Some(&i) => i,
            None => sibling_positions.last().map(|&i| i + 1).unwrap_or(self.nodes.len()),
        };
        self.nodes.insert(insert_at, node);
    }

    /// Delete: first to the trash, then (from the trash) permanently with all descendants.
    pub fn delete(&mut self, id: &str) -> Result<(), BookmarkError> {
        if ROOTS.contains(&id) {
            return Err(BookmarkError::Root);
        }
        let node = self.get(id).ok_or(BookmarkError::NotFound)?.clone();
        if node.parent.as_deref() == Some(ROOT_TRASH) || self.is_descendant(id, ROOT_TRASH) {
            let mut doomed = HashSet::from([id.to_owned()]);
            loop {
                let before = doomed.len();
                for n in &self.nodes {
                    if n.parent.as_ref().is_some_and(|p| doomed.contains(p)) {
                        doomed.insert(n.id.clone());
                    }
                }
                if doomed.len() == before {
                    break;
                }
            }
            self.nodes.retain(|n| !doomed.contains(&n.id));
        } else {
            let from = node.parent.clone();
            self.move_to(id, ROOT_TRASH, None)?;
            if let Some(n) = self.get_mut(id) {
                n.trashed_from = from;
            }
        }
        Ok(())
    }

    pub fn restore(&mut self, id: &str) -> Result<(), BookmarkError> {
        let from = self
            .get(id)
            .ok_or(BookmarkError::NotFound)?
            .trashed_from
            .clone()
            .filter(|p| self.get(p).is_some_and(|n| n.is_folder()))
            .unwrap_or_else(|| ROOT_OTHER.to_owned());
        self.move_to(id, &from, None)
    }

    pub fn empty_trash(&mut self) {
        let ids: Vec<String> = self.children(ROOT_TRASH).iter().map(|n| n.id.clone()).collect();
        for id in ids {
            let _ = self.delete(&id);
        }
    }

    /// Bookmarks (not in the trash) whose URL matches exactly.
    pub fn find_url(&self, url: &str) -> Vec<&BookmarkNode> {
        self.nodes
            .iter()
            .filter(|n| n.url.as_deref() == Some(url) && !self.is_descendant(&n.id, ROOT_TRASH))
            .collect()
    }

    /// Case-insensitive search over titles and URLs, excluding the trash.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&BookmarkNode> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        self.nodes
            .iter()
            .filter(|n| n.url.is_some() && !self.is_descendant(&n.id, ROOT_TRASH))
            .filter(|n| {
                n.title.to_lowercase().contains(&q)
                    || n.url.as_deref().is_some_and(|u| u.to_lowercase().contains(&q))
            })
            .take(limit)
            .collect()
    }

    // ------------------------------------------------------------------ Netscape format

    /// Import a Netscape bookmark file (what every browser exports) into a new folder under
    /// `parent`. Returns the number of bookmarks imported.
    pub fn import_netscape(&mut self, html: &str, parent: &str, folder_name: &str) -> Result<usize, BookmarkError> {
        let root = self.add(parent, folder_name, None, None)?;
        let mut stack = vec![root.clone()];
        let mut pending_folder: Option<String> = None;
        let mut count = 0;
        let lower = html.to_ascii_lowercase();
        let mut i = 0;
        while let Some(rel) = lower[i..].find('<') {
            let start = i + rel;
            let Some(end_rel) = lower[start..].find('>') else { break };
            let end = start + end_rel;
            let tag = &lower[start + 1..end];
            let raw_tag = &html[start + 1..end];
            i = end + 1;
            if tag.starts_with("h3") {
                let close = lower[i..].find("</h3").map(|p| i + p).unwrap_or(i);
                pending_folder = Some(decode_text(&html[i..close]));
                i = close;
            } else if tag.starts_with("dl") {
                let name = pending_folder.take();
                let folder = match name {
                    Some(name) => {
                        let parent_id = stack.last().expect("non-empty").clone();
                        self.add(&parent_id, &name, None, None)?
                    },
                    // The outermost <DL> belongs to the import folder itself.
                    None if stack.len() == 1 && count == 0 => root.clone(),
                    None => stack.last().expect("non-empty").clone(),
                };
                stack.push(folder);
            } else if tag.starts_with("/dl") {
                if stack.len() > 1 {
                    stack.pop();
                }
            } else if tag.starts_with("a ") || tag == "a" {
                let close = lower[i..].find("</a").map(|p| i + p).unwrap_or(i);
                let title = decode_text(&html[i..close]);
                i = close;
                if let Some(href) = attr(raw_tag, "href") {
                    let parent_id = stack.last().expect("non-empty").clone();
                    if self.add(&parent_id, &title, Some(&href), None).is_ok() {
                        count += 1;
                    }
                }
            }
        }
        Ok(count)
    }

    /// Export everything except the trash as a Netscape bookmark file.
    pub fn export_netscape(&self) -> String {
        let mut out = String::from(
            "<!DOCTYPE NETSCAPE-Bookmark-file-1>\n<!-- This is an automatically generated file. -->\n\
             <META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n\
             <TITLE>Bookmarks</TITLE>\n<H1>Bookmarks</H1>\n<DL><p>\n",
        );
        for root in [ROOT_BAR, ROOT_SPEED_DIAL, ROOT_OTHER] {
            self.export_folder(root, 1, &mut out);
        }
        out.push_str("</DL><p>\n");
        out
    }

    fn export_folder(&self, id: &str, depth: usize, out: &mut String) {
        let indent = "    ".repeat(depth);
        let Some(node) = self.get(id) else { return };
        let extra = if id == ROOT_BAR { " PERSONAL_TOOLBAR_FOLDER=\"true\"" } else { "" };
        out.push_str(&format!(
            "{indent}<DT><H3 ADD_DATE=\"{}\"{extra}>{}</H3>\n{indent}<DL><p>\n",
            node.added / 1000,
            html_escape::encode_text(&node.title)
        ));
        for child in self.children(id) {
            if child.is_folder() {
                self.export_folder(&child.id, depth + 1, out);
            } else if let Some(url) = &child.url {
                out.push_str(&format!(
                    "{indent}    <DT><A HREF=\"{}\" ADD_DATE=\"{}\">{}</A>\n",
                    html_escape::encode_double_quoted_attribute(url),
                    child.added / 1000,
                    html_escape::encode_text(&child.title)
                ));
            }
        }
        out.push_str(&format!("{indent}</DL><p>\n"));
    }
}

fn decode_text(s: &str) -> String {
    let no_tags: String = {
        let mut out = String::new();
        let mut in_tag = false;
        for c in s.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => out.push(c),
                _ => {},
            }
        }
        out
    };
    html_escape::decode_html_entities(no_tags.trim()).into_owned()
}

/// Read an attribute from a raw tag body (`A HREF="..." ADD_DATE="..."`), case-insensitively.
fn attr(raw_tag: &str, name: &str) -> Option<String> {
    let lower = raw_tag.to_ascii_lowercase();
    let mut search = 0;
    while let Some(rel) = lower[search..].find(name) {
        let at = search + rel;
        search = at + name.len();
        let before_ok = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = lower[at + name.len()..].trim_start();
        if !before_ok || !rest.starts_with('=') {
            continue;
        }
        let value_start = raw_tag.len() - rest.len() + 1;
        let value = raw_tag[value_start..].trim_start();
        let (quote, body) = match value.chars().next() {
            Some(q @ ('"' | '\'')) => (Some(q), &value[1..]),
            _ => (None, value),
        };
        let end = match quote {
            Some(q) => body.find(q).unwrap_or(body.len()),
            None => body.find(char::is_whitespace).unwrap_or(body.len()),
        };
        return Some(html_escape::decode_html_entities(&body[..end]).into_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> Bookmarks {
        let mut b = Bookmarks { nodes: Vec::new() };
        b.ensure_roots();
        b
    }

    #[test]
    fn default_has_roots_and_speed_dial() {
        let b = Bookmarks::default();
        for r in ROOTS {
            assert!(b.get(r).is_some());
        }
        assert_eq!(b.children(ROOT_SPEED_DIAL).len(), 8);
    }

    #[test]
    fn add_rejects_bad_urls_and_non_folders() {
        let mut b = empty();
        assert_eq!(b.add(ROOT_BAR, "x", Some("javascript:alert(1)"), None), Err(BookmarkError::BadUrl));
        let id = b.add(ROOT_BAR, "x", Some("https://x.test/"), None).unwrap();
        assert_eq!(b.add(&id, "y", Some("https://y.test/"), None), Err(BookmarkError::NotAFolder));
    }

    #[test]
    fn ordering_and_moves() {
        let mut b = empty();
        let a = b.add(ROOT_BAR, "a", Some("https://a.test/"), None).unwrap();
        let c = b.add(ROOT_BAR, "c", Some("https://c.test/"), None).unwrap();
        let bb = b.add(ROOT_BAR, "b", Some("https://b.test/"), Some(1)).unwrap();
        let titles: Vec<_> = b.children(ROOT_BAR).iter().map(|n| n.title.clone()).collect();
        assert_eq!(titles, ["a", "b", "c"]);
        b.move_to(&c, ROOT_BAR, Some(0)).unwrap();
        let ids: Vec<_> = b.children(ROOT_BAR).iter().map(|n| n.id.clone()).collect();
        assert_eq!(ids, [c.clone(), a, bb]);
        let folder = b.add(ROOT_OTHER, "f", None, None).unwrap();
        let sub = b.add(&folder, "sub", None, None).unwrap();
        assert_eq!(b.move_to(&folder, &sub, None), Err(BookmarkError::Cycle));
        assert_eq!(b.move_to(ROOT_BAR, ROOT_OTHER, None), Err(BookmarkError::Root));
    }

    #[test]
    fn trash_restore_and_permanent_delete() {
        let mut b = empty();
        let f = b.add(ROOT_BAR, "f", None, None).unwrap();
        let inner = b.add(&f, "i", Some("https://i.test/"), None).unwrap();
        b.delete(&f).unwrap();
        assert_eq!(b.get(&f).unwrap().parent.as_deref(), Some(ROOT_TRASH));
        assert!(b.find_url("https://i.test/").is_empty());
        b.restore(&f).unwrap();
        assert_eq!(b.get(&f).unwrap().parent.as_deref(), Some(ROOT_BAR));
        b.delete(&f).unwrap();
        b.empty_trash();
        assert!(b.get(&f).is_none());
        assert!(b.get(&inner).is_none());
    }

    #[test]
    fn netscape_round_trip() {
        let html = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=UTF-8">
<TITLE>Bookmarks</TITLE>
<H1>Bookmarks</H1>
<DL><p>
    <DT><H3 ADD_DATE="1" PERSONAL_TOOLBAR_FOLDER="true">Bookmarks bar</H3>
    <DL><p>
        <DT><A HREF="https://servo.org/" ADD_DATE="2">Servo &amp; friends</A>
        <DT><H3>Games</H3>
        <DL><p>
            <DT><A href='https://store.steampowered.com/'>Steam</A>
        </DL><p>
    </DL><p>
    <DT><A HREF="https://example.com/?a=1&amp;b=2">Example</A>
    <DT><A HREF="javascript:void(0)">Bad</A>
</DL><p>"#;
        let mut b = empty();
        let n = b.import_netscape(html, ROOT_OTHER, "Imported").unwrap();
        assert_eq!(n, 3);
        assert_eq!(b.find_url("https://example.com/?a=1&b=2").len(), 1);
        let servo = &b.find_url("https://servo.org/")[0];
        assert_eq!(servo.title, "Servo & friends");
        let games = b.nodes.iter().find(|n| n.title == "Games").unwrap();
        assert_eq!(b.children(&games.id)[0].title, "Steam");

        let exported = b.export_netscape();
        let mut again = empty();
        assert_eq!(again.import_netscape(&exported, ROOT_OTHER, "Again").unwrap(), 3);
        assert!(exported.contains("Servo &amp; friends"));
    }

    #[test]
    fn search_skips_trash() {
        let mut b = empty();
        let a = b.add(ROOT_BAR, "Rust lang", Some("https://rust-lang.org/"), None).unwrap();
        assert_eq!(b.search("rust", 10).len(), 1);
        b.delete(&a).unwrap();
        assert!(b.search("rust", 10).is_empty());
    }

    #[test]
    fn repair_orphans() {
        let mut b = empty();
        b.nodes.push(BookmarkNode {
            id: "x".into(),
            parent: Some("gone".into()),
            title: "x".into(),
            url: Some("https://x.test/".into()),
            added: 0,
            color: None,
            trashed_from: None,
        });
        b.ensure_roots();
        assert_eq!(b.get("x").unwrap().parent.as_deref(), Some(ROOT_OTHER));
    }
}

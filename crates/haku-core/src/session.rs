/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Tabs, workspaces, tab islands, split screen and the recently-closed stack.
//!
//! This is a pure data model: it never touches the engine. The browser applies an operation
//! here, then reconciles engine webviews against [`Session::tabs`] (create missing ones, drop
//! removed ones, show the active one). That keeps every rule testable without a window.

use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const START_URL: &str = "haku://start/";
const MAX_CLOSED: usize = 25;

pub type TabId = String;
pub type WorkspaceId = String;
pub type IslandId = String;

fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

use crate::now_ms;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Tab {
    pub id: TabId,
    pub workspace_id: WorkspaceId,
    pub url: String,
    pub title: String,
    pub pinned: bool,
    pub muted: bool,
    pub island_id: Option<IslandId>,
    pub emoji: Option<String>,
    /// Session history as reported by the engine, used to restore back/forward.
    pub history: Vec<String>,
    pub history_index: usize,
    /// Page zoom factor (1.0 = 100%).
    pub zoom: f32,
    pub last_active: i64,
    /// A snoozed tab has no live webview; it is recreated when activated.
    pub snoozed: bool,
    pub opener: Option<TabId>,
    pub reader: bool,
}

impl Default for Tab {
    fn default() -> Self {
        Self {
            id: new_id(),
            workspace_id: String::new(),
            url: START_URL.into(),
            title: String::new(),
            pinned: false,
            muted: false,
            island_id: None,
            emoji: None,
            history: Vec::new(),
            history_index: 0,
            zoom: 1.0,
            last_active: 0,
            snoozed: false,
            opener: None,
            reader: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Island {
    pub id: IslandId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub emoji: Option<String>,
    /// `#rrggbb`; `None` means the theme accent.
    pub color: Option<String>,
    pub collapsed: bool,
    /// Created automatically from link opens (dissolves when it shrinks to one tab).
    pub automatic: bool,
}

impl Default for Island {
    fn default() -> Self {
        Self {
            id: new_id(),
            workspace_id: String::new(),
            name: String::new(),
            emoji: None,
            color: None,
            collapsed: false,
            automatic: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    /// Icon id from the UI icon set (home, game, moon, book, star, code, music, ...).
    pub icon: String,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: "Workspace".into(),
            icon: "star".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ClosedTab {
    pub tab: Tab,
    /// Index within its workspace at the time it was closed.
    pub index: usize,
    pub closed_at: i64,
}

/// Where a new tab goes in its workspace's tab strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Directly after the given tab (context menu "New tab to the right").
    After,
    /// After the opener's other children (links open in order, like every major browser).
    AfterOpenerChildren,
    End,
}

#[derive(Clone, Debug)]
pub struct NewTab {
    pub url: String,
    pub background: bool,
    pub workspace: Option<WorkspaceId>,
    pub relative_to: Option<TabId>,
    pub placement: Placement,
    /// Opened from a link in `relative_to` (drives automatic tab islands).
    pub from_link: bool,
}

impl NewTab {
    pub fn url(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            background: false,
            workspace: None,
            relative_to: None,
            placement: Placement::End,
            from_link: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(default, rename_all = "camelCase")]
#[ts(export)]
pub struct Session {
    pub workspaces: Vec<Workspace>,
    pub active_workspace: WorkspaceId,
    /// All tabs; order inside one workspace is the tab strip order.
    pub tabs: Vec<Tab>,
    /// Active tab per workspace.
    pub active_tabs: BTreeMap<WorkspaceId, TabId>,
    pub islands: Vec<Island>,
    #[ts(as = "Vec<ClosedTab>")]
    pub closed: VecDeque<ClosedTab>,
    /// Two tabs shown side by side (left, right).
    pub split: Option<(TabId, TabId)>,
    /// Most-recently-used order of tab ids (front = most recent).
    #[ts(as = "Vec<TabId>")]
    pub mru: VecDeque<TabId>,
}

impl Default for Session {
    fn default() -> Self {
        let mut session = Self {
            workspaces: Vec::new(),
            active_workspace: String::new(),
            tabs: Vec::new(),
            active_tabs: BTreeMap::new(),
            islands: Vec::new(),
            closed: VecDeque::new(),
            split: None,
            mru: VecDeque::new(),
        };
        for (name, icon) in [("Home", "home"), ("Gaming", "game"), ("Focus", "moon")] {
            session.workspaces.push(Workspace {
                id: new_id(),
                name: name.into(),
                icon: icon.into(),
            });
        }
        session.active_workspace = session.workspaces[0].id.clone();
        session.new_tab(NewTab::url(START_URL));
        session
    }
}

impl Session {
    // ------------------------------------------------------------------ queries

    pub fn tab(&self, id: &str) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    fn tab_mut(&mut self, id: &str) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    /// Tabs of one workspace in strip order.
    pub fn workspace_tabs(&self, workspace: &str) -> Vec<&Tab> {
        self.tabs
            .iter()
            .filter(|t| t.workspace_id == workspace)
            .collect()
    }

    pub fn active_tab_id(&self) -> Option<&TabId> {
        self.active_tabs.get(&self.active_workspace)
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.active_tab_id().and_then(|id| self.tab(id))
    }

    fn index_in_workspace(&self, id: &str) -> Option<usize> {
        let tab = self.tab(id)?;
        self.workspace_tabs(&tab.workspace_id)
            .iter()
            .position(|t| t.id == id)
    }

    /// Global index of the n-th tab of a workspace (or the slot after the last one).
    fn global_index_for(&self, workspace: &str, n: usize) -> usize {
        let mut seen = 0;
        let mut last_global = None;
        for (gi, tab) in self.tabs.iter().enumerate() {
            if tab.workspace_id == workspace {
                if seen == n {
                    return gi;
                }
                seen += 1;
                last_global = Some(gi);
            }
        }
        last_global.map(|g| g + 1).unwrap_or(self.tabs.len())
    }

    pub fn island(&self, id: &str) -> Option<&Island> {
        self.islands.iter().find(|i| i.id == id)
    }

    // ------------------------------------------------------------------ tabs

    pub fn new_tab(&mut self, spec: NewTab) -> TabId {
        let workspace = spec
            .workspace
            .clone()
            .filter(|w| self.workspaces.iter().any(|x| &x.id == w))
            .unwrap_or_else(|| self.active_workspace.clone());
        let mut tab = Tab {
            workspace_id: workspace.clone(),
            url: spec.url.clone(),
            last_active: now_ms(),
            opener: spec.relative_to.clone(),
            ..Tab::default()
        };
        let pinned_count = self
            .workspace_tabs(&workspace)
            .iter()
            .filter(|t| t.pinned)
            .count();
        let list_len = self.workspace_tabs(&workspace).len();
        let position = match (&spec.relative_to, spec.placement) {
            (Some(rel), Placement::After) => self.index_in_workspace(rel).map(|i| i + 1),
            (Some(rel), Placement::AfterOpenerChildren) => self.index_in_workspace(rel).map(|i| {
                let list = self.workspace_tabs(&workspace);
                let mut pos = i + 1;
                while pos < list.len() && list[pos].opener.as_deref() == Some(rel.as_str()) {
                    pos += 1;
                }
                pos
            }),
            _ => None,
        }
        .unwrap_or(list_len)
        .max(pinned_count);

        if spec.from_link
            && let Some(rel) = &spec.relative_to
            && !spec.url.starts_with("haku:")
        {
            tab.island_id = self.island_for_link(rel);
        }

        let id = tab.id.clone();
        let global = self.global_index_for(&workspace, position);
        self.tabs.insert(global, tab);
        if !spec.background || !self.active_tabs.contains_key(&workspace) {
            self.activate(&id);
        } else {
            self.mru.push_back(id.clone());
        }
        id
    }

    /// The island a link opened from `opener` joins, creating an automatic one if needed.
    /// Automatic islands are only created when the `auto_islands` setting is on; the caller
    /// passes `from_link = false` otherwise.
    fn island_for_link(&mut self, opener: &str) -> Option<IslandId> {
        let opener_tab = self.tab(opener)?.clone();
        if let Some(existing) = opener_tab.island_id {
            return Some(existing);
        }
        let island = Island {
            workspace_id: opener_tab.workspace_id.clone(),
            name: host_label(&opener_tab.url),
            automatic: true,
            ..Island::default()
        };
        let id = island.id.clone();
        self.islands.push(island);
        if let Some(t) = self.tab_mut(opener) {
            t.island_id = Some(id.clone());
        }
        Some(id)
    }

    pub fn activate(&mut self, id: &str) -> bool {
        let Some(tab) = self.tab_mut(id) else {
            return false;
        };
        tab.last_active = now_ms();
        tab.snoozed = false;
        let workspace = tab.workspace_id.clone();
        // Activating a tab in a collapsed island expands it.
        if let Some(island_id) = tab.island_id.clone()
            && let Some(island) = self.islands.iter_mut().find(|i| i.id == island_id)
        {
            island.collapsed = false;
        }
        self.active_workspace = workspace.clone();
        self.active_tabs.insert(workspace, id.to_owned());
        self.mru.retain(|t| t != id);
        self.mru.push_front(id.to_owned());
        true
    }

    /// Close a tab. Returns false if it does not exist. Closing the last tab of a workspace
    /// opens a fresh start page there so a workspace is never empty.
    pub fn close(&mut self, id: &str) -> bool {
        let Some(index) = self.index_in_workspace(id) else {
            return false;
        };
        let global = self.tabs.iter().position(|t| t.id == id).expect("exists");
        let tab = self.tabs.remove(global);
        let workspace = tab.workspace_id.clone();
        self.closed.push_front(ClosedTab {
            tab: tab.clone(),
            index,
            closed_at: now_ms(),
        });
        self.closed.truncate(MAX_CLOSED);
        self.mru.retain(|t| t != id);
        if let Some((a, b)) = &self.split
            && (a == id || b == id)
        {
            self.split = None;
        }
        self.cleanup_islands();

        if self.active_tabs.get(&workspace).map(String::as_str) == Some(id) {
            self.active_tabs.remove(&workspace);
            let remaining = self.workspace_tabs(&workspace);
            if remaining.is_empty() {
                let fresh = Tab {
                    workspace_id: workspace.clone(),
                    ..Tab::default()
                };
                let fresh_id = fresh.id.clone();
                self.tabs.push(fresh);
                if self.active_workspace == workspace {
                    self.activate(&fresh_id);
                } else {
                    self.active_tabs.insert(workspace, fresh_id);
                }
            } else {
                // Prefer the most recently used tab of the same workspace, like Opera.
                let next = self
                    .mru
                    .iter()
                    .find(|m| remaining.iter().any(|t| &t.id == *m))
                    .cloned()
                    .unwrap_or_else(|| {
                        remaining[index.saturating_sub(1).min(remaining.len() - 1)]
                            .id
                            .clone()
                    });
                if self.active_workspace == workspace {
                    self.activate(&next);
                } else {
                    self.active_tabs.insert(workspace, next);
                }
            }
        }
        true
    }

    /// Close every unpinned tab in the same workspace except `id`.
    pub fn close_others(&mut self, id: &str) -> usize {
        let Some(ws) = self.tab(id).map(|t| t.workspace_id.clone()) else {
            return 0;
        };
        let victims: Vec<TabId> = self
            .workspace_tabs(&ws)
            .iter()
            .filter(|t| t.id != id && !t.pinned)
            .map(|t| t.id.clone())
            .collect();
        for v in &victims {
            self.close(v);
        }
        self.activate(id);
        victims.len()
    }

    /// Close unpinned tabs to the right of `id` (the clicked tab, not the active one).
    pub fn close_to_right(&mut self, id: &str) -> usize {
        let Some(ws) = self.tab(id).map(|t| t.workspace_id.clone()) else {
            return 0;
        };
        let Some(index) = self.index_in_workspace(id) else {
            return 0;
        };
        let victims: Vec<TabId> = self
            .workspace_tabs(&ws)
            .iter()
            .skip(index + 1)
            .filter(|t| !t.pinned)
            .map(|t| t.id.clone())
            .collect();
        for v in &victims {
            self.close(v);
        }
        victims.len()
    }

    /// Reopen the most recently closed tab at its old position. Returns the new tab id.
    pub fn reopen_closed(&mut self) -> Option<TabId> {
        let closed = self.closed.pop_front()?;
        let mut tab = closed.tab;
        if !self.workspaces.iter().any(|w| w.id == tab.workspace_id) {
            tab.workspace_id = self.active_workspace.clone();
        }
        if tab
            .island_id
            .as_ref()
            .is_some_and(|i| self.island(i).is_none())
        {
            tab.island_id = None;
        }
        tab.id = new_id();
        tab.snoozed = false;
        let ws = tab.workspace_id.clone();
        let global = self.global_index_for(&ws, closed.index);
        let id = tab.id.clone();
        self.tabs.insert(global.min(self.tabs.len()), tab);
        self.activate(&id);
        Some(id)
    }

    pub fn duplicate(&mut self, id: &str) -> Option<TabId> {
        let source = self.tab(id)?.clone();
        let new_id = self.new_tab(NewTab {
            url: source.url.clone(),
            background: false,
            workspace: Some(source.workspace_id.clone()),
            relative_to: Some(id.to_owned()),
            placement: Placement::After,
            from_link: false,
        });
        if let Some(t) = self.tab_mut(&new_id) {
            t.history = source.history;
            t.history_index = source.history_index;
            t.island_id = source.island_id;
            t.zoom = source.zoom;
            t.title = source.title;
        }
        Some(new_id)
    }

    /// Toggle pin. Pinned tabs live at the front of their workspace.
    pub fn set_pinned(&mut self, id: &str, pinned: bool) {
        let Some(tab) = self.tab_mut(id) else { return };
        if tab.pinned == pinned {
            return;
        }
        tab.pinned = pinned;
        if pinned {
            tab.island_id = None;
        }
        let ws = tab.workspace_id.clone();
        let pinned_count = self
            .workspace_tabs(&ws)
            .iter()
            .filter(|t| t.pinned && t.id != id)
            .count();
        self.move_tab(id, pinned_count);
        self.cleanup_islands();
    }

    pub fn set_muted(&mut self, id: &str, muted: bool) {
        if let Some(t) = self.tab_mut(id) {
            t.muted = muted;
        }
    }

    pub fn set_emoji(&mut self, id: &str, emoji: Option<String>) {
        if let Some(t) = self.tab_mut(id) {
            t.emoji = emoji.filter(|e| !e.is_empty());
        }
    }

    pub fn set_zoom(&mut self, id: &str, zoom: f32) -> f32 {
        let z = (zoom * 100.0).round() / 100.0;
        let z = z.clamp(0.25, 5.0);
        if let Some(t) = self.tab_mut(id) {
            t.zoom = z;
        }
        z
    }

    /// Move a tab to `to` within its workspace. Pinned tabs stay ahead of unpinned ones.
    pub fn move_tab(&mut self, id: &str, to: usize) {
        let Some(global) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };
        let tab = self.tabs.remove(global);
        let ws = tab.workspace_id.clone();
        let list = self.workspace_tabs(&ws);
        let pinned = list.iter().filter(|t| t.pinned).count();
        let to = if tab.pinned {
            to.min(pinned)
        } else {
            to.clamp(pinned, list.len())
        };
        let g = self.global_index_for(&ws, to);
        self.tabs.insert(g.min(self.tabs.len()), tab);
    }

    /// Move a tab into another workspace (end of its strip).
    pub fn move_to_workspace(&mut self, id: &str, workspace: &str) {
        if !self.workspaces.iter().any(|w| w.id == workspace) {
            return;
        }
        let Some(from_ws) = self.tab(id).map(|t| t.workspace_id.clone()) else {
            return;
        };
        if from_ws == workspace {
            return;
        }
        let was_active = self.active_tabs.get(&from_ws).map(String::as_str) == Some(id);
        let global = self.tabs.iter().position(|t| t.id == id).expect("exists");
        let mut tab = self.tabs.remove(global);
        tab.workspace_id = workspace.to_owned();
        tab.island_id = None;
        let g = self.global_index_for(workspace, usize::MAX);
        self.tabs.insert(g.min(self.tabs.len()), tab);
        if was_active {
            self.active_tabs.remove(&from_ws);
            if let Some(next) = self.workspace_tabs(&from_ws).first().map(|t| t.id.clone()) {
                self.active_tabs.insert(from_ws.clone(), next);
            } else {
                let fresh = Tab {
                    workspace_id: from_ws.clone(),
                    ..Tab::default()
                };
                self.active_tabs.insert(from_ws, fresh.id.clone());
                self.tabs.push(fresh);
            }
        }
        self.active_tabs
            .entry(workspace.to_owned())
            .or_insert_with(|| id.to_owned());
        self.cleanup_islands();
    }

    /// Record what the engine reports after a navigation or title change.
    pub fn update_page(&mut self, id: &str, url: Option<&str>, title: Option<&str>) {
        if let Some(t) = self.tab_mut(id) {
            if let Some(url) = url {
                t.url = url.to_owned();
            }
            if let Some(title) = title {
                t.title = title.to_owned();
            }
        }
    }

    pub fn update_history(&mut self, id: &str, entries: Vec<String>, index: usize) {
        if let Some(t) = self.tab_mut(id) {
            t.history_index = index.min(entries.len().saturating_sub(1));
            t.history = entries;
        }
    }

    /// Mark a background tab snoozed (its webview is dropped to save memory).
    pub fn snooze(&mut self, id: &str) -> bool {
        let active = self.active_tab_id().cloned();
        let split_ids = self.split.clone();
        let Some(t) = self.tab_mut(id) else {
            return false;
        };
        let in_split = split_ids.is_some_and(|(a, b)| a == t.id || b == t.id);
        if Some(&t.id) == active.as_ref() || in_split || t.snoozed {
            return false;
        }
        t.snoozed = true;
        true
    }

    /// Next tab for Ctrl+Tab. `mru` cycles by recency (Opera's "Cycle tabs in most recently
    /// used order"), otherwise by strip order. Collapsed islands are skipped.
    pub fn cycle(&self, forward: bool, mru: bool) -> Option<TabId> {
        let active = self.active_tab_id()?;
        let visible: Vec<&Tab> = self
            .workspace_tabs(&self.active_workspace)
            .into_iter()
            .filter(|t| {
                t.id == *active
                    || t.island_id
                        .as_ref()
                        .and_then(|i| self.island(i))
                        .is_none_or(|i| !i.collapsed)
            })
            .collect();
        if visible.len() < 2 {
            return None;
        }
        if mru {
            let order: Vec<&TabId> = self
                .mru
                .iter()
                .filter(|id| visible.iter().any(|t| &&t.id == id))
                .collect();
            return if forward { order.get(1) } else { order.last() }.map(|s| (*s).clone());
        }
        let pos = visible.iter().position(|t| &t.id == active)?;
        let next = if forward {
            (pos + 1) % visible.len()
        } else {
            (pos + visible.len() - 1) % visible.len()
        };
        Some(visible[next].id.clone())
    }

    /// Ctrl+1..8 select the n-th tab; Ctrl+9 the last.
    pub fn nth(&self, n: usize) -> Option<TabId> {
        let list = self.workspace_tabs(&self.active_workspace);
        let index = if n >= 9 {
            list.len().checked_sub(1)?
        } else {
            n.checked_sub(1)?
        };
        list.get(index).map(|t| t.id.clone())
    }

    // ------------------------------------------------------------------ islands

    pub fn create_island(&mut self, tab_ids: &[TabId], name: Option<String>) -> Option<IslandId> {
        let first = self.tab(tab_ids.first()?)?.clone();
        if first.pinned {
            return None;
        }
        let island = Island {
            workspace_id: first.workspace_id.clone(),
            name: name.unwrap_or_else(|| host_label(&first.url)),
            ..Island::default()
        };
        let id = island.id.clone();
        self.islands.push(island);
        // Gather the tabs next to the first one so an island is always contiguous.
        let mut anchor = self.index_in_workspace(&first.id)?;
        for tid in tab_ids {
            let Some(tab) = self.tab_mut(tid) else {
                continue;
            };
            if tab.workspace_id != first.workspace_id || tab.pinned {
                continue;
            }
            tab.island_id = Some(id.clone());
            if tid != &first.id {
                anchor += 1;
                self.move_tab(tid, anchor);
            }
        }
        self.cleanup_islands();
        Some(id)
    }

    pub fn add_to_island(&mut self, tab_id: &str, island_id: &str) {
        let Some(island) = self.island(island_id).cloned() else {
            return;
        };
        let Some(tab) = self.tab(tab_id).cloned() else {
            return;
        };
        if tab.workspace_id != island.workspace_id || tab.pinned {
            return;
        }
        if let Some(t) = self.tab_mut(tab_id) {
            t.island_id = Some(island_id.to_owned());
        }
        let last = self
            .workspace_tabs(&island.workspace_id)
            .iter()
            .rposition(|t| t.island_id.as_deref() == Some(island_id) && t.id != tab_id);
        if let Some(last) = last {
            let current = self.index_in_workspace(tab_id).unwrap_or(0);
            let target = if current < last { last } else { last + 1 };
            self.move_tab(tab_id, target);
        }
        if let Some(i) = self.islands.iter_mut().find(|i| i.id == island_id) {
            i.automatic = false;
        }
        self.cleanup_islands();
    }

    pub fn remove_from_island(&mut self, tab_id: &str) {
        if let Some(t) = self.tab_mut(tab_id) {
            t.island_id = None;
        }
        self.cleanup_islands();
    }

    pub fn ungroup_island(&mut self, island_id: &str) {
        for t in self.tabs.iter_mut() {
            if t.island_id.as_deref() == Some(island_id) {
                t.island_id = None;
            }
        }
        self.islands.retain(|i| i.id != island_id);
    }

    pub fn close_island(&mut self, island_id: &str) {
        let ids: Vec<TabId> = self
            .tabs
            .iter()
            .filter(|t| t.island_id.as_deref() == Some(island_id))
            .map(|t| t.id.clone())
            .collect();
        for id in ids {
            self.close(&id);
        }
        self.islands.retain(|i| i.id != island_id);
    }

    pub fn update_island(
        &mut self,
        island_id: &str,
        name: Option<String>,
        emoji: Option<Option<String>>,
        color: Option<Option<String>>,
        collapsed: Option<bool>,
    ) {
        let active = self.active_tab().and_then(|t| t.island_id.clone());
        let Some(island) = self.islands.iter_mut().find(|i| i.id == island_id) else {
            return;
        };
        if let Some(name) = name {
            island.name = name;
            island.automatic = false;
        }
        if let Some(emoji) = emoji {
            island.emoji = emoji;
        }
        if let Some(color) = color {
            island.color = color;
        }
        if let Some(collapsed) = collapsed {
            island.collapsed = collapsed;
        }
        // Collapsing the island that holds the active tab moves focus out of it.
        if collapsed == Some(true) && active.as_deref() == Some(island_id) {
            let ws = self.active_workspace.clone();
            let next = self
                .mru
                .iter()
                .find(|id| {
                    self.tab(id).is_some_and(|t| {
                        t.workspace_id == ws && t.island_id.as_deref() != Some(island_id)
                    })
                })
                .cloned();
            match next {
                Some(next) => {
                    self.activate(&next);
                }
                None => {
                    self.new_tab(NewTab::url(START_URL));
                }
            }
        }
    }

    /// Drop islands that no longer have tabs, and automatic islands left with one tab.
    fn cleanup_islands(&mut self) {
        let counts = self
            .tabs
            .iter()
            .fold(BTreeMap::<&str, usize>::new(), |mut m, t| {
                if let Some(i) = &t.island_id {
                    *m.entry(i.as_str()).or_default() += 1;
                }
                m
            });
        let dead: Vec<IslandId> = self
            .islands
            .iter()
            .filter(|i| {
                let n = counts.get(i.id.as_str()).copied().unwrap_or(0);
                n == 0 || (i.automatic && n == 1)
            })
            .map(|i| i.id.clone())
            .collect();
        for id in &dead {
            for t in self.tabs.iter_mut() {
                if t.island_id.as_ref() == Some(id) {
                    t.island_id = None;
                }
            }
        }
        self.islands.retain(|i| !dead.contains(&i.id));
    }

    // ------------------------------------------------------------------ workspaces

    pub fn add_workspace(&mut self, name: &str, icon: &str) -> WorkspaceId {
        let ws = Workspace {
            name: name.to_owned(),
            icon: icon.to_owned(),
            ..Workspace::default()
        };
        let id = ws.id.clone();
        self.workspaces.push(ws);
        self.new_tab(NewTab {
            workspace: Some(id.clone()),
            ..NewTab::url(START_URL)
        });
        id
    }

    pub fn rename_workspace(&mut self, id: &str, name: &str, icon: Option<&str>) {
        if let Some(w) = self.workspaces.iter_mut().find(|w| w.id == id) {
            w.name = name.to_owned();
            if let Some(icon) = icon {
                w.icon = icon.to_owned();
            }
        }
    }

    pub fn move_workspace(&mut self, id: &str, to: usize) {
        if let Some(from) = self.workspaces.iter().position(|w| w.id == id) {
            let ws = self.workspaces.remove(from);
            let to = to.min(self.workspaces.len());
            self.workspaces.insert(to, ws);
        }
    }

    /// Remove a workspace and all its tabs. The last workspace cannot be removed.
    pub fn remove_workspace(&mut self, id: &str) -> bool {
        if self.workspaces.len() <= 1 || !self.workspaces.iter().any(|w| w.id == id) {
            return false;
        }
        let ids: Vec<TabId> = self
            .workspace_tabs(id)
            .iter()
            .map(|t| t.id.clone())
            .collect();
        for tid in &ids {
            if let Some(g) = self.tabs.iter().position(|t| &t.id == tid) {
                let tab = self.tabs.remove(g);
                self.closed.push_front(ClosedTab {
                    tab,
                    index: 0,
                    closed_at: now_ms(),
                });
            }
            self.mru.retain(|m| m != tid);
        }
        self.closed.truncate(MAX_CLOSED);
        self.islands.retain(|i| i.workspace_id != id);
        self.active_tabs.remove(id);
        self.workspaces.retain(|w| w.id != id);
        if self.active_workspace == id {
            let first = self.workspaces[0].id.clone();
            self.switch_workspace(&first);
        }
        if let Some((a, b)) = &self.split
            && (ids.contains(a) || ids.contains(b))
        {
            self.split = None;
        }
        true
    }

    pub fn switch_workspace(&mut self, id: &str) -> bool {
        if !self.workspaces.iter().any(|w| w.id == id) {
            return false;
        }
        self.active_workspace = id.to_owned();
        let target = self
            .active_tabs
            .get(id)
            .cloned()
            .filter(|t| self.tab(t).is_some())
            .or_else(|| self.workspace_tabs(id).first().map(|t| t.id.clone()));
        match target {
            Some(t) => {
                self.activate(&t);
            }
            None => {
                self.new_tab(NewTab {
                    workspace: Some(id.to_owned()),
                    ..NewTab::url(START_URL)
                });
            }
        }
        true
    }

    // ------------------------------------------------------------------ split screen

    /// Show `left` and `right` side by side. Both must exist in the active workspace.
    pub fn set_split(&mut self, left: &str, right: &str) -> bool {
        let ok = left != right
            && [left, right].iter().all(|id| {
                self.tab(id)
                    .is_some_and(|t| t.workspace_id == self.active_workspace)
            });
        if ok {
            self.split = Some((left.to_owned(), right.to_owned()));
            self.activate(left);
        }
        ok
    }

    /// The tab after `id` in strip order (wrapping), used by "Split with next tab".
    pub fn next_in_strip(&self, id: &str) -> Option<TabId> {
        let tab = self.tab(id)?;
        let list = self.workspace_tabs(&tab.workspace_id);
        if list.len() < 2 {
            return None;
        }
        let pos = list.iter().position(|t| t.id == id)?;
        Some(list[(pos + 1) % list.len()].id.clone())
    }

    pub fn clear_split(&mut self) {
        self.split = None;
    }

    /// Validate a session loaded from disk: drop references to missing objects and make sure
    /// every workspace has an active tab.
    pub fn repair(&mut self) {
        if self.workspaces.is_empty() {
            *self = Session::default();
            return;
        }
        let ws_ids: Vec<WorkspaceId> = self.workspaces.iter().map(|w| w.id.clone()).collect();
        let fallback = ws_ids[0].clone();
        for t in self.tabs.iter_mut() {
            if !ws_ids.contains(&t.workspace_id) {
                t.workspace_id = fallback.clone();
            }
            t.zoom = if t.zoom.is_finite() {
                t.zoom.clamp(0.25, 5.0)
            } else {
                1.0
            };
        }
        self.islands.retain(|i| ws_ids.contains(&i.workspace_id));
        let island_ids: Vec<IslandId> = self.islands.iter().map(|i| i.id.clone()).collect();
        for t in self.tabs.iter_mut() {
            if t.island_id
                .as_ref()
                .is_some_and(|i| !island_ids.contains(i))
                || t.pinned
            {
                t.island_id = None;
            }
        }
        self.cleanup_islands();
        let tab_ids: Vec<TabId> = self.tabs.iter().map(|t| t.id.clone()).collect();
        self.mru.retain(|m| tab_ids.contains(m));
        self.active_tabs
            .retain(|w, t| ws_ids.contains(w) && tab_ids.contains(t));
        if let Some((a, b)) = &self.split
            && (!tab_ids.contains(a) || !tab_ids.contains(b))
        {
            self.split = None;
        }
        if !ws_ids.contains(&self.active_workspace) {
            self.active_workspace = fallback;
        }
        for ws in &ws_ids {
            if self.workspace_tabs(ws).is_empty() {
                self.tabs.push(Tab {
                    workspace_id: ws.clone(),
                    ..Tab::default()
                });
            }
            if !self.active_tabs.contains_key(ws) {
                let first = self.workspace_tabs(ws)[0].id.clone();
                self.active_tabs.insert(ws.clone(), first);
            }
        }
    }
}

/// A short label for a URL's site: `www.youtube.com` -> `youtube.com`.
pub fn host_label(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.trim_start_matches("www.").to_owned())
        })
        .unwrap_or_else(|| "Island".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_with(n: usize) -> (Session, Vec<TabId>) {
        let mut s = Session::default();
        let first = s.active_tab_id().unwrap().clone();
        let mut ids = vec![first];
        for i in 1..n {
            ids.push(s.new_tab(NewTab::url(format!("https://site{i}.test/"))));
        }
        (s, ids)
    }

    fn order(s: &Session) -> Vec<TabId> {
        s.workspace_tabs(&s.active_workspace)
            .iter()
            .map(|t| t.id.clone())
            .collect()
    }

    #[test]
    fn default_session_has_three_workspaces_and_a_tab() {
        let s = Session::default();
        assert_eq!(s.workspaces.len(), 3);
        assert_eq!(s.tabs.len(), 1);
        assert_eq!(s.active_tab().unwrap().url, START_URL);
    }

    #[test]
    fn new_tab_to_the_right_of_clicked_tab() {
        let (mut s, ids) = session_with(4);
        s.activate(&ids[3]);
        let new = s.new_tab(NewTab {
            relative_to: Some(ids[0].clone()),
            placement: Placement::After,
            ..NewTab::url("https://x.test/")
        });
        assert_eq!(order(&s)[1], new);
    }

    #[test]
    fn links_open_after_previous_children() {
        let (mut s, ids) = session_with(3);
        let spec = |u: &str| NewTab {
            relative_to: Some(ids[0].clone()),
            placement: Placement::AfterOpenerChildren,
            background: true,
            ..NewTab::url(u)
        };
        let a = s.new_tab(spec("https://a.test/"));
        let b = s.new_tab(spec("https://b.test/"));
        assert_eq!(&order(&s)[..3], &[ids[0].clone(), a, b]);
        assert_eq!(s.active_tab_id().unwrap(), &ids[2]);
    }

    #[test]
    fn auto_island_from_links_and_dissolve() {
        let (mut s, ids) = session_with(2);
        s.update_page(&ids[0], Some("https://www.youtube.com/"), None);
        let child = s.new_tab(NewTab {
            relative_to: Some(ids[0].clone()),
            placement: Placement::AfterOpenerChildren,
            from_link: true,
            ..NewTab::url("https://www.youtube.com/watch?v=1")
        });
        let island = s.tab(&child).unwrap().island_id.clone().unwrap();
        assert_eq!(s.tab(&ids[0]).unwrap().island_id.as_ref(), Some(&island));
        assert_eq!(s.island(&island).unwrap().name, "youtube.com");
        s.close(&child);
        // Automatic island with a single tab left dissolves.
        assert!(s.island(&island).is_none());
        assert!(s.tab(&ids[0]).unwrap().island_id.is_none());
    }

    #[test]
    fn close_activates_mru_and_never_empties_workspace() {
        let (mut s, ids) = session_with(3);
        s.activate(&ids[0]);
        s.activate(&ids[2]);
        s.close(&ids[2]);
        assert_eq!(s.active_tab_id().unwrap(), &ids[0]);
        s.close(&ids[0]);
        s.close(&ids[1]);
        assert_eq!(s.workspace_tabs(&s.active_workspace.clone()).len(), 1);
        assert_eq!(s.active_tab().unwrap().url, START_URL);
    }

    #[test]
    fn close_to_right_uses_clicked_tab_and_keeps_pinned() {
        let (mut s, ids) = session_with(5);
        s.set_pinned(&ids[4], true);
        s.activate(&ids[0]);
        let closed = s.close_to_right(&ids[1]);
        // ids[4] is pinned (moved to the front), so it survives.
        assert_eq!(closed, 2);
        assert_eq!(
            order(&s),
            vec![ids[4].clone(), ids[0].clone(), ids[1].clone()]
        );
    }

    #[test]
    fn reopen_restores_position() {
        let (mut s, ids) = session_with(4);
        s.close(&ids[1]);
        let back = s.reopen_closed().unwrap();
        assert_eq!(order(&s)[1], back);
        assert_eq!(s.active_tab_id().unwrap(), &back);
    }

    #[test]
    fn pinned_tabs_stay_in_front() {
        let (mut s, ids) = session_with(3);
        s.set_pinned(&ids[2], true);
        assert_eq!(order(&s)[0], ids[2]);
        s.move_tab(&ids[0], 0);
        assert_eq!(order(&s)[0], ids[2]);
    }

    #[test]
    fn island_creation_is_contiguous_and_collapse_moves_focus() {
        let (mut s, ids) = session_with(4);
        let island = s
            .create_island(&[ids[0].clone(), ids[3].clone()], Some("Mine".into()))
            .unwrap();
        assert_eq!(&order(&s)[..2], &[ids[0].clone(), ids[3].clone()]);
        s.activate(&ids[1]);
        s.activate(&ids[0]);
        s.update_island(&island, None, None, None, Some(true));
        assert_eq!(s.active_tab_id().unwrap(), &ids[1]);
        assert!(s.island(&island).unwrap().collapsed);
        // Ctrl+Tab skips collapsed islands.
        let next = s.cycle(true, false).unwrap();
        assert_eq!(next, ids[2]);
    }

    #[test]
    fn workspace_lifecycle() {
        let mut s = Session::default();
        let ws = s.add_workspace("Work", "code");
        assert_eq!(s.active_workspace, ws);
        assert_eq!(s.workspace_tabs(&ws).len(), 1);
        let home = s.workspaces[0].id.clone();
        s.switch_workspace(&home);
        assert!(s.remove_workspace(&ws));
        assert!(s.workspace_tabs(&ws).is_empty());
        while s.workspaces.len() > 1 {
            let id = s.workspaces.last().unwrap().id.clone();
            s.remove_workspace(&id);
        }
        let last = s.workspaces[0].id.clone();
        assert!(!s.remove_workspace(&last));
    }

    #[test]
    fn split_next_is_strip_neighbor_and_clears_on_close() {
        let (mut s, ids) = session_with(3);
        assert_eq!(s.next_in_strip(&ids[1]).unwrap(), ids[2]);
        assert_eq!(s.next_in_strip(&ids[2]).unwrap(), ids[0]);
        assert!(s.set_split(&ids[0], &ids[1]));
        s.close(&ids[1]);
        assert!(s.split.is_none());
    }

    #[test]
    fn mru_cycling() {
        let (mut s, ids) = session_with(3);
        s.activate(&ids[0]);
        s.activate(&ids[2]);
        assert_eq!(s.cycle(true, true).unwrap(), ids[0]);
    }

    #[test]
    fn nth_tab_shortcuts() {
        let (s, ids) = session_with(4);
        assert_eq!(s.nth(1).unwrap(), ids[0]);
        assert_eq!(s.nth(9).unwrap(), ids[3]);
        assert!(s.nth(0).is_none());
    }

    #[test]
    fn snooze_never_hits_active_or_split() {
        let (mut s, ids) = session_with(3);
        assert!(!s.snooze(&ids[2]));
        assert!(s.snooze(&ids[0]));
        s.activate(&ids[0]);
        assert!(!s.tab(&ids[0]).unwrap().snoozed);
    }

    #[test]
    fn repair_fixes_dangling_references() {
        let (mut s, ids) = session_with(2);
        s.tabs[0].workspace_id = "gone".into();
        s.tabs[1].island_id = Some("gone".into());
        s.active_tabs.insert("gone".into(), ids[0].clone());
        s.split = Some(("x".into(), ids[0].clone()));
        s.repair();
        assert!(
            s.tabs
                .iter()
                .all(|t| s.workspaces.iter().any(|w| w.id == t.workspace_id))
        );
        assert!(s.tabs[1].island_id.is_none());
        assert!(s.split.is_none());
        for w in &s.workspaces {
            assert!(s.active_tabs.contains_key(&w.id));
        }
    }

    #[test]
    fn serde_round_trip() {
        let (s, _) = session_with(3);
        let text = serde_json::to_string(&s).unwrap();
        let back: Session = serde_json::from_str(&text).unwrap();
        assert_eq!(back, s);
    }
}

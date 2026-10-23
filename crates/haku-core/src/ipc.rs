/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The UI <-> core protocol.
//!
//! The UI shell sends [`Command`]s (JSON, `type`-tagged) and receives [`Event`]s. Both are
//! exported to TypeScript, so the UI and the core cannot drift apart silently. Every command is
//! validated by the core; the UI is never trusted to have checked anything.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::bookmarks::Bookmarks;
use crate::history::{UrlStat, Visit};
use crate::records::{DownloadRecord, Pinboards, SiteMods};
use crate::session::{IslandId, Session, TabId, WorkspaceId};
use crate::settings::Settings;
use crate::storage::ProfileInfo;
use crate::themes::{Palette, ThemeInfo, UserTheme};

/// A rectangle in CSS pixels of the UI shell (top-left origin).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum WindowAction {
    Minimize,
    ToggleMaximize,
    Close,
    /// Start an OS window drag (the UI calls this on mousedown in a drag region).
    Drag,
    ToggleFullscreen,
    NewWindow,
    NewPrivateWindow,
    Quit,
}

/// Where a navigation from the address bar or a link should go.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum Disposition {
    CurrentTab,
    NewTab,
    BackgroundTab,
    NewWindow,
    PrivateWindow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum DownloadAction {
    Pause,
    Resume,
    Cancel,
    Retry,
    Open,
    ShowInFolder,
    Remove,
    DeleteFile,
}

/// Page actions from menus, shortcuts and the context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum PageAction {
    CopyAddress,
    ViewSource,
    ToggleReader,
    ToggleForceDark,
    SavePage,
    Screenshot,
    QrCode,
    Translate,
    Wayback,
    AddToSpeedDial,
    Bookmark,
    /// Forward one of the engine's own context-menu actions (cut/copy/paste/select all/...).
    EngineCut,
    EngineCopy,
    EnginePaste,
    EngineSelectAll,
    CopyLink,
    OpenLinkInNewTab,
    OpenLinkInBackgroundTab,
    OpenLinkInPrivateWindow,
    CopyImageLink,
    OpenImageInNewTab,
    SaveLinkAs,
    SaveImageAs,
    SearchSelection,
    CopySelection,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum Command {
    /// The UI finished loading and wants the initial state.
    Ready,
    /// Where pages go and whether a UI overlay covers them (then all input goes to the UI).
    Layout {
        page: Rect,
        split: Option<Rect>,
        overlay: bool,
        /// Areas the compositor blurs before drawing the UI (glass effects over pages).
        blur: Vec<Rect>,
    },
    Window { action: WindowAction },

    // ---- navigation and tabs
    Navigate {
        tab: Option<TabId>,
        input: String,
        disposition: Disposition,
    },
    Back { tab: Option<TabId> },
    Forward { tab: Option<TabId> },
    Reload { tab: Option<TabId>, bypass_cache: bool },
    Stop { tab: Option<TabId> },
    NewTab {
        url: Option<String>,
        background: bool,
        relative_to: Option<TabId>,
    },
    CloseTab { tab: TabId },
    CloseOthers { tab: TabId },
    CloseToRight { tab: TabId },
    ReopenClosed,
    ActivateTab { tab: TabId },
    MoveTab { tab: TabId, index: usize },
    MoveTabToWorkspace { tab: TabId, workspace: WorkspaceId },
    PinTab { tab: TabId, pinned: bool },
    MuteTab { tab: TabId, muted: bool },
    SetTabEmoji { tab: TabId, emoji: Option<String> },
    DuplicateTab { tab: TabId },
    CycleTab { forward: bool },
    SelectNthTab { n: usize },
    SetZoom { tab: Option<TabId>, zoom: f32 },
    ZoomStep { delta: i32 },

    // ---- islands, workspaces, split screen
    CreateIsland { tabs: Vec<TabId>, name: Option<String> },
    AddToIsland { tab: TabId, island: IslandId },
    RemoveFromIsland { tab: TabId },
    /// `None` leaves a field unchanged; an empty `emoji`/`color` clears it.
    UpdateIsland {
        island: IslandId,
        name: Option<String>,
        emoji: Option<String>,
        color: Option<String>,
        collapsed: Option<bool>,
    },
    UngroupIsland { island: IslandId },
    CloseIsland { island: IslandId },
    AddWorkspace { name: String, icon: String },
    UpdateWorkspace { workspace: WorkspaceId, name: String, icon: Option<String> },
    RemoveWorkspace { workspace: WorkspaceId },
    SwitchWorkspace { workspace: WorkspaceId },
    MoveWorkspace { workspace: WorkspaceId, index: usize },
    SetSplit { left: TabId, right: TabId },
    ClearSplit,

    // ---- settings and themes
    PatchSettings { patch: serde_json::Value },
    ResetSettings,
    SaveUserTheme { theme: UserTheme },
    DeleteUserTheme { id: String },
    PickWallpaper,

    // ---- bookmarks
    AddBookmark {
        parent: String,
        title: String,
        url: Option<String>,
        index: Option<usize>,
    },
    UpdateBookmark {
        id: String,
        title: String,
        url: Option<String>,
        color: Option<String>,
    },
    MoveBookmark { id: String, parent: String, index: Option<usize> },
    DeleteBookmark { id: String },
    RestoreBookmark { id: String },
    EmptyTrash,
    /// Star button: bookmark the active page in the bookmarks bar, or remove it if present.
    ToggleBookmark,
    ImportBookmarks,
    ExportBookmarks,

    // ---- history and suggestions
    QueryHistory {
        request: u32,
        text: String,
        from: Option<i64>,
        to: Option<i64>,
        limit: u32,
        offset: u32,
    },
    DeleteHistoryVisits { ids: Vec<i64> },
    DeleteHistoryRange { from: i64, to: i64 },
    Suggest { request: u32, text: String },

    // ---- downloads
    Download { id: String, action: DownloadAction },
    ClearDownloads,

    // ---- find in page
    Find { text: String, forward: bool, match_case: bool },
    FindStop,

    // ---- page actions and engine prompts
    Page { action: PageAction, tab: Option<TabId>, data: Option<String> },
    DialogResponse { id: u32, confirmed: bool, value: Option<String> },
    SelectResponse { id: u32, selected: Vec<usize> },
    ColorResponse { id: u32, color: Option<String> },
    ContextMenuClosed { id: u32 },
    PermissionResponse { id: u32, allow: bool, remember: bool },
    AuthResponse { id: u32, username: Option<String>, password: Option<String> },
    FilePickerResponse { id: u32, paths: Vec<String> },

    // ---- data
    ClearBrowsingData {
        from: Option<i64>,
        history: bool,
        cookies: bool,
        cache: bool,
        downloads: bool,
        site_settings: bool,
    },
    SavePinboards { pinboards: Pinboards },
    SaveSiteMods { mods: SiteMods },

    // ---- misc
    Panic,
    PlaySound { sound: String },
    OpenExternal { url: String },
    Log { level: String, message: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum Security {
    /// haku:// pages.
    Internal,
    Secure,
    Insecure,
    /// file:// and data: pages.
    Local,
    None,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TabMeta {
    pub tab: TabId,
    pub url: String,
    pub title: String,
    pub loading: bool,
    /// PNG data URL.
    pub favicon: Option<String>,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub security: Security,
    pub status: Option<String>,
    pub zoom: f32,
    pub audible: bool,
    pub blocked: u32,
    pub crashed: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum SuggestionKind {
    Url,
    Search,
    History,
    Bookmark,
    OpenTab,
    Calculator,
    Conversion,
    Command,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Suggestion {
    pub kind: SuggestionKind,
    pub title: String,
    /// What navigating to this suggestion opens (URL, or the input for a search).
    pub url: String,
    pub detail: Option<String>,
    /// Tab id for `open-tab` suggestions (switch instead of navigating).
    pub tab: Option<TabId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ContextMenuInfo {
    pub id: u32,
    pub tab: TabId,
    /// Window position in UI CSS pixels.
    pub x: f32,
    pub y: f32,
    pub link_url: Option<String>,
    pub image_url: Option<String>,
    pub editable: bool,
    pub selection: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum DialogKind {
    Alert,
    Confirm,
    Prompt,
    BeforeUnload,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SelectOption {
    pub index: usize,
    pub label: String,
    pub disabled: bool,
    pub group: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EngineInfo {
    pub app_version: String,
    pub servo_version: String,
    pub user_agent: String,
    pub os: String,
    pub private: bool,
    pub profile: ProfileInfo,
    pub profiles: Vec<ProfileInfo>,
    pub data_dir: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Metrics {
    /// Browser CPU use, percent of all cores.
    pub cpu: f32,
    /// Browser memory, MiB.
    pub memory_mb: f32,
    pub system_memory_mb: f32,
    pub download_bps: f32,
    pub upload_bps: f32,
    pub per_tab: Vec<TabMetric>,
    pub limits_enforced: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TabMetric {
    pub tab: TabId,
    pub cpu: f32,
    pub memory_mb: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum Event {
    Init {
        engine: EngineInfo,
        session: Session,
        settings: Settings,
        palette: Palette,
        themes: Vec<ThemeInfo>,
        bookmarks: Bookmarks,
        downloads: Vec<DownloadRecord>,
        pinboards: Pinboards,
        site_mods: SiteMods,
        system_dark: bool,
    },
    Session { session: Session },
    Settings { settings: Settings, palette: Palette, themes: Vec<ThemeInfo> },
    TabMeta { meta: TabMeta },
    TabClosed { tab: TabId },
    Bookmarks { bookmarks: Bookmarks },
    HistoryResult { request: u32, visits: Vec<Visit> },
    Suggestions { request: u32, text: String, items: Vec<Suggestion> },
    TopSites { sites: Vec<UrlStat> },
    Downloads { items: Vec<DownloadRecord> },
    DownloadProgress { id: String, received: u64, total: Option<u64>, bps: f32 },
    FindResult { matches: u32, active: u32 },
    Dialog {
        id: u32,
        tab: TabId,
        kind: DialogKind,
        message: String,
        default_value: Option<String>,
        origin: String,
    },
    Select {
        id: u32,
        tab: TabId,
        rect: Rect,
        options: Vec<SelectOption>,
        selected: Vec<usize>,
        multiple: bool,
    },
    ColorPicker { id: u32, tab: TabId, rect: Rect, color: Option<String> },
    ContextMenu { menu: ContextMenuInfo },
    Permission { id: u32, tab: TabId, origin: String, kind: String },
    Auth { id: u32, tab: TabId, url: String, proxy: bool },
    HideControl { id: u32 },
    Toast { text: String, kind: String },
    Fullscreen { on: bool },
    WindowState { maximized: bool, fullscreen: bool, focused: bool },
    Metrics { metrics: Metrics },
    Blocked { tab: TabId, count: u32, total: u64 },
    Panic { active: bool },
    Pinboards { pinboards: Pinboards },
    SiteMods { mods: SiteMods },
    /// Something the core wants the UI to open (settings page, panel, ...).
    Open { target: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_wire_format() {
        let c: Command = serde_json::from_str(
            r#"{"type":"navigate","tab":null,"input":"servo.org","disposition":"new-tab"}"#,
        )
        .unwrap();
        assert_eq!(
            c,
            Command::Navigate {
                tab: None,
                input: "servo.org".into(),
                disposition: Disposition::NewTab
            }
        );
        let c: Command =
            serde_json::from_str(r#"{"type":"reload","tab":"t1","bypassCache":true}"#).unwrap();
        assert!(matches!(c, Command::Reload { bypass_cache: true, .. }));
        let c: Command = serde_json::from_str(r#"{"type":"ready"}"#).unwrap();
        assert_eq!(c, Command::Ready);
    }

    #[test]
    fn unknown_commands_are_rejected() {
        assert!(serde_json::from_str::<Command>(r#"{"type":"format-disk"}"#).is_err());
        assert!(serde_json::from_str::<Command>(r#"{"type":"close-tab"}"#).is_err());
    }

    #[test]
    fn event_wire_format() {
        let e = Event::FindResult { matches: 3, active: 1 };
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"type":"find-result","matches":3,"active":1}"#
        );
        let e = Event::DownloadProgress {
            id: "d".into(),
            received: 1,
            total: None,
            bps: 0.0,
        };
        assert!(serde_json::to_string(&e).unwrap().contains("\"received\":1"));
    }
}

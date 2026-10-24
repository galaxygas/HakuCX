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
use crate::settings::{Cleaner, Settings};
use crate::storage::ProfileInfo;
use crate::sync::SyncedTab;
use crate::themes::{Palette, ThemeInfo, UserTheme};
use crate::vault::Address;

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
    ScrollToTop,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum Command {
    /// The UI finished loading and wants the initial state.
    Ready,
    /// Where pages go and whether a UI overlay covers them (then all input goes to the UI).
    Layout {
        page: Rect,
        split: Option<Rect>,
        /// Where the open web panel (sidebar messenger etc.) is drawn.
        panel: Option<Rect>,
        /// Something (a floating panel, menu, dialog) covers the page: page input goes to the UI.
        overlay: bool,
        /// A menu, popup or dialog is open: all input, including web panels, goes to the UI.
        modal: bool,
        /// Areas the compositor blurs before drawing the UI (glass effects over pages).
        blur: Vec<Rect>,
        /// Interactive UI drawn on top of a page (find bar, split divider): input there goes
        /// to the UI even when nothing else covers the page.
        regions: Vec<Rect>,
        /// Where an extension's popup page is drawn (above the UI).
        #[serde(default)]
        ext_popup: Option<Rect>,
    },
    Window {
        action: WindowAction,
    },

    // ---- navigation and tabs
    Navigate {
        tab: Option<TabId>,
        input: String,
        disposition: Disposition,
    },
    Back {
        tab: Option<TabId>,
    },
    Forward {
        tab: Option<TabId>,
    },
    Reload {
        tab: Option<TabId>,
        bypass_cache: bool,
    },
    Stop {
        tab: Option<TabId>,
    },
    NewTab {
        url: Option<String>,
        background: bool,
        relative_to: Option<TabId>,
    },
    CloseTab {
        tab: TabId,
    },
    CloseOthers {
        tab: TabId,
    },
    CloseToRight {
        tab: TabId,
    },
    ReopenClosed,
    ActivateTab {
        tab: TabId,
    },
    MoveTab {
        tab: TabId,
        index: usize,
    },
    MoveTabToWorkspace {
        tab: TabId,
        workspace: WorkspaceId,
    },
    PinTab {
        tab: TabId,
        pinned: bool,
    },
    MuteTab {
        tab: TabId,
        muted: bool,
    },
    SetTabEmoji {
        tab: TabId,
        emoji: Option<String>,
    },
    DuplicateTab {
        tab: TabId,
    },
    CycleTab {
        forward: bool,
    },
    SelectNthTab {
        n: usize,
    },
    SetZoom {
        tab: Option<TabId>,
        zoom: f32,
    },
    ZoomStep {
        delta: i32,
    },

    // ---- sidebar web panels
    /// Show (creating if needed) the web panel `id` loading `url`.
    OpenWebPanel {
        id: String,
        url: String,
    },
    /// Hide the visible web panel (it keeps running in the background).
    HideWebPanel,
    /// Close a web panel's page entirely.
    CloseWebPanel {
        id: String,
    },
    WebPanelNavigate {
        id: String,
        action: String,
    },

    // ---- islands, workspaces, split screen
    CreateIsland {
        tabs: Vec<TabId>,
        name: Option<String>,
    },
    AddToIsland {
        tab: TabId,
        island: IslandId,
    },
    RemoveFromIsland {
        tab: TabId,
    },
    /// `None` leaves a field unchanged; an empty `emoji`/`color` clears it.
    UpdateIsland {
        island: IslandId,
        name: Option<String>,
        emoji: Option<String>,
        color: Option<String>,
        collapsed: Option<bool>,
    },
    UngroupIsland {
        island: IslandId,
    },
    CloseIsland {
        island: IslandId,
    },
    AddWorkspace {
        name: String,
        icon: String,
    },
    UpdateWorkspace {
        workspace: WorkspaceId,
        name: String,
        icon: Option<String>,
    },
    RemoveWorkspace {
        workspace: WorkspaceId,
    },
    SwitchWorkspace {
        workspace: WorkspaceId,
    },
    MoveWorkspace {
        workspace: WorkspaceId,
        index: usize,
    },
    SetSplit {
        left: TabId,
        right: TabId,
    },
    ClearSplit,

    // ---- settings and themes
    PatchSettings {
        patch: serde_json::Value,
    },
    ResetSettings,
    SaveUserTheme {
        theme: UserTheme,
    },
    DeleteUserTheme {
        id: String,
    },
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
    MoveBookmark {
        id: String,
        parent: String,
        index: Option<usize>,
    },
    DeleteBookmark {
        id: String,
    },
    RestoreBookmark {
        id: String,
    },
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
    DeleteHistoryVisits {
        ids: Vec<i64>,
    },
    DeleteHistoryRange {
        from: i64,
        to: i64,
    },
    Suggest {
        request: u32,
        text: String,
    },

    // ---- downloads
    Download {
        id: String,
        action: DownloadAction,
    },
    ClearDownloads,

    // ---- find in page
    Find {
        text: String,
        forward: bool,
        match_case: bool,
    },
    FindStop,

    // ---- page actions and engine prompts
    Page {
        action: PageAction,
        tab: Option<TabId>,
        data: Option<String>,
    },
    DialogResponse {
        id: u32,
        confirmed: bool,
        value: Option<String>,
    },
    SelectResponse {
        id: u32,
        selected: Vec<usize>,
    },
    ColorResponse {
        id: u32,
        color: Option<String>,
    },
    ContextMenuClosed {
        id: u32,
    },
    PermissionResponse {
        id: u32,
        allow: bool,
        remember: bool,
    },
    AuthResponse {
        id: u32,
        username: Option<String>,
        password: Option<String>,
    },
    FilePickerResponse {
        id: u32,
        paths: Vec<String>,
    },

    // ---- data
    ClearBrowsingData {
        from: Option<i64>,
        history: bool,
        cookies: bool,
        cache: bool,
        downloads: bool,
        site_settings: bool,
    },
    SavePinboards {
        pinboards: Pinboards,
    },
    SaveSiteMods {
        mods: SiteMods,
    },

    // ---- tools
    /// Sizes and counts for the GX Cleaner panel.
    CleanerStats,
    RunCleaner {
        cleaner: Cleaner,
    },
    /// Fetch a public http(s) resource (feeds, deals, weather) without cookies; text up to 5 MB.
    HttpGet {
        request: u32,
        url: String,
    },
    /// Visible text of a tab's page (for AI prompts), up to 40,000 characters.
    GetPageText {
        request: u32,
        tab: Option<TabId>,
    },
    AiChat {
        request: u32,
        messages: Vec<AiMessage>,
    },
    /// Store (or with `None`, delete) the AI provider key in the OS credential store.
    SetAiKey {
        key: Option<String>,
    },
    AiKeyStatus,

    // ---- profiles, files, app
    TopSites {
        limit: u32,
    },
    OpenProfile {
        id: String,
    },
    AddProfile {
        name: String,
        kind: crate::storage::ProfileKind,
    },
    /// Ask the user for a text file (mods, settings) and send its content back.
    OpenTextFile {
        request: u32,
        extensions: Vec<String>,
    },
    SaveTextFile {
        suggested_name: String,
        content: String,
    },
    PickDownloadDir,
    Relaunch,

    // ---- extensions
    /// Ask for a `.crx`, `.xpi` or `.zip` file and install it (after the permission prompt).
    ExtInstallFile,
    /// Developer mode: ask for a folder and load it unpacked.
    ExtLoadUnpacked,
    /// Answer to an [`Event::ExtInstallPrompt`].
    ExtInstallAnswer {
        request: u32,
        accept: bool,
    },
    ExtSetEnabled {
        id: String,
        enabled: bool,
    },
    ExtSetPrivate {
        id: String,
        allow: bool,
    },
    ExtSetPinned {
        id: String,
        pinned: bool,
    },
    ExtRemove {
        id: String,
    },
    ExtReload {
        id: String,
    },
    ExtOpenOptions {
        id: String,
    },
    ExtSetDeveloperMode {
        on: bool,
    },
    ExtSetShortcut {
        id: String,
        command: String,
        accelerator: String,
    },
    /// The toolbar button: opens the popup or fires `action.onClicked`. `anchor` is the button.
    ExtActionClick {
        id: String,
        anchor: Rect,
    },
    ExtPopupClose,
    /// A context-menu item an extension added.
    ExtMenuClick {
        ext: String,
        item: String,
        menu: u32,
    },
    /// Answer to an [`Event::ExtPermissionPrompt`] (`permissions.request`).
    ExtPermissionAnswer {
        request: u32,
        grant: bool,
    },
    ExtToggleRuleset {
        id: String,
        ruleset: String,
        enabled: bool,
    },

    // ---- passwords, autofill, sync
    /// Open the vault with its recovery password (when Windows cannot unlock it).
    VaultUnlock {
        password: String,
    },
    /// Set or remove (`None`) the vault's recovery password.
    VaultSetRecovery {
        password: Option<String>,
    },
    /// Show a saved password or card number, after Windows Hello. Answered with
    /// [`Event::Revealed`].
    VaultReveal {
        request: u32,
        id: String,
    },
    SaveLogin {
        login: LoginInput,
    },
    SaveCard {
        card: CardInput,
    },
    SaveAddress {
        address: Address,
    },
    /// Delete a login, card or address.
    VaultDelete {
        id: String,
    },
    /// Allow offering to save passwords on a site again.
    NeverSaveRemove {
        origin: String,
    },
    /// Answer to [`Event::PasswordPrompt`]: `save`, `never` or `dismiss`. The user may have
    /// corrected the username or password in the prompt.
    PasswordPromptAnswer {
        prompt: u32,
        action: String,
        username: String,
        password: String,
    },
    /// Fill a suggestion from [`Event::AutofillSuggest`] into the tab's focused form.
    AutofillPick {
        tab: TabId,
        id: String,
    },
    PasswordsImport,
    PasswordsExport,
    /// Start syncing through `folder` with `passphrase` (joining the folder if other devices
    /// already use it).
    SyncSetup {
        folder: String,
        passphrase: String,
    },
    SyncNow,
    /// Stop syncing and forget the passphrase on this device.
    SyncDisconnect,

    // ---- Windows integration and updates
    /// Register HakuCX with Windows as a browser and open Windows' default-apps settings.
    MakeDefaultBrowser,
    /// Remove HakuCX's browser registration from Windows.
    UnregisterBrowser,
    CheckForUpdates,
    /// Run the downloaded, verified installer and restart.
    InstallUpdate,
    OpenCrashReports,

    // ---- media
    /// Control a tab's audio or video (miniplayer).
    Media {
        tab: TabId,
        action: MediaAction,
    },

    // ---- My Flow
    /// Show a QR code that connects a phone.
    FlowPair,
    FlowCancelPairing,
    /// A note or link from this computer.
    FlowSend {
        text: String,
    },
    /// Send a tab's address (the active tab when `None`).
    FlowSendPage {
        tab: Option<TabId>,
    },
    /// Pick a file and send it.
    FlowSendFile,
    /// Open a link or file from My Flow.
    FlowOpen {
        id: String,
    },
    /// Show a received file in its folder.
    FlowShowFile {
        id: String,
    },
    FlowRemove {
        id: String,
    },
    FlowClear,
    /// Disconnect a phone (it must scan a new QR code to connect again).
    FlowForgetDevice {
        device: String,
    },
    /// The My Flow panel is open: clear the unread count.
    FlowSeen,

    // ---- GX Lights
    /// Try to reach Razer Chroma and OpenRGB again now.
    LightsReconnect,

    // ---- misc
    /// Put text on the system clipboard (the UI's own clipboard API is off in Servo).
    CopyText {
        text: String,
    },
    Panic,
    PlaySound {
        sound: String,
    },
    /// While on, browser shortcuts are not run so the UI can record a key combination.
    KeyCapture {
        on: bool,
    },
    OpenExternal {
        url: String,
    },
    Log {
        level: String,
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AiMessage {
    /// `user` or `assistant`.
    pub role: String,
    pub content: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CleanerStats {
    /// Bytes used by the profile folder (engine storage, history, settings, ...).
    pub profile_bytes: u64,
    pub history_visits: u64,
    pub downloads: u32,
    /// Bytes of completed downloaded files that still exist.
    pub download_bytes: u64,
    pub sites_with_data: u32,
    pub cache_entries: u32,
    pub open_tabs: u32,
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
    /// The page's audio or video, for the miniplayer.
    pub media: Option<MediaInfo>,
}

/// What a tab is playing (its most recent audio or video element).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaInfo {
    pub paused: bool,
    pub video: bool,
    /// From the page's Media Session, when it sets one.
    pub title: String,
    pub artist: String,
    pub album: String,
    /// `https:` image URL.
    pub artwork: Option<String>,
    pub has_prev: bool,
    pub has_next: bool,
    /// A video that can play in the pop-out window (one with a plain web address).
    pub poppable: bool,
    /// Playing in the pop-out window right now.
    pub popped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
#[ts(export)]
pub enum MediaAction {
    Play,
    Pause,
    Next,
    Prev,
    /// Move the tab's video into the pop-out window.
    PopOut,
    /// Bring a popped-out video back to its tab.
    Return,
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
    /// Items extensions added with `contextMenus`/`menus` that apply to this click.
    #[serde(default)]
    pub extension_items: Vec<ExtMenuItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SystemInfo {
    /// Windows' choice for web links is HakuCX.
    pub is_default: bool,
    /// This copy is registered with Windows as a browser.
    pub registered: bool,
    /// Installed by the HakuCX installer (as opposed to portable or built from source).
    pub installed: bool,
    /// Running from the portable zip (data kept next to the program).
    pub portable: bool,
    pub windows: bool,
    pub crash_reports: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UpdateStatus {
    /// `unavailable` (no update key in this build), `idle`, `checking`, `up-to-date`,
    /// `available` (portable copies: download it yourself), `downloading`, `ready` or `error`.
    pub state: String,
    pub current: String,
    pub version: Option<String>,
    pub page: Option<String>,
    /// 0..1 while downloading.
    pub progress: f32,
    pub message: String,
    pub last_check: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FlowStatus {
    /// `off` (My Flow is turned off), `ready`, or `unavailable` (see `message`).
    pub state: String,
    pub message: String,
    pub entries: Vec<crate::flow::FlowEntry>,
    pub devices: Vec<crate::flow::FlowDevice>,
    /// The address a phone opens (shown as a QR code) while pairing.
    pub pairing: Option<FlowPairing>,
    /// Where phones reach this computer (`192.168.1.5:47652`), while the server runs.
    pub address: Option<String>,
    pub unread: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FlowPairing {
    pub url: String,
    pub expires: i64,
}

/// One way GX Lights reaches devices (Razer Chroma or OpenRGB).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LightLink {
    /// `off` (not wanted), `connecting`, `connected` or `unavailable` (see `message`).
    pub state: String,
    /// Device names (OpenRGB) or device types (Razer) currently lit.
    pub devices: Vec<String>,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LightsStatus {
    pub razer: LightLink,
    pub openrgb: LightLink,
    /// The sound level meter for the music visualizer could be opened.
    pub meter: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VaultStatus {
    /// `ready`, `needs-password` (Windows lost the key; the recovery password opens it),
    /// `unavailable` (private window of a profile with no vault) or `error`.
    pub state: String,
    pub message: String,
    pub recovery_password: bool,
    /// Windows Hello can verify the user before secrets are shown.
    pub hello: bool,
    /// Changes are saved (not in private windows).
    pub writable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoginInfo {
    pub id: String,
    pub origin: String,
    pub username: String,
    pub note: String,
    pub created: i64,
    pub modified: i64,
    pub last_used: i64,
    pub times_used: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CardInfo {
    pub id: String,
    pub nickname: String,
    pub holder: String,
    pub brand: String,
    pub last4: String,
    pub exp_month: u8,
    pub exp_year: u16,
}

/// A login the user adds or edits. `password: None` keeps the saved one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoginInput {
    pub id: Option<String>,
    /// A site address (`https://example.com/login` or `example.com`).
    pub site: String,
    pub username: String,
    pub password: Option<String>,
    pub note: String,
}

/// A card the user adds or edits. `number: None` keeps the saved one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CardInput {
    pub id: Option<String>,
    pub nickname: String,
    pub holder: String,
    pub number: Option<String>,
    pub exp_month: u8,
    pub exp_year: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AutofillItem {
    pub id: String,
    pub label: String,
    pub sublabel: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SyncDevice {
    pub id: String,
    pub name: String,
    pub written_at: i64,
    pub tabs: Vec<SyncedTab>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SyncStatus {
    /// `off`, `needs-passphrase`, `idle`, `syncing` or `error`.
    pub state: String,
    pub message: String,
    pub last_sync: Option<i64>,
    pub this_device: String,
    pub devices: Vec<SyncDevice>,
}

/// A context-menu item contributed by an extension, already filtered for this click.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExtMenuItem {
    pub ext: String,
    pub id: String,
    pub parent: Option<String>,
    pub title: String,
    /// `normal`, `checkbox`, `radio` or `separator`.
    pub kind: String,
    pub checked: bool,
    pub enabled: bool,
    /// The extension's icon (a `haku-ext://` URL), shown on top-level items.
    pub icon: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExtCommandInfo {
    pub name: String,
    pub description: String,
    pub shortcut: String,
}

/// An installed extension, as the Extensions page and the toolbar show it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExtensionInfo {
    pub id: String,
    pub runtime_id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub manifest_version: u8,
    /// A `haku-ext://` URL of the best icon, if any.
    pub icon: Option<String>,
    pub enabled: bool,
    pub allow_private: bool,
    pub pinned: bool,
    pub unpacked: bool,
    pub path: String,
    pub homepage: Option<String>,
    pub has_options: bool,
    pub has_popup: bool,
    /// Toolbar button state for the active tab.
    pub action_title: String,
    pub badge_text: String,
    pub badge_color: String,
    pub action_icon: Option<String>,
    pub action_enabled: bool,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub commands: Vec<ExtCommandInfo>,
    /// Static rulesets: (id, enabled).
    pub rulesets: Vec<(String, bool)>,
    /// APIs or features this extension uses that HakuCX cannot provide yet.
    pub unsupported: Vec<String>,
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
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
// `Init` carries the whole browser state once per UI load; events are built and serialized
// immediately, so their size in memory does not matter.
#[allow(clippy::large_enum_variant)]
pub enum Event {
    Init {
        engine: EngineInfo,
        session: Session,
        settings: Box<Settings>,
        palette: Palette,
        themes: Vec<ThemeInfo>,
        bookmarks: Bookmarks,
        downloads: Vec<DownloadRecord>,
        pinboards: Pinboards,
        site_mods: SiteMods,
        system_dark: bool,
    },
    Session {
        session: Session,
    },
    Settings {
        settings: Box<Settings>,
        palette: Palette,
        themes: Vec<ThemeInfo>,
    },
    TabMeta {
        meta: TabMeta,
    },
    TabClosed {
        tab: TabId,
    },
    Bookmarks {
        bookmarks: Bookmarks,
    },
    HistoryResult {
        request: u32,
        visits: Vec<Visit>,
    },
    Suggestions {
        request: u32,
        text: String,
        items: Vec<Suggestion>,
    },
    TopSites {
        sites: Vec<UrlStat>,
    },
    Downloads {
        items: Vec<DownloadRecord>,
    },
    DownloadProgress {
        id: String,
        received: u64,
        total: Option<u64>,
        bps: f32,
    },
    FindResult {
        matches: u32,
        active: u32,
    },
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
    ColorPicker {
        id: u32,
        tab: TabId,
        rect: Rect,
        color: Option<String>,
    },
    ContextMenu {
        menu: ContextMenuInfo,
    },
    Permission {
        id: u32,
        tab: TabId,
        origin: String,
        kind: String,
    },
    Auth {
        id: u32,
        tab: TabId,
        url: String,
        proxy: bool,
    },
    HideControl {
        id: u32,
    },
    Toast {
        text: String,
        kind: String,
    },
    Fullscreen {
        on: bool,
    },
    WindowState {
        maximized: bool,
        fullscreen: bool,
        focused: bool,
    },
    Metrics {
        metrics: Metrics,
    },
    Blocked {
        tab: TabId,
        count: u32,
        total: u64,
    },
    Panic {
        active: bool,
    },
    CleanerStats {
        stats: CleanerStats,
    },
    CleanerDone {
        summary: String,
    },
    HttpResult {
        request: u32,
        status: u16,
        body: String,
        error: Option<String>,
    },
    PageText {
        request: u32,
        title: String,
        url: String,
        text: String,
    },
    AiReply {
        request: u32,
        text: String,
        error: Option<String>,
    },
    AiKey {
        stored: bool,
    },
    /// A page tried to open a window without a click or key press first.
    PopupBlocked {
        tab: TabId,
        host: String,
    },
    TextFile {
        request: u32,
        name: String,
        content: String,
    },
    /// Unread count parsed from a web panel's page title ("(3) Discord").
    PanelBadge {
        id: String,
        count: u32,
    },
    PanelState {
        id: String,
        loading: bool,
        url: String,
        can_go_back: bool,
    },
    Extensions {
        items: Vec<ExtensionInfo>,
        developer_mode: bool,
    },
    /// Show an extension popup under `anchor` at this size (CSS px).
    ExtPopup {
        id: String,
        anchor: Rect,
        width: f32,
        height: f32,
    },
    ExtPopupClosed,
    /// Confirm installing a package: what it can do.
    ExtInstallPrompt {
        request: u32,
        name: String,
        version: String,
        warnings: Vec<String>,
        update: bool,
    },
    /// `permissions.request` from an extension.
    ExtPermissionPrompt {
        request: u32,
        name: String,
        warnings: Vec<String>,
    },
    /// The vault's contents without secrets (passwords and card numbers stay in the core).
    Vault {
        status: VaultStatus,
        logins: Vec<LoginInfo>,
        never_save: Vec<String>,
        cards: Vec<CardInfo>,
        addresses: Vec<Address>,
    },
    /// A secret the user asked to see (`None` if verification failed or was cancelled).
    Revealed {
        request: u32,
        value: Option<String>,
    },
    /// Offer to save (or update) a password the user just submitted.
    PasswordPrompt {
        prompt: u32,
        tab: TabId,
        origin: String,
        username: String,
        password: String,
        update: bool,
    },
    /// Saved accounts, addresses or cards for the focused field. `rect` is the field in UI
    /// coordinates.
    AutofillSuggest {
        tab: TabId,
        rect: Rect,
        kind: String,
        items: Vec<AutofillItem>,
    },
    AutofillHide,
    Sync {
        status: SyncStatus,
    },
    /// How HakuCX is set up in Windows.
    System {
        info: SystemInfo,
    },
    Update {
        status: UpdateStatus,
    },
    Lights {
        status: LightsStatus,
    },
    Flow {
        status: FlowStatus,
    },
    Pinboards {
        pinboards: Pinboards,
    },
    SiteMods {
        mods: SiteMods,
    },
    /// Something the core wants the UI to open (settings page, panel, ...).
    Open {
        target: String,
    },
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
        assert!(matches!(
            c,
            Command::Reload {
                bypass_cache: true,
                ..
            }
        ));
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
        let e = Event::FindResult {
            matches: 3,
            active: 1,
        };
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
        assert!(
            serde_json::to_string(&e)
                .unwrap()
                .contains("\"received\":1")
        );
    }
}

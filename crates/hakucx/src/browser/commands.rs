/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! IPC command dispatch. The UI is not trusted: every command is parsed into the typed
//! [`Command`] enum (unknown or malformed messages are dropped) and validated here.

use haku_core::bookmarks::{ROOT_BAR, ROOT_OTHER, ROOT_SPEED_DIAL};
use haku_core::history::Transition;
use haku_core::ipc::{Command, Disposition, Event, PageAction, WindowAction};
use haku_core::session::{NewTab, Placement, START_URL};
use log::{debug, info, warn};
use servo::ContextMenuAction;

use super::tabs::is_internal;
use super::{Browser, Layout, to_pixels};

const MAX_IPC_MESSAGE: usize = 8 * 1024 * 1024;

impl Browser {
    pub(crate) fn handle_ipc(&self, message: &str) {
        if message.len() > MAX_IPC_MESSAGE {
            warn!("dropped oversized IPC message");
            return;
        }
        let command: Command = match serde_json::from_str(message) {
            Ok(c) => c,
            Err(e) => {
                warn!("dropped malformed IPC message: {e}");
                return;
            }
        };
        self.run_command(command);
    }

    pub(crate) fn run_command(&self, command: Command) {
        match command {
            Command::Ready => {
                self.ui_ready.set(true);
                self.send_init();
                self.start_background_services();
            }
            Command::Layout {
                page,
                split,
                panel,
                overlay,
                modal,
                blur: _,
                regions,
                ext_popup,
            } => {
                let scale = self.ui_scale();
                *self.layout.borrow_mut() = Layout {
                    page: to_pixels(page, scale),
                    split: split.map(|r| to_pixels(r, scale)),
                    panel: panel.map(|r| to_pixels(r, scale)),
                    popup: ext_popup.map(|r| to_pixels(r, scale)),
                    overlay,
                    modal,
                    regions: regions
                        .into_iter()
                        .take(32)
                        .map(|r| to_pixels(r, scale))
                        .collect(),
                };
                self.resize_panes();
                self.resize_web_panel();
                self.ext_resize_popup();
                self.window.request_redraw();
            }
            Command::Window { action } => self.window_action(action),

            // ---------------------------------------------------------- navigation
            Command::Navigate {
                tab,
                input,
                disposition,
            } => match disposition {
                Disposition::CurrentTab => {
                    if let Some(tab) = self.target_tab(tab)
                        && self.navigate_input(&tab, &input)
                    {
                        // Typing a URL hands the keyboard to the page, like every browser.
                        self.focus_page();
                    }
                }
                Disposition::NewTab | Disposition::BackgroundTab => {
                    let target = {
                        let s = self.settings.borrow();
                        haku_core::omnibox::resolve(&input, &s.search, self.private)
                    };
                    if let Some(target) = target {
                        let active = self.active_tab_id();
                        let id = self.session.borrow_mut().new_tab(NewTab {
                            url: target.url().to_owned(),
                            background: disposition == Disposition::BackgroundTab,
                            workspace: None,
                            relative_to: active,
                            placement: Placement::After,
                            from_link: false,
                        });
                        self.navigate_tab(&id, target.url(), Transition::Typed);
                    }
                }
                Disposition::NewWindow | Disposition::PrivateWindow => {
                    let target = {
                        let s = self.settings.borrow();
                        haku_core::omnibox::resolve(&input, &s.search, true)
                    };
                    self.spawn_window(
                        target.map(|t| t.url().to_owned()),
                        disposition == Disposition::PrivateWindow,
                    );
                }
            },
            Command::Back { tab } => {
                if let Some(tab) = self.target_tab(tab) {
                    self.go_back(&tab);
                }
            }
            Command::Forward { tab } => {
                if let Some(tab) = self.target_tab(tab) {
                    self.go_forward(&tab);
                }
            }
            Command::Reload { tab, bypass_cache } => {
                if let Some(tab) = self.target_tab(tab) {
                    if bypass_cache {
                        self.servo.network_manager().clear_cache();
                    }
                    match self.webview_for_tab(&tab) {
                        Some(w)
                            if self
                                .views
                                .borrow()
                                .get(&tab)
                                .is_some_and(|v| v.internal.is_none()) =>
                        {
                            if let Some(v) = self.views.borrow_mut().get_mut(&tab) {
                                v.crashed = None;
                                v.next_transition = Transition::Reload;
                            }
                            w.reload();
                        }
                        _ => {
                            // Internal page: the UI re-renders it; just tell it.
                            self.after_tab_update(&tab);
                        }
                    }
                }
            }
            Command::Stop { tab } => {
                // Servo 0.7 has no stop-loading API; navigating to the current history entry
                // is not a stop either, so the UI hides the stop button (see engine limits).
                let _ = tab;
            }
            Command::NewTab {
                url,
                background,
                relative_to,
            } => {
                let url = url.unwrap_or_else(|| START_URL.to_owned());
                let placement = if relative_to.is_some() {
                    Placement::After
                } else {
                    Placement::End
                };
                let id = self.session.borrow_mut().new_tab(NewTab {
                    url: url.clone(),
                    background,
                    workspace: None,
                    relative_to,
                    placement,
                    from_link: false,
                });
                if is_internal(&url) {
                    self.session.borrow_mut().update_page(
                        &id,
                        Some(&url),
                        Some(&super::tabs::internal_title(&url)),
                    );
                    self.session_changed();
                } else {
                    self.navigate_tab(&id, &url, Transition::Typed);
                }
                self.play_sound("tab-open");
            }
            Command::CloseTab { tab } => {
                self.drop_controls_for_tab(&tab);
                let closing_last_window_tab = self.session.borrow().tabs.len() == 1;
                if closing_last_window_tab
                    && self
                        .session
                        .borrow()
                        .tab(&tab)
                        .is_some_and(|t| t.url == START_URL)
                {
                    self.window_action(WindowAction::Close);
                    return;
                }
                if self.session.borrow_mut().close(&tab) {
                    self.play_sound("tab-close");
                    self.session_changed();
                }
            }
            Command::CloseOthers { tab } => {
                self.session.borrow_mut().close_others(&tab);
                self.session_changed();
            }
            Command::CloseToRight { tab } => {
                self.session.borrow_mut().close_to_right(&tab);
                self.session_changed();
            }
            Command::ReopenClosed => {
                if let Some(id) = self.session.borrow_mut().reopen_closed() {
                    let url = self
                        .session
                        .borrow()
                        .tab(&id)
                        .map(|t| t.url.clone())
                        .unwrap_or_default();
                    self.navigate_tab(&id, &url, Transition::Link);
                }
                self.session_changed();
            }
            Command::ActivateTab { tab } => {
                if self.session.borrow_mut().activate(&tab) {
                    self.play_sound("switch");
                    self.session_changed();
                }
            }
            Command::MoveTab { tab, index } => {
                self.session.borrow_mut().move_tab(&tab, index);
                self.session_changed();
            }
            Command::MoveTabToWorkspace { tab, workspace } => {
                self.session
                    .borrow_mut()
                    .move_to_workspace(&tab, &workspace);
                self.session_changed();
            }
            Command::PinTab { tab, pinned } => {
                self.session.borrow_mut().set_pinned(&tab, pinned);
                self.session_changed();
            }
            Command::MuteTab { tab, muted } => {
                self.session.borrow_mut().set_muted(&tab, muted);
                self.session_changed();
                self.after_tab_update(&tab);
            }
            Command::SetTabEmoji { tab, emoji } => {
                let emoji = emoji.map(|e| e.chars().take(4).collect::<String>());
                self.session.borrow_mut().set_emoji(&tab, emoji);
                self.session_changed();
            }
            Command::DuplicateTab { tab } => {
                if let Some(id) = self.session.borrow_mut().duplicate(&tab) {
                    let url = self
                        .session
                        .borrow()
                        .tab(&id)
                        .map(|t| t.url.clone())
                        .unwrap_or_default();
                    self.navigate_tab(&id, &url, Transition::Link);
                }
                self.session_changed();
            }
            Command::CycleTab { forward } => {
                let mru = self.settings.borrow().tabs.mru_cycling;
                let next = self.session.borrow().cycle(forward, mru);
                if let Some(next) = next {
                    self.session.borrow_mut().activate(&next);
                    self.session_changed();
                }
            }
            Command::SelectNthTab { n } => {
                let next = self.session.borrow().nth(n);
                if let Some(next) = next {
                    self.session.borrow_mut().activate(&next);
                    self.session_changed();
                }
            }
            Command::SetZoom { tab, zoom } => {
                if let Some(tab) = self.target_tab(tab) {
                    self.set_tab_zoom(&tab, zoom);
                }
            }
            Command::ZoomStep { delta } => {
                if let Some(tab) = self.active_tab_id() {
                    let current = self
                        .session
                        .borrow()
                        .tab(&tab)
                        .map(|t| t.zoom)
                        .unwrap_or(1.0);
                    let next = match delta {
                        0 => 1.0,
                        d => zoom_step(current, d > 0),
                    };
                    self.set_tab_zoom(&tab, next);
                }
            }

            // ---------------------------------------------------------- web panels
            Command::OpenWebPanel { id, url } => self.open_web_panel(&id, &url),
            Command::HideWebPanel => self.hide_web_panel(),
            Command::CloseWebPanel { id } => self.close_web_panel(&id),
            Command::WebPanelNavigate { id, action } => self.web_panel_navigate(&id, &action),

            // ---------------------------------------------------------- islands, workspaces
            Command::CreateIsland { tabs, name } => {
                self.session.borrow_mut().create_island(&tabs, name);
                self.session_changed();
            }
            Command::AddToIsland { tab, island } => {
                self.session.borrow_mut().add_to_island(&tab, &island);
                self.session_changed();
            }
            Command::RemoveFromIsland { tab } => {
                self.session.borrow_mut().remove_from_island(&tab);
                self.session_changed();
            }
            Command::UpdateIsland {
                island,
                name,
                emoji,
                color,
                collapsed,
            } => {
                let to_patch = |v: Option<String>| v.map(|s| Some(s).filter(|s| !s.is_empty()));
                let color = to_patch(color)
                    .map(|c| c.filter(|c| haku_core::themes::Rgb::parse(c).is_some()));
                self.session.borrow_mut().update_island(
                    &island,
                    name.map(|n| n.chars().take(60).collect()),
                    to_patch(emoji),
                    color,
                    collapsed,
                );
                self.session_changed();
            }
            Command::UngroupIsland { island } => {
                self.session.borrow_mut().ungroup_island(&island);
                self.session_changed();
            }
            Command::CloseIsland { island } => {
                self.session.borrow_mut().close_island(&island);
                self.session_changed();
            }
            Command::AddWorkspace { name, icon } => {
                let name: String = name.trim().chars().take(40).collect();
                let name = if name.is_empty() {
                    "Workspace".to_owned()
                } else {
                    name
                };
                self.session.borrow_mut().add_workspace(&name, &icon);
                self.session_changed();
            }
            Command::UpdateWorkspace {
                workspace,
                name,
                icon,
            } => {
                let name: String = name.trim().chars().take(40).collect();
                if !name.is_empty() {
                    self.session
                        .borrow_mut()
                        .rename_workspace(&workspace, &name, icon.as_deref());
                    self.session_changed();
                }
            }
            Command::RemoveWorkspace { workspace } => {
                self.session.borrow_mut().remove_workspace(&workspace);
                self.session_changed();
            }
            Command::SwitchWorkspace { workspace } => {
                self.session.borrow_mut().switch_workspace(&workspace);
                self.session_changed();
            }
            Command::MoveWorkspace { workspace, index } => {
                self.session.borrow_mut().move_workspace(&workspace, index);
                self.session_changed();
            }
            Command::SetSplit { left, right } => {
                if self.settings.borrow().appearance.split_screen {
                    self.session.borrow_mut().set_split(&left, &right);
                    self.session_changed();
                }
            }
            Command::ClearSplit => {
                self.session.borrow_mut().clear_split();
                self.session_changed();
            }

            // ---------------------------------------------------------- settings
            Command::PatchSettings { patch } => {
                let result = self.settings.borrow_mut().apply_patch(&patch);
                match result {
                    Ok(()) => {
                        self.settings_changed();
                        self.after_settings_change(&patch);
                    }
                    Err(e) => {
                        warn!("rejected settings patch: {e}");
                        self.toast("That setting could not be changed.", "error");
                        self.settings_changed();
                    }
                }
            }
            Command::ResetSettings => {
                *self.settings.borrow_mut() = Default::default();
                self.settings_changed();
                self.restyle_all_pages();
            }
            Command::SaveUserTheme { theme } => {
                let valid = [
                    &theme.dark_primary,
                    &theme.dark_secondary,
                    &theme.light_primary,
                    &theme.light_secondary,
                ]
                .iter()
                .all(|c| haku_core::themes::Rgb::parse(c).is_some());
                if !valid || theme.name.trim().is_empty() {
                    self.toast("A theme needs a name and valid colors.", "error");
                    return;
                }
                let mut theme = theme;
                if !theme.id.starts_with("user:") {
                    theme.id = format!("user:{}", uuid_short());
                }
                theme.name = theme.name.trim().chars().take(40).collect();
                {
                    let mut themes = self.user_themes.borrow_mut();
                    themes.themes.retain(|t| t.id != theme.id);
                    themes.themes.push(theme);
                }
                self.save_named("user-themes");
                self.settings_changed();
            }
            Command::DeleteUserTheme { id } => {
                self.user_themes.borrow_mut().themes.retain(|t| t.id != id);
                self.save_named("user-themes");
                if self.settings.borrow().appearance.theme == id {
                    self.settings.borrow_mut().appearance.theme = "ultraviolet".into();
                }
                self.settings_changed();
            }
            Command::PickWallpaper => self.defer(|b| b.pick_wallpaper()),

            // ---------------------------------------------------------- bookmarks
            Command::AddBookmark {
                parent,
                title,
                url,
                index,
            } => {
                let title: String = title.chars().take(500).collect();
                let result =
                    self.bookmarks
                        .borrow_mut()
                        .add(&parent, &title, url.as_deref(), index);
                match result {
                    Ok(_) => self.bookmarks_changed(),
                    Err(e) => self.toast(e.to_string(), "error"),
                }
            }
            Command::UpdateBookmark {
                id,
                title,
                url,
                color,
            } => {
                let result = self
                    .bookmarks
                    .borrow_mut()
                    .rename(&id, &title, url.as_deref());
                if let Err(e) = result {
                    self.toast(e.to_string(), "error");
                    return;
                }
                let color = color.filter(|c| haku_core::themes::Rgb::parse(c).is_some());
                self.bookmarks.borrow_mut().set_color(&id, color);
                self.bookmarks_changed();
            }
            Command::MoveBookmark { id, parent, index } => {
                let result = self.bookmarks.borrow_mut().move_to(&id, &parent, index);
                match result {
                    Ok(()) => self.bookmarks_changed(),
                    Err(e) => self.toast(e.to_string(), "error"),
                }
            }
            Command::DeleteBookmark { id } => {
                if self.bookmarks.borrow_mut().delete(&id).is_ok() {
                    self.bookmarks_changed();
                }
            }
            Command::RestoreBookmark { id } => {
                if self.bookmarks.borrow_mut().restore(&id).is_ok() {
                    self.bookmarks_changed();
                }
            }
            Command::EmptyTrash => {
                self.bookmarks.borrow_mut().empty_trash();
                self.bookmarks_changed();
            }
            Command::ToggleBookmark => {
                let Some(tab) = self.active_tab_id() else {
                    return;
                };
                let Some((url, title)) = self
                    .session
                    .borrow()
                    .tab(&tab)
                    .map(|t| (t.url.clone(), t.title.clone()))
                else {
                    return;
                };
                let existing: Vec<String> = self
                    .bookmarks
                    .borrow()
                    .find_url(&url)
                    .iter()
                    .map(|b| b.id.clone())
                    .collect();
                if existing.is_empty() {
                    let added = self
                        .bookmarks
                        .borrow_mut()
                        .add(ROOT_BAR, &title, Some(&url), None);
                    if added.is_ok() {
                        self.toast("Added to bookmarks", "info");
                    }
                } else {
                    for id in existing {
                        let _ = self.bookmarks.borrow_mut().delete(&id);
                    }
                    self.toast("Removed from bookmarks", "info");
                }
                self.bookmarks_changed();
            }
            Command::ImportBookmarks => self.defer(|b| b.import_bookmarks()),
            Command::ExportBookmarks => self.defer(|b| b.export_bookmarks()),

            // ---------------------------------------------------------- history
            Command::QueryHistory {
                request,
                text,
                from,
                to,
                limit,
                offset,
            } => {
                let visits = self
                    .history
                    .query(&text, from, to, limit.min(500) as usize, offset as usize)
                    .unwrap_or_else(|e| {
                        warn!("history query: {e}");
                        Vec::new()
                    });
                self.emit(Event::HistoryResult { request, visits });
            }
            Command::DeleteHistoryVisits { ids } => {
                for id in ids {
                    let _ = self.history.delete_visit(id);
                }
            }
            Command::DeleteHistoryRange { from, to } => {
                let _ = self.history.delete_range(from, to);
            }
            Command::Suggest { request, text } => {
                let items = self.suggestions(&text);
                self.emit(Event::Suggestions {
                    request,
                    text,
                    items,
                });
            }

            // ---------------------------------------------------------- downloads
            Command::Download { id, action } => self.download_action(&id, action),
            Command::ClearDownloads => {
                self.downloads.borrow_mut().clear_finished(None);
                self.downloads_changed();
            }

            // ---------------------------------------------------------- find
            Command::Find {
                text,
                forward,
                match_case,
            } => {
                let text: String = text.chars().take(500).collect();
                if text.is_empty() {
                    self.stop_find();
                    self.emit(Event::FindResult {
                        matches: 0,
                        active: 0,
                    });
                } else {
                    self.find_in_page(&text, forward, match_case);
                }
            }
            Command::FindStop => self.stop_find(),

            // ---------------------------------------------------------- page actions, prompts
            Command::Page { action, tab, data } => self.page_action(action, tab, data),
            Command::DialogResponse {
                id,
                confirmed,
                value,
            } => self.answer_dialog(id, confirmed, value),
            Command::SelectResponse { id, selected } => self.answer_select(id, selected),
            Command::ColorResponse { id, color } => self.answer_color(id, color),
            Command::ContextMenuClosed { id } => self.context_menu_closed(id),
            Command::PermissionResponse {
                id,
                allow,
                remember,
            } => self.answer_permission(id, allow, remember),
            Command::AuthResponse {
                id,
                username,
                password,
            } => self.answer_auth(id, username, password),
            Command::FilePickerResponse { .. } => {}

            // ---------------------------------------------------------- data
            Command::ClearBrowsingData {
                from,
                history,
                cookies,
                cache,
                downloads,
                site_settings,
            } => {
                self.clear_browsing_data(from, history, cookies, cache, downloads, site_settings);
            }
            Command::SavePinboards { pinboards } => {
                *self.pinboards.borrow_mut() = pinboards;
                self.save_named("pinboards");
                self.emit(Event::Pinboards {
                    pinboards: self.pinboards.borrow().clone(),
                });
            }
            Command::SaveSiteMods { mods } => {
                *self.site_mods.borrow_mut() = mods;
                self.save_named("site-mods");
                self.emit(Event::SiteMods {
                    mods: self.site_mods.borrow().clone(),
                });
                self.restyle_all_pages();
            }

            // ---------------------------------------------------------- tools
            Command::CleanerStats => self.cleaner_stats(),
            Command::RunCleaner { cleaner } => self.run_cleaner(&cleaner),
            Command::HttpGet { request, url } => self.http_get(request, url),
            Command::GetPageText { request, tab } => self.page_text(request, tab),
            Command::AiChat { request, messages } => self.ai_chat(request, messages),
            Command::SetAiKey { key } => self.set_ai_key(key),
            Command::AiKeyStatus => self.emit(Event::AiKey {
                stored: crate::browser::tools::ai_key().is_some(),
            }),

            // ---------------------------------------------------------- profiles, files, app
            Command::TopSites { limit } => {
                let sites = if self.private {
                    Vec::new()
                } else {
                    self.history
                        .top_sites(limit.min(50) as usize)
                        .unwrap_or_default()
                };
                self.emit(Event::TopSites { sites });
            }
            Command::OpenProfile { id } => {
                if self.profiles.borrow().get(&id).is_some()
                    && let Ok(exe) = std::env::current_exe()
                {
                    let _ = std::process::Command::new(exe)
                        .arg("--profile")
                        .arg(&id)
                        .spawn();
                }
            }
            Command::AddProfile { name, kind } => {
                let color = match kind {
                    haku_core::storage::ProfileKind::Streaming => "#9146ff",
                    haku_core::storage::ProfileKind::Roguelike => "#fa1e4e",
                    haku_core::storage::ProfileKind::Standard => "#00e5c3",
                };
                let info = self.profiles.borrow_mut().add(&name, color, kind);
                if let Err(e) = self.profiles.borrow().save(&self.data_root) {
                    self.toast(format!("Could not save the profile: {e}"), "error");
                    return;
                }
                self.toast(format!("Profile \"{}\" created", info.name), "info");
                self.send_init();
            }
            Command::OpenTextFile {
                request,
                extensions,
            } => self.defer(move |b| b.open_text_file(request, &extensions)),
            Command::SaveTextFile {
                suggested_name,
                content,
            } => self.defer(move |b| b.save_text_file(&suggested_name, &content)),
            Command::PickDownloadDir => self.defer(|b| b.pick_download_dir()),
            Command::Relaunch => {
                if let Ok(exe) = std::env::current_exe() {
                    let mut command = std::process::Command::new(exe);
                    command.arg("--profile").arg(&self.profile.id);
                    if self.private {
                        command.arg("--private");
                    }
                    self.save_if_due(true);
                    if command.spawn().is_ok() {
                        self.exit_requested.set(true);
                    }
                }
            }

            // ---------------------------------------------------------- extensions
            // File dialogs are modal: open them outside this dispatch.
            Command::ExtInstallFile => self.defer(|b| b.ext_install_file()),
            Command::ExtLoadUnpacked => self.defer(|b| b.ext_load_unpacked()),
            Command::ExtInstallAnswer { request, accept } => {
                self.ext_install_answer(request, accept)
            }
            Command::ExtSetEnabled { id, enabled } => self.ext_set_enabled(&id, enabled),
            Command::ExtSetPrivate { id, allow } => {
                self.ext_update_record(&id, |r| r.allow_private = allow)
            }
            Command::ExtSetPinned { id, pinned } => {
                self.ext_update_record(&id, |r| r.pinned = pinned)
            }
            Command::ExtRemove { id } => self.ext_remove(&id),
            Command::ExtReload { id } => self.ext_reload(&id),
            Command::ExtOpenOptions { id } => self.ext_open_options(&id),
            Command::ExtSetDeveloperMode { on } => self.ext_set_developer_mode(on),
            Command::ExtSetShortcut {
                id,
                command,
                accelerator,
            } => self.ext_set_shortcut(&id, &command, &accelerator),
            Command::ExtActionClick { id, anchor } => self.ext_action_click(&id, anchor),
            Command::ExtPopupClose => self.ext_close_popup(),
            Command::ExtMenuClick { ext, item, menu } => self.ext_menu_click(&ext, &item, menu),
            Command::ExtPermissionAnswer { request, grant } => {
                self.ext_permission_answer(request, grant)
            }
            Command::ExtToggleRuleset {
                id,
                ruleset,
                enabled,
            } => self.ext_toggle_ruleset(&id, &ruleset, enabled),

            // ---------------------------------------------------------- passwords, autofill, sync
            Command::VaultUnlock { password } => self.vault_unlock(&password),
            Command::VaultSetRecovery { password } => self.vault_set_recovery(password),
            Command::VaultReveal { request, id } => self.vault_reveal(request, id),
            Command::SaveLogin { login } => self.vault_save_login(login),
            Command::SaveCard { card } => self.vault_save_card(card),
            Command::SaveAddress { address } => self.vault_save_address(address),
            Command::VaultDelete { id } => self.vault_delete(&id),
            Command::NeverSaveRemove { origin } => self.vault_never_remove(&origin),
            Command::PasswordPromptAnswer {
                prompt,
                action,
                username,
                password,
            } => self.vault_prompt_answer(prompt, &action, username, password),
            Command::AutofillPick { tab, id } => self.vault_pick(tab, id),
            // File dialogs are modal: open them outside this dispatch.
            Command::PasswordsImport => self.defer(|b| b.vault_import()),
            Command::PasswordsExport => self.vault_export(),
            Command::SyncSetup { folder, passphrase } => self.sync_setup(folder, passphrase),
            Command::SyncNow => self.sync_now(),
            Command::SyncDisconnect => self.sync_disconnect(),
            Command::MakeDefaultBrowser => self.make_default_browser(),
            Command::UnregisterBrowser => self.unregister_browser(),
            Command::CheckForUpdates => self.check_for_updates(),
            Command::InstallUpdate => self.install_update(),
            Command::OpenCrashReports => self.open_crash_reports(),

            // ---------------------------------------------------------- misc
            Command::Panic => self.toggle_panic(),
            Command::PlaySound { sound } => self.play_sound(&sound),
            Command::KeyCapture { on } => self.key_capture.set(on),
            Command::OpenExternal { url } => {
                if let Ok(parsed) = url::Url::parse(&url)
                    && matches!(parsed.scheme(), "http" | "https" | "mailto")
                {
                    let _ = open::that_detached(parsed.as_str());
                }
            }
            Command::Log { level, message } => {
                let message: String = message.chars().take(2000).collect();
                match level.as_str() {
                    "error" => log::error!("[ui] {message}"),
                    "warn" => warn!("[ui] {message}"),
                    _ => debug!("[ui] {message}"),
                }
            }
        }
    }

    /// Side effects of settings that live outside the settings document.
    fn after_settings_change(&self, patch: &serde_json::Value) {
        let touches = |section: &str| patch.get(section).is_some();
        if touches("features") || touches("privacy") {
            self.restyle_all_pages();
        }
        if touches("privacy") {
            self.reload_filters();
        }
        if touches("control") {
            self.apply_limits();
        }
    }

    fn set_tab_zoom(&self, tab: &str, zoom: f32) {
        let zoom = self.session.borrow_mut().set_zoom(tab, zoom);
        if let Some(w) = self.webview_for_tab(tab) {
            w.set_page_zoom(zoom);
        }
        self.session_dirty.set(true);
        self.after_tab_update(&tab.to_owned());
    }

    pub(crate) fn page_action(
        &self,
        action: PageAction,
        tab: Option<String>,
        data: Option<String>,
    ) {
        let Some(tab) = self.target_tab(tab) else {
            return;
        };
        let (url, title) = self
            .session
            .borrow()
            .tab(&tab)
            .map(|t| (t.url.clone(), t.title.clone()))
            .unwrap_or_default();
        let engine = |a: ContextMenuAction| {
            self.context_menu_engine_action(a);
        };
        let open_new = |u: &str, background: bool| {
            let id = self.session.borrow_mut().new_tab(NewTab {
                url: u.to_owned(),
                background,
                workspace: None,
                relative_to: Some(tab.clone()),
                placement: Placement::AfterOpenerChildren,
                from_link: self.settings.borrow().tabs.auto_islands,
            });
            self.navigate_tab(&id, u, Transition::Link);
        };
        match action {
            PageAction::CopyAddress => self.copy_to_clipboard(&url),
            PageAction::ViewSource => {
                if url.starts_with("http") {
                    let encoded: String =
                        url::form_urlencoded::byte_serialize(url.as_bytes()).collect();
                    let source = format!("haku://source/?url={encoded}");
                    let id = self.session.borrow_mut().new_tab(NewTab {
                        url: source.clone(),
                        background: false,
                        workspace: None,
                        relative_to: Some(tab.clone()),
                        placement: Placement::After,
                        from_link: false,
                    });
                    self.navigate_tab(&id, &source, Transition::Link);
                }
            }
            PageAction::ToggleReader => {
                let reading = url.starts_with("haku://reader");
                if reading {
                    self.go_back(&tab);
                } else if url.starts_with("http") {
                    // The reader is an internal page shown over the tab's (parked) web page;
                    // Back returns to the page.
                    self.navigate_tab(&tab, &format!("haku://reader/?tab={tab}"), Transition::Link);
                }
            }
            PageAction::ToggleForceDark => {
                let host = url::Url::parse(&url)
                    .ok()
                    .and_then(|u| u.host_str().map(str::to_owned))
                    .unwrap_or_default();
                {
                    let mut s = self.settings.borrow_mut();
                    let f = &mut s.features;
                    if !f.force_dark_pages {
                        f.force_dark_pages = true;
                        f.force_dark_exceptions.retain(|d| *d != host);
                    } else if f.force_dark_exceptions.contains(&host) {
                        f.force_dark_exceptions.retain(|d| *d != host);
                    } else if !host.is_empty() {
                        f.force_dark_exceptions.push(host);
                    }
                }
                self.settings_changed();
                self.restyle_all_pages();
            }
            PageAction::SavePage => {
                if url.starts_with("http") {
                    // Save the page's document as served (HTML without subresources).
                    let _ = self.start_download(url.clone(), None, None, Some(tab.clone()));
                }
            }
            PageAction::Screenshot => self.capture_screenshot(&tab),
            PageAction::QrCode => self.emit(Event::Open {
                target: format!("qr:{url}"),
            }),
            PageAction::Translate => {
                let target = self.settings.borrow().languages.translate_target.clone();
                let encoded: String =
                    url::form_urlencoded::byte_serialize(url.as_bytes()).collect();
                open_new(
                    &format!(
                        "https://translate.google.com/translate?sl=auto&tl={target}&u={encoded}"
                    ),
                    false,
                );
            }
            PageAction::Wayback => open_new(&format!("https://web.archive.org/web/*/{url}"), false),
            PageAction::AddToSpeedDial => {
                if self
                    .bookmarks
                    .borrow_mut()
                    .add(ROOT_SPEED_DIAL, &title, Some(&url), None)
                    .is_ok()
                {
                    self.bookmarks_changed();
                    self.toast("Added to Speed Dial", "info");
                }
            }
            PageAction::Bookmark => {
                if self
                    .bookmarks
                    .borrow_mut()
                    .add(ROOT_OTHER, &title, Some(&url), None)
                    .is_ok()
                {
                    self.bookmarks_changed();
                    self.toast("Added to bookmarks", "info");
                }
            }
            PageAction::EngineCut => engine(ContextMenuAction::Cut),
            PageAction::EngineCopy => engine(ContextMenuAction::Copy),
            PageAction::EnginePaste => engine(ContextMenuAction::Paste),
            PageAction::EngineSelectAll => engine(ContextMenuAction::SelectAll),
            PageAction::CopyLink | PageAction::CopyImageLink => {
                if let Some(d) = data {
                    self.copy_to_clipboard(&d);
                }
            }
            PageAction::OpenLinkInNewTab | PageAction::OpenImageInNewTab => {
                if let Some(d) = data.filter(|d| d.starts_with("http")) {
                    open_new(&d, false);
                }
            }
            PageAction::OpenLinkInBackgroundTab => {
                if let Some(d) = data.filter(|d| d.starts_with("http")) {
                    open_new(&d, true);
                }
            }
            PageAction::OpenLinkInPrivateWindow => {
                if let Some(d) = data.filter(|d| d.starts_with("http")) {
                    self.spawn_window(Some(d), true);
                }
            }
            PageAction::SaveLinkAs | PageAction::SaveImageAs => {
                if let Some(d) = data.filter(|d| d.starts_with("http")) {
                    let _ = self.start_download(d, None, Some(url.clone()), Some(tab.clone()));
                }
            }
            PageAction::SearchSelection if data.is_none() => {
                // Read the page's selection, then search for it.
                if let Some(webview) = self.webview_for_tab(&tab) {
                    let browser = self.rc();
                    let tab2 = tab.clone();
                    webview.evaluate_javascript(
                        "String(window.getSelection() || '').slice(0, 500)",
                        move |result| {
                            if let Ok(servo::JSValue::String(text)) = result
                                && !text.trim().is_empty()
                            {
                                let tab3 = tab2.clone();
                                browser.defer(move |b| {
                                    b.page_action(
                                        PageAction::SearchSelection,
                                        Some(tab3),
                                        Some(text),
                                    )
                                });
                            }
                        },
                    );
                }
            }
            PageAction::SearchSelection => {
                if let Some(text) = data {
                    let target = {
                        let s = self.settings.borrow();
                        let engine = s
                            .search
                            .engines
                            .iter()
                            .find(|e| e.id == s.search.default_engine)
                            .cloned();
                        engine.map(|e| haku_core::omnibox::search_with(&e, &text))
                    };
                    if let Some(t) = target {
                        open_new(t.url(), false);
                    }
                }
            }
            PageAction::ScrollToTop => {
                if let Some(w) = self.webview_for_tab(&tab) {
                    w.evaluate_javascript(
                        "window.scrollTo({ top: 0, behavior: 'smooth' })",
                        |_| {},
                    );
                }
            }
            PageAction::CopySelection => {
                if let Some(text) = data {
                    self.copy_to_clipboard(&text);
                }
            }
        }
    }

    pub(crate) fn copy_to_clipboard(&self, text: &str) {
        // The UI shell owns a focused document and can use the async clipboard API; doing it
        // there keeps one clipboard path.
        let quoted = serde_json::to_string(text).unwrap_or_default();
        if let Some(ui) = self.ui_webview() {
            ui.evaluate_javascript(
                format!("window.__haku && window.__haku.copy({quoted})"),
                |_| {},
            );
        }
        if self.settings.borrow().accessibility.copy_confirmations {
            self.toast("Copied to clipboard", "info");
        }
    }

    fn capture_screenshot(&self, tab: &str) {
        let Some(webview) = self.webview_for_tab(tab) else {
            self.toast("Snapshots work on web pages.", "info");
            return;
        };
        let browser = self.rc();
        webview.take_screenshot(None, move |result| match result {
            Ok(image) => {
                let dir = dirs::picture_dir()
                    .unwrap_or_else(std::env::temp_dir)
                    .join("HakuCX");
                let _ = std::fs::create_dir_all(&dir);
                let name = format!("Snapshot {}.png", haku_core::now_ms());
                let path = dir.join(name);
                match image.save(&path) {
                    Ok(()) => {
                        browser.toast(format!("Snapshot saved to {}", path.display()), "info")
                    }
                    Err(e) => browser.toast(format!("Snapshot failed: {e}"), "error"),
                }
            }
            Err(e) => browser.toast(format!("Snapshot failed: {e:?}"), "error"),
        });
    }

    fn import_bookmarks(&self) {
        let Some(path) = rfd::FileDialog::new()
            .set_parent(&self.window)
            .add_filter("Bookmarks (HTML)", &["html", "htm"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read_to_string(&path) {
            Ok(html) => {
                let result = self
                    .bookmarks
                    .borrow_mut()
                    .import_netscape(&html, ROOT_OTHER, "Imported");
                match result {
                    Ok(n) => {
                        self.bookmarks_changed();
                        self.toast(format!("Imported {n} bookmarks"), "info");
                    }
                    Err(e) => self.toast(e.to_string(), "error"),
                }
            }
            Err(e) => self.toast(format!("Could not read the file: {e}"), "error"),
        }
    }

    fn export_bookmarks(&self) {
        let Some(path) = rfd::FileDialog::new()
            .set_parent(&self.window)
            .set_file_name("bookmarks.html")
            .add_filter("Bookmarks (HTML)", &["html"])
            .save_file()
        else {
            return;
        };
        let html = self.bookmarks.borrow().export_netscape();
        match std::fs::write(&path, html) {
            Ok(()) => self.toast("Bookmarks exported", "info"),
            Err(e) => self.toast(format!("Could not save: {e}"), "error"),
        }
    }

    fn pick_wallpaper(&self) {
        let Some(paths) = &self.paths else {
            self.toast(
                "Custom wallpapers are not saved in private windows.",
                "info",
            );
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_parent(&self.window)
            .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif"])
            .pick_file()
        else {
            return;
        };
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let dest_name = format!("custom-{}.{ext}", haku_core::now_ms());
        let dest = paths.wallpapers().join(&dest_name);
        match std::fs::copy(&path, &dest) {
            Ok(_) => {
                {
                    let mut s = self.settings.borrow_mut();
                    s.appearance.wallpaper = "custom".into();
                    s.appearance.custom_wallpaper = Some(dest_name);
                    s.appearance.show_wallpaper = true;
                }
                self.settings_changed();
            }
            Err(e) => self.toast(format!("Could not use that image: {e}"), "error"),
        }
    }

    fn clear_browsing_data(
        &self,
        from: Option<i64>,
        history: bool,
        cookies: bool,
        cache: bool,
        downloads: bool,
        site_settings: bool,
    ) {
        let from = from.unwrap_or(i64::MIN);
        if history {
            let _ = self.history.delete_range(from, i64::MAX);
            self.session.borrow_mut().closed.clear();
            self.session_changed();
        }
        if cookies {
            // Servo clears all cookies at once; a time range is not supported by the engine.
            self.servo.site_data_manager().clear_cookies(None);
            let all = servo::StorageType::all();
            let sites: Vec<String> = self
                .servo
                .site_data_manager()
                .site_data(all)
                .iter()
                .map(|s| s.name())
                .collect();
            let refs: Vec<&str> = sites.iter().map(String::as_str).collect();
            self.servo.site_data_manager().clear_site_data(&refs, all);
        }
        if cache {
            self.servo.network_manager().clear_cache();
        }
        if downloads {
            self.downloads
                .borrow_mut()
                .clear_finished((from != i64::MIN).then_some(i64::MAX));
            self.downloads_changed();
        }
        if site_settings {
            self.settings.borrow_mut().permissions.sites.clear();
            self.settings_changed();
        }
        self.toast("Browsing data deleted", "info");
    }

    fn toggle_panic(&self) {
        if self.panic_active.get() {
            if let Some(saved) = self.panic_restore.borrow_mut().take() {
                *self.session.borrow_mut() = saved;
            }
            self.panic_active.set(false);
            self.emit(Event::Panic { active: false });
            self.session_changed();
            return;
        }
        let urls = self.settings.borrow().panic.urls.clone();
        let Some(url) = urls.first().cloned() else {
            return;
        };
        *self.panic_restore.borrow_mut() = Some(self.session.borrow().clone());
        let mut fresh = haku_core::session::Session::default();
        fresh.tabs.clear();
        fresh.active_tabs.clear();
        let id = fresh.new_tab(NewTab::url(url.clone()));
        fresh.repair();
        *self.session.borrow_mut() = fresh;
        self.panic_active.set(true);
        self.emit(Event::Panic { active: true });
        self.session_changed();
        self.navigate_tab(&id, &url, Transition::Typed);
        info!("panic mode on");
    }
}

fn zoom_step(current: f32, up: bool) -> f32 {
    const STEPS: [f32; 13] = [
        0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0,
    ];
    const MORE: [f32; 3] = [2.5, 3.0, 4.0];
    let all: Vec<f32> = STEPS.iter().chain(MORE.iter()).copied().collect();
    if up {
        all.iter()
            .copied()
            .find(|s| *s > current + 0.001)
            .unwrap_or(5.0)
    } else {
        all.iter()
            .rev()
            .copied()
            .find(|s| *s < current - 0.001)
            .unwrap_or(0.25)
    }
}

fn uuid_short() -> String {
    format!("{:x}", haku_core::now_ms())
}

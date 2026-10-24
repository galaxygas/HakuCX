/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! The browser side of the `chrome.*` / `browser.*` methods (`extensions/api.js` is the page
//! side). Every call is checked here again: the page-side permission gating only decides
//! which namespaces exist.

use std::collections::BTreeSet;
use std::io::Cursor;

use base64::Engine as _;
use haku_core::bookmarks::{
    BookmarkNode, Bookmarks, ROOT_BAR, ROOT_OTHER, ROOT_SPEED_DIAL, ROOT_TRASH,
};
use haku_core::extensions::Manifest;
use haku_core::extensions::dnr;
use haku_core::extensions::manifest::{ContentScript, RunAt};
use haku_core::extensions::matching::{MatchPattern, UrlFilter};
use haku_core::extensions::registry::Quota;
use haku_core::history::Transition;
use haku_core::ipc::{DownloadAction, Event, Rect, WindowAction};
use haku_core::records::{DownloadRecord, DownloadState};
use haku_core::session::{NewTab, START_URL, TabId};
use serde_json::{Value, json};
use servo::JSValue;

use super::Browser;
use super::extensions::{Endpoint, PermissionRequest};
use super::tabs::Pane;
use crate::extensions::{self as ext, Alarm, ContextKind, MenuItem};

/// Methods content scripts may call (Chrome's content-script API surface).
const CONTENT_METHODS: &[&str] = &[
    "events.listen",
    "runtime.sendMessage",
    "runtime.reply",
    "runtime.getPlatformInfo",
    "runtime.getBrowserInfo",
    "port.connect",
    "port.post",
    "port.disconnect",
    "storage.get",
    "storage.set",
    "storage.remove",
    "storage.clear",
    "storage.getBytesInUse",
    "storage.managed",
    "i18n.getAcceptLanguages",
    "i18n.detectLanguage",
    "extension.isAllowedIncognitoAccess",
    "extension.isAllowedFileSchemeAccess",
    "scripting.result",
];

/// API namespaces that need a manifest permission.
const GATED: &[(&str, &[&str])] = &[
    ("contextMenus.", &["contextMenus", "menus"]),
    ("notifications.", &["notifications"]),
    ("cookies.", &["cookies"]),
    (
        "dnr.",
        &[
            "declarativeNetRequest",
            "declarativeNetRequestWithHostAccess",
            "declarativeNetRequestFeedback",
        ],
    ),
    ("webRequest.", &["webRequest"]),
    ("history.", &["history"]),
    ("downloads.", &["downloads"]),
    ("bookmarks.", &["bookmarks"]),
    ("offscreen.", &["offscreen"]),
    ("alarms.", &["alarms"]),
    ("idle.", &["idle"]),
];

const NO_RECEIVER: &str = "Could not establish connection. Receiving end does not exist.";

static NULL: Value = Value::Null;

fn arg(args: &[Value], i: usize) -> &Value {
    args.get(i).unwrap_or(&NULL)
}

fn str_of(v: &Value) -> Option<&str> {
    v.as_str()
}

fn strings(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// A script's completion value as JSON (DOM objects become `{}`, as in Chrome).
pub fn js_to_json(v: JSValue) -> Value {
    match v {
        JSValue::Undefined | JSValue::Null => Value::Null,
        JSValue::Boolean(b) => json!(b),
        JSValue::Number(n) => serde_json::Number::from_f64(n)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        JSValue::String(s) => json!(s),
        JSValue::Element(_) | JSValue::ShadowRoot(_) | JSValue::Frame(_) | JSValue::Window(_) => {
            json!({})
        }
        JSValue::Array(items) => Value::Array(items.into_iter().map(js_to_json).collect()),
        JSValue::Object(map) => {
            Value::Object(map.into_iter().map(|(k, v)| (k, js_to_json(v))).collect())
        }
    }
}

/// A script injection waiting for its result.
pub struct PendingScript {
    pub to: Endpoint,
    pub call: u64,
}

/// What to inject.
enum Injection {
    /// Source text: the result is its completion value.
    Code(String),
    /// A function's source and its arguments: the result is its (awaited) return value.
    Func(String, Vec<Value>),
}

impl Browser {
    /// Run one API call. `None` means the answer comes later (or the call needs none).
    pub(crate) fn ext_call(
        &self,
        from: &Endpoint,
        call: u64,
        method: &str,
        args: Vec<Value>,
    ) -> Option<Result<Value, String>> {
        if from.kind == ContextKind::Content && !CONTENT_METHODS.contains(&method) {
            return Some(Err(format!("{method} is not available in content scripts")));
        }
        if let Some((_, perms)) = GATED.iter().find(|(prefix, _)| method.starts_with(prefix)) {
            let allowed = self
                .ext
                .borrow()
                .running
                .get(&from.ext)
                .is_some_and(|r| perms.iter().any(|p| r.ext.has_permission(p)));
            if !allowed {
                return Some(Err(format!(
                    "{method} needs the \"{}\" permission",
                    perms[0]
                )));
            }
        }
        let a = &args;
        let id = from.ext.as_str();
        let result = match method {
            // ---------------------------------------------------------------- plumbing
            "events.listen" => {
                self.ext_listen(from, str_of(arg(a, 0)).unwrap_or_default(), arg(a, 1));
                return None;
            }
            "runtime.reply" => {
                let reply = arg(a, 0).as_u64().unwrap_or(0);
                self.ext_handle_reply(reply, arg(a, 1).clone(), arg(a, 2).as_bool().unwrap_or(false), arg(a, 3).as_bool().unwrap_or(false));
                return None;
            }
            "port.connect" => {
                let frame = arg(a, 3).as_i64();
                self.ext_port_connect(from, str_of(arg(a, 0)).unwrap_or_default().to_owned(), arg(a, 1), str_of(arg(a, 2)).unwrap_or_default().to_owned(), frame);
                return None;
            }
            "port.post" => {
                self.ext_port_post(from, str_of(arg(a, 0)).unwrap_or_default(), arg(a, 1).clone());
                return None;
            }
            "port.disconnect" => {
                self.ext_port_disconnect(from, str_of(arg(a, 0)).unwrap_or_default());
                return None;
            }
            "webRequest.decide" => {
                if let Some(request) = arg(a, 0).as_u64().filter(|r| *r != 0) {
                    self.ext_web_request_decided(request, arg(a, 1));
                }
                return None;
            }
            "webRequest.handlerBehaviorChanged" => Ok(Value::Null),
            "scripting.result" => {
                let pending = arg(a, 0).as_u64().and_then(|n| self.ext.borrow_mut().scripts.remove(&n));
                if let Some(p) = pending {
                    let result = match str_of(arg(a, 2)) {
                        Some(e) => Err(e.to_owned()),
                        None => Ok(json!([{ "frameId": 0, "result": arg(a, 1), "documentId": "" }])),
                    };
                    self.ext_reply(&p.to, p.call, result);
                }
                return None;
            }

            // ---------------------------------------------------------------- runtime
            "runtime.sendMessage" => return self.ext_send_message(from, call, arg(a, 0), arg(a, 1).clone()),
            "runtime.getPlatformInfo" => {
                let os = match std::env::consts::OS {
                    "windows" => "win",
                    "macos" => "mac",
                    other => other,
                };
                let arch = match std::env::consts::ARCH {
                    "x86_64" => "x86-64",
                    "aarch64" => "arm64",
                    "x86" => "x86-32",
                    other => other,
                };
                Ok(json!({ "os": os, "arch": arch, "nacl_arch": arch }))
            }
            "runtime.getBrowserInfo" => Ok(json!({ "name": "HakuCX", "vendor": "HakuCX", "version": env!("CARGO_PKG_VERSION"), "buildID": "" })),
            "runtime.openOptionsPage" => {
                self.ext_open_options(id);
                Ok(Value::Null)
            }
            "runtime.reload" => {
                let id = id.to_owned();
                self.defer(move |b| b.ext_reload(&id));
                return None;
            }
            "runtime.setUninstallURL" => {
                let url = str_of(arg(a, 0)).unwrap_or_default().to_owned();
                if !url.is_empty() && !(url.starts_with("http://") || url.starts_with("https://")) {
                    Err("Invalid URL: only http and https uninstall pages are allowed".to_owned())
                } else {
                    if let Some(r) = self.ext.borrow_mut().running.get_mut(id) {
                        r.uninstall_url = (!url.is_empty()).then_some(url);
                    }
                    Ok(Value::Null)
                }
            }
            // HakuCX does not update store-installed extensions automatically yet.
            "runtime.requestUpdateCheck" => Ok(json!({ "status": "no_update" })),
            "runtime.getBackgroundPage" => Err("getBackgroundPage is not available: HakuCX runs each extension page in its own context".to_owned()),
            "runtime.getContexts" => Ok(self.ext_contexts(id, arg(a, 0))),

            // ---------------------------------------------------------------- storage
            "storage.get" | "storage.set" | "storage.remove" | "storage.clear" | "storage.getBytesInUse" | "storage.setAccessLevel" => self.ext_storage(from, method, a),
            "storage.managed" => Ok(json!({})),

            // ---------------------------------------------------------------- i18n, extension
            "i18n.getAcceptLanguages" => Ok(json!(self.settings.borrow().languages.accept_languages)),
            // No language detector is built in.
            "i18n.detectLanguage" => Ok(json!({ "isReliable": false, "languages": [] })),
            "extension.isAllowedIncognitoAccess" => Ok(json!(self.ext.borrow().running.get(id).is_some_and(|r| r.ext.record.allow_private))),
            "extension.isAllowedFileSchemeAccess" => Ok(json!(false)),

            // ---------------------------------------------------------------- tabs
            "tabs.query" => Ok(self.ext_tabs_query(id, arg(a, 0))),
            "tabs.get" => self.ext_tab_arg(arg(a, 0)).map(|t| self.ext_tab_json(id, &t)),
            "tabs.getCurrent" => Ok(match (&from.tab, from.kind) {
                (Some(t), ContextKind::Tab) => self.ext_tab_json(id, t),
                _ => Value::Null,
            }),
            "tabs.create" => self.ext_tabs_create(from, arg(a, 0)),
            "tabs.update" => self.ext_tabs_update(from, arg(a, 0), arg(a, 1)),
            "tabs.remove" => {
                let mut tabs = Vec::new();
                for v in arg(a, 0).as_array().cloned().unwrap_or_default() {
                    match self.ext_tab_arg(&v) {
                        Ok(t) => tabs.push(t),
                        Err(e) => return Some(Err(e)),
                    }
                }
                for t in &tabs {
                    self.session.borrow_mut().close(t);
                }
                self.session_changed();
                Ok(Value::Null)
            }
            "tabs.reload" => self.ext_tab_or_active(arg(a, 0)).map(|t| {
                if let Some(w) = self.webview_for_tab(&t) {
                    w.reload();
                }
                Value::Null
            }),
            "tabs.duplicate" => self.ext_tab_arg(arg(a, 0)).and_then(|t| {
                let new = self.session.borrow_mut().duplicate(&t).ok_or_else(|| "Could not duplicate the tab".to_owned())?;
                let url = self.session.borrow().tab(&new).map(|t| t.url.clone()).unwrap_or_default();
                self.navigate_tab(&new, &url, Transition::Link);
                Ok(self.ext_tab_json(id, &new))
            }),
            "tabs.sendMessage" => return self.ext_tabs_send_message(from, call, arg(a, 0), arg(a, 1).clone(), arg(a, 2).as_i64()),
            "tabs.captureVisibleTab" => return self.ext_capture_visible(from, call, arg(a, 0)),
            "tabs.executeScript" => {
                let details = arg(a, 1);
                let tab = match self.ext_tab_or_active(arg(a, 0)) {
                    Ok(t) => t,
                    Err(e) => return Some(Err(e)),
                };
                let code = match (str_of(&details["code"]), str_of(&details["file"])) {
                    (Some(code), _) => Ok(code.to_owned()),
                    (None, Some(file)) => self.ext_read_text(id, file),
                    _ => Err("Either code or file must be specified".to_owned()),
                };
                return match code.and_then(|c| self.ext_check_frames(details).map(|_| c)) {
                    Ok(code) => self.ext_inject(from, call, &tab, Injection::Code(code), false),
                    Err(e) => Some(Err(e)),
                };
            }
            "tabs.insertCSS" | "tabs.removeCSS" => {
                let details = arg(a, 1);
                let tab = match self.ext_tab_or_active(arg(a, 0)) {
                    Ok(t) => t,
                    Err(e) => return Some(Err(e)),
                };
                let css = match (str_of(&details["code"]), str_of(&details["file"])) {
                    (Some(code), _) => Ok(code.to_owned()),
                    (None, Some(file)) => self.ext_read_text(id, file),
                    _ => Err("Either code or file must be specified".to_owned()),
                };
                css.and_then(|css| self.ext_css(id, &tab, &css, method == "tabs.insertCSS"))
            }
            "tabs.highlight" => {
                let indices = match arg(a, 0).get("tabs") {
                    Some(Value::Array(v)) => v.iter().filter_map(Value::as_u64).collect::<Vec<_>>(),
                    Some(v) => v.as_u64().into_iter().collect(),
                    None => Vec::new(),
                };
                let tab = indices.first().and_then(|i| self.session.borrow().tabs.get(*i as usize).map(|t| t.id.clone()));
                match tab {
                    Some(t) => {
                        self.session.borrow_mut().activate(&t);
                        self.session_changed();
                        Ok(self.ext_window_json(id, true))
                    }
                    None => Err("No tab at that index".to_owned()),
                }
            }
            "tabs.move" => {
                let ids = match arg(a, 0) {
                    Value::Array(v) => v.clone(),
                    v => vec![v.clone()],
                };
                let mut index = arg(a, 1).get("index").and_then(Value::as_i64).unwrap_or(-1);
                let mut moved = Vec::new();
                for v in ids {
                    let t = match self.ext_tab_arg(&v) {
                        Ok(t) => t,
                        Err(e) => return Some(Err(e)),
                    };
                    let len = self.session.borrow().tabs.len();
                    let to = if index < 0 || index as usize >= len { len.saturating_sub(1) } else { index as usize };
                    self.session.borrow_mut().move_tab(&t, to);
                    if index >= 0 {
                        index += 1;
                    }
                    moved.push(t);
                }
                self.session_changed();
                let list: Vec<Value> = moved.iter().map(|t| self.ext_tab_json(id, t)).collect();
                Ok(if list.len() == 1 { list[0].clone() } else { Value::Array(list) })
            }
            // Chrome's zoom factors are absolute; HakuCX keeps tab zoom relative to the default.
            "tabs.getZoom" => self.ext_tab_or_active(arg(a, 0)).map(|t| json!(self.session.borrow().tab(&t).map(|t| t.zoom).unwrap_or(1.0) * self.default_zoom())),
            "tabs.setZoom" => self.ext_tab_or_active(arg(a, 0)).map(|t| {
                let factor = arg(a, 1).as_f64().unwrap_or(0.0) as f32;
                let default = self.default_zoom();
                let zoom = self.session.borrow_mut().set_zoom(&t, if factor <= 0.0 { 1.0 } else { factor / default });
                if let Some(w) = self.webview_for_tab(&t) {
                    w.set_page_zoom(zoom * default);
                }
                self.after_tab_update(&t);
                self.session_changed();
                Value::Null
            }),
            "tabs.discard" => self.ext_tab_or_active(arg(a, 0)).map(|t| {
                if self.session.borrow_mut().snooze(&t) {
                    self.session_changed();
                }
                self.ext_tab_json(id, &t)
            }),
            "tabs.group" => self.ext_tabs_group(arg(a, 0)),
            "tabs.ungroup" => {
                for v in match arg(a, 0) {
                    Value::Array(v) => v.clone(),
                    v => vec![v.clone()],
                } {
                    if let Ok(t) = self.ext_tab_arg(&v) {
                        self.session.borrow_mut().remove_from_island(&t);
                    }
                }
                self.session_changed();
                Ok(Value::Null)
            }

            // ---------------------------------------------------------------- windows
            "windows.get" | "windows.getCurrent" => {
                let (window, options) = if method == "windows.get" { (arg(a, 0).as_i64(), arg(a, 1)) } else { (Some(1), arg(a, 0)) };
                if window.is_some_and(|w| w != 1 && w != -2) {
                    Err(format!("No window with id: {}.", window.unwrap_or_default()))
                } else {
                    Ok(self.ext_window_json(id, options.get("populate").and_then(Value::as_bool).unwrap_or(false)))
                }
            }
            "windows.getAll" => Ok(json!([self.ext_window_json(id, arg(a, 0).get("populate").and_then(Value::as_bool).unwrap_or(false))])),
            "windows.create" => self.ext_windows_create(from, arg(a, 0)),
            "windows.update" => {
                if arg(a, 0).as_i64().is_some_and(|w| w != 1 && w != -2) {
                    Err(format!("No window with id: {}.", arg(a, 0)))
                } else {
                    self.ext_windows_update(arg(a, 1));
                    Ok(self.ext_window_json(id, false))
                }
            }
            "windows.remove" => {
                if arg(a, 0).as_i64() == Some(1) {
                    self.window_action(WindowAction::Close);
                    Ok(Value::Null)
                } else {
                    Err(format!("No window with id: {}.", arg(a, 0)))
                }
            }

            // ---------------------------------------------------------------- action
            m if m.starts_with("action.") => self.ext_action_api(from, m, a),

            // ---------------------------------------------------------------- context menus
            "contextMenus.create" => self.ext_menu_create(id, arg(a, 0)),
            "contextMenus.update" => {
                let item = str_of(arg(a, 0)).unwrap_or_default().to_owned();
                let props = arg(a, 1).clone();
                let mut st = self.ext.borrow_mut();
                match st.running.get_mut(id).and_then(|r| r.menus.iter_mut().find(|m| m.id == item)) {
                    Some(m) => {
                        apply_menu_props(m, &props);
                        Ok(Value::Null)
                    }
                    None => Err(format!("Cannot find menu item with id {item}")),
                }
            }
            "contextMenus.remove" => {
                let item = str_of(arg(a, 0)).unwrap_or_default().to_owned();
                let mut st = self.ext.borrow_mut();
                let Some(r) = st.running.get_mut(id) else { return Some(Ok(Value::Null)) };
                if !r.menus.iter().any(|m| m.id == item) {
                    Err(format!("Cannot find menu item with id {item}"))
                } else {
                    // Remove the item and everything under it.
                    let mut doomed = BTreeSet::from([item]);
                    loop {
                        let before = doomed.len();
                        for m in &r.menus {
                            if m.parent.as_ref().is_some_and(|p| doomed.contains(p)) {
                                doomed.insert(m.id.clone());
                            }
                        }
                        if doomed.len() == before {
                            break;
                        }
                    }
                    r.menus.retain(|m| !doomed.contains(&m.id));
                    Ok(Value::Null)
                }
            }
            "contextMenus.removeAll" => {
                if let Some(r) = self.ext.borrow_mut().running.get_mut(id) {
                    r.menus.clear();
                }
                Ok(Value::Null)
            }
            "contextMenus.refresh" => Ok(Value::Null),

            // ---------------------------------------------------------------- alarms
            "alarms.create" => {
                let name = str_of(arg(a, 0)).unwrap_or_default().to_owned();
                let info = arg(a, 1);
                let now = haku_core::now_ms() as f64;
                let period = info.get("periodInMinutes").and_then(Value::as_f64);
                let when = info
                    .get("when")
                    .and_then(Value::as_f64)
                    .or_else(|| info.get("delayInMinutes").and_then(Value::as_f64).map(|d| now + d * 60_000.0))
                    .or_else(|| period.map(|p| now + p * 60_000.0))
                    .unwrap_or(now);
                if let Some(r) = self.ext.borrow_mut().running.get_mut(id) {
                    r.alarms.insert(name, Alarm { when_ms: when, period_minutes: period });
                }
                Ok(Value::Null)
            }
            "alarms.get" => {
                let name = str_of(arg(a, 0)).unwrap_or_default();
                Ok(self.ext.borrow().running.get(id).and_then(|r| r.alarms.get(name).map(|al| alarm_json(name, al))).unwrap_or(Value::Null))
            }
            "alarms.getAll" => Ok(Value::Array(self.ext.borrow().running.get(id).map(|r| r.alarms.iter().map(|(n, al)| alarm_json(n, al)).collect()).unwrap_or_default())),
            "alarms.clear" => {
                let name = str_of(arg(a, 0)).unwrap_or_default();
                Ok(json!(self.ext.borrow_mut().running.get_mut(id).is_some_and(|r| r.alarms.remove(name).is_some())))
            }
            "alarms.clearAll" => {
                let had = self.ext.borrow_mut().running.get_mut(id).is_some_and(|r| {
                    let had = !r.alarms.is_empty();
                    r.alarms.clear();
                    had
                });
                Ok(json!(had))
            }

            // ---------------------------------------------------------------- notifications
            "notifications.create" | "notifications.update" => {
                let note_id = str_of(arg(a, 0)).map(str::to_owned).unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
                let options = arg(a, 1);
                let name = self.ext.borrow().running.get(id).map(|r| r.ext.name.clone()).unwrap_or_default();
                let title = str_of(&options["title"]).unwrap_or_default();
                let message = str_of(&options["message"]).unwrap_or_default();
                self.emit(Event::Toast { text: format!("{name}: {title} {message}").trim().to_owned(), kind: "notification".into() });
                Ok(if method == "notifications.create" { json!(note_id) } else { json!(true) })
            }
            "notifications.clear" => Ok(json!(true)),
            "notifications.getAll" => Ok(json!({})),
            "notifications.getPermissionLevel" => Ok(json!("granted")),

            // ---------------------------------------------------------------- scripting
            "scripting.executeScript" => return self.ext_scripting_execute(from, call, arg(a, 0)),
            "scripting.insertCSS" | "scripting.removeCSS" => {
                let inj = arg(a, 0);
                let tab = match self.ext_tab_arg(&inj["target"]["tabId"]) {
                    Ok(t) => t,
                    Err(e) => return Some(Err(e)),
                };
                let css = match (str_of(&inj["css"]), inj.get("files")) {
                    (Some(css), _) => Ok(css.to_owned()),
                    (None, Some(files)) => strings(files).iter().map(|f| self.ext_read_text(id, f)).collect::<Result<Vec<_>, _>>().map(|v| v.join("\n")),
                    _ => Err("Exactly one of css or files must be specified".to_owned()),
                };
                css.and_then(|css| self.ext_css(id, &tab, &css, method == "scripting.insertCSS")).map(|_| Value::Null)
            }
            "scripting.registerContentScripts" | "scripting.updateContentScripts" => self.ext_register_scripts(id, arg(a, 0), method == "scripting.updateContentScripts"),
            "scripting.getRegisteredContentScripts" => {
                let ids = arg(a, 0).get("ids").map(strings);
                let st = self.ext.borrow();
                Ok(Value::Array(
                    st.running
                        .get(id)
                        .map(|r| {
                            r.registered_scripts
                                .iter()
                                .filter(|(k, _)| ids.as_ref().is_none_or(|ids| ids.contains(k)))
                                .map(|(k, s)| registered_script_json(k, s, r.persisted_scripts.contains(k)))
                                .collect()
                        })
                        .unwrap_or_default(),
                ))
            }
            "scripting.unregisterContentScripts" => {
                let ids = arg(a, 0).get("ids").map(strings);
                {
                    let mut st = self.ext.borrow_mut();
                    if let Some(r) = st.running.get_mut(id) {
                        if let Some(ids) = ids.as_ref()
                            && let Some(missing) = ids.iter().find(|i| !r.registered_scripts.contains_key(*i))
                        {
                            return Some(Err(format!("Nonexistent script ID '{missing}'")));
                        }
                        r.registered_scripts.retain(|k, _| ids.as_ref().is_some_and(|ids| !ids.contains(k)));
                        let keep: BTreeSet<String> = r.registered_scripts.keys().cloned().collect();
                        r.persisted_scripts.retain(|k| keep.contains(k));
                    }
                }
                self.ext_save_registered_scripts(id);
                self.refresh_content_script(id);
                Ok(Value::Null)
            }

            // ---------------------------------------------------------------- cookies
            m if m.starts_with("cookies.") => self.ext_cookies(id, m, arg(a, 0)),

            // ---------------------------------------------------------------- declarativeNetRequest
            m if m.starts_with("dnr.") => self.ext_dnr(id, m, arg(a, 0)),

            // ---------------------------------------------------------------- commands, permissions
            "commands.getAll" => {
                let st = self.ext.borrow();
                Ok(Value::Array(
                    st.running
                        .get(id)
                        .map(|r| {
                            r.ext
                                .manifest
                                .commands
                                .iter()
                                .map(|(name, spec)| {
                                    json!({
                                        "name": name,
                                        "description": r.ext.messages.localize(&spec.description, &r.ext.record.runtime_id),
                                        "shortcut": r.ext.record.shortcuts.get(name).cloned().or_else(|| spec.suggested_key.clone()).unwrap_or_default(),
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                ))
            }
            "permissions.getAll" => {
                let st = self.ext.borrow();
                let Some(r) = st.running.get(id) else { return Some(Ok(Value::Null)) };
                let mut perms: Vec<String> = r.ext.manifest.permissions.iter().filter(|p| !haku_core::extensions::manifest::is_host_pattern(p)).cloned().collect();
                perms.extend(r.ext.record.granted.iter().filter(|p| !haku_core::extensions::manifest::is_host_pattern(p)).cloned());
                Ok(json!({ "permissions": perms, "origins": r.ext.host_patterns() }))
            }
            "permissions.contains" => {
                let st = self.ext.borrow();
                let Some(r) = st.running.get(id) else { return Some(Ok(json!(false))) };
                let perms = strings(&arg(a, 0)["permissions"]);
                let origins = strings(&arg(a, 0)["origins"]);
                let hosts = r.ext.host_patterns();
                Ok(json!(perms.iter().all(|p| r.ext.has_permission(p)) && origins.iter().all(|o| origin_covered(&hosts, o))))
            }
            "permissions.request" => return self.ext_permissions_request(from, call, arg(a, 0)),
            "permissions.remove" => self.ext_permissions_remove(id, arg(a, 0)),

            // ---------------------------------------------------------------- history
            "history.search" => {
                let q = arg(a, 0);
                let text = str_of(&q["text"]).unwrap_or_default();
                // Chrome's default is the last 24 hours.
                let start = q.get("startTime").and_then(Value::as_f64).map(|v| v as i64).unwrap_or(haku_core::now_ms() - 86_400_000);
                let end = q.get("endTime").and_then(Value::as_f64).map(|v| v as i64).unwrap_or(i64::MAX);
                let max = q.get("maxResults").and_then(Value::as_u64).unwrap_or(100) as usize;
                self.history
                    .search_urls(text, start, end, if max == 0 { 100_000 } else { max })
                    .map(|rows| {
                        Value::Array(
                            rows.into_iter()
                                .map(|(row, u)| json!({ "id": row.to_string(), "url": u.url, "title": u.title, "lastVisitTime": u.last_visit, "visitCount": u.visit_count, "typedCount": u.typed_count }))
                                .collect(),
                        )
                    })
                    .map_err(|e| e.to_string())
            }
            "history.getVisits" => {
                let url = str_of(&arg(a, 0)["url"]).unwrap_or_default();
                self.history
                    .visits_for_url(url)
                    .map(|rows| {
                        Value::Array(
                            rows.into_iter()
                                .map(|(row, visit, at, t)| {
                                    let transition = match t {
                                        Transition::Typed => "typed",
                                        Transition::Bookmark => "auto_bookmark",
                                        Transition::Reload => "reload",
                                        Transition::Link => "link",
                                    };
                                    json!({ "id": row.to_string(), "visitId": visit.to_string(), "visitTime": at, "referringVisitId": "0", "transition": transition, "isLocal": true })
                                })
                                .collect(),
                        )
                    })
                    .map_err(|e| e.to_string())
            }
            "history.addUrl" => {
                let d = arg(a, 0);
                let url = str_of(&d["url"]).unwrap_or_default();
                if self.private {
                    Ok(Value::Null)
                } else if url::Url::parse(url).is_err() {
                    Err("Invalid URL".to_owned())
                } else {
                    let at = d.get("visitTime").and_then(Value::as_f64).map(|v| v as i64).unwrap_or_else(haku_core::now_ms);
                    let transition = if str_of(&d["transition"]) == Some("typed") { Transition::Typed } else { Transition::Link };
                    self.history.record_visit(url, str_of(&d["title"]).unwrap_or_default(), transition, at).map(|_| Value::Null).map_err(|e| e.to_string())
                }
            }
            "history.deleteUrl" => self.history.delete_url(str_of(&arg(a, 0)["url"]).unwrap_or_default()).map(|_| Value::Null).map_err(|e| e.to_string()),
            "history.deleteRange" => {
                let d = arg(a, 0);
                let from_ms = d.get("startTime").and_then(Value::as_f64).unwrap_or(0.0) as i64;
                let to_ms = d.get("endTime").and_then(Value::as_f64).unwrap_or(0.0) as i64;
                self.history.delete_range(from_ms, to_ms).map(|_| Value::Null).map_err(|e| e.to_string())
            }
            "history.deleteAll" => self.history.clear().map(|_| Value::Null).map_err(|e| e.to_string()),

            // ---------------------------------------------------------------- downloads
            m if m.starts_with("downloads.") => self.ext_downloads(id, m, a),

            // ---------------------------------------------------------------- management
            "management.getSelf" => Ok(self.ext_management_info(id)),
            "management.getAll" | "management.get" => {
                let others = self.ext.borrow().running.get(id).is_some_and(|r| r.ext.has_permission("management"));
                if method == "management.get" {
                    let want = str_of(arg(a, 0)).unwrap_or_default();
                    let target = self.ext.borrow().running.values().find(|r| r.ext.record.runtime_id == want || r.id() == want).map(|r| r.id().to_owned());
                    match target {
                        Some(t) if t == id || others => Ok(self.ext_management_info(&t)),
                        _ => Err(format!("Failed to find extension with id {want}.")),
                    }
                } else if others {
                    let ids: Vec<String> = self.ext.borrow().running.keys().cloned().collect();
                    Ok(Value::Array(ids.iter().map(|i| self.ext_management_info(i)).collect()))
                } else {
                    Ok(json!([self.ext_management_info(id)]))
                }
            }

            // ---------------------------------------------------------------- idle
            "idle.queryState" => {
                let seconds = arg(a, 0).as_f64().unwrap_or(60.0).max(15.0);
                Ok(json!(if idle_ms() as f64 >= seconds * 1000.0 { "idle" } else { "active" }))
            }
            "idle.setDetectionInterval" => {
                if let Some(r) = self.ext.borrow_mut().running.get_mut(id) {
                    r.idle_interval = arg(a, 0).as_f64().unwrap_or(60.0).max(15.0);
                }
                return None;
            }

            // ---------------------------------------------------------------- offscreen
            "offscreen.createDocument" => {
                let has = self.ext.borrow().running.get(id).is_some_and(|r| r.offscreen.is_some());
                if has {
                    Err("Only a single offscreen document may be created.".to_owned())
                } else {
                    let path = str_of(&arg(a, 0)["url"]).unwrap_or_default().to_owned();
                    self.ext_create_offscreen(id, &path)
                }
            }
            "offscreen.closeDocument" => {
                let webview = self.ext.borrow_mut().running.get_mut(id).and_then(|r| r.offscreen.take());
                match webview {
                    Some(w) => {
                        self.ext_context_gone(Some(w.id()), None);
                        Ok(Value::Null)
                    }
                    None => Err("No current offscreen document.".to_owned()),
                }
            }
            "offscreen.hasDocument" => Ok(json!(self.ext.borrow().running.get(id).is_some_and(|r| r.offscreen.is_some()))),

            // ---------------------------------------------------------------- bookmarks
            m if m.starts_with("bookmarks.") => self.ext_bookmarks(m, a),

            _ => Err(format!("{method} is not supported by HakuCX yet")),
        };
        Some(result)
    }

    // ------------------------------------------------------------------ events

    fn ext_listen(&self, from: &Endpoint, event: &str, extra: &Value) {
        if event.is_empty() || event.len() > 200 {
            return;
        }
        let mut st = self.ext.borrow_mut();
        let Some(r) = st.running.get_mut(&from.ext) else {
            return;
        };
        if from.kind == ContextKind::Content {
            if let Some(tab) = from.tab.clone() {
                r.content_listening
                    .entry(tab)
                    .or_default()
                    .insert((from.frame, event.to_owned()));
            }
            return;
        }
        r.listening
            .entry(from.webview.id())
            .or_default()
            .insert(event.to_owned());
        // Blocking webRequest listeners are asked from the background context only.
        if event == "webRequest.onBeforeRequest" && from.kind == ContextKind::Background {
            let filter = extra.get(0).cloned().unwrap_or(Value::Null);
            let urls = strings(&filter["urls"]);
            let types = strings(&filter["types"]);
            let blocking = strings(extra.get(1).unwrap_or(&NULL))
                .iter()
                .any(|s| s == "blocking");
            r.web_request
                .push((UrlFilter::new(&urls, &[], &[], &[]), types, blocking));
        }
    }

    // ------------------------------------------------------------------ messaging

    fn ext_send_message(
        &self,
        from: &Endpoint,
        call: u64,
        target: &Value,
        message: Value,
    ) -> Option<Result<Value, String>> {
        let sender = self.ext_sender_json(from);
        // To another extension: runtime.onMessageExternal in its extension pages.
        if let Some(other) = target.as_str().filter(|t| !t.is_empty()) {
            let other_id = self
                .ext
                .borrow()
                .running
                .values()
                .find(|r| r.ext.record.runtime_id == other || r.id() == other)
                .map(|r| r.id().to_owned());
            if other_id.as_deref() != Some(from.ext.as_str()) {
                let Some(other_id) = other_id else {
                    return Some(Err(NO_RECEIVER.to_owned()));
                };
                let targets =
                    self.ext_listeners(&other_id, "runtime.onMessageExternal", false, None);
                return if self.ext_ask(
                    targets,
                    "runtime.onMessageExternal",
                    vec![message, sender],
                    from,
                    call,
                ) {
                    None
                } else {
                    Some(Err(NO_RECEIVER.to_owned()))
                };
            }
        }
        let mut targets = self.ext_listeners(&from.ext, "runtime.onMessage", false, None);
        if from.kind != ContextKind::Content {
            targets.retain(|t| t.webview.id() != from.webview.id());
        }
        if self.ext_ask(
            targets,
            "runtime.onMessage",
            vec![message, sender],
            from,
            call,
        ) {
            None
        } else {
            Some(Err(NO_RECEIVER.to_owned()))
        }
    }

    fn ext_tabs_send_message(
        &self,
        from: &Endpoint,
        call: u64,
        tab: &Value,
        message: Value,
        frame: Option<i64>,
    ) -> Option<Result<Value, String>> {
        let tab = match self.ext_tab_arg(tab) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        let mut targets = self.ext_listeners(&from.ext, "runtime.onMessage", true, Some(&tab));
        if let Some(f) = frame {
            targets.retain(|t| t.frame == f);
        }
        let sender = self.ext_sender_json(from);
        if self.ext_ask(
            targets,
            "runtime.onMessage",
            vec![message, sender],
            from,
            call,
        ) {
            None
        } else {
            Some(Err(NO_RECEIVER.to_owned()))
        }
    }

    fn ext_contexts(&self, id: &str, filter: &Value) -> Value {
        let types = strings(&filter["contextTypes"]);
        let pages: Vec<(ContextKind, servo::WebView)> = self
            .ext
            .borrow()
            .pages
            .values()
            .filter(|(e, k, _)| {
                e == id && (types.is_empty() || types.iter().any(|t| t == k.name()))
            })
            .map(|(_, k, w)| (*k, w.clone()))
            .collect();
        let mut out = Vec::new();
        for (kind, webview) in pages {
            let url = webview.url().map(|u| u.to_string()).unwrap_or_default();
            let tab = self
                .tab_for_webview(&webview)
                .map(|t| self.ext_tab_int(&t))
                .unwrap_or(-1);
            out.push(json!({
                "contextType": kind.name(),
                "contextId": format!("{:?}", webview.id()),
                "tabId": tab,
                "windowId": if matches!(kind, ContextKind::Background | ContextKind::Offscreen) { -1 } else { 1 },
                "documentUrl": url,
                "documentOrigin": url::Url::parse(&url).map(|u| u.origin().ascii_serialization()).unwrap_or_default(),
                "frameId": 0,
                "incognito": self.private,
            }));
        }
        Value::Array(out)
    }

    // ------------------------------------------------------------------ storage

    fn ext_storage(&self, from: &Endpoint, method: &str, a: &[Value]) -> Result<Value, String> {
        let area = str_of(arg(a, 0)).unwrap_or("local").to_owned();
        if !matches!(area.as_str(), "local" | "sync" | "session") {
            return Err(format!("Unknown storage area {area}"));
        }
        let id = from.ext.as_str();
        let changes = {
            let mut st = self.ext.borrow_mut();
            let Some(r) = st.running.get_mut(id) else {
                return Ok(Value::Null);
            };
            if area == "session" && from.kind == ContextKind::Content && !r.session_for_content {
                return Err(
                    "Access to storage.session is not allowed from content scripts".to_owned(),
                );
            }
            let quota = match area.as_str() {
                "sync" => Quota::SYNC,
                "session" => Quota::SESSION,
                _ if r.ext.has_permission("unlimitedStorage") => Quota::UNLIMITED,
                _ => Quota::LOCAL,
            };
            let storage_area = r.storage.area_mut(&area).expect("checked above");
            match method {
                "storage.get" => return Ok(Value::Object(storage_area.get(arg(a, 1)))),
                "storage.getBytesInUse" => return Ok(json!(storage_area.bytes_in_use(arg(a, 1)))),
                "storage.set" => {
                    let items = arg(a, 1).as_object().cloned().unwrap_or_default();
                    storage_area.set(&items, quota)?
                }
                "storage.remove" => storage_area.remove(&strings(arg(a, 1))),
                "storage.clear" => storage_area.clear(),
                "storage.setAccessLevel" => {
                    if from.kind == ContextKind::Content {
                        return Err(
                            "Only trusted extension contexts can change storage access".to_owned()
                        );
                    }
                    if area != "session" {
                        return Err("Access level can only be set for storage.session".to_owned());
                    }
                    r.session_for_content =
                        str_of(&arg(a, 1)["accessLevel"]) == Some("TRUSTED_AND_UNTRUSTED_CONTEXTS");
                    return Ok(Value::Null);
                }
                _ => return Err(format!("{method} is not supported")),
            }
        };
        if changes.is_empty() {
            return Ok(Value::Null);
        }
        if area != "session" {
            self.ext_save_storage(id);
        }
        let include_content = area != "session"
            || self
                .ext
                .borrow()
                .running
                .get(id)
                .is_some_and(|r| r.session_for_content);
        let changes = Value::Object(changes);
        self.ext_event(
            id,
            "storage.onChanged",
            vec![changes.clone(), json!(area)],
            include_content,
        );
        self.ext_event(
            id,
            &format!("storage.{area}.onChanged"),
            vec![changes],
            include_content,
        );
        Ok(Value::Null)
    }

    // ------------------------------------------------------------------ tabs

    /// A tab from an API tab id.
    pub(crate) fn ext_tab_arg(&self, v: &Value) -> Result<TabId, String> {
        let n = v.as_i64().ok_or_else(|| "Invalid tab id".to_owned())?;
        self.ext_tab_of(n)
            .filter(|t| self.session.borrow().tab(t).is_some())
            .ok_or_else(|| format!("No tab with id: {n}."))
    }

    fn ext_tab_or_active(&self, v: &Value) -> Result<TabId, String> {
        if v.is_null() {
            self.active_tab_id()
                .ok_or_else(|| "No active tab".to_owned())
        } else {
            self.ext_tab_arg(v)
        }
    }

    fn ext_tabs_query(&self, id: &str, q: &Value) -> Value {
        let window_ok = match q.get("windowId").and_then(Value::as_i64) {
            Some(w) => w == 1 || w == -2,
            None => true,
        } && q.get("currentWindow").and_then(Value::as_bool) != Some(false)
            && q.get("lastFocusedWindow").and_then(Value::as_bool) != Some(false)
            && q.get("windowType")
                .and_then(Value::as_str)
                .is_none_or(|t| t == "normal");
        if !window_ok {
            return json!([]);
        }
        let patterns: Vec<MatchPattern> = strings(q.get("url").unwrap_or(&NULL))
            .iter()
            .filter_map(|p| MatchPattern::parse(p))
            .collect();
        let title = q
            .get("title")
            .and_then(Value::as_str)
            .and_then(haku_core::extensions::matching::Glob::parse);
        let ids: Vec<TabId> = self
            .session
            .borrow()
            .tabs
            .iter()
            .map(|t| t.id.clone())
            .collect();
        let mut out = Vec::new();
        for t in ids {
            let tab = self.ext_tab_json(id, &t);
            let field_ok = |key: &str| match q.get(key) {
                Some(Value::Null) | None => true,
                Some(want) => tab.get(key) == Some(want),
            };
            if ![
                "active",
                "pinned",
                "audible",
                "discarded",
                "highlighted",
                "status",
                "index",
                "autoDiscardable",
            ]
            .iter()
            .all(|k| field_ok(k))
            {
                continue;
            }
            if let Some(m) = q.get("muted").and_then(Value::as_bool)
                && tab["mutedInfo"]["muted"].as_bool() != Some(m)
            {
                continue;
            }
            if !patterns.is_empty()
                && !tab
                    .get("url")
                    .and_then(Value::as_str)
                    .is_some_and(|u| patterns.iter().any(|p| p.matches(u)))
            {
                continue;
            }
            if let Some(g) = title.as_ref()
                && !tab
                    .get("title")
                    .and_then(Value::as_str)
                    .is_some_and(|s| g.matches(s))
            {
                continue;
            }
            if q.get("groupId")
                .and_then(Value::as_i64)
                .is_some_and(|g| tab["groupId"].as_i64() != Some(g))
            {
                continue;
            }
            out.push(tab);
        }
        Value::Array(out)
    }

    /// Resolve a URL an extension wants to open. Relative URLs are inside the extension.
    fn ext_resolve_url(&self, ext_id: &str, url: &str) -> Result<String, String> {
        if url.is_empty() {
            return Ok(START_URL.to_owned());
        }
        let parsed = match url::Url::parse(url) {
            Ok(u) => u,
            Err(_) => url::Url::parse(&ext::base_url(ext_id))
                .and_then(|b| b.join(url))
                .map_err(|e| e.to_string())?,
        };
        match parsed.scheme() {
            "http" | "https" | "ftp" => {
                if let Some(other) = ext::id_of_url(&parsed)
                    && other != ext_id
                {
                    let accessible = self
                        .ext
                        .borrow()
                        .running
                        .get(&other)
                        .is_some_and(|r| r.ext.is_web_accessible(parsed.path(), None));
                    if !accessible {
                        return Err(format!("Cannot open {parsed}"));
                    }
                }
                Ok(parsed.to_string())
            }
            "about" if parsed.path() == "blank" => Ok("about:blank".to_owned()),
            "about" | "chrome" | "edge" | "opera" | "brave"
                if matches!(parsed.path(), "newtab" | "newtab/")
                    || parsed.host_str() == Some("newtab") =>
            {
                Ok(START_URL.to_owned())
            }
            other => Err(format!("Extensions cannot open {other}: URLs")),
        }
    }

    fn ext_tabs_create(&self, from: &Endpoint, p: &Value) -> Result<Value, String> {
        let url = self.ext_resolve_url(&from.ext, str_of(&p["url"]).unwrap_or_default())?;
        let mut spec = NewTab::url(url.clone());
        spec.background = p.get("active").and_then(Value::as_bool) == Some(false);
        if let Some(opener) = p.get("openerTabId").filter(|v| !v.is_null()) {
            spec.relative_to = Some(self.ext_tab_arg(opener)?);
        }
        let tab = self.session.borrow_mut().new_tab(spec);
        if let Some(index) = p.get("index").and_then(Value::as_u64) {
            self.session.borrow_mut().move_tab(&tab, index as usize);
        }
        if p.get("pinned").and_then(Value::as_bool) == Some(true) {
            self.session.borrow_mut().set_pinned(&tab, true);
        }
        self.navigate_tab(&tab, &url, Transition::Link);
        Ok(self.ext_tab_json(&from.ext, &tab))
    }

    fn ext_tabs_update(&self, from: &Endpoint, tab: &Value, p: &Value) -> Result<Value, String> {
        let tab = self.ext_tab_or_active(tab)?;
        if let Some(url) = str_of(&p["url"]) {
            let url = self.ext_resolve_url(&from.ext, url)?;
            self.navigate_tab(&tab, &url, Transition::Link);
        }
        if p.get("active").and_then(Value::as_bool) == Some(true)
            || p.get("highlighted").and_then(Value::as_bool) == Some(true)
        {
            self.session.borrow_mut().activate(&tab);
        }
        if let Some(pinned) = p.get("pinned").and_then(Value::as_bool) {
            self.session.borrow_mut().set_pinned(&tab, pinned);
        }
        if let Some(muted) = p.get("muted").and_then(Value::as_bool) {
            self.session.borrow_mut().set_muted(&tab, muted);
        }
        if p.get("autoDiscardable").is_some() || p.get("openerTabId").is_some() {
            // Accepted and ignored: HakuCX snoozes tabs by its own rules and tracks openers
            // itself.
        }
        self.session_changed();
        Ok(self.ext_tab_json(&from.ext, &tab))
    }

    fn ext_tabs_group(&self, options: &Value) -> Result<Value, String> {
        let tabs: Vec<TabId> = match &options["tabIds"] {
            Value::Array(v) => v
                .iter()
                .map(|t| self.ext_tab_arg(t))
                .collect::<Result<_, _>>()?,
            v => vec![self.ext_tab_arg(v)?],
        };
        if tabs.is_empty() {
            return Err("No tabs given".to_owned());
        }
        let island = match options.get("groupId").and_then(Value::as_i64) {
            Some(g) => {
                let island = self
                    .ext_tab_of(g)
                    .filter(|i| self.session.borrow().island(i).is_some())
                    .ok_or_else(|| format!("No group with id: {g}."))?;
                for t in &tabs {
                    self.session.borrow_mut().add_to_island(t, &island);
                }
                island
            }
            None => self
                .session
                .borrow_mut()
                .create_island(&tabs, None)
                .ok_or_else(|| "Could not group these tabs".to_owned())?,
        };
        self.session_changed();
        Ok(json!(self.ext_tab_int(&island)))
    }

    /// The tab's URL if the extension may script it (host permission or activeTab).
    fn ext_check_host(&self, ext_id: &str, tab: &TabId) -> Result<String, String> {
        let url = self.tab_meta(tab).map(|m| m.url).unwrap_or_default();
        if super::tabs::is_internal(&url)
            || self
                .views
                .borrow()
                .get(tab)
                .is_some_and(|v| v.internal.is_some())
        {
            return Err("Cannot access contents of a HakuCX page".to_owned());
        }
        let st = self.ext.borrow();
        let Some(r) = st.running.get(ext_id) else {
            return Err("Extension is not running".to_owned());
        };
        if url.starts_with(&r.base())
            || r.ext.can_access_url(&url)
            || r.active_tab_grants.contains(tab)
        {
            Ok(url)
        } else {
            Err(format!(
                "Cannot access contents of url \"{url}\". Extension manifest must request permission to access this host."
            ))
        }
    }

    fn ext_check_frames(&self, details: &Value) -> Result<(), String> {
        if details
            .get("frameId")
            .and_then(Value::as_i64)
            .is_some_and(|f| f != 0)
        {
            return Err("HakuCX can only inject into the top frame".to_owned());
        }
        Ok(())
    }

    fn ext_read_text(&self, ext_id: &str, file: &str) -> Result<String, String> {
        let path = self
            .ext
            .borrow()
            .running
            .get(ext_id)
            .and_then(|r| r.ext.file(file));
        path.and_then(|p| std::fs::read_to_string(p).ok())
            .ok_or_else(|| format!("Could not load file: '{file}'."))
    }

    /// Run script in a tab's top frame with a content-script API in scope.
    fn ext_inject(
        &self,
        from: &Endpoint,
        call: u64,
        tab: &TabId,
        injection: Injection,
        mv3: bool,
    ) -> Option<Result<Value, String>> {
        if let Err(e) = self.ext_check_host(&from.ext, tab) {
            return Some(Err(e));
        }
        let Some(webview) = self.webview_for_tab(tab) else {
            return Some(Err("The tab has no page loaded".to_owned()));
        };
        let locale = self.ext_ui_locale();
        let prelude = {
            let st = self.ext.borrow();
            let Some(r) = st.running.get(&from.ext) else {
                return Some(Err("Extension is not running".to_owned()));
            };
            format!(
                "const chrome = ({})({}); const browser = chrome;",
                ext::api_expr(),
                r.config(ContextKind::Content, &locale, self.private)
            )
        };
        match injection {
            Injection::Func(func, args) => {
                let script_id = self.ext.borrow_mut().next();
                self.ext.borrow_mut().scripts.insert(
                    script_id,
                    PendingScript {
                        to: from.clone(),
                        call,
                    },
                );
                let script = format!(
                    "{{ {prelude}\n(async () => ({func})(...{args}))().then((v) => chrome.__result({script_id}, v === undefined ? null : v, null), (e) => chrome.__result({script_id}, null, String((e && e.message) || e))); }}",
                    args = serde_json::to_string(&args).unwrap_or_else(|_| "[]".into())
                );
                let browser = self.rc();
                webview.evaluate_javascript(script, move |result| {
                    if let Err(e) = result {
                        let message = format!("{e:?}");
                        browser.defer(move |b| {
                            let pending = b.ext.borrow_mut().scripts.remove(&script_id);
                            if let Some(p) = pending {
                                b.ext_reply(&p.to, p.call, Err(message));
                            }
                        });
                    }
                });
            }
            Injection::Code(code) => {
                let script = format!("{{ {prelude}\n{code}\n}}");
                let browser = self.rc();
                let to = from.clone();
                webview.evaluate_javascript(script, move |result| {
                    let reply = match result {
                        Ok(v) => {
                            let v = js_to_json(v);
                            Ok(if mv3 {
                                json!([{ "frameId": 0, "result": v, "documentId": "" }])
                            } else {
                                json!([v])
                            })
                        }
                        Err(e) => Err(format!("{e:?}")),
                    };
                    browser.defer(move |b| b.ext_reply(&to, call, reply));
                });
            }
        }
        None
    }

    fn ext_scripting_execute(
        &self,
        from: &Endpoint,
        call: u64,
        inj: &Value,
    ) -> Option<Result<Value, String>> {
        let target = &inj["target"];
        let tab = match self.ext_tab_arg(&target["tabId"]) {
            Ok(t) => t,
            Err(e) => return Some(Err(e)),
        };
        if target
            .get("frameIds")
            .and_then(Value::as_array)
            .is_some_and(|f| f.iter().any(|x| x.as_i64() != Some(0)))
            || target
                .get("documentIds")
                .and_then(Value::as_array)
                .is_some_and(|d| !d.is_empty())
        {
            return Some(Err("HakuCX can only inject into the top frame".to_owned()));
        }
        let injection = match (str_of(&inj["func"]), inj.get("files")) {
            (Some(func), _) => Injection::Func(
                func.to_owned(),
                inj.get("args")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
            ),
            (None, Some(files)) => match strings(files)
                .iter()
                .map(|f| self.ext_read_text(&from.ext, f))
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(parts) => Injection::Code(parts.join("\n;\n")),
                Err(e) => return Some(Err(e)),
            },
            _ => {
                return Some(Err(
                    "Exactly one of 'func' and 'files' must be specified".to_owned()
                ));
            }
        };
        self.ext_inject(from, call, &tab, injection, true)
    }

    fn ext_css(&self, ext_id: &str, tab: &TabId, css: &str, insert: bool) -> Result<Value, String> {
        self.ext_check_host(ext_id, tab)?;
        let webview = self
            .webview_for_tab(tab)
            .ok_or_else(|| "The tab has no page loaded".to_owned())?;
        use sha2::Digest as _;
        let key = format!("{ext_id}-{:x}", sha2::Sha256::digest(css.as_bytes()));
        let key_js = serde_json::to_string(&key).unwrap_or_default();
        let script = if insert {
            format!(
                "(() => {{ const s = document.createElement('style'); s.setAttribute('data-haku-ext-css', {key_js}); s.textContent = {}; (document.head || document.documentElement).appendChild(s); }})()",
                serde_json::to_string(css).unwrap_or_default()
            )
        } else {
            format!(
                "(() => {{ for (const s of document.querySelectorAll('style[data-haku-ext-css]')) if (s.getAttribute('data-haku-ext-css') === {key_js}) {{ s.remove(); break; }} }})()"
            )
        };
        webview.evaluate_javascript(script, |_| {});
        Ok(Value::Null)
    }

    fn ext_capture_visible(
        &self,
        from: &Endpoint,
        call: u64,
        options: &Value,
    ) -> Option<Result<Value, String>> {
        let Some(tab) = self.active_tab_id() else {
            return Some(Err("No active tab".to_owned()));
        };
        let allowed = self.ext.borrow().running.get(&from.ext).is_some_and(|r| {
            r.active_tab_grants.contains(&tab)
                || r.ext.host_patterns().iter().any(|p| p == "<all_urls>")
        });
        if !allowed {
            return Some(Err(
                "Either the '<all_urls>' or 'activeTab' permission is required.".to_owned(),
            ));
        }
        let Some(webview) = self.pane_webview(Pane::Main) else {
            return Some(Err("Cannot capture a HakuCX page".to_owned()));
        };
        let png = str_of(&options["format"]) == Some("png");
        let quality = options
            .get("quality")
            .and_then(Value::as_u64)
            .unwrap_or(92)
            .clamp(1, 100) as u8;
        let browser = self.rc();
        let to = from.clone();
        webview.take_screenshot(None, move |result| {
            let reply = match result {
                Ok(image) => encode_capture(image, png, quality),
                Err(e) => Err(format!("Capture failed: {e:?}")),
            };
            browser.defer(move |b| b.ext_reply(&to, call, reply));
        });
        None
    }

    // ------------------------------------------------------------------ windows

    fn ext_window_json(&self, ext_id: &str, populate: bool) -> Value {
        let scale = self.scale() as f64;
        let size = self.window.outer_size();
        let pos = self.window.outer_position().unwrap_or_default();
        let state = if self.window.fullscreen().is_some() {
            "fullscreen"
        } else if self.window.is_minimized() == Some(true) {
            "minimized"
        } else if self.window.is_maximized() {
            "maximized"
        } else {
            "normal"
        };
        let mut v = json!({
            "id": 1,
            "focused": self.window.has_focus(),
            "top": (pos.y as f64 / scale).round(),
            "left": (pos.x as f64 / scale).round(),
            "width": (size.width as f64 / scale).round(),
            "height": (size.height as f64 / scale).round(),
            "incognito": self.private,
            "type": "normal",
            "state": state,
            "alwaysOnTop": false,
        });
        if populate {
            let ids: Vec<TabId> = self
                .session
                .borrow()
                .tabs
                .iter()
                .map(|t| t.id.clone())
                .collect();
            v["tabs"] = Value::Array(ids.iter().map(|t| self.ext_tab_json(ext_id, t)).collect());
        }
        v
    }

    /// `windows.create`: each HakuCX window is its own process, so extensions cannot address
    /// new windows. Private windows open as a new private window; other requests open their
    /// URLs as tabs in this window.
    fn ext_windows_create(&self, from: &Endpoint, p: &Value) -> Result<Value, String> {
        let urls: Vec<String> = strings(&p["url"])
            .iter()
            .map(|u| self.ext_resolve_url(&from.ext, u))
            .collect::<Result<_, _>>()?;
        if p.get("incognito").and_then(Value::as_bool) == Some(true) && !self.private {
            self.spawn_window(urls.first().cloned(), true);
            return Ok(
                json!({ "id": -1, "focused": true, "incognito": true, "type": "normal", "state": "normal", "alwaysOnTop": false, "tabs": [] }),
            );
        }
        for url in if urls.is_empty() {
            vec![START_URL.to_owned()]
        } else {
            urls
        } {
            let tab = self.session.borrow_mut().new_tab(NewTab::url(url.clone()));
            self.navigate_tab(&tab, &url, Transition::Link);
        }
        if let Some(t) = p.get("tabId").filter(|v| !v.is_null()) {
            let tab = self.ext_tab_arg(t)?;
            self.session.borrow_mut().activate(&tab);
            self.session_changed();
        }
        Ok(self.ext_window_json(&from.ext, true))
    }

    fn ext_windows_update(&self, p: &Value) {
        let scale = self.scale() as f64;
        match str_of(&p["state"]) {
            Some("minimized") => self.window.set_minimized(true),
            Some("maximized") => self.window.set_maximized(true),
            Some("fullscreen") => self
                .window
                .set_fullscreen(Some(winit::window::Fullscreen::Borderless(None))),
            Some("normal") => {
                self.window.set_fullscreen(None);
                self.window.set_maximized(false);
                self.window.set_minimized(false);
            }
            _ => {}
        }
        if let (Some(x), Some(y)) = (
            p.get("left").and_then(Value::as_f64),
            p.get("top").and_then(Value::as_f64),
        ) {
            self.window
                .set_outer_position(winit::dpi::PhysicalPosition::new(x * scale, y * scale));
        }
        if let (Some(w), Some(h)) = (
            p.get("width").and_then(Value::as_f64),
            p.get("height").and_then(Value::as_f64),
        ) {
            let _ = self
                .window
                .request_inner_size(winit::dpi::PhysicalSize::new(
                    (w * scale).max(200.0),
                    (h * scale).max(150.0),
                ));
        }
        if p.get("focused").and_then(Value::as_bool) == Some(true) {
            self.window.focus_window();
        }
        if p.get("drawAttention").and_then(Value::as_bool) == Some(true) {
            self.window
                .request_user_attention(Some(winit::window::UserAttentionType::Informational));
        }
    }

    // ------------------------------------------------------------------ action

    fn ext_action_api(&self, from: &Endpoint, method: &str, a: &[Value]) -> Result<Value, String> {
        let id = from.ext.as_str();
        let d = arg(a, 0);
        let tab = match d
            .get("tabId")
            .or(if d.is_number() { Some(d) } else { None })
            .filter(|v| !v.is_null())
        {
            Some(v) => Some(self.ext_tab_arg(v)?),
            None => None,
        };
        let name = method.trim_start_matches("action.");
        if name == "openPopup" {
            let width = self.window.inner_size().width as f32 / self.ui_scale();
            self.ext_action_click(
                id,
                Rect {
                    x: width - 48.0,
                    y: 40.0,
                    w: 32.0,
                    h: 32.0,
                },
            );
            return Ok(Value::Null);
        }
        let mut st = self.ext.borrow_mut();
        let Some(r) = st.running.get_mut(id) else {
            return Ok(Value::Null);
        };
        if name.starts_with("get") || name == "isEnabled" {
            let props = r.action_for(tab.as_ref());
            return Ok(match name {
                "getBadgeText" => json!(props.badge_text.unwrap_or_default()),
                "getBadgeBackgroundColor" => {
                    color_array(props.badge_color.as_deref().unwrap_or("#d93025"))
                }
                "getBadgeTextColor" => {
                    color_array(props.badge_text_color.as_deref().unwrap_or("#ffffff"))
                }
                "getTitle" => json!(props.title.unwrap_or_else(|| r.ext.name.clone())),
                "getPopup" => json!(
                    props
                        .popup
                        .filter(|p| !p.is_empty())
                        .map(|p| format!("{}{}", r.base(), p.trim_start_matches('/')))
                        .unwrap_or_default()
                ),
                "isEnabled" => json!(props.enabled.unwrap_or(true)),
                "getUserSettings" => json!({ "isOnToolbar": r.ext.record.pinned }),
                _ => return Err(format!("action.{name} is not supported")),
            });
        }
        let props = match tab.as_ref() {
            Some(t) => r.tab_action.entry(t.clone()).or_default(),
            None => &mut r.action,
        };
        match name {
            "setBadgeText" => {
                props.badge_text = d.get("text").and_then(Value::as_str).map(str::to_owned)
            }
            "setBadgeBackgroundColor" => props.badge_color = Some(css_color(&d["color"])?),
            "setBadgeTextColor" => props.badge_text_color = Some(css_color(&d["color"])?),
            "setTitle" => props.title = d.get("title").and_then(Value::as_str).map(str::to_owned),
            "setPopup" => props.popup = Some(str_of(&d["popup"]).unwrap_or_default().to_owned()),
            "setIcon" => {
                props.icon = match &d["path"] {
                    Value::String(p) => Some(p.clone()),
                    Value::Object(sizes) => {
                        let mut best: Option<(u32, String)> = None;
                        for (size, path) in sizes {
                            let (Ok(size), Some(path)) = (size.parse::<u32>(), path.as_str())
                            else {
                                continue;
                            };
                            if best.as_ref().is_none_or(|(s, _)| {
                                (*s < 32 && size > *s) || (size >= 32 && size < *s)
                            }) {
                                best = Some((size, path.to_owned()));
                            }
                        }
                        best.map(|(_, p)| p)
                    }
                    _ => None,
                }
            }
            "enable" => props.enabled = Some(true),
            "disable" => props.enabled = Some(false),
            _ => return Err(format!("action.{name} is not supported")),
        }
        st.info_dirty = true;
        Ok(Value::Null)
    }

    // ------------------------------------------------------------------ context menus

    fn ext_menu_create(&self, id: &str, p: &Value) -> Result<Value, String> {
        let item = str_of(&p["id"]).map(str::to_owned).ok_or_else(|| {
            "Extensions using event pages or service workers must pass an id parameter".to_owned()
        })?;
        let kind = str_of(&p["type"]).unwrap_or("normal").to_owned();
        if kind != "separator" && str_of(&p["title"]).is_none() {
            return Err("Title is required for menu items".to_owned());
        }
        let mut st = self.ext.borrow_mut();
        let Some(r) = st.running.get_mut(id) else {
            return Ok(Value::Null);
        };
        if r.menus.iter().any(|m| m.id == item) {
            return Err(format!("Cannot create item with duplicate id {item}"));
        }
        let parent = p.get("parentId").filter(|v| !v.is_null()).map(|v| {
            v.as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| v.to_string())
        });
        if let Some(parent) = parent.as_ref()
            && !r.menus.iter().any(|m| &m.id == parent)
        {
            return Err(format!("Cannot find menu item with id {parent}"));
        }
        let mut menu = MenuItem {
            id: item,
            parent,
            title: String::new(),
            kind,
            checked: false,
            enabled: true,
            visible: true,
            contexts: vec!["page".to_owned()],
            document_patterns: None,
            target_patterns: None,
        };
        apply_menu_props(&mut menu, p);
        r.menus.push(menu);
        Ok(Value::Null)
    }

    // ------------------------------------------------------------------ content scripts

    fn ext_register_scripts(
        &self,
        id: &str,
        scripts: &Value,
        update: bool,
    ) -> Result<Value, String> {
        let list = scripts.as_array().cloned().unwrap_or_default();
        {
            let mut st = self.ext.borrow_mut();
            let Some(r) = st.running.get_mut(id) else {
                return Ok(Value::Null);
            };
            let mut staged = r.registered_scripts.clone();
            let mut persisted = r.persisted_scripts.clone();
            for s in &list {
                let script_id = str_of(&s["id"])
                    .ok_or_else(|| "A script id is required".to_owned())?
                    .to_owned();
                if script_id.starts_with('_') {
                    return Err(format!("Script's ID '{script_id}' must not start with '_'"));
                }
                let existing = staged.get(&script_id).cloned();
                if update != existing.is_some() {
                    return Err(if update {
                        format!("Script with ID '{script_id}' does not exist")
                    } else {
                        format!("Duplicate script ID '{script_id}'")
                    });
                }
                let base = existing.unwrap_or(ContentScript {
                    matches: Vec::new(),
                    exclude_matches: Vec::new(),
                    include_globs: Vec::new(),
                    exclude_globs: Vec::new(),
                    js: Vec::new(),
                    css: Vec::new(),
                    run_at: RunAt::DocumentIdle,
                    all_frames: false,
                    match_about_blank: false,
                });
                let pick = |key: &str, old: &Vec<String>| {
                    s.get(key).map(strings).unwrap_or_else(|| old.clone())
                };
                let entry = ContentScript {
                    matches: pick("matches", &base.matches),
                    exclude_matches: pick("excludeMatches", &base.exclude_matches),
                    include_globs: base.include_globs.clone(),
                    exclude_globs: base.exclude_globs.clone(),
                    js: pick("js", &base.js),
                    css: pick("css", &base.css),
                    run_at: match str_of(&s["runAt"]) {
                        Some("document_start") => RunAt::DocumentStart,
                        Some("document_end") => RunAt::DocumentEnd,
                        Some("document_idle") => RunAt::DocumentIdle,
                        _ => base.run_at,
                    },
                    all_frames: s
                        .get("allFrames")
                        .and_then(Value::as_bool)
                        .unwrap_or(base.all_frames),
                    match_about_blank: s
                        .get("matchOriginAsFallback")
                        .and_then(Value::as_bool)
                        .unwrap_or(base.match_about_blank),
                };
                if entry.matches.is_empty() {
                    return Err(format!(
                        "Script with ID '{script_id}' must specify 'matches'"
                    ));
                }
                if entry.js.is_empty() && entry.css.is_empty() {
                    return Err(format!(
                        "Script with ID '{script_id}' must specify at least one js or css file"
                    ));
                }
                for file in entry.js.iter().chain(&entry.css) {
                    if r.ext.file(file).is_none_or(|p| !p.is_file()) {
                        return Err(format!("Could not load file: '{file}'."));
                    }
                }
                if s.get("persistAcrossSessions")
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
                {
                    persisted.insert(script_id.clone());
                } else {
                    persisted.remove(&script_id);
                }
                staged.insert(script_id, entry);
            }
            r.registered_scripts = staged;
            r.persisted_scripts = persisted;
        }
        self.ext_save_registered_scripts(id);
        self.refresh_content_script(id);
        Ok(Value::Null)
    }

    // ------------------------------------------------------------------ cookies

    fn ext_cookies(&self, id: &str, method: &str, d: &Value) -> Result<Value, String> {
        let hosts = self
            .ext
            .borrow()
            .running
            .get(id)
            .map(|r| r.ext.host_patterns())
            .unwrap_or_default();
        let can = |url: &str| {
            hosts
                .iter()
                .filter_map(|p| MatchPattern::parse(p))
                .any(|p| p.matches(url))
        };
        let manager = self.servo.site_data_manager();
        match method {
            "cookies.getAllCookieStores" => {
                let ids: Vec<TabId> = self
                    .session
                    .borrow()
                    .tabs
                    .iter()
                    .map(|t| t.id.clone())
                    .collect();
                let tab_ids: Vec<i64> = ids.iter().map(|t| self.ext_tab_int(t)).collect();
                Ok(json!([{ "id": "0", "tabIds": tab_ids, "incognito": self.private }]))
            }
            "cookies.get" | "cookies.remove" | "cookies.set" => {
                let url_text = str_of(&d["url"]).ok_or_else(|| "A url is required".to_owned())?;
                let url = url::Url::parse(url_text)
                    .map_err(|_| format!("Invalid url: \"{url_text}\""))?;
                if !can(url.as_str()) {
                    return Err(format!(
                        "No host permissions for cookies at url: \"{url}\"."
                    ));
                }
                let name = str_of(&d["name"]).unwrap_or_default().to_owned();
                match method {
                    "cookies.get" => Ok(manager
                        .cookies_for_url(url.clone(), servo::CookieSource::HTTP)
                        .iter()
                        .find(|c| c.name() == name)
                        .map(|c| cookie_json(c, &url))
                        .unwrap_or(Value::Null)),
                    "cookies.remove" => {
                        let existing = manager
                            .cookies_for_url(url.clone(), servo::CookieSource::HTTP)
                            .into_iter()
                            .find(|c| c.name() == name);
                        match existing {
                            Some(c) => {
                                let mut gone = cookie::Cookie::new(name.clone(), "");
                                if let Some(domain) = c.domain() {
                                    gone.set_domain(domain.to_owned());
                                }
                                gone.set_path(c.path().unwrap_or("/").to_owned());
                                gone.set_expires(cookie::time::OffsetDateTime::UNIX_EPOCH);
                                manager.set_cookie_for_url(url.clone(), gone, None);
                                Ok(json!({ "url": url.as_str(), "name": name, "storeId": "0" }))
                            }
                            None => Ok(Value::Null),
                        }
                    }
                    _ => {
                        let mut c = cookie::Cookie::new(
                            name.clone(),
                            str_of(&d["value"]).unwrap_or_default().to_owned(),
                        );
                        if let Some(domain) = str_of(&d["domain"]) {
                            c.set_domain(domain.to_owned());
                        }
                        c.set_path(str_of(&d["path"]).unwrap_or("/").to_owned());
                        c.set_secure(d.get("secure").and_then(Value::as_bool).unwrap_or(false));
                        c.set_http_only(
                            d.get("httpOnly").and_then(Value::as_bool).unwrap_or(false),
                        );
                        match str_of(&d["sameSite"]) {
                            Some("no_restriction") => c.set_same_site(cookie::SameSite::None),
                            Some("lax") => c.set_same_site(cookie::SameSite::Lax),
                            Some("strict") => c.set_same_site(cookie::SameSite::Strict),
                            _ => {}
                        }
                        if let Some(at) = d.get("expirationDate").and_then(Value::as_f64) {
                            let at = cookie::time::OffsetDateTime::from_unix_timestamp(at as i64)
                                .map_err(|e| e.to_string())?;
                            c.set_expires(at);
                        }
                        let result = cookie_json(&c, &url);
                        manager.set_cookie_for_url(url, c, None);
                        Ok(result)
                    }
                }
            }
            "cookies.getAll" => {
                // Servo looks cookies up by URL; without a URL, ask for each site it has data
                // for. Cookies scoped to a sub-path are only found when `url` names that path.
                let mut urls: Vec<url::Url> = Vec::new();
                if let Some(u) = str_of(&d["url"]) {
                    urls.push(url::Url::parse(u).map_err(|_| format!("Invalid url: \"{u}\""))?);
                } else if let Some(domain) = str_of(&d["domain"]) {
                    let domain = domain.trim_start_matches('.');
                    for scheme in ["https", "http"] {
                        if let Ok(u) = url::Url::parse(&format!("{scheme}://{domain}/")) {
                            urls.push(u);
                        }
                    }
                } else {
                    for site in manager.site_data(servo::StorageType::Cookies) {
                        for scheme in ["https", "http"] {
                            if let Ok(u) = url::Url::parse(&format!("{scheme}://{}/", site.name()))
                            {
                                urls.push(u);
                            }
                        }
                    }
                }
                let mut seen = BTreeSet::new();
                let mut out = Vec::new();
                for url in urls.into_iter().filter(|u| can(u.as_str())) {
                    for c in manager.cookies_for_url(url.clone(), servo::CookieSource::HTTP) {
                        let v = cookie_json(&c, &url);
                        let key = (
                            v["domain"].to_string(),
                            v["path"].to_string(),
                            v["name"].to_string(),
                        );
                        let keep = str_of(&d["name"]).is_none_or(|n| c.name() == n)
                            && str_of(&d["path"]).is_none_or(|p| v["path"] == p)
                            && d.get("secure")
                                .and_then(Value::as_bool)
                                .is_none_or(|s| v["secure"] == s)
                            && d.get("session")
                                .and_then(Value::as_bool)
                                .is_none_or(|s| v["session"] == s)
                            && str_of(&d["domain"]).is_none_or(|dm| {
                                v["domain"].as_str().is_some_and(|cd| {
                                    cd.trim_start_matches('.')
                                        .ends_with(dm.trim_start_matches('.'))
                                })
                            });
                        if keep && seen.insert(key) {
                            out.push(v);
                        }
                    }
                }
                Ok(Value::Array(out))
            }
            _ => Err(format!("{method} is not supported")),
        }
    }

    // ------------------------------------------------------------------ declarativeNetRequest

    fn ext_dnr(&self, id: &str, method: &str, o: &Value) -> Result<Value, String> {
        let name = method.trim_start_matches("dnr.");
        let result = {
            let mut st = self.ext.borrow_mut();
            let Some(r) = st.running.get_mut(id) else {
                return Ok(Value::Null);
            };
            match name {
                "updateDynamicRules" | "updateSessionRules" => {
                    let session = name == "updateSessionRules";
                    let limit = if session { 5000 } else { 30000 };
                    let remove: Vec<i64> = o
                        .get("removeRuleIds")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(Value::as_i64).collect())
                        .unwrap_or_default();
                    let add = o
                        .get("addRules")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    for rule in &add {
                        dnr::Rule::parse(rule).map_err(|e| e.to_string())?;
                    }
                    let list = if session {
                        &mut r.session_rules
                    } else {
                        &mut r.dynamic_rules
                    };
                    let mut next: Vec<Value> = list
                        .iter()
                        .filter(|v| !v["id"].as_i64().is_some_and(|i| remove.contains(&i)))
                        .cloned()
                        .collect();
                    for rule in add {
                        let rid = rule["id"].as_i64().unwrap_or_default();
                        if next.iter().any(|v| v["id"].as_i64() == Some(rid)) {
                            return Err(format!("Rule with id {rid} does not have a unique ID."));
                        }
                        next.push(rule);
                    }
                    if next.len() > limit {
                        return Err(format!(
                            "The number of {} rules exceeds the limit of {limit}.",
                            if session { "session" } else { "dynamic" }
                        ));
                    }
                    *list = next;
                    r.rebuild_rules();
                    Ok((Value::Null, !session))
                }
                "getDynamicRules" | "getSessionRules" => {
                    let list = if name == "getSessionRules" {
                        &r.session_rules
                    } else {
                        &r.dynamic_rules
                    };
                    let ids: Option<Vec<i64>> = o
                        .get("ruleIds")
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(Value::as_i64).collect());
                    Ok((
                        Value::Array(
                            list.iter()
                                .filter(|v| {
                                    ids.as_ref().is_none_or(|ids| {
                                        v["id"].as_i64().is_some_and(|i| ids.contains(&i))
                                    })
                                })
                                .cloned()
                                .collect(),
                        ),
                        false,
                    ))
                }
                "getEnabledRulesets" => Ok((
                    json!(
                        r.ext
                            .manifest
                            .rulesets
                            .iter()
                            .filter(|rs| r
                                .ext
                                .record
                                .ruleset_overrides
                                .get(&rs.id)
                                .copied()
                                .unwrap_or(rs.enabled))
                            .map(|rs| rs.id.clone())
                            .collect::<Vec<_>>()
                    ),
                    false,
                )),
                "updateEnabledRulesets" => {
                    let enable = strings(o.get("enableRulesetIds").unwrap_or(&NULL));
                    let disable = strings(o.get("disableRulesetIds").unwrap_or(&NULL));
                    for rs in enable.iter().chain(&disable) {
                        if !r.ext.manifest.rulesets.iter().any(|x| &x.id == rs) {
                            return Err(format!("Invalid ruleset id: {rs}."));
                        }
                    }
                    for rs in disable {
                        r.ext.record.ruleset_overrides.insert(rs, false);
                    }
                    for rs in enable {
                        r.ext.record.ruleset_overrides.insert(rs, true);
                    }
                    let overrides = r.ext.record.ruleset_overrides.clone();
                    r.rebuild_rules();
                    if let Some(rec) = st.list.get_mut(id) {
                        rec.ruleset_overrides = overrides;
                    }
                    st.info_dirty = true;
                    Ok((Value::Null, true))
                }
                "updateStaticRules" => {
                    let ruleset = str_of(&o["rulesetId"]).unwrap_or_default().to_owned();
                    if !r.ext.manifest.rulesets.iter().any(|x| x.id == ruleset) {
                        return Err(format!("Invalid ruleset id: {ruleset}."));
                    }
                    let set = r.disabled_static.entry(ruleset).or_default();
                    for i in o
                        .get("disableRuleIds")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_i64)
                    {
                        set.insert(i);
                    }
                    for i in o
                        .get("enableRuleIds")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_i64)
                    {
                        set.remove(&i);
                    }
                    r.rebuild_rules();
                    Ok((Value::Null, true))
                }
                "getDisabledRuleIds" => Ok((
                    json!(
                        r.disabled_static
                            .get(str_of(&o["rulesetId"]).unwrap_or_default())
                            .map(|s| s.iter().copied().collect::<Vec<_>>())
                            .unwrap_or_default()
                    ),
                    false,
                )),
                "getAvailableStaticRuleCount" => {
                    let used: usize = r
                        .static_rules
                        .iter()
                        .filter(|(k, _)| {
                            r.ext
                                .record
                                .ruleset_overrides
                                .get(*k)
                                .copied()
                                .unwrap_or_else(|| {
                                    r.ext
                                        .manifest
                                        .rulesets
                                        .iter()
                                        .any(|x| &x.id == *k && x.enabled)
                                })
                        })
                        .map(|(_, v)| v.rules.len())
                        .sum();
                    Ok((json!(300_000usize.saturating_sub(used)), false))
                }
                "isRegexSupported" => {
                    let regex = str_of(&o["regex"]).unwrap_or_default();
                    let case = o
                        .get("isCaseSensitive")
                        .and_then(Value::as_bool)
                        .unwrap_or(true);
                    Ok((
                        match regex::RegexBuilder::new(regex)
                            .case_insensitive(!case)
                            .size_limit(2 * 1024 * 1024)
                            .build()
                        {
                            Ok(_) => json!({ "isSupported": true }),
                            Err(regex::Error::CompiledTooBig(_)) => {
                                json!({ "isSupported": false, "reason": "memoryLimitExceeded" })
                            }
                            Err(_) => json!({ "isSupported": false, "reason": "syntaxError" }),
                        },
                        false,
                    ))
                }
                "setExtensionActionOptions" => {
                    if let Some(show) = o
                        .get("displayActionCountAsBadgeText")
                        .and_then(Value::as_bool)
                    {
                        r.badge_from_rule_count = show;
                    }
                    if let Some(update) = o.get("tabUpdate") {
                        let tab = update["tabId"]
                            .as_i64()
                            .and_then(|t| st.int_tabs.get(&t).cloned())
                            .ok_or_else(|| "Invalid tab id".to_owned())?;
                        let r = st.running.get_mut(id).expect("checked above");
                        let count = r.matched.entry(tab).or_default();
                        *count = (*count as i64 + update["increment"].as_i64().unwrap_or(0)).max(0)
                            as u32;
                    }
                    st.info_dirty = true;
                    Ok((Value::Null, false))
                }
                "getMatchedRules" => {
                    let can = r.ext.has_permission("declarativeNetRequestFeedback")
                        || r.ext.has_permission("activeTab");
                    if !can {
                        return Err(
                            "The extension must have the declarativeNetRequestFeedback permission"
                                .to_owned(),
                        );
                    }
                    let tab = o.get("tabId").and_then(Value::as_i64);
                    let min = o.get("minTimeStamp").and_then(Value::as_f64).unwrap_or(0.0);
                    let rules: Vec<Value> = r
                        .matched_rules
                        .iter()
                        .filter(|(t, _, _, at)| tab.is_none_or(|x| x == *t) && *at >= min)
                        .map(|(t, rule, set, at)| json!({ "rule": { "ruleId": rule, "rulesetId": set }, "tabId": t, "timeStamp": at }))
                        .collect();
                    Ok((json!({ "rulesMatchedInfo": rules }), false))
                }
                "testMatchOutcome" => {
                    let url = str_of(&o["url"]).unwrap_or_default();
                    let initiator = str_of(&o["initiator"])
                        .and_then(|i| url::Url::parse(i).ok())
                        .and_then(|u| u.host_str().map(str::to_owned));
                    let req = dnr::Request {
                        url,
                        initiator_host: initiator.as_deref(),
                        resource_type: str_of(&o["type"]).unwrap_or("main_frame"),
                        method: str_of(&o["method"]).unwrap_or("get"),
                        tab_id: o.get("tabId").and_then(Value::as_i64).unwrap_or(-1),
                    };
                    let matched = r.rules.evaluate(&req, &r.base()).map(|outcome| vec![json!({ "ruleId": outcome.rule_id, "rulesetId": r.ruleset_of(outcome.rule_id) })]).unwrap_or_default();
                    Ok((json!({ "matchedRules": matched }), false))
                }
                _ => Err(format!("declarativeNetRequest.{name} is not supported")),
            }
        };
        let (value, persist) = result?;
        if persist {
            self.ext_save_dynamic_rules(id);
            self.save_extension_list();
        }
        Ok(value)
    }

    // ------------------------------------------------------------------ permissions

    fn ext_permissions_request(
        &self,
        from: &Endpoint,
        call: u64,
        p: &Value,
    ) -> Option<Result<Value, String>> {
        let perms = strings(&p["permissions"]);
        let origins = strings(&p["origins"]);
        let (name, missing_perms, missing_origins) = {
            let st = self.ext.borrow();
            let Some(r) = st.running.get(&from.ext) else {
                return Some(Ok(json!(false)));
            };
            let optional = &r.ext.manifest.optional_permissions;
            for item in perms.iter().chain(&origins) {
                let declared = optional.contains(item)
                    || (haku_core::extensions::manifest::is_host_pattern(item)
                        && origin_covered(
                            &optional
                                .iter()
                                .filter(|o| haku_core::extensions::manifest::is_host_pattern(o))
                                .cloned()
                                .collect::<Vec<_>>(),
                            item,
                        ));
                if !declared
                    && !r.ext.manifest.has_permission(item)
                    && !r.ext.manifest.host_permissions.contains(item)
                {
                    return Some(Err(format!(
                        "Only permissions specified in the manifest may be requested. ({item})"
                    )));
                }
            }
            let hosts = r.ext.host_patterns();
            let missing_perms: Vec<String> = perms
                .iter()
                .filter(|x| !r.ext.has_permission(x))
                .cloned()
                .collect();
            let missing_origins: Vec<String> = origins
                .iter()
                .filter(|o| !origin_covered(&hosts, o))
                .cloned()
                .collect();
            (r.ext.name.clone(), missing_perms, missing_origins)
        };
        if missing_perms.is_empty() && missing_origins.is_empty() {
            return Some(Ok(json!(true)));
        }
        let warnings = Manifest::from_value(json!({ "manifest_version": 3, "name": "-", "version": "1", "permissions": missing_perms, "host_permissions": missing_origins }))
            .map(|m| m.permission_warnings())
            .unwrap_or_default();
        let request = self.next_id();
        self.ext.borrow_mut().permission_requests.insert(
            request,
            PermissionRequest {
                ext: from.ext.clone(),
                permissions: missing_perms,
                origins: missing_origins,
                to: from.clone(),
                call,
            },
        );
        self.emit(Event::ExtPermissionPrompt {
            request,
            name,
            warnings,
        });
        None
    }

    pub(crate) fn ext_permission_answer(&self, request: u32, grant: bool) {
        let Some(PermissionRequest {
            ext: id,
            permissions: perms,
            origins,
            to,
            call,
        }) = self.ext.borrow_mut().permission_requests.remove(&request)
        else {
            return;
        };
        if grant {
            self.ext_update_record(&id, |r| {
                for p in perms.iter().chain(&origins) {
                    if !r.granted.contains(p) {
                        r.granted.push(p.clone());
                    }
                }
            });
            self.ext_event(
                &id,
                "permissions.onAdded",
                vec![json!({ "permissions": perms, "origins": origins })],
                false,
            );
        }
        if call != 0 {
            self.ext_reply(&to, call, Ok(json!(grant)));
        }
    }

    fn ext_permissions_remove(&self, id: &str, p: &Value) -> Result<Value, String> {
        let perms = strings(&p["permissions"]);
        let origins = strings(&p["origins"]);
        {
            let st = self.ext.borrow();
            let Some(r) = st.running.get(id) else {
                return Ok(json!(false));
            };
            if perms.iter().any(|x| r.ext.manifest.permissions.contains(x))
                || origins
                    .iter()
                    .any(|o| r.ext.manifest.host_permissions.contains(o))
            {
                return Err("You cannot remove required permissions.".to_owned());
            }
        }
        let mut removed = false;
        self.ext_update_record(id, |r| {
            let before = r.granted.len();
            r.granted
                .retain(|g| !perms.contains(g) && !origins.contains(g));
            removed = r.granted.len() != before;
        });
        if removed {
            self.ext_event(
                id,
                "permissions.onRemoved",
                vec![json!({ "permissions": perms, "origins": origins })],
                false,
            );
        }
        Ok(json!(removed))
    }

    // ------------------------------------------------------------------ downloads

    fn ext_downloads(&self, id: &str, method: &str, a: &[Value]) -> Result<Value, String> {
        let record_of = |v: &Value| -> Result<String, String> {
            let n = v.as_i64().ok_or_else(|| "Invalid download id".to_owned())?;
            self.ext_tab_of(n)
                .filter(|d| self.downloads.borrow().items.iter().any(|r| &r.id == d))
                .ok_or_else(|| format!("Invalid download id {n}"))
        };
        let action = |v: &Value, action: DownloadAction| -> Result<Value, String> {
            let rec = record_of(v)?;
            self.download_action(&rec, action);
            Ok(Value::Null)
        };
        match method {
            "downloads.download" => {
                let o = arg(a, 0);
                let url = str_of(&o["url"])
                    .ok_or_else(|| "A url is required".to_owned())?
                    .to_owned();
                if !(url.starts_with("http://")
                    || url.starts_with("https://")
                    || url.starts_with("data:"))
                {
                    return Err("Invalid URL".to_owned());
                }
                if str_of(&o["method"]).is_some_and(|m| !m.eq_ignore_ascii_case("GET"))
                    || o.get("body").is_some_and(|b| !b.is_null())
                    || o.get("headers")
                        .and_then(Value::as_array)
                        .is_some_and(|h| !h.is_empty())
                {
                    return Err(
                        "HakuCX supports GET downloads without extra headers only".to_owned()
                    );
                }
                let filename = str_of(&o["filename"]).map(|f| {
                    haku_core::records::sanitize_file_name(
                        f.rsplit(['/', '\\']).next().unwrap_or(f),
                    )
                });
                match self.start_download(url, filename, None, None) {
                    Some(rec) => Ok(json!(self.ext_tab_int(&rec))),
                    None => Err("Download canceled by the user".to_owned()),
                }
            }
            "downloads.search" => {
                let q = arg(a, 0);
                let ids: Vec<DownloadRecord> = self.downloads.borrow().items.clone();
                let want_id = q.get("id").and_then(Value::as_i64);
                let mut out = Vec::new();
                for rec in ids {
                    let item = self.ext_download_json(&rec);
                    let ok = want_id.is_none_or(|w| item["id"].as_i64() == Some(w))
                        && str_of(&q["url"]).is_none_or(|u| item["url"] == u)
                        && str_of(&q["filename"]).is_none_or(|f| item["filename"] == f)
                        && str_of(&q["state"]).is_none_or(|s| item["state"] == s)
                        && str_of(&q["query"])
                            .is_none_or(|t| rec.url.contains(t) || rec.file_name.contains(t));
                    if ok {
                        out.push(item);
                    }
                }
                if str_of(&q["orderBy"]).is_some_and(|o| o.contains("-startTime")) {
                    out.reverse();
                }
                if let Some(limit) = q.get("limit").and_then(Value::as_u64).filter(|l| *l > 0) {
                    out.truncate(limit as usize);
                }
                Ok(Value::Array(out))
            }
            "downloads.open" => action(arg(a, 0), DownloadAction::Open),
            "downloads.show" => {
                action(arg(a, 0), DownloadAction::ShowInFolder).map(|_| json!(true))
            }
            "downloads.cancel" => action(arg(a, 0), DownloadAction::Cancel),
            "downloads.pause" => action(arg(a, 0), DownloadAction::Pause),
            "downloads.resume" => action(arg(a, 0), DownloadAction::Resume),
            "downloads.showDefaultFolder" => {
                let _ = open::that_detached(self.download_dir());
                Ok(Value::Null)
            }
            "downloads.erase" => {
                let q = arg(a, 0);
                let want = q.get("id").and_then(Value::as_i64);
                let records: Vec<DownloadRecord> = self.downloads.borrow().items.clone();
                let mut erased = Vec::new();
                for rec in records {
                    let n = self.ext_tab_int(&rec.id);
                    if want.is_none_or(|w| w == n) && !rec.is_active() {
                        self.download_action(&rec.id, DownloadAction::Remove);
                        erased.push(n);
                    }
                }
                let _ = id;
                Ok(json!(erased))
            }
            _ => Err(format!("{method} is not supported")),
        }
    }

    pub(crate) fn ext_download_json(&self, rec: &DownloadRecord) -> Value {
        let state = match rec.state {
            DownloadState::InProgress | DownloadState::Paused => "in_progress",
            DownloadState::Completed => "complete",
            DownloadState::Cancelled | DownloadState::Failed(_) => "interrupted",
        };
        let iso = |ms: i64| {
            time::OffsetDateTime::from_unix_timestamp_nanos(ms as i128 * 1_000_000)
                .ok()
                .and_then(|t| {
                    t.format(&time::format_description::well_known::Rfc3339)
                        .ok()
                })
                .unwrap_or_default()
        };
        let mut v = json!({
            "id": self.ext_tab_int(&rec.id),
            "url": rec.url,
            "finalUrl": rec.url,
            "referrer": rec.referrer.clone().unwrap_or_default(),
            "filename": rec.path,
            "incognito": self.private,
            "danger": "safe",
            "mime": rec.mime,
            "startTime": iso(rec.started_at),
            "state": state,
            "paused": rec.state == DownloadState::Paused,
            "canResume": rec.state == DownloadState::Paused,
            "bytesReceived": rec.received_bytes,
            "totalBytes": rec.total_bytes.map(|t| t as i64).unwrap_or(-1),
            "fileSize": rec.total_bytes.map(|t| t as i64).unwrap_or(-1),
            "exists": rec.state == DownloadState::Completed && std::path::Path::new(&rec.path).exists(),
        });
        if let Some(end) = rec.ended_at {
            v["endTime"] = json!(iso(end));
        }
        match &rec.state {
            DownloadState::Cancelled => v["error"] = json!("USER_CANCELED"),
            DownloadState::Failed(_) => v["error"] = json!("NETWORK_FAILED"),
            _ => {}
        }
        v
    }

    // ------------------------------------------------------------------ management, offscreen

    fn ext_management_info(&self, id: &str) -> Value {
        let st = self.ext.borrow();
        let Some(r) = st.running.get(id) else {
            return Value::Null;
        };
        let m = &r.ext.manifest;
        let base = r.base();
        json!({
            "id": r.ext.record.runtime_id,
            "name": r.ext.name,
            "shortName": m.short_name.clone().unwrap_or_else(|| r.ext.name.clone()),
            "description": r.ext.description,
            "version": m.version,
            "mayDisable": true,
            "enabled": true,
            "homepageUrl": m.homepage_url.clone().unwrap_or_default(),
            "optionsUrl": m.options_page.as_ref().map(|p| format!("{base}{}", p.trim_start_matches('/'))).unwrap_or_default(),
            "installType": if r.ext.record.source == haku_core::extensions::ExtensionSource::Unpacked { "development" } else { "normal" },
            "type": "extension",
            "offlineEnabled": false,
            "permissions": m.permissions.iter().filter(|p| !haku_core::extensions::manifest::is_host_pattern(p)).collect::<Vec<_>>(),
            "hostPermissions": r.ext.host_patterns(),
            "icons": m.icons.iter().map(|(size, p)| json!({ "size": size, "url": format!("{base}{}", p.trim_start_matches('/')) })).collect::<Vec<_>>(),
        })
    }

    fn ext_create_offscreen(&self, id: &str, path: &str) -> Result<Value, String> {
        let url = {
            let st = self.ext.borrow();
            let Some(r) = st.running.get(id) else {
                return Ok(Value::Null);
            };
            if r.ext.file(path).is_none_or(|p| !p.is_file()) {
                return Err(format!("Could not load file: '{path}'."));
            }
            url::Url::parse(&format!("{}{}", r.base(), path.trim_start_matches('/')))
                .map_err(|e| e.to_string())?
        };
        let webview = servo::WebViewBuilder::new(&self.servo, self.ext_context.clone())
            .url(url)
            .hidpi_scale_factor(euclid::Scale::new(1.0))
            .delegate(self.rc())
            .build();
        webview.hide();
        let mut st = self.ext.borrow_mut();
        st.pages.insert(
            webview.id(),
            (id.to_owned(), ContextKind::Offscreen, webview.clone()),
        );
        if let Some(r) = st.running.get_mut(id) {
            r.offscreen = Some(webview);
        }
        Ok(Value::Null)
    }

    // ------------------------------------------------------------------ bookmarks

    fn ext_bookmarks(&self, method: &str, a: &[Value]) -> Result<Value, String> {
        let visible = |b: &Bookmarks, id: &str| -> bool {
            id != ROOT_TRASH && b.get(id).is_some() && !in_trash(b, id)
        };
        let changed = |this: &Browser| this.bookmarks_changed();
        match method {
            "bookmarks.getTree" => {
                let b = self.bookmarks.borrow();
                let children: Vec<Value> = [ROOT_BAR, ROOT_OTHER, ROOT_SPEED_DIAL]
                    .iter()
                    .filter_map(|r| b.get(r))
                    .map(|n| bookmark_json(&b, n, true))
                    .collect();
                Ok(json!([{ "id": "0", "title": "", "children": children }]))
            }
            "bookmarks.get" => {
                let b = self.bookmarks.borrow();
                strings(arg(a, 0))
                    .iter()
                    .map(|i| {
                        if visible(&b, i) {
                            Ok(bookmark_json(&b, b.get(i).expect("visible"), false))
                        } else {
                            Err("Can't find bookmark for id.".to_owned())
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()
                    .map(Value::Array)
            }
            "bookmarks.getChildren" => {
                let b = self.bookmarks.borrow();
                let parent = str_of(arg(a, 0)).unwrap_or_default();
                if parent == "0" {
                    return Ok(Value::Array(
                        [ROOT_BAR, ROOT_OTHER, ROOT_SPEED_DIAL]
                            .iter()
                            .filter_map(|r| b.get(r))
                            .map(|n| bookmark_json(&b, n, false))
                            .collect(),
                    ));
                }
                if !visible(&b, parent) {
                    return Err("Can't find parent bookmark for id.".to_owned());
                }
                Ok(Value::Array(
                    b.children(parent)
                        .into_iter()
                        .map(|n| bookmark_json(&b, n, false))
                        .collect(),
                ))
            }
            "bookmarks.getRecent" => {
                let b = self.bookmarks.borrow();
                let n = arg(a, 0).as_u64().unwrap_or(10).max(1) as usize;
                let mut items: Vec<&BookmarkNode> = b
                    .nodes
                    .iter()
                    .filter(|x| x.url.is_some() && !in_trash(&b, &x.id))
                    .collect();
                items.sort_by_key(|x| std::cmp::Reverse(x.added));
                Ok(Value::Array(
                    items
                        .into_iter()
                        .take(n)
                        .map(|x| bookmark_json(&b, x, false))
                        .collect(),
                ))
            }
            "bookmarks.search" => {
                let b = self.bookmarks.borrow();
                let q = arg(a, 0);
                let mut items: Vec<&BookmarkNode> = match str_of(&q["query"]) {
                    Some(text) => b.search(text, 10_000),
                    None => b
                        .nodes
                        .iter()
                        .filter(|x| {
                            !in_trash(&b, &x.id)
                                && !haku_core::bookmarks::ROOTS.contains(&x.id.as_str())
                        })
                        .collect(),
                };
                if let Some(url) = str_of(&q["url"]) {
                    items.retain(|x| x.url.as_deref() == Some(url));
                }
                if let Some(title) = str_of(&q["title"]) {
                    items.retain(|x| x.title == title);
                }
                Ok(Value::Array(
                    items
                        .into_iter()
                        .map(|x| bookmark_json(&b, x, false))
                        .collect(),
                ))
            }
            "bookmarks.create" => {
                let d = arg(a, 0);
                let parent = str_of(&d["parentId"]).unwrap_or(ROOT_OTHER).to_owned();
                if !visible(&self.bookmarks.borrow(), &parent) {
                    return Err("Can't find parent bookmark for id.".to_owned());
                }
                let new = self
                    .bookmarks
                    .borrow_mut()
                    .add(
                        &parent,
                        str_of(&d["title"]).unwrap_or_default(),
                        str_of(&d["url"]),
                        d.get("index").and_then(Value::as_u64).map(|i| i as usize),
                    )
                    .map_err(|e| e.to_string())?;
                changed(self);
                let b = self.bookmarks.borrow();
                Ok(bookmark_json(&b, b.get(&new).expect("just added"), false))
            }
            "bookmarks.update" => {
                let target = str_of(arg(a, 0)).unwrap_or_default().to_owned();
                let d = arg(a, 1);
                let title = {
                    let b = self.bookmarks.borrow();
                    if !visible(&b, &target) {
                        return Err("Can't find bookmark for id.".to_owned());
                    }
                    str_of(&d["title"]).map(str::to_owned).unwrap_or_else(|| {
                        b.get(&target).map(|n| n.title.clone()).unwrap_or_default()
                    })
                };
                self.bookmarks
                    .borrow_mut()
                    .rename(&target, &title, str_of(&d["url"]))
                    .map_err(|e| e.to_string())?;
                changed(self);
                let b = self.bookmarks.borrow();
                Ok(bookmark_json(&b, b.get(&target).expect("visible"), false))
            }
            "bookmarks.remove" => {
                let target = str_of(arg(a, 0)).unwrap_or_default().to_owned();
                if !visible(&self.bookmarks.borrow(), &target) {
                    return Err("Can't find bookmark for id.".to_owned());
                }
                // Removed bookmarks go to HakuCX's trash, where the user can restore them.
                self.bookmarks
                    .borrow_mut()
                    .delete(&target)
                    .map_err(|e| e.to_string())?;
                changed(self);
                Ok(Value::Null)
            }
            "bookmarks.move" => {
                let target = str_of(arg(a, 0)).unwrap_or_default().to_owned();
                let d = arg(a, 1);
                let parent = {
                    let b = self.bookmarks.borrow();
                    if !visible(&b, &target) {
                        return Err("Can't find bookmark for id.".to_owned());
                    }
                    str_of(&d["parentId"])
                        .map(str::to_owned)
                        .or_else(|| b.get(&target).and_then(|n| n.parent.clone()))
                        .unwrap_or_else(|| ROOT_OTHER.to_owned())
                };
                if !visible(&self.bookmarks.borrow(), &parent) {
                    return Err("Can't find parent bookmark for id.".to_owned());
                }
                self.bookmarks
                    .borrow_mut()
                    .move_to(
                        &target,
                        &parent,
                        d.get("index").and_then(Value::as_u64).map(|i| i as usize),
                    )
                    .map_err(|e| e.to_string())?;
                changed(self);
                let b = self.bookmarks.borrow();
                Ok(bookmark_json(&b, b.get(&target).expect("visible"), false))
            }
            _ => Err(format!("{method} is not supported")),
        }
    }
}

fn apply_menu_props(m: &mut MenuItem, p: &Value) {
    if let Some(t) = p.get("title").and_then(Value::as_str) {
        m.title = t.to_owned();
    }
    if let Some(k) = p.get("type").and_then(Value::as_str) {
        m.kind = k.to_owned();
    }
    if let Some(c) = p.get("checked").and_then(Value::as_bool) {
        m.checked = c;
    }
    if let Some(e) = p.get("enabled").and_then(Value::as_bool) {
        m.enabled = e;
    }
    if let Some(v) = p.get("visible").and_then(Value::as_bool) {
        m.visible = v;
    }
    if let Some(c) = p.get("contexts") {
        m.contexts = strings(c);
    }
    if let Some(d) = p.get("documentUrlPatterns") {
        m.document_patterns = (!d.is_null()).then(|| UrlFilter::new(&strings(d), &[], &[], &[]));
    }
    if let Some(t) = p.get("targetUrlPatterns") {
        m.target_patterns = (!t.is_null()).then(|| UrlFilter::new(&strings(t), &[], &[], &[]));
    }
    if let Some(parent) = p.get("parentId") {
        m.parent = parent
            .as_str()
            .map(str::to_owned)
            .or_else(|| (!parent.is_null()).then(|| parent.to_string()));
    }
}

fn alarm_json(name: &str, a: &Alarm) -> Value {
    let mut v = json!({ "name": name, "scheduledTime": a.when_ms });
    if let Some(p) = a.period_minutes {
        v["periodInMinutes"] = json!(p);
    }
    v
}

fn registered_script_json(id: &str, s: &ContentScript, persist: bool) -> Value {
    json!({
        "id": id,
        "matches": s.matches,
        "excludeMatches": s.exclude_matches,
        "js": s.js,
        "css": s.css,
        "allFrames": s.all_frames,
        "runAt": match s.run_at { RunAt::DocumentStart => "document_start", RunAt::DocumentEnd => "document_end", RunAt::DocumentIdle => "document_idle" },
        "matchOriginAsFallback": s.match_about_blank,
        "persistAcrossSessions": persist,
        "world": "ISOLATED",
    })
}

/// Is `origin` (a match pattern) within the granted host patterns?
fn origin_covered(granted: &[String], origin: &str) -> bool {
    if granted.iter().any(|g| g == origin || g == "<all_urls>") {
        return true;
    }
    // A pattern is covered if a granted pattern matches a URL standing for it.
    let sample = origin
        .replace("*://", "https://")
        .replace("://*.", "://")
        .replace('*', "x");
    granted
        .iter()
        .filter_map(|g| MatchPattern::parse(g))
        .any(|g| g.matches(&sample))
}

fn css_color(v: &Value) -> Result<String, String> {
    match v {
        Value::String(s) if !s.is_empty() => Ok(s.clone()),
        Value::Array(c) if c.len() == 4 => {
            let n = |i: usize| c[i].as_f64().unwrap_or(0.0).clamp(0.0, 255.0);
            Ok(format!(
                "rgba({}, {}, {}, {:.3})",
                n(0) as u8,
                n(1) as u8,
                n(2) as u8,
                n(3) / 255.0
            ))
        }
        _ => Err("Invalid color".to_owned()),
    }
}

/// `[r, g, b, a]` for a `#rrggbb` or `rgba(...)` color.
fn color_array(css: &str) -> Value {
    if let Some(hex) = css.strip_prefix('#')
        && hex.len() == 6
        && let Ok(n) = u32::from_str_radix(hex, 16)
    {
        return json!([(n >> 16) & 255, (n >> 8) & 255, n & 255, 255]);
    }
    if let Some(inner) = css.strip_prefix("rgba(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<f64> = inner
            .split(',')
            .filter_map(|p| p.trim().parse().ok())
            .collect();
        if parts.len() == 4 {
            return json!([
                parts[0] as u8,
                parts[1] as u8,
                parts[2] as u8,
                (parts[3] * 255.0).round() as u8
            ]);
        }
    }
    json!([0, 0, 0, 255])
}

fn cookie_json(c: &cookie::Cookie, url: &url::Url) -> Value {
    let host_only = c.domain().is_none();
    let domain = c
        .domain()
        .map(str::to_owned)
        .unwrap_or_else(|| url.host_str().unwrap_or_default().to_owned());
    let expires = match c.expires() {
        Some(cookie::Expiration::DateTime(t)) => Some(t.unix_timestamp() as f64),
        _ => c
            .max_age()
            .map(|age| (haku_core::now_ms() / 1000) as f64 + age.whole_seconds() as f64),
    };
    let mut v = json!({
        "name": c.name(),
        "value": c.value(),
        "domain": domain,
        "hostOnly": host_only,
        "path": c.path().unwrap_or("/"),
        "secure": c.secure().unwrap_or(false),
        "httpOnly": c.http_only().unwrap_or(false),
        "sameSite": match c.same_site() {
            Some(cookie::SameSite::None) => "no_restriction",
            Some(cookie::SameSite::Lax) => "lax",
            Some(cookie::SameSite::Strict) => "strict",
            None => "unspecified",
        },
        "session": expires.is_none(),
        "storeId": "0",
    });
    if let Some(e) = expires {
        v["expirationDate"] = json!(e);
    }
    v
}

fn encode_capture(image: servo::RgbaImage, png: bool, quality: u8) -> Result<Value, String> {
    let mut bytes = Vec::new();
    if png {
        image
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
    } else {
        let rgb = image::DynamicImage::ImageRgba8(image).to_rgb8();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality)
            .encode_image(&rgb)
            .map_err(|e| e.to_string())?;
    }
    Ok(json!(format!(
        "data:image/{};base64,{}",
        if png { "png" } else { "jpeg" },
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )))
}

fn in_trash(b: &Bookmarks, id: &str) -> bool {
    let mut cur = b.get(id).and_then(|n| n.parent.clone());
    let mut steps = 0;
    while let Some(p) = cur {
        if p == ROOT_TRASH {
            return true;
        }
        steps += 1;
        if steps > 10_000 {
            return false;
        }
        cur = b.get(&p).and_then(|n| n.parent.clone());
    }
    false
}

fn bookmark_json(b: &Bookmarks, n: &BookmarkNode, recurse: bool) -> Value {
    let root_title = match n.id.as_str() {
        ROOT_BAR => Some("Bookmarks bar"),
        ROOT_OTHER => Some("Other bookmarks"),
        ROOT_SPEED_DIAL => Some("Speed Dial"),
        _ => None,
    };
    let parent = n.parent.clone().unwrap_or_else(|| "0".to_owned());
    let index = match n.parent.as_deref() {
        Some(p) => b.children(p).iter().position(|x| x.id == n.id).unwrap_or(0),
        None => [ROOT_BAR, ROOT_OTHER, ROOT_SPEED_DIAL]
            .iter()
            .position(|r| *r == n.id)
            .unwrap_or(0),
    };
    let mut v = json!({
        "id": n.id,
        "parentId": parent,
        "index": index,
        "title": root_title.map(str::to_owned).unwrap_or_else(|| n.title.clone()),
        "dateAdded": n.added,
    });
    match &n.url {
        Some(u) => v["url"] = json!(u),
        None => {
            if root_title.is_some() {
                v["folderType"] = json!(match n.id.as_str() {
                    ROOT_BAR => "bookmarks-bar",
                    _ => "other",
                });
            }
            if recurse {
                v["children"] = Value::Array(
                    b.children(&n.id)
                        .into_iter()
                        .map(|c| bookmark_json(b, c, true))
                        .collect(),
                );
            }
        }
    }
    v
}

/// Milliseconds since the last keyboard or mouse input anywhere on the system.
#[cfg(windows)]
pub fn idle_ms() -> u64 {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: `info` is a valid, correctly sized LASTINPUTINFO.
    if unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        // SAFETY: no preconditions.
        u64::from(unsafe { GetTickCount() }.wrapping_sub(info.dwTime))
    } else {
        0
    }
}

#[cfg(not(windows))]
pub fn idle_ms() -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_round_trip() {
        assert_eq!(color_array("#ff8000"), json!([255, 128, 0, 255]));
        assert_eq!(
            css_color(&json!([255, 0, 0, 255])).unwrap(),
            "rgba(255, 0, 0, 1.000)"
        );
        assert_eq!(
            color_array(&css_color(&json!([10, 20, 30, 128])).unwrap()),
            json!([10, 20, 30, 128])
        );
        assert!(css_color(&json!(5)).is_err());
    }

    #[test]
    fn origins_are_covered_by_broader_patterns() {
        let granted = vec!["*://*.example.com/*".to_owned()];
        assert!(origin_covered(&granted, "https://www.example.com/*"));
        assert!(origin_covered(&granted, "*://*.example.com/*"));
        assert!(!origin_covered(&granted, "https://example.org/*"));
        assert!(origin_covered(
            &["<all_urls>".to_owned()],
            "https://a.test/*"
        ));
    }

    #[test]
    fn script_values_become_json() {
        let mut map = std::collections::HashMap::new();
        map.insert("a".to_owned(), JSValue::Number(1.0));
        map.insert("el".to_owned(), JSValue::Element("x".into()));
        let v = js_to_json(JSValue::Array(vec![
            JSValue::Undefined,
            JSValue::Object(map),
            JSValue::String("s".into()),
        ]));
        assert_eq!(v, json!([null, { "a": 1.0, "el": {} }, "s"]));
    }
}

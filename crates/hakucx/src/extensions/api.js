/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// The WebExtension API (`chrome.*` and `browser.*`) for one extension in one context.
//
// This file is an expression: `(function (config) { ... return api; })`. The browser wraps it
// for extension pages (background, popup, options) and for content scripts. Calls go to the
// browser as console lines with a private prefix and the extension's per-session token; the
// browser checks the token and, for extension pages, the page's origin.
//
// Methods take an optional trailing callback (chrome.* style, with runtime.lastError) and
// otherwise return a Promise (browser.* and MV3 style). Both namespaces are the same object.

(function (config) {
  "use strict";
  const PREFIX = "\u0001haku-ext:";
  // Keep our own references: page scripts can replace globals later.
  const debug = console.debug.bind(console);
  const stringify = JSON.stringify;
  const Promise_ = Promise;
  const isContent = config.context === "content";
  const frameId = isContent ? (window.top === window ? 0 : Math.floor(Math.random() * 2147483647) + 1) : 0;

  // Several API instances can share a page (content scripts, scripting.executeScript), and
  // answers reach all of them: start call ids at random so they do not collide.
  let nextCall = Math.floor(Math.random() * 0x3fffffff) + 1;
  const pending = new Map();
  let lastError = null;

  const sendRaw = (c, m, a) => {
    const msg = { t: config.token, c, m, a };
    if (isContent) {
      msg.f = frameId;
      msg.u = location.href;
    }
    debug(PREFIX + stringify(msg));
  };
  const call = (method, args) =>
    new Promise_((resolve, reject) => {
      const c = nextCall++;
      pending.set(c, { resolve, reject });
      sendRaw(c, method, args === undefined ? [] : args);
    });
  const post = (method, args) => sendRaw(0, method, args === undefined ? [] : args);
  const toMessage = (e) => (e && e.message) || String(e);

  // A method with an optional trailing callback.
  const method = (name, shape) =>
    function (...args) {
      const cb = typeof args[args.length - 1] === "function" ? args.pop() : null;
      let promise;
      try {
        promise = call(name, shape ? shape(...args) : args);
      } catch (e) {
        promise = Promise_.reject(e);
      }
      if (!cb) return promise;
      promise.then(
        (r) => cb(r),
        (e) => {
          lastError = { message: toMessage(e) };
          try {
            cb();
          } finally {
            lastError = null;
          }
        },
      );
      return undefined;
    };

  // ---------------------------------------------------------------- events
  const events = new Map();
  const announced = new Set();
  const listenersOf = (name) => events.get(name) || [];
  const makeEvent = (name, options = {}) => ({
    addListener(fn, ...extra) {
      if (typeof fn !== "function") return;
      const list = events.get(name) || [];
      if (!list.some((l) => l.fn === fn)) list.push({ fn, extra });
      events.set(name, list);
      if (!announced.has(name) || options.everyListener) {
        announced.add(name);
        post("events.listen", [name, extra]);
      }
    },
    removeListener(fn) {
      events.set(name, listenersOf(name).filter((l) => l.fn !== fn));
    },
    hasListener(fn) {
      return listenersOf(name).some((l) => l.fn === fn);
    },
    hasListeners() {
      return listenersOf(name).length > 0;
    },
    // Declarative events (rules) are not supported; keep the methods so feature checks pass.
    addRules(_r, cb) {
      if (cb) cb([]);
    },
    getRules(_i, cb) {
      if (cb) cb([]);
    },
    removeRules(_i, cb) {
      if (cb) cb();
    },
  });
  const fire = (name, args) => {
    for (const { fn } of listenersOf(name).slice()) {
      try {
        fn(...args);
      } catch (e) {
        console.error(e);
      }
    }
  };

  // ---------------------------------------------------------------- messaging and ports
  // Ask every listener; the first response wins. A listener that returns `true` or a
  // Promise answers later.
  const answer = (name, args, replyId) => {
    const list = listenersOf(name).slice();
    let responded = false;
    let waiting = false;
    const respond = (value, isError) => {
      if (responded) return;
      responded = true;
      post("runtime.reply", [replyId, value === undefined ? null : value, true, !!isError]);
    };
    for (const { fn } of list) {
      let ret;
      try {
        ret = fn(...args, (r) => respond(r));
      } catch (e) {
        console.error(e);
        continue;
      }
      if (ret === true) waiting = true;
      else if (ret && typeof ret.then === "function") {
        waiting = true;
        ret.then(
          (r) => respond(r),
          (e) => respond(toMessage(e), true),
        );
      }
    }
    if (!waiting && !responded) post("runtime.reply", [replyId, null, false, false]);
  };

  const ports = new Map();
  const makePort = (id, name, sender) => {
    const port = {
      name: name || "",
      sender,
      onMessage: makeEvent("port." + id + ".message"),
      onDisconnect: makeEvent("port." + id + ".disconnect"),
      postMessage(message) {
        if (!ports.has(id)) throw new Error("Attempting to use a disconnected port object");
        post("port.post", [id, message]);
      },
      disconnect() {
        if (!ports.has(id)) return;
        ports.delete(id);
        post("port.disconnect", [id]);
      },
      error: null,
    };
    ports.set(id, port);
    return port;
  };
  const connect = (target, info) => {
    const id = "p" + Date.now().toString(36) + Math.random().toString(36).slice(2);
    const port = makePort(id, info && info.name, undefined);
    post("port.connect", [id, target, (info && info.name) || "", (info && info.frameId) || null]);
    return port;
  };

  // ---------------------------------------------------------------- receive from the browser
  const receive = (msg) => {
    if (msg.fr !== undefined && msg.fr !== null && msg.fr !== frameId) return;
    if (msg.c) {
      const p = pending.get(msg.c);
      if (!p) return;
      pending.delete(msg.c);
      if (msg.e !== undefined) p.reject(new Error(msg.e));
      else p.resolve(msg.r === null ? undefined : msg.r);
      return;
    }
    const args = msg.a || [];
    switch (msg.ev) {
      case "runtime.onMessage":
      case "runtime.onMessageExternal":
        answer(msg.ev, args, msg.r);
        return;
      case "webRequest.onBeforeRequest":
        decideRequest(args[0], msg.r);
        return;
      case "port.open": {
        const [id, name, sender] = args;
        const port = makePort(id, name, sender);
        fire("runtime.onConnect", [port]);
        return;
      }
      case "port.message": {
        const port = ports.get(args[0]);
        if (port) fire("port." + args[0] + ".message", [args[1], port]);
        return;
      }
      case "port.disconnected": {
        const port = ports.get(args[0]);
        if (!port) return;
        ports.delete(args[0]);
        if (args[1]) port.error = { message: args[1] };
        lastError = args[1] ? { message: args[1] } : null;
        try {
          fire("port." + args[0] + ".disconnect", [port]);
        } finally {
          lastError = null;
        }
        return;
      }
      default:
        fire(msg.ev, args);
    }
  };
  if (isContent) {
    if (window.__hakuExtRegister) window.__hakuExtRegister(config.token, receive);
  } else {
    Object.defineProperty(window, config.dispatch, { value: receive, configurable: false });
  }

  // webRequest.onBeforeRequest with "blocking": the first listener that cancels or redirects wins.
  const decideRequest = (details, replyId) => {
    let result = {};
    for (const { fn, extra } of listenersOf("webRequest.onBeforeRequest")) {
      const filter = extra[0] || {};
      if (Array.isArray(filter.types) && !filter.types.includes(details.type)) continue;
      try {
        const r = fn(details);
        if (r && (r.cancel || r.redirectUrl)) {
          result = r;
          break;
        }
      } catch (e) {
        console.error(e);
      }
    }
    post("webRequest.decide", [replyId, result.cancel ? { cancel: true } : result.redirectUrl ? { redirectUrl: String(result.redirectUrl) } : {}]);
  };

  // ---------------------------------------------------------------- namespaces
  const manifest = config.manifest;
  const base = config.base;
  const getURL = (path) => base + String(path || "").replace(/^\//, "");
  const getMessage = (name, subs) => {
    const key = String(name).toLowerCase();
    const predefined = {
      "@@extension_id": config.runtimeId,
      "@@ui_locale": config.uiLocale.replace("-", "_"),
      "@@bidi_dir": "ltr",
      "@@bidi_reversed_dir": "rtl",
      "@@bidi_start_edge": "left",
      "@@bidi_end_edge": "right",
    };
    if (key in predefined) return predefined[key];
    const entry = config.messages[key];
    if (!entry) return "";
    const list = subs === undefined ? [] : Array.isArray(subs) ? subs.map(String) : [String(subs)];
    let text = entry.message.replace(/\$([a-zA-Z0-9_@]+)\$/g, (m, ph) => {
      const content = entry.placeholders[ph.toLowerCase()];
      return content === undefined ? m : content;
    });
    text = text.replace(/\$(\$|[1-9])/g, (m, d) => (d === "$" ? "$" : list[Number(d) - 1] || ""));
    return text;
  };

  const runtime = {
    id: config.runtimeId,
    getURL,
    getManifest: () => JSON.parse(stringify(manifest)),
    get lastError() {
      return lastError || undefined;
    },
    getPlatformInfo: method("runtime.getPlatformInfo"),
    // sendMessage(extensionId?, message, options?): with two arguments the first is an
    // extension id unless the second is an options object (Chrome's rule).
    sendMessage: method("runtime.sendMessage", (...args) => {
      const isOptions = (o) => o && typeof o === "object" && "includeTlsChannelId" in o;
      let ext = null;
      if (args.length >= 3 || (args.length === 2 && (typeof args[0] === "string" || args[0] === null) && !isOptions(args[1]))) {
        ext = args.shift();
      }
      if (ext === config.runtimeId) ext = null;
      return [ext, args[0] === undefined ? null : args[0]];
    }),
    connect: (...args) => {
      const info = typeof args[0] === "string" ? args[1] : args[0];
      return connect({ kind: "extension" }, info);
    },
    openOptionsPage: method("runtime.openOptionsPage"),
    reload: () => post("runtime.reload"),
    setUninstallURL: method("runtime.setUninstallURL"),
    requestUpdateCheck: method("runtime.requestUpdateCheck"),
    getBackgroundPage: method("runtime.getBackgroundPage"),
    getContexts: method("runtime.getContexts"),
    onMessage: makeEvent("runtime.onMessage"),
    onMessageExternal: makeEvent("runtime.onMessageExternal"),
    onConnect: makeEvent("runtime.onConnect"),
    onConnectExternal: makeEvent("runtime.onConnectExternal"),
    onInstalled: makeEvent("runtime.onInstalled"),
    onStartup: makeEvent("runtime.onStartup"),
    onSuspend: makeEvent("runtime.onSuspend"),
    onUpdateAvailable: makeEvent("runtime.onUpdateAvailable"),
  };

  const area = (name) => ({
    get: method("storage.get", (keys) => [name, keys === undefined ? null : keys]),
    set: method("storage.set", (items) => [name, items || {}]),
    remove: method("storage.remove", (keys) => [name, Array.isArray(keys) ? keys : [keys]]),
    clear: method("storage.clear", () => [name]),
    getBytesInUse: method("storage.getBytesInUse", (keys) => [name, keys === undefined ? null : keys]),
    setAccessLevel: method("storage.setAccessLevel", (o) => [name, o || {}]),
    onChanged: makeEvent("storage." + name + ".onChanged"),
    QUOTA_BYTES: name === "sync" ? 102400 : 10485760,
    QUOTA_BYTES_PER_ITEM: name === "sync" ? 8192 : undefined,
    MAX_ITEMS: name === "sync" ? 512 : undefined,
  });
  const storage = {
    local: area("local"),
    sync: area("sync"),
    session: area("session"),
    managed: { get: method("storage.managed"), onChanged: makeEvent("storage.managed.onChanged") },
    onChanged: makeEvent("storage.onChanged"),
  };

  const i18n = {
    getMessage,
    getUILanguage: () => config.uiLocale,
    getAcceptLanguages: method("i18n.getAcceptLanguages"),
    detectLanguage: method("i18n.detectLanguage"),
  };

  const extension = {
    getURL,
    inIncognitoContext: !!config.incognito,
    getBackgroundPage: () => null,
    getViews: () => [],
    isAllowedIncognitoAccess: method("extension.isAllowedIncognitoAccess"),
    isAllowedFileSchemeAccess: method("extension.isAllowedFileSchemeAccess"),
    sendMessage: runtime.sendMessage,
    onMessage: runtime.onMessage,
    get lastError() {
      return lastError || undefined;
    },
  };

  const api = { runtime, storage, i18n, extension };

  if (!isContent) {
    const tabs = {
      TAB_ID_NONE: -1,
      query: method("tabs.query", (q) => [q || {}]),
      get: method("tabs.get"),
      getCurrent: method("tabs.getCurrent"),
      create: method("tabs.create", (p) => [p || {}]),
      update: method("tabs.update", (...a) => (typeof a[0] === "number" ? [a[0], a[1] || {}] : [null, a[0] || {}])),
      remove: method("tabs.remove", (ids) => [Array.isArray(ids) ? ids : [ids]]),
      reload: method("tabs.reload", (...a) => (typeof a[0] === "number" ? [a[0], a[1] || {}] : [null, a[0] || {}])),
      duplicate: method("tabs.duplicate"),
      sendMessage: method("tabs.sendMessage", (tabId, message, options) => [tabId, message === undefined ? null : message, (options && options.frameId) ?? null]),
      connect: (tabId, info) => connect({ kind: "tab", tabId }, info),
      captureVisibleTab: method("tabs.captureVisibleTab", (...a) => [a.find((x) => typeof x === "object" && x) || {}]),
      executeScript: method("tabs.executeScript", (...a) => (typeof a[0] === "number" ? [a[0], a[1] || {}] : [null, a[0] || {}])),
      insertCSS: method("tabs.insertCSS", (...a) => (typeof a[0] === "number" ? [a[0], a[1] || {}] : [null, a[0] || {}])),
      removeCSS: method("tabs.removeCSS", (...a) => (typeof a[0] === "number" ? [a[0], a[1] || {}] : [null, a[0] || {}])),
      highlight: method("tabs.highlight"),
      move: method("tabs.move"),
      getZoom: method("tabs.getZoom"),
      setZoom: method("tabs.setZoom", (...a) => (a.length === 2 ? a : [null, a[0]])),
      discard: method("tabs.discard"),
      group: method("tabs.group"),
      ungroup: method("tabs.ungroup"),
      onCreated: makeEvent("tabs.onCreated"),
      onUpdated: makeEvent("tabs.onUpdated"),
      onActivated: makeEvent("tabs.onActivated"),
      onRemoved: makeEvent("tabs.onRemoved"),
      onMoved: makeEvent("tabs.onMoved"),
      onReplaced: makeEvent("tabs.onReplaced"),
      onHighlighted: makeEvent("tabs.onHighlighted"),
      onAttached: makeEvent("tabs.onAttached"),
      onDetached: makeEvent("tabs.onDetached"),
      onZoomChange: makeEvent("tabs.onZoomChange"),
    };
    const windows = {
      WINDOW_ID_NONE: -1,
      WINDOW_ID_CURRENT: -2,
      get: method("windows.get"),
      getCurrent: method("windows.getCurrent"),
      getLastFocused: method("windows.getCurrent"),
      getAll: method("windows.getAll"),
      create: method("windows.create", (p) => [p || {}]),
      update: method("windows.update"),
      remove: method("windows.remove"),
      onCreated: makeEvent("windows.onCreated"),
      onRemoved: makeEvent("windows.onRemoved"),
      onFocusChanged: makeEvent("windows.onFocusChanged"),
      onBoundsChanged: makeEvent("windows.onBoundsChanged"),
    };
    const action = {
      setBadgeText: method("action.setBadgeText", (d) => [d || {}]),
      getBadgeText: method("action.getBadgeText", (d) => [d || {}]),
      setBadgeBackgroundColor: method("action.setBadgeBackgroundColor", (d) => [d || {}]),
      getBadgeBackgroundColor: method("action.getBadgeBackgroundColor", (d) => [d || {}]),
      setBadgeTextColor: method("action.setBadgeTextColor", (d) => [d || {}]),
      getBadgeTextColor: method("action.getBadgeTextColor", (d) => [d || {}]),
      setTitle: method("action.setTitle", (d) => [d || {}]),
      getTitle: method("action.getTitle", (d) => [d || {}]),
      setPopup: method("action.setPopup", (d) => [d || {}]),
      getPopup: method("action.getPopup", (d) => [d || {}]),
      setIcon: function (details, cb) {
        const send = (d) => method("action.setIcon")(d, cb);
        const data = details && details.imageData;
        if (!data) return send({ tabId: details && details.tabId, path: details && details.path });
        // ImageData -> data: URL (one size is enough for our toolbar).
        const img = data.width ? data : data[Object.keys(data).sort((a, b) => b - a)[0]];
        try {
          const canvas = document.createElement("canvas");
          canvas.width = img.width;
          canvas.height = img.height;
          canvas.getContext("2d").putImageData(img, 0, 0);
          return send({ tabId: details.tabId, path: canvas.toDataURL("image/png") });
        } catch (e) {
          return send({ tabId: details.tabId });
        }
      },
      enable: method("action.enable", (tabId) => [tabId ?? null]),
      disable: method("action.disable", (tabId) => [tabId ?? null]),
      isEnabled: method("action.isEnabled", (tabId) => [tabId ?? null]),
      openPopup: method("action.openPopup"),
      getUserSettings: method("action.getUserSettings"),
      onClicked: makeEvent("action.onClicked"),
    };
    const contextMenus = {
      ACTION_MENU_TOP_LEVEL_LIMIT: 6,
      create(props, cb) {
        const id = props.id !== undefined ? String(props.id) : "m" + Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
        if (typeof props.onclick === "function") {
          const handler = props.onclick;
          contextMenus.onClicked.addListener((info, tab) => {
            if (String(info.menuItemId) === id) handler(info, tab);
          });
        }
        const clean = Object.assign({}, props, { id, onclick: undefined });
        method("contextMenus.create")(clean, cb || (() => {}));
        return props.id !== undefined ? props.id : id;
      },
      update: method("contextMenus.update", (id, props) => [String(id), props || {}]),
      remove: method("contextMenus.remove", (id) => [String(id)]),
      removeAll: method("contextMenus.removeAll"),
      refresh: method("contextMenus.refresh"),
      onClicked: makeEvent("contextMenus.onClicked"),
      onShown: makeEvent("contextMenus.onShown"),
      onHidden: makeEvent("contextMenus.onHidden"),
    };
    const alarms = {
      create: method("alarms.create", (...a) => (typeof a[0] === "string" ? [a[0], a[1] || {}] : ["", a[0] || {}])),
      get: method("alarms.get", (name) => [name || ""]),
      getAll: method("alarms.getAll"),
      clear: method("alarms.clear", (name) => [name || ""]),
      clearAll: method("alarms.clearAll"),
      onAlarm: makeEvent("alarms.onAlarm"),
    };
    const notifications = {
      create: method("notifications.create", (...a) => (typeof a[0] === "string" ? [a[0], a[1] || {}] : [null, a[0] || {}])),
      update: method("notifications.update"),
      clear: method("notifications.clear"),
      getAll: method("notifications.getAll"),
      getPermissionLevel: method("notifications.getPermissionLevel"),
      onClicked: makeEvent("notifications.onClicked"),
      onClosed: makeEvent("notifications.onClosed"),
      onButtonClicked: makeEvent("notifications.onButtonClicked"),
      onShowSettings: makeEvent("notifications.onShowSettings"),
    };
    const scripting = {
      executeScript: method("scripting.executeScript", (inj) => {
        const i = Object.assign({}, inj);
        if (typeof i.func === "function") i.func = i.func.toString();
        if (typeof i.function === "function") i.func = i.function.toString();
        delete i.function;
        return [i];
      }),
      insertCSS: method("scripting.insertCSS", (i) => [i || {}]),
      removeCSS: method("scripting.removeCSS", (i) => [i || {}]),
      registerContentScripts: method("scripting.registerContentScripts", (s) => [s || []]),
      getRegisteredContentScripts: method("scripting.getRegisteredContentScripts", (f) => [f || {}]),
      unregisterContentScripts: method("scripting.unregisterContentScripts", (f) => [f || {}]),
      updateContentScripts: method("scripting.updateContentScripts", (s) => [s || []]),
    };
    const cookies = {
      get: method("cookies.get", (d) => [d || {}]),
      getAll: method("cookies.getAll", (d) => [d || {}]),
      set: method("cookies.set", (d) => [d || {}]),
      remove: method("cookies.remove", (d) => [d || {}]),
      getAllCookieStores: method("cookies.getAllCookieStores"),
      onChanged: makeEvent("cookies.onChanged"),
    };
    const dnr = {
      MAX_NUMBER_OF_DYNAMIC_RULES: 30000,
      MAX_NUMBER_OF_SESSION_RULES: 5000,
      MAX_NUMBER_OF_ENABLED_STATIC_RULESETS: 50,
      GUARANTEED_MINIMUM_STATIC_RULES: 30000,
      DYNAMIC_RULESET_ID: "_dynamic",
      SESSION_RULESET_ID: "_session",
      updateDynamicRules: method("dnr.updateDynamicRules", (o) => [o || {}]),
      getDynamicRules: method("dnr.getDynamicRules", (f) => [f || {}]),
      updateSessionRules: method("dnr.updateSessionRules", (o) => [o || {}]),
      getSessionRules: method("dnr.getSessionRules", (f) => [f || {}]),
      updateEnabledRulesets: method("dnr.updateEnabledRulesets", (o) => [o || {}]),
      getEnabledRulesets: method("dnr.getEnabledRulesets"),
      updateStaticRules: method("dnr.updateStaticRules", (o) => [o || {}]),
      getDisabledRuleIds: method("dnr.getDisabledRuleIds", (o) => [o || {}]),
      getAvailableStaticRuleCount: method("dnr.getAvailableStaticRuleCount"),
      isRegexSupported: method("dnr.isRegexSupported", (o) => [o || {}]),
      setExtensionActionOptions: method("dnr.setExtensionActionOptions", (o) => [o || {}]),
      getMatchedRules: method("dnr.getMatchedRules", (f) => [f || {}]),
      testMatchOutcome: method("dnr.testMatchOutcome", (r) => [r || {}]),
      onRuleMatchedDebug: makeEvent("dnr.onRuleMatchedDebug"),
    };
    const webRequestEvent = (name) => makeEvent("webRequest." + name, { everyListener: true });
    const webRequest = {
      MAX_HANDLER_BEHAVIOR_CHANGED_CALLS_PER_10_MINUTES: 20,
      handlerBehaviorChanged: method("webRequest.handlerBehaviorChanged"),
      onBeforeRequest: webRequestEvent("onBeforeRequest"),
      onBeforeSendHeaders: webRequestEvent("onBeforeSendHeaders"),
      onSendHeaders: webRequestEvent("onSendHeaders"),
      onHeadersReceived: webRequestEvent("onHeadersReceived"),
      onAuthRequired: webRequestEvent("onAuthRequired"),
      onResponseStarted: webRequestEvent("onResponseStarted"),
      onBeforeRedirect: webRequestEvent("onBeforeRedirect"),
      onCompleted: webRequestEvent("onCompleted"),
      onErrorOccurred: webRequestEvent("onErrorOccurred"),
    };
    const commands = {
      getAll: method("commands.getAll"),
      onCommand: makeEvent("commands.onCommand"),
    };
    const permissions = {
      contains: method("permissions.contains", (p) => [p || {}]),
      getAll: method("permissions.getAll"),
      request: method("permissions.request", (p) => [p || {}]),
      remove: method("permissions.remove", (p) => [p || {}]),
      onAdded: makeEvent("permissions.onAdded"),
      onRemoved: makeEvent("permissions.onRemoved"),
    };
    const history = {
      search: method("history.search", (q) => [q || {}]),
      addUrl: method("history.addUrl", (d) => [d || {}]),
      deleteUrl: method("history.deleteUrl", (d) => [d || {}]),
      deleteRange: method("history.deleteRange", (d) => [d || {}]),
      deleteAll: method("history.deleteAll"),
      getVisits: method("history.getVisits", (d) => [d || {}]),
      onVisited: makeEvent("history.onVisited"),
      onVisitRemoved: makeEvent("history.onVisitRemoved"),
    };
    const downloads = {
      download: method("downloads.download", (o) => [o || {}]),
      search: method("downloads.search", (q) => [q || {}]),
      open: method("downloads.open"),
      show: method("downloads.show"),
      showDefaultFolder: method("downloads.showDefaultFolder"),
      cancel: method("downloads.cancel"),
      pause: method("downloads.pause"),
      resume: method("downloads.resume"),
      erase: method("downloads.erase", (q) => [q || {}]),
      onCreated: makeEvent("downloads.onCreated"),
      onChanged: makeEvent("downloads.onChanged"),
      onErased: makeEvent("downloads.onErased"),
    };
    const management = {
      getSelf: method("management.getSelf"),
      getAll: method("management.getAll"),
      get: method("management.get"),
      onEnabled: makeEvent("management.onEnabled"),
      onDisabled: makeEvent("management.onDisabled"),
      onInstalled: makeEvent("management.onInstalled"),
      onUninstalled: makeEvent("management.onUninstalled"),
    };
    const idle = {
      queryState: method("idle.queryState"),
      setDetectionInterval: (seconds) => post("idle.setDetectionInterval", [Number(seconds) || 60]),
      onStateChanged: makeEvent("idle.onStateChanged"),
    };
    const offscreen = {
      Reason: { AUDIO_PLAYBACK: "AUDIO_PLAYBACK", CLIPBOARD: "CLIPBOARD", DOM_PARSER: "DOM_PARSER", DOM_SCRAPING: "DOM_SCRAPING", BLOBS: "BLOBS", WORKERS: "WORKERS", LOCAL_STORAGE: "LOCAL_STORAGE", MATCH_MEDIA: "MATCH_MEDIA", GEOLOCATION: "GEOLOCATION" },
      createDocument: method("offscreen.createDocument", (p) => [p || {}]),
      closeDocument: method("offscreen.closeDocument"),
      hasDocument: method("offscreen.hasDocument"),
    };
    const bookmarks = {
      getTree: method("bookmarks.getTree"),
      get: method("bookmarks.get", (ids) => [Array.isArray(ids) ? ids : [ids]]),
      getChildren: method("bookmarks.getChildren"),
      getRecent: method("bookmarks.getRecent"),
      search: method("bookmarks.search", (q) => [typeof q === "string" ? { query: q } : q || {}]),
      create: method("bookmarks.create", (b) => [b || {}]),
      update: method("bookmarks.update"),
      remove: method("bookmarks.remove"),
      removeTree: method("bookmarks.remove"),
      move: method("bookmarks.move"),
      onCreated: makeEvent("bookmarks.onCreated"),
      onRemoved: makeEvent("bookmarks.onRemoved"),
      onChanged: makeEvent("bookmarks.onChanged"),
      onMoved: makeEvent("bookmarks.onMoved"),
    };

    Object.assign(api, { tabs, windows, alarms, i18n, commands, permissions, management, idle });
    // MV2 names and Firefox names point at the same objects.
    api.action = action;
    api.browserAction = action;
    api.pageAction = Object.assign({}, action, { show: method("action.enable", (t) => [t ?? null]), hide: method("action.disable", (t) => [t ?? null]) });
    const has = (p) => config.permissions.includes(p);
    if (has("contextMenus") || has("menus")) {
      api.contextMenus = contextMenus;
      api.menus = contextMenus;
    }
    if (has("notifications")) api.notifications = notifications;
    if (has("scripting") || config.mv === 2) api.scripting = scripting;
    if (has("cookies")) api.cookies = cookies;
    if (has("declarativeNetRequest") || has("declarativeNetRequestWithHostAccess") || has("declarativeNetRequestFeedback")) api.declarativeNetRequest = dnr;
    if (has("webRequest")) api.webRequest = webRequest;
    if (has("history")) api.history = history;
    if (has("downloads")) api.downloads = downloads;
    if (has("offscreen")) api.offscreen = offscreen;
    if (has("bookmarks")) api.bookmarks = bookmarks;

    // MV3 service workers run in a page here; give them the worker globals they expect.
    if (config.context === "background" && config.serviceWorker) {
      if (typeof self.importScripts !== "function") {
        self.importScripts = (...urls) => {
          for (const u of urls) {
            const xhr = new XMLHttpRequest();
            xhr.open("GET", new URL(u, location.href).href, false);
            xhr.send();
            if (xhr.status !== 200 && xhr.status !== 0) throw new Error("importScripts failed: " + u);
            (0, eval)(xhr.responseText + "\n//# sourceURL=" + u);
          }
        };
      }
      if (!self.registration) self.registration = { scope: base, active: {}, showNotification: (title, o) => (api.notifications ? api.notifications.create({ type: "basic", title, message: (o && o.body) || "" }) : undefined) };
      if (!self.clients) self.clients = { matchAll: () => Promise_.resolve([]), claim: () => Promise_.resolve(), openWindow: (url) => tabs.create({ url }) };
      if (!self.skipWaiting) self.skipWaiting = () => Promise_.resolve();
    }
  }

  // Firefox's runtime.getBrowserInfo.
  api.runtime.getBrowserInfo = method("runtime.getBrowserInfo");
  // scripting.executeScript reports a function's (possibly async) result through this.
  if (isContent) Object.defineProperty(api, "__result", { value: (id, value, error) => post("scripting.result", [id, value, error]) });
  return api;
});

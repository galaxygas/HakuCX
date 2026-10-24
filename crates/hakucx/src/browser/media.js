/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Media in web pages, for the miniplayer and video pop out. Runs in the page's own world (Servo
// has no isolated worlds), so it holds no secrets: it reports what is playing on the page
// channel and does what the browser asks through window.__hakuMedia (play, pause, next,
// previous, pop out), which a page could do to its own media anyway.
//
// Servo forwards a page's Media Session title, artist and album to the browser, but not its
// artwork or its next/previous-track handlers, and only for the most recent session; so the
// Media Session API is watched here, per page.
(() => {
  if (window.__hakuMedia) return;
  const PREFIX = "\u0001haku-page:";
  const post = (msg) => { try { console.debug(PREFIX + JSON.stringify(msg)); } catch (_) {} };
  // Filled in by the browser: show the pop-out button on videos.
  let button = __HAKU_POPOUT_BUTTON__;

  const art = new WeakMap();
  const handlers = new Map();
  let meta = null;
  let current = null;
  let popped = null;
  let audible = 0;
  let queued = false;

  try {
    const Original = window.MediaMetadata;
    if (typeof Original === "function") {
      const Wrapped = function MediaMetadata(init) {
        const m = new Original(init);
        try { if (init && Array.isArray(init.artwork)) art.set(m, init.artwork.slice(0, 16)); } catch (_) {}
        return m;
      };
      Wrapped.prototype = Original.prototype;
      Object.defineProperty(window, "MediaMetadata", { value: Wrapped, writable: true, configurable: true });
    }
    const proto = window.MediaSession && window.MediaSession.prototype;
    if (proto) {
      const d = Object.getOwnPropertyDescriptor(proto, "metadata");
      if (d && d.set && d.get) {
        Object.defineProperty(proto, "metadata", {
          configurable: true,
          enumerable: d.enumerable,
          get() { return d.get.call(this); },
          set(v) { d.set.call(this, v); meta = v || null; schedule(); },
        });
      }
      const setActionHandler = proto.setActionHandler;
      if (typeof setActionHandler === "function") {
        proto.setActionHandler = function (action, handler) {
          if (typeof handler === "function") handlers.set(action, handler);
          else handlers.delete(action);
          schedule();
          return setActionHandler.call(this, action, handler);
        };
      }
    }
  } catch (_) {}

  const srcOf = (el) => (el && (el.currentSrc || el.src)) || "";
  const poppable = (el) => !!el && el.tagName === "VIDEO" && /^https?:/i.test(srcOf(el));
  const text = (v) => (typeof v === "string" ? v.slice(0, 300) : "");

  function artwork() {
    const list = meta && art.get(meta);
    if (!list || !list.length) return "";
    try {
      const best = list.reduce((a, b) => (parseInt(b && b.sizes, 10) || 0) > (parseInt(a && a.sizes, 10) || 0) ? b : a);
      const url = new URL(String(best.src), location.href);
      return /^https?:$/.test(url.protocol) ? url.href.slice(0, 2048) : "";
    } catch (_) {
      return "";
    }
  }

  function report() {
    queued = false;
    const el = current && current.isConnected ? current : null;
    if (!el) current = null;
    post({
      type: "media",
      playing: audible > 0,
      present: !!el && (srcOf(el) !== "" || !!el.srcObject),
      paused: !el || el.paused || el.ended,
      video: !!el && el.tagName === "VIDEO",
      title: text(meta && meta.title),
      artist: text(meta && meta.artist),
      album: text(meta && meta.album),
      artwork: artwork(),
      hasPrev: handlers.has("previoustrack"),
      hasNext: handlers.has("nexttrack"),
      poppable: poppable(el),
      popped: !!popped,
    });
  }
  function schedule() {
    if (!queued) { queued = true; setTimeout(report, 120); }
  }

  // Servo (0.7) fires timeupdate only after the element's text track list exists, which it
  // creates when something reads `textTracks` (its own controls do); players without native
  // controls would otherwise never see their position move.
  document.addEventListener("loadstart", (e) => {
    if (e.target instanceof HTMLMediaElement) void e.target.textTracks;
  }, true);

  document.addEventListener("playing", (e) => {
    const el = e.target;
    if (!(el instanceof HTMLMediaElement)) return;
    void el.textTracks;
    current = el;
    if (!el.__hakuPlaying && !el.muted) { el.__hakuPlaying = true; audible++; }
    if (window.__hakuMuted) el.muted = true;
    if (popped === el) popped = null;
    schedule();
  }, true);
  const stop = (e) => {
    const el = e.target;
    if (!(el instanceof HTMLMediaElement)) return;
    if (el.__hakuPlaying) { el.__hakuPlaying = false; audible = Math.max(0, audible - 1); }
    schedule();
  };
  document.addEventListener("pause", stop, true);
  document.addEventListener("ended", stop, true);
  document.addEventListener("emptied", stop, true);
  document.addEventListener("volumechange", schedule, true);
  document.addEventListener("loadedmetadata", schedule, true);

  // The video to pop out: the one playing most recently, else the biggest one on screen.
  function pickVideo() {
    if (poppable(current)) return current;
    let best = null, area = 0;
    for (const v of document.querySelectorAll("video")) {
      const r = v.getBoundingClientRect();
      const a = r.width * r.height;
      if (a > area && poppable(v)) { best = v; area = a; }
    }
    return best;
  }

  function popOut(el, auto) {
    el = el || pickVideo();
    if (!el) { post({ type: "popout-unavailable", reason: document.querySelector("video") ? "stream" : "none" }); return; }
    if (auto && (el.paused || el.ended)) return;
    post({
      type: "popout",
      src: srcOf(el).slice(0, 8192),
      time: el.currentTime || 0,
      paused: el.paused || el.ended,
      volume: el.volume,
      muted: el.muted,
      poster: (el.poster || "").slice(0, 2048),
      title: text((meta && meta.title) || document.title),
      width: el.videoWidth || 0,
      height: el.videoHeight || 0,
      auto: !!auto,
    });
    popped = el;
    el.pause();
    hideButton();
  }

  function call(action) {
    const handler = handlers.get(action);
    if (!handler) return false;
    try { handler({ action }); } catch (_) {}
    return true;
  }

  Object.defineProperty(window, "__hakuMedia", {
    value: (action, arg) => {
      const el = current;
      switch (action) {
        case "play":
          if (!call("play") && el) el.play().catch(() => {});
          break;
        case "pause":
          if (!call("pause") && el) el.pause();
          break;
        case "next":
          call("nexttrack");
          break;
        case "prev":
          call("previoustrack");
          break;
        case "popout":
          popOut(null, arg && arg.auto);
          break;
        case "return":
        case "closed": {
          const v = popped;
          popped = null;
          if (v && arg && typeof arg.time === "number" && isFinite(arg.time)) {
            try { v.currentTime = arg.time; } catch (_) {}
          }
          if (v && action === "return" && arg && arg.play) v.play().catch(() => {});
          schedule();
          break;
        }
        case "button":
          button = !!arg;
          if (!button) hideButton();
          break;
      }
    },
  });

  // ---- the pop-out button shown over a video under the pointer
  let host = null, shown = null, hideTimer = 0, lastMove = 0;
  function makeButton() {
    host = document.createElement("haku-popout");
    host.style.cssText = "all:initial;position:fixed;z-index:2147483647;display:none;";
    const root = host.attachShadow ? host.attachShadow({ mode: "closed" }) : host;
    const b = document.createElement("button");
    b.textContent = "⧉  Pop out";
    b.title = "Watch this video in a small window that stays on top";
    b.style.cssText = "all:initial;font:600 12px system-ui,sans-serif;color:#fff;background:rgba(18,16,24,.82);" +
      "border:1px solid rgba(255,255,255,.25);border-radius:6px;padding:5px 10px;cursor:pointer;";
    b.addEventListener("click", (e) => { e.preventDefault(); e.stopPropagation(); popOut(shown, false); }, true);
    for (const t of ["mousedown", "mouseup", "pointerdown", "pointerup"]) b.addEventListener(t, (e) => e.stopPropagation(), true);
    root.appendChild(b);
    (document.body || document.documentElement).appendChild(host);
  }
  function hideButton() {
    if (host) host.style.display = "none";
    shown = null;
  }
  document.addEventListener("mousemove", (e) => {
    if (!button || e.timeStamp - lastMove < 120) return;
    lastMove = e.timeStamp;
    let under = null;
    for (const v of document.querySelectorAll("video")) {
      const r = v.getBoundingClientRect();
      if (r.width >= 200 && r.height >= 112 && e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom && poppable(v) && v !== popped) { under = v; break; }
    }
    if (!under) { if (shown) hideButton(); return; }
    if (!host || !host.isConnected) makeButton();
    const r = under.getBoundingClientRect();
    host.style.left = Math.round(r.left + r.width / 2 - 45) + "px";
    // Below Servo's own control bar, which sits along the top of the video.
    host.style.top = Math.round(Math.max(r.top, 0) + 52) + "px";
    host.style.display = "block";
    shown = under;
    clearTimeout(hideTimer);
    hideTimer = setTimeout(hideButton, 2500);
  }, true);
})();

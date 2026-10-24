/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

//! Scripts HakuCX injects into web pages.
//!
//! Servo has no isolated worlds, so these run in the page's own JavaScript context. They never
//! receive privileges: the only way back to the core is the page channel (`console.debug` with
//! [`super::PAGE_PREFIX`]), which accepts a short allow-list of message types (see
//! `delegate.rs`). Anything a page could forge there, it could already do by navigating.

use std::rc::Rc;

use servo::{UserContentManager, UserScript};

/// Installed into every page webview at creation.
/// Login and form detection for the password manager (it never fills anything by itself).
const AUTOFILL: &str = include_str!("autofill.js");
/// Media state for the miniplayer and the video pop-out button (see the file's header).
const MEDIA: &str = include_str!("media.js");

/// The media script, with the pop-out button on or off.
pub fn media_script(popout_button: bool) -> String {
    MEDIA.replace(
        "__HAKU_POPOUT_BUTTON__",
        if popout_button { "true" } else { "false" },
    )
}

/// Turn the pop-out button on or off in a page that is already open.
pub fn media_button_script(on: bool) -> String {
    format!("window.__hakuMedia && window.__hakuMedia('button', {on});")
}

/// Ask a page's media script to do `action` (`play`, `pause`, `next`, `prev`, `popout`,
/// `return`, `closed`), with `arg` as JSON.
pub fn media_action_script(action: &str, arg: &serde_json::Value) -> String {
    let action = serde_json::to_string(action).unwrap_or_else(|_| "\"\"".into());
    format!("window.__hakuMedia && window.__hakuMedia({action}, {arg});")
}

pub fn install(manager: &Rc<UserContentManager>, settings: &haku_core::settings::Settings) {
    manager.add_script(Rc::new(UserScript::new(BOOTSTRAP.to_owned(), None)));
    manager.add_script(Rc::new(UserScript::new(AUTOFILL.to_owned(), None)));
    let video = &settings.video;
    manager.add_script(Rc::new(UserScript::new(
        media_script(video.popout && video.video_pickup),
        None,
    )));
    let privacy = privacy_signals(
        settings.privacy.do_not_track,
        settings.privacy.global_privacy_control,
    );
    if !privacy.is_empty() {
        manager.add_script(Rc::new(UserScript::new(privacy, None)));
    }
}

/// CSS for page features: RGX (more contrast and color for videos and images) and the focus
/// highlight (a short accent outline on the focused element). `accent` is `#rrggbb`.
pub fn feature_css(rgx_videos: bool, rgx_images: bool, focus_accent: Option<&str>) -> String {
    let mut css = String::new();
    if rgx_videos {
        css.push_str(
            "
video { filter: contrast(1.08) saturate(1.22) !important; }",
        );
    }
    if rgx_images {
        css.push_str(
            "
img, picture { filter: contrast(1.05) saturate(1.15) !important; }",
        );
    }
    if let Some(accent) = focus_accent.filter(|a| {
        a.len() == 7 && a.starts_with('#') && a[1..].chars().all(|c| c.is_ascii_hexdigit())
    }) {
        css.push_str(&format!(
            "
@keyframes __haku_focus {{ from {{ outline-color: {accent}; }} to {{ outline-color: transparent; }} }}
:focus-visible {{ outline: 3px solid {accent} !important; outline-offset: 2px !important; animation: __haku_focus 1.2s ease-out forwards; }}"
        ));
    }
    css
}

/// Do Not Track and Global Privacy Control as the JavaScript signals pages read
/// (`navigator.doNotTrack`, `navigator.globalPrivacyControl`). The engine does not let the
/// browser add request headers, so the `DNT`/`Sec-GPC` headers are not sent.
pub fn privacy_signals(dnt: bool, gpc: bool) -> String {
    let mut s = String::new();
    if dnt {
        s.push_str("try { Object.defineProperty(Navigator.prototype, 'doNotTrack', { get: () => '1', configurable: true }); } catch (_) {}\n");
    }
    if gpc {
        s.push_str("try { Object.defineProperty(Navigator.prototype, 'globalPrivacyControl', { get: () => true, configurable: true }); } catch (_) {}\n");
    }
    s
}

/// Downloads for `<a download>`, the extension content-script relay and generic element hiding.
const BOOTSTRAP: &str = r#"(() => {
  if (window.__hakuBoot) return;
  Object.defineProperty(window, "__hakuBoot", { value: true });
  const PREFIX = "\u0001haku-page:";
  const post = (msg) => { try { console.debug(PREFIX + JSON.stringify(msg)); } catch (_) {} };
  // Extension content-script relay: the browser can only run script in the top frame, so it
  // hands messages here; each frame gives them to its own content scripts and forwards them to
  // its child frames.
  const extHandlers = new Map();
  Object.defineProperty(window, "__hakuExtRegister", { value: (token, fn) => {
    let set = extHandlers.get(token);
    if (!set) extHandlers.set(token, (set = new Set()));
    set.add(fn);
  } });
  const relay = (token, payload) => {
    for (const handler of extHandlers.get(token) || []) {
      try { handler(payload); } catch (e) { console.error(e); }
    }
    for (let i = 0; i < window.frames.length; i++) {
      try { window.frames[i].postMessage({ __hakuExtRelay: token, payload }, "*"); } catch (_) {}
    }
  };
  Object.defineProperty(window, "__hakuRelay", { value: relay });
  window.addEventListener("message", (e) => {
    const d = e.data;
    if (d && typeof d.__hakuExtRelay === "string" && e.source === window.parent && window.parent !== window) relay(d.__hakuExtRelay, d.payload);
  }, true);
  // Search pop-up: report text selected with the mouse (not in form fields), and when it
  // goes away. The browser decides whether to show anything.
  let selectionShown = false;
  document.addEventListener("mouseup", (event) => {
    if (event.button !== 0) return;
    setTimeout(() => {
      const sel = window.getSelection();
      const text = sel ? sel.toString().trim() : "";
      const active = document.activeElement;
      const editable = active && (active.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(active.tagName));
      if (!text || text.length > 500 || editable || !sel.rangeCount) return;
      const r = sel.getRangeAt(0).getBoundingClientRect();
      if (r.width < 1 && r.height < 1) return;
      selectionShown = true;
      post({ type: "selection", text, rect: { x: r.left, y: r.top, w: r.width, h: r.height } });
    }, 0);
  }, true);
  const clearSelection = () => {
    if (!selectionShown) return;
    selectionShown = false;
    post({ type: "selection", text: "", rect: { x: 0, y: 0, w: 0, h: 0 } });
  };
  document.addEventListener("mousedown", clearSelection, true);
  document.addEventListener("keydown", clearSelection, true);
  window.addEventListener("scroll", clearSelection, true);
  const MAX_INLINE = 64 * 1024 * 1024;
  document.addEventListener("click", (event) => {
    if (event.defaultPrevented || event.button !== 0) return;
    const a = event.target && event.target.closest ? event.target.closest("a[download]") : null;
    if (!a || !a.href) return;
    const filename = a.getAttribute("download") || "";
    if (a.href.startsWith("blob:")) {
      event.preventDefault();
      fetch(a.href).then((r) => r.blob()).then((blob) => {
        if (blob.size > MAX_INLINE) { post({ type: "download-error", reason: "too-large" }); return; }
        const reader = new FileReader();
        reader.onload = () => post({ type: "download-data", dataUrl: reader.result, filename });
        reader.readAsDataURL(blob);
      }).catch(() => post({ type: "download-error", reason: "blob" }));
      return;
    }
    if (a.href.startsWith("data:")) {
      event.preventDefault();
      if (a.href.length > MAX_INLINE) { post({ type: "download-error", reason: "too-large" }); return; }
      post({ type: "download-data", dataUrl: a.href, filename });
      return;
    }
    if (a.href.startsWith("http:") || a.href.startsWith("https:")) {
      event.preventDefault();
      post({ type: "download", url: a.href, filename });
    }
  }, true);
  // Generic element hiding: report class and id names the page uses (each once) so the
  // browser can answer with the matching hide rules. Names never leave the browser.
  const seenClasses = new Set(), seenIds = new Set();
  let queued = false;
  const collect = () => {
    queued = false;
    const classes = [], ids = [];
    for (const el of document.querySelectorAll("[class],[id]")) {
      if (el.id && !seenIds.has(el.id) && seenIds.size < 20000) { seenIds.add(el.id); ids.push(el.id); }
      const list = el.classList;
      for (let i = 0; i < list.length; i++) {
        const c = list[i];
        if (!seenClasses.has(c) && seenClasses.size < 20000) { seenClasses.add(c); classes.push(c); }
      }
    }
    if (classes.length || ids.length) post({ type: "cosmetic", classes, ids });
  };
  const schedule = () => { if (!queued) { queued = true; setTimeout(collect, 400); } };
  const start = () => {
    collect();
    new MutationObserver(schedule).observe(document.documentElement, { childList: true, subtree: true, attributes: true, attributeFilter: ["class", "id"] });
  };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start, { once: true });
  else start();
})();"#;

/// Appends generic element-hiding rules to the page's HakuCX cosmetic style element.
pub fn append_cosmetic_script(css: &str) -> String {
    let css = serde_json::to_string(css).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(() => {{
  let style = document.getElementById("__haku_cosmetic");
  if (!style) {{
    style = document.createElement("style");
    style.id = "__haku_cosmetic";
    (document.head || document.documentElement).appendChild(style);
  }}
  style.textContent += "\n" + {css};
}})();"#
    )
}

pub const MUTE_ALL: &str = r#"(() => {
  window.__hakuMuted = true;
  const apply = () => document.querySelectorAll("video, audio").forEach((m) => {
    if (!m.muted) { m.dataset.hakuMuted = "1"; m.muted = true; }
  });
  apply();
  if (!window.__hakuMuteObserver) {
    window.__hakuMuteObserver = new MutationObserver(() => { if (window.__hakuMuted) apply(); });
    window.__hakuMuteObserver.observe(document.documentElement, { childList: true, subtree: true });
  }
})();"#;

pub const UNMUTE_ALL: &str = r#"(() => {
  if (!window.__hakuMuted) return;
  window.__hakuMuted = false;
  document.querySelectorAll("video, audio").forEach((m) => {
    if (m.dataset.hakuMuted === "1") { delete m.dataset.hakuMuted; m.muted = false; }
  });
})();"#;

/// `document.contentType`, used to detect responses that are downloads, not pages.
pub const CONTENT_TYPE: &str = "document.contentType || ''";

/// MIME types the engine renders as a page. Anything else that a top-level navigation lands on
/// is treated as a download.
pub fn renderable_mime(mime: &str) -> bool {
    let m = mime
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    m.is_empty()
        || m.starts_with("text/")
        || m.starts_with("image/")
        || m.starts_with("video/")
        || m.starts_with("audio/")
        || matches!(
            m.as_str(),
            "application/xhtml+xml"
                | "application/xml"
                | "application/json"
                | "application/javascript"
                | "application/ld+json"
                | "application/rss+xml"
                | "application/atom+xml"
                | "image/svg+xml"
                | "application/wasm"
        )
}

/// File extensions that are always downloads when a page links to them directly.
pub fn download_extension(url: &url::Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    const EXTENSIONS: &[&str] = &[
        ".zip", ".7z", ".rar", ".tar", ".gz", ".tgz", ".xz", ".bz2", ".zst", ".exe", ".msi",
        ".msix", ".appx", ".dmg", ".pkg", ".deb", ".rpm", ".apk", ".iso", ".img", ".bin",
        ".torrent", ".jar", ".crx", ".xpi", ".docx", ".xlsx", ".pptx", ".odt", ".ods", ".odp",
    ];
    EXTENSIONS.iter().any(|e| path.ends_with(e))
}

/// Find-in-page. Returns `"<matches>,<active>"` (active is 1-based, 0 when none).
pub fn find_script(text: &str, forward: bool, match_case: bool) -> String {
    let needle = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into());
    format!(
        r##"(() => {{
  const needle = {needle};
  const forward = {forward};
  const matchCase = {match_case};
  const state = window.__hakuFind || (window.__hakuFind = {{ query: null, matchCase: false, index: -1 }});
  const clear = () => {{
    document.querySelectorAll("mark[data-haku-find]").forEach((m) => {{
      const parent = m.parentNode;
      if (!parent) return;
      parent.replaceChild(document.createTextNode(m.textContent), m);
      parent.normalize();
    }});
  }};
  if (state.query !== needle || state.matchCase !== matchCase) {{
    clear();
    state.query = needle; state.matchCase = matchCase; state.index = -1;
    if (needle) {{
      const walker = document.createTreeWalker(document.body || document.documentElement, NodeFilter.SHOW_TEXT, {{
        acceptNode: (n) => {{
          const p = n.parentElement;
          if (!p || /^(SCRIPT|STYLE|NOSCRIPT|TEXTAREA|INPUT)$/.test(p.tagName)) return NodeFilter.FILTER_REJECT;
          return NodeFilter.FILTER_ACCEPT;
        }}
      }});
      const nodes = [];
      while (walker.nextNode()) nodes.push(walker.currentNode);
      const q = matchCase ? needle : needle.toLowerCase();
      for (const node of nodes) {{
        const hay = matchCase ? node.nodeValue : node.nodeValue.toLowerCase();
        let at = hay.indexOf(q);
        if (at < 0) continue;
        const frag = document.createDocumentFragment();
        let last = 0;
        while (at >= 0) {{
          frag.appendChild(document.createTextNode(node.nodeValue.slice(last, at)));
          const mark = document.createElement("mark");
          mark.setAttribute("data-haku-find", "");
          mark.style.cssText = "background:#ffe14d;color:#000;border-radius:2px";
          mark.textContent = node.nodeValue.slice(at, at + needle.length);
          frag.appendChild(mark);
          last = at + needle.length;
          at = hay.indexOf(q, last);
        }}
        frag.appendChild(document.createTextNode(node.nodeValue.slice(last)));
        node.parentNode.replaceChild(frag, node);
      }}
    }}
  }}
  const marks = Array.from(document.querySelectorAll("mark[data-haku-find]"));
  if (!marks.length) return "0,0";
  if (state.index >= 0 && marks[state.index]) marks[state.index].style.background = "#ffe14d";
  state.index = forward ? (state.index + 1) % marks.length : (state.index - 1 + marks.length) % marks.length;
  const current = marks[state.index];
  current.style.background = "#ff9632";
  current.scrollIntoView({{ block: "center", inline: "nearest" }});
  return marks.length + "," + (state.index + 1);
}})()"##
    )
}

pub const FIND_CLEAR: &str = r#"(() => {
  document.querySelectorAll("mark[data-haku-find]").forEach((m) => {
    const parent = m.parentNode;
    if (!parent) return;
    parent.replaceChild(document.createTextNode(m.textContent), m);
    parent.normalize();
  });
  window.__hakuFind = { query: null, matchCase: false, index: -1 };
})()"#;

/// A built-in shader: a named CSS filter applied to the whole page.
pub struct Shader {
    pub id: &'static str,
    pub filter: &'static str,
}

/// HakuCX's own filter definitions for the GX-style shader names.
pub const SHADERS: &[Shader] = &[
    Shader {
        id: "shift-fx",
        filter: "hue-rotate(35deg) saturate(1.25)",
    },
    Shader {
        id: "midnight-fx",
        filter: "brightness(0.82) contrast(1.12) sepia(0.2) hue-rotate(190deg) saturate(0.85)",
    },
    Shader {
        id: "fade-fx",
        filter: "contrast(0.86) saturate(0.7) brightness(1.06)",
    },
    Shader {
        id: "boost-fx",
        filter: "contrast(1.16) saturate(1.5)",
    },
    Shader {
        id: "mono-core",
        filter: "grayscale(1) contrast(1.12)",
    },
    Shader {
        id: "duo-fx",
        filter: "grayscale(1) sepia(1) hue-rotate(220deg) saturate(2.4) contrast(1.05)",
    },
];

/// Page styling that depends on settings: force dark, shader and per-site CSS. Applied by a
/// script so it can be updated live on already-open pages.
pub fn style_script(force_dark: bool, shader_filter: Option<&str>, site_css: &str) -> String {
    let mut css = String::new();
    let mut filters = Vec::new();
    if force_dark {
        filters.push("invert(0.9) hue-rotate(180deg)".to_owned());
    }
    if let Some(f) = shader_filter {
        filters.push(f.to_owned());
    }
    if !filters.is_empty() {
        css.push_str(&format!(
            "html {{ filter: {} !important; }}\n",
            filters.join(" ")
        ));
    }
    if force_dark {
        // Undo the inversion for media so photos and video look right.
        css.push_str("img, video, picture, canvas, iframe, [style*=\"background-image\"] { filter: invert(1) hue-rotate(180deg) !important; }\n");
        css.push_str("html { background: #fff !important; }\n");
    }
    css.push_str(site_css);
    let css = serde_json::to_string(&css).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(() => {{
  const css = {css};
  let style = document.getElementById("__haku_style");
  if (!css) {{ if (style) style.remove(); return; }}
  if (!style) {{
    style = document.createElement("style");
    style.id = "__haku_style";
    (document.head || document.documentElement).appendChild(style);
  }}
  if (style.textContent !== css) style.textContent = css;
}})()"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_rules() {
        assert!(renderable_mime("text/html; charset=utf-8"));
        assert!(renderable_mime("image/png"));
        assert!(!renderable_mime("application/zip"));
        assert!(!renderable_mime("application/octet-stream"));
    }

    #[test]
    fn extensions() {
        let u = |s: &str| url::Url::parse(s).unwrap();
        assert!(download_extension(&u("https://x.test/a/setup.EXE")));
        assert!(!download_extension(&u("https://x.test/page.html")));
        assert!(!download_extension(&u("https://x.test/zip")));
    }

    #[test]
    fn find_script_escapes_input() {
        let s = find_script("\"); alert(1); (\"", true, false);
        assert!(s.contains(r#"const needle = "\"); alert(1); (\"";"#));
    }

    #[test]
    fn style_script_escapes_css() {
        let s = style_script(false, None, "</style><script>alert(1)</script>");
        assert!(s.contains("const css = \""));
        assert!(!s.contains("const css = </style>"));
    }
}

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Login and form detection for the password manager and autofill. It never fills anything
// itself: it reports submitted logins (so HakuCX can offer to save them) and which kind of
// field has focus (so HakuCX can show saved accounts, addresses or cards in its own UI). Only
// the top frame reports. The browser decides everything from the tab's real address, not
// from what this script says.
(() => {
  if (window.top !== window || window.__hakuAutofill) return;
  Object.defineProperty(window, "__hakuAutofill", { value: true });
  const PREFIX = "\u0001haku-page:";
  const debug = console.debug.bind(console);
  const post = (msg) => {
    try {
      debug(PREFIX + JSON.stringify(msg));
    } catch (_) {}
  };

  const visible = (el) => {
    if (!el || el.disabled || el.type === "hidden") return false;
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  };
  const words = (el) => [el.name, el.id, el.placeholder, el.getAttribute("aria-label"), el.autocomplete].filter(Boolean).join(" ").toLowerCase();
  const token = (el) => (el.getAttribute("autocomplete") || "").toLowerCase().split(/\s+/).filter((t) => t && !/^(section-|shipping|billing|home|work|mobile)/.test(t)).pop() || "";
  const textish = (el) => el instanceof HTMLInputElement && ["text", "email", "tel", ""].includes(el.type);

  const CARD = /\b(cc-|card.?num|cardnumber|card.?holder|name.?on.?card|cc.?(num|name|exp)|expir|security.?code)/;
  const ADDRESS = /(address|street|addr|city|town|zip|postal|postcode|province|state\b|country|phone|tel\b|given|family|first.?name|last.?name|surname|company|organi[sz]ation)/;

  const passwordFields = (root) => Array.from((root || document).querySelectorAll("input[type=password]")).filter(visible);

  // The username field for a password field: one marked as such, else the nearest visible
  // text field before it.
  const usernameFor = (pw, root) => {
    const scope = (pw && pw.form) || root || document;
    const fields = Array.from(scope.querySelectorAll("input")).filter((el) => textish(el) && visible(el));
    const marked = fields.find((el) => /^(username|email)$/.test(token(el)));
    if (marked) return marked;
    if (!pw) return fields.find((el) => /user|login|email|account|identifier/.test(words(el))) || null;
    let best = null;
    for (const el of fields) {
      if (el.compareDocumentPosition(pw) & Node.DOCUMENT_POSITION_FOLLOWING) best = el;
    }
    return best;
  };

  const classify = (el) => {
    if (!(el instanceof HTMLInputElement || el instanceof HTMLSelectElement)) return null;
    if (el instanceof HTMLInputElement && el.type === "password") return token(el) === "new-password" ? null : "login";
    const t = token(el);
    const w = words(el);
    if (t.startsWith("cc-") || CARD.test(w)) return "card";
    if (/^(username|email)$/.test(t) && passwordFields(el.form || document).length) return "login";
    if (textish(el) && passwordFields(el.form || null).length && usernameFor(passwordFields(el.form || null)[0], el.form) === el) return "login";
    if (t && /^(name|given-name|family-name|organization|street-address|address-line\d|address-level\d|postal-code|country|country-name|tel|email)$/.test(t)) return "address";
    if (el.form && ADDRESS.test(w) && !/search|query/.test(w)) return "address";
    return null;
  };

  document.addEventListener(
    "focusin",
    (e) => {
      const el = e.target;
      const kind = classify(el);
      if (!kind) return;
      const r = el.getBoundingClientRect();
      post({ type: "field-focus", kind, rect: { x: r.left, y: r.top, w: r.width, h: r.height } });
    },
    true,
  );

  // ---- submitted logins
  let last = "";
  let lastAt = 0;
  const capture = (root) => {
    const pws = passwordFields(root).filter((el) => el.value);
    if (!pws.length) return;
    // Change-password forms (current, new, confirm): the new one. Sign-up forms (password,
    // confirm): the first.
    let pw = pws[0];
    if (pws.length >= 3) pw = pws[1];
    else if (pws.length === 2 && pws[0].value !== pws[1].value && token(pws[1]) === "new-password") pw = pws[1];
    const user = usernameFor(pws[0], root);
    const username = user ? user.value.trim() : "";
    const key = username + "\u0000" + pw.value;
    if (key === last && Date.now() - lastAt < 5000) return;
    last = key;
    lastAt = Date.now();
    post({ type: "login-submit", username, password: pw.value });
  };
  document.addEventListener("submit", (e) => capture(e.target), true);
  document.addEventListener(
    "click",
    (e) => {
      const b = e.target && e.target.closest ? e.target.closest("button, input[type=submit], input[type=button], [role=button]") : null;
      if (!b) return;
      const text = (b.textContent || b.value || b.getAttribute("aria-label") || "").toLowerCase();
      const submitLike = b.type === "submit" || /log\s?in|sign\s?in|sign\s?up|submit|continue|next|register|anmelden|connexion|entrar|iniciar/.test(text);
      if (submitLike) capture(b.form || b.closest("form") || null);
    },
    true,
  );
  document.addEventListener(
    "keydown",
    (e) => {
      if (e.key === "Enter" && e.target instanceof HTMLInputElement && (e.target.type === "password" || classify(e.target) === "login")) capture(e.target.form || null);
    },
    true,
  );
})();

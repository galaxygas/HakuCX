/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Fills one saved login, address or card into the form that has focus. HakuCX runs it only
// after the user picked the entry in its own UI. `__KIND__` and `__DATA__` are replaced with
// JSON before it runs. Returns how many fields it filled.
(() => {
  const kind = __KIND__;
  const data = __DATA__;
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
  const visible = (el) => {
    if (!el || el.disabled || el.readOnly || el.type === "hidden") return false;
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  };
  const words = (el) => [el.name, el.id, el.placeholder, el.getAttribute("aria-label")].filter(Boolean).join(" ").toLowerCase();
  const token = (el) => (el.getAttribute("autocomplete") || "").toLowerCase().split(/\s+/).filter((t) => t && !/^(section-|shipping|billing|home|work|mobile)/.test(t)).pop() || "";
  const fire = (el) => {
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  };
  const put = (el, value) => {
    if (!el || value === undefined || value === null || value === "") return 0;
    if (el instanceof HTMLSelectElement) {
      const v = String(value).toLowerCase();
      const opt = Array.from(el.options).find((o) => o.value.toLowerCase() === v || o.text.trim().toLowerCase() === v);
      if (!opt) return 0;
      el.value = opt.value;
    } else {
      setter.call(el, String(value));
    }
    fire(el);
    return 1;
  };
  const active = document.activeElement;
  const root = (active && active.form) || document;
  const fields = Array.from(root.querySelectorAll("input, select")).filter(visible);
  let filled = 0;

  if (kind === "login") {
    const pw = fields.find((el) => el.type === "password" && token(el) !== "new-password") || fields.find((el) => el.type === "password");
    const textish = (el) => el instanceof HTMLInputElement && ["text", "email", "tel", ""].includes(el.type);
    let user = fields.find((el) => textish(el) && /^(username|email)$/.test(token(el)));
    if (!user && pw) {
      for (const el of fields) if (textish(el) && el.compareDocumentPosition(pw) & Node.DOCUMENT_POSITION_FOLLOWING) user = el;
    }
    if (!user && active && textish(active)) user = active;
    if (user && data.username) filled += put(user, data.username);
    if (pw) filled += put(pw, data.password);
    return filled;
  }

  const RULES = {
    address: [
      ["full_name", ["name"], /^(full.?name|your.?name|name)$|\bfull.?name\b/],
      ["given_name", ["given-name"], /first.?name|given.?name|forename/],
      ["family_name", ["family-name"], /last.?name|surname|family.?name/],
      ["organization", ["organization"], /company|organi[sz]ation/],
      ["line1", ["street-address", "address-line1"], /^(?!.*e.?mail).*(address.?(line)?.?1|street|addr1|^address$)/],
      ["line2", ["address-line2"], /address.?(line)?.?2|addr2|apartment|suite|\bapt\b/],
      ["city", ["address-level2"], /city|town|locality/],
      ["region", ["address-level1"], /state|province|region|county/],
      ["postal_code", ["postal-code"], /zip|postal|post.?code/],
      ["country", ["country", "country-name"], /country/],
      ["phone", ["tel"], /phone|mobile|\btel\b/],
      ["email", ["email"], /e.?mail/],
    ],
    card: [
      ["holder", ["cc-name"], /name.?on.?card|card.?holder|cc.?name/],
      ["number", ["cc-number"], /card.?num|cardnumber|cc.?num|credit.?card/],
      ["exp", ["cc-exp"], /^(?!.*(month|year)).*(exp(iry|iration)?(.?date)?|mm.?\/?.?yy)/],
      ["exp_month", ["cc-exp-month"], /exp.*month|cc.?month|\bmm\b|month/],
      ["exp_year", ["cc-exp-year"], /exp.*year|cc.?year|\byy(yy)?\b|year/],
    ],
  }[kind] || [];
  const used = new Set();
  for (const [key, tokens, pattern] of RULES) {
    const el = fields.find((f) => !used.has(f) && f.type !== "password" && tokens.includes(token(f))) || fields.find((f) => !used.has(f) && f.type !== "password" && !token(f) && pattern.test(words(f)));
    if (!el) continue;
    let value = data[key];
    if (key === "exp_year" && el instanceof HTMLSelectElement) {
      const full = String(value);
      const opt = Array.from(el.options).find((o) => [o.value, o.text.trim()].some((t) => t === full || t === full.slice(-2)));
      if (opt) value = opt.value;
    }
    if (key === "exp_year" && el instanceof HTMLInputElement && (el.maxLength === 2 || /\byy\b/.test(words(el)))) value = String(value).slice(-2);
    if (key === "exp_month" && el instanceof HTMLSelectElement) {
      const n = Number(value);
      const opt = Array.from(el.options).find((o) => Number(o.value) === n || Number(o.text) === n);
      if (opt) value = opt.value;
    }
    const n = put(el, value);
    if (n) used.add(el);
    filled += n;
  }
  // A single textarea for the street address.
  if (kind === "address") {
    const area = Array.from(root.querySelectorAll("textarea")).find((t) => visible(t) && (token(t) === "street-address" || /address|street/.test(words(t))));
    if (area && data.street) {
      area.value = data.street;
      fire(area);
      filled += 1;
    }
  }
  return filled;
})()

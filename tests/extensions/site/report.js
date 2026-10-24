/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Checks what the sample extensions did to this page, shows it, and reports it to the test
// server (see ../server.py).
(function () {
  const expect = [
    ["blocked.js blocked by a static declarativeNetRequest rule", () => !window.loaded.blocked],
    ["dynamic-blocked.js blocked by a dynamic rule", () => !window.loaded.dynamicBlocked],
    ["ff-cancel.js cancelled by a blocking webRequest listener", () => !window.loaded.ffCancel],
    ["allowed.js loaded", () => !!window.loaded.allowed],
    ["redirect-me.png redirected to the extension's green image", () => imageIsGreen()],
    ["content script banner present", () => !!document.getElementById("haku-test-banner")],
    ["content script got a reply from its background page", () => /hi from background/.test(bannerText())],
    ["content script port answered", () => /pong/.test(bannerText())],
  ];

  function bannerText() {
    const b = document.getElementById("haku-test-banner");
    return b ? b.textContent : "";
  }

  function imageIsGreen() {
    const img = document.getElementById("redir");
    if (!img.complete || !img.naturalWidth) return false;
    const c = document.createElement("canvas");
    c.width = img.naturalWidth;
    c.height = img.naturalHeight;
    const ctx = c.getContext("2d");
    ctx.drawImage(img, 0, 0);
    const [r, g] = ctx.getImageData(0, 0, 1, 1).data;
    return g > 100 && r < 60;
  }

  function run() {
    const table = document.getElementById("results");
    const results = {};
    table.textContent = "";
    for (const [name, test] of expect) {
      let ok = false;
      try {
        ok = !!test();
      } catch (e) {
        ok = false;
      }
      results[name] = ok;
      const row = table.insertRow();
      row.insertCell().textContent = name;
      const cell = row.insertCell();
      cell.textContent = ok ? "PASS" : "FAIL";
      cell.className = ok ? "pass" : "fail";
    }
    fetch("/report", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ from: "page", results, banner: bannerText() }) }).catch(() => {});
  }

  // Content scripts run at document_end/idle and talk to their background page first.
  window.addEventListener("load", () => setTimeout(run, 2500));
})();

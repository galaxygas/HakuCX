/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Firefox style: the promise-based `browser` namespace and a blocking webRequest listener.
browser.webRequest.onBeforeRequest.addListener(
  (details) => (details.url.endsWith("/ff-cancel.js") ? { cancel: true } : {}),
  { urls: ["http://127.0.0.1/*"], types: ["script"] },
  ["blocking"],
);

browser.runtime.getPlatformInfo().then((info) =>
  fetch("http://127.0.0.1:8765/report", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ from: "firefox-mv2", event: "started", id: browser.runtime.id, os: info.os }),
  }).catch(() => {}),
);

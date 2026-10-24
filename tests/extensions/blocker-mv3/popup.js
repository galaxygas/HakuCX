/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

chrome.runtime.sendMessage({ type: "stats" }, (reply) => {
  document.getElementById("stats").textContent = chrome.runtime.lastError
    ? "Error: " + chrome.runtime.lastError.message
    : `${reply.matched} requests matched a rule`;
  fetch("http://127.0.0.1:8765/report", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ from: "blocker-popup", reply }) }).catch(() => {});
});
chrome.storage.local.get("installedAt").then(({ installedAt }) => {
  document.getElementById("installed").textContent = installedAt ? "Installed " + new Date(installedAt).toLocaleTimeString() : "";
});
document.getElementById("options").addEventListener("click", () => chrome.runtime.openOptionsPage());

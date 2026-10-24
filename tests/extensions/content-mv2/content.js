/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Runs on the test site: shows a banner, talks to the background page by message and by port.
const banner = document.createElement("div");
banner.id = "haku-test-banner";
banner.textContent = chrome.i18n.getMessage("bannerHello", location.host);
document.body.prepend(banner);

const line = (text) => {
  const p = document.createElement("div");
  p.textContent = text;
  banner.appendChild(p);
};

chrome.runtime.sendMessage({ type: "hello" }, (reply) => {
  if (chrome.runtime.lastError) line("message error: " + chrome.runtime.lastError.message);
  else line(`${reply.reply} (tab ${reply.tabId}, visit ${reply.visits})`);
});

const port = chrome.runtime.connect({ name: "test" });
port.onMessage.addListener((msg) => line("port: " + msg));
port.postMessage("ping");

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.type === "toggle") {
    banner.hidden = !banner.hidden;
    sendResponse({ hidden: banner.hidden });
  } else if (message.type === "mark") {
    line("marked: " + message.text);
  }
});

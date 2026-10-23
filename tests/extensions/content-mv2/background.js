/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Persistent background page (MV2): answers the content script, keeps a port open with it, and
// owns the context-menu item, the toolbar button and the keyboard shortcut.
const report = (event, data) =>
  fetch("http://127.0.0.1:8765/report", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ from: "content-mv2", event, ...data }),
  }).catch(() => {});

chrome.runtime.onInstalled.addListener((details) => {
  chrome.contextMenus.create({ id: "haku-mark", title: "Mark with HakuCX Test", contexts: ["page", "selection"] });
  report("installed", { reason: details.reason });
});

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.type === "hello") {
    chrome.storage.local.get({ visits: 0 }, ({ visits }) => {
      chrome.storage.local.set({ visits: visits + 1 });
      sendResponse({ reply: "hi from background", tabId: sender.tab && sender.tab.id, visits: visits + 1 });
    });
    return true;
  }
});

chrome.runtime.onConnect.addListener((port) => {
  port.onMessage.addListener((msg) => {
    if (msg === "ping") port.postMessage("pong");
  });
});

const toggle = (tab, via) => {
  chrome.tabs.sendMessage(tab.id, { type: "toggle" }, (reply) => {
    report("toggle", { via, ok: !chrome.runtime.lastError, hidden: reply && reply.hidden });
  });
};

chrome.browserAction.onClicked.addListener((tab) => toggle(tab, "button"));
chrome.commands.onCommand.addListener((command, tab) => {
  if (command === "toggle-banner") chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => tabs[0] && toggle(tabs[0], "shortcut"));
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  chrome.tabs.sendMessage(tab.id, { type: "mark", text: info.selectionText || info.pageUrl });
  report("menu", { item: info.menuItemId, selection: info.selectionText || null, url: tab.url });
});

chrome.tabs.onUpdated.addListener((tabId, change, tab) => {
  if (change.status === "complete" && tab.url && tab.url.startsWith("http://127.0.0.1")) report("tab-complete", { tabId, title: tab.title });
});

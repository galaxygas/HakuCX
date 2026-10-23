/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Service worker: adds a dynamic rule, shows the blocked count on the badge, and answers the
// popup. Everything it observes is reported to the test server.
const report = (event, data) =>
  fetch("http://127.0.0.1:8765/report", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ from: "blocker-mv3", event, ...data }),
  }).catch(() => {});

chrome.runtime.onInstalled.addListener(async (details) => {
  await chrome.declarativeNetRequest.updateDynamicRules({
    removeRuleIds: [100],
    addRules: [{ id: 100, priority: 1, action: { type: "block" }, condition: { urlFilter: "dynamic-blocked.js", resourceTypes: ["script"] } }],
  });
  await chrome.declarativeNetRequest.setExtensionActionOptions({ displayActionCountAsBadgeText: true });
  await chrome.storage.local.set({ installedAt: Date.now(), reason: details.reason });
  const rules = await chrome.declarativeNetRequest.getDynamicRules();
  const enabled = await chrome.declarativeNetRequest.getEnabledRulesets();
  chrome.alarms.create("tick", { delayInMinutes: 0.05 });
  report("installed", { reason: details.reason, dynamicRules: rules.map((r) => r.id), enabled });
});

chrome.alarms.onAlarm.addListener((alarm) => report("alarm", { name: alarm.name }));

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message && message.type === "stats") {
    chrome.declarativeNetRequest.getMatchedRules({}).then((info) => {
      sendResponse({ matched: info.rulesMatchedInfo.length, from: sender.url });
    });
    return true;
  }
  return false;
});

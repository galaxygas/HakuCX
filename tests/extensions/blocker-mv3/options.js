/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

const box = document.getElementById("static");
chrome.declarativeNetRequest.getEnabledRulesets().then((ids) => (box.checked = ids.includes("default")));
box.addEventListener("change", async () => {
  await chrome.declarativeNetRequest.updateEnabledRulesets(box.checked ? { enableRulesetIds: ["default"] } : { disableRulesetIds: ["default"] });
  document.getElementById("saved").textContent = "Saved.";
});
localStorage.setItem("optionsOpened", String(Date.now()));

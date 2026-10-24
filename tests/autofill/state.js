/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Shows every field's value, so a screenshot shows what autofill filled.
(function () {
  const show = () => {
    const out = [];
    for (const el of document.querySelectorAll("input, select, textarea")) {
      if (el.type === "submit" || el.type === "button") continue;
      out.push((el.name || el.id) + " = " + JSON.stringify(el.value));
    }
    document.getElementById("state").textContent = out.join("\n");
  };
  document.addEventListener("input", show, true);
  document.addEventListener("change", show, true);
  window.addEventListener("load", show);
})();

"use strict";
(() => {
  const PREFIX = "\u0001haku-ipc:";
  const send = (msg) => console.debug(PREFIX + JSON.stringify(msg));
  const $ = (id) => document.getElementById(id);
  let tabs = [];
  let active = 0;

  function renderTabs() {
    const root = $("tabs");
    root.textContent = "";
    tabs.forEach((t, i) => {
      const el = document.createElement("div");
      el.className = "tab" + (i === active ? " active" : "") + (t.loading ? " loading" : "");
      const title = document.createElement("span");
      title.textContent = t.title || t.url || "New tab";
      const x = document.createElement("span");
      x.className = "x";
      x.textContent = "×";
      x.addEventListener("click", (e) => {
        e.stopPropagation();
        send({ type: "close-tab", index: i });
      });
      el.append(title, x);
      el.addEventListener("click", () => send({ type: "activate-tab", index: i }));
      root.append(el);
    });
    const cur = tabs[active];
    if (cur && document.activeElement !== $("address")) {
      $("address").value = cur.url.startsWith("haku://newtab") ? "" : cur.url;
    }
  }

  window.__haku = {
    receive(ev) {
      switch (ev.type) {
        case "tabs":
          tabs = ev.tabs;
          active = ev.active;
          renderTabs();
          break;
        case "blocked":
          $("blocked").textContent = ev.count + " blocked";
          break;
        case "screenshot":
          $("status").textContent = ev.ok
            ? "screenshot " + ev.w + "x" + ev.h + " -> " + ev.path
            : "screenshot failed " + ev.error;
          break;
        case "eval-result":
          $("status").textContent = "user script in page: " + ev.value;
          break;
      }
    },
  };

  const reportRect = () => {
    const r = $("viewport").getBoundingClientRect();
    send({ type: "page-rect", x: r.left, y: r.top, w: r.width, h: r.height });
  };
  new ResizeObserver(reportRect).observe($("viewport"));
  window.addEventListener("resize", reportRect);

  document.querySelectorAll("[data-cmd]").forEach((b) =>
    b.addEventListener("click", () => send({ type: b.dataset.cmd })),
  );
  $("newtab").addEventListener("click", () => send({ type: "new-tab" }));
  $("shot").addEventListener("click", () => send({ type: "screenshot" }));
  $("evalbtn").addEventListener("click", () => send({ type: "eval-page" }));
  $("address").addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      send({ type: "navigate", url: $("address").value });
      $("address").blur();
    }
  });
  const setMenu = (open) => {
    $("menu").hidden = !open;
    send({ type: "overlay", open });
  };
  $("menubtn").addEventListener("click", () => setMenu($("menu").hidden));
  $("closemenu").addEventListener("click", () => setMenu(false));

  reportRect();
  send({ type: "ready" });
})();

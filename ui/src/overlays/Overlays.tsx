/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useEffect } from "react";
import { Icon, Logo } from "../components/Icon";
import { send } from "../ipc/bridge";
import { clsx } from "../lib/util";
import { useStore } from "../state/store";
import { ExtPopupHost } from "../shell/ExtensionButtons";
import { EasySetup } from "./EasySetup";
import { ExtPrompts } from "./ExtPrompts";
import { AutofillHost, PasswordPromptHost } from "./AutofillOverlays";
import { Menus } from "./Menus";
import { Modals } from "./Modals";
import { PageDialogs } from "./PageDialogs";
import { Palette } from "./Palette";
import { Popups } from "./Popups";
import { SidebarSetup } from "./SidebarSetup";

export function Overlays() {
  const popup = useStore((s) => s.popup);
  const menu = useStore((s) => s.menu);
  const drawer = useStore((s) => s.drawer);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      const s = useStore.getState();
      if (s.autofill) useStore.setState({ autofill: null });
      else if (s.menu) s.openMenu(null);
      else if (s.extPopup) send({ type: "ext-popup-close" });
      else if (s.popup) s.openPopup(null);
      else if (s.palette_open) s.setPalette(null);
      else if (s.drawer) s.setDrawer(null);
      else if (s.modal) s.openModal(null);
      else if (s.find.open) s.setFindOpen(false);
      else if (s.panel && !s.settings?.sidebar.pinnedPanels.includes(s.panel)) s.setPanel(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <>
      {(popup || menu || drawer) && (
        <div
          className="scrim"
          onMouseDown={() => {
            const s = useStore.getState();
            s.openMenu(null);
            s.openPopup(null);
            s.setDrawer(null);
          }}
        />
      )}
      <EasySetup />
      <SidebarSetup />
      <ExtPopupHost />
      <Popups />
      <Menus />
      <Palette />
      <Modals />
      <PageDialogs />
      <AutofillHost />
      <PasswordPromptHost />
      <ExtPrompts />
      <Toasts />
      <Splash />
    </>
  );
}

function Toasts() {
  const toasts = useStore((s) => s.toasts);
  const dismiss = useStore((s) => s.dismissToast);
  return (
    <div className="toasts" data-hit={toasts.length ? "" : undefined}>
      {toasts.map((t) => (
        <div key={t.id} className={clsx("toast", t.kind)} onClick={() => dismiss(t.id)}>
          <Icon name={t.kind === "error" ? "alert" : t.kind === "notification" ? "bell" : "ok"} size={15} />
          <span>{t.text}</span>
          {t.action && (
            <button
              className="toast-action"
              onClick={(e) => {
                e.stopPropagation();
                t.action!.run();
                dismiss(t.id);
              }}
            >
              {t.action.label}
            </button>
          )}
        </div>
      ))}
    </div>
  );
}

function Splash() {
  const splash = useStore((s) => s.splash);
  const ready = useStore((s) => s.ready);
  const end = useStore((s) => s.endSplash);
  useEffect(() => {
    if (!ready) return;
    if (!splash) return;
    const t = window.setTimeout(end, 900);
    return () => window.clearTimeout(t);
  }, [ready, splash, end]);
  if (!splash && ready) return null;
  return (
    <div className="splash" onClick={end}>
      <div className="splash-mark">
        <Logo size={72} />
      </div>
      <div className="display splash-title">HakuCX</div>
    </div>
  );
}

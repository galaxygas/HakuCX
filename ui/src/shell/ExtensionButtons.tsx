/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Extension toolbar buttons and the frame an extension popup is shown in. The popup page
// itself is a separate webview the core draws into the transparent hole of `ExtPopupHost`.

import { send } from "../ipc/bridge";
import type { ExtensionInfo } from "../ipc/generated/ExtensionInfo";
import type { Rect } from "../ipc/generated/Rect";
import { clsx } from "../lib/util";
import { ExtIcon } from "../pages/ExtensionsPage";
import { useStore } from "../state/store";

function rectOf(el: HTMLElement): Rect {
  const r = el.getBoundingClientRect();
  return { x: r.left, y: r.top, w: r.width, h: r.height };
}

/** Click an extension's toolbar button: its popup, or `action.onClicked`. */
export function activateExtension(ext: ExtensionInfo, anchor: HTMLElement) {
  send({ type: "ext-action-click", id: ext.id, anchor: rectOf(anchor) });
}

export function ExtensionButton({ ext, className }: { ext: ExtensionInfo; className?: string }) {
  const open = useStore((s) => s.extPopup?.id === ext.id);
  return (
    <button
      className={clsx("tool-btn ext-btn", open && "active", !ext.actionEnabled && "dim", className)}
      title={ext.actionTitle}
      onClick={(e) => activateExtension(ext, e.currentTarget)}
    >
      <ExtIcon src={ext.actionIcon ?? ext.icon} size={18} />
      {ext.badgeText && (
        <span className="ext-badge" style={{ background: ext.badgeColor }}>
          {ext.badgeText.slice(0, 4)}
        </span>
      )}
    </button>
  );
}

/** Pinned extensions, shown in the toolbar. */
export function PinnedExtensions() {
  const extensions = useStore((s) => s.extensions);
  const pinned = extensions.filter((e) => e.enabled && e.pinned && e.version);
  if (!pinned.length) return null;
  return (
    <>
      {pinned.map((ext) => (
        <ExtensionButton key={ext.id} ext={ext} />
      ))}
    </>
  );
}

const MIN_W = 25;
const MIN_H = 25;
const MAX_W = 800;
const MAX_H = 600;

/**
 * The open extension popup: a bordered frame around a hole the core fills with the page.
 *
 * The core draws the popup page below the UI layer (so menus and dialogs can cover it), which
 * means opaque browser chrome would hide it: it is kept inside the page area, just under the
 * button that opened it.
 */
export function ExtPopupHost() {
  const popup = useStore((s) => s.extPopup);
  if (!popup) return null;
  const area = document.querySelector(".viewport")?.getBoundingClientRect();
  const minTop = area ? area.top + 6 : 0;
  const minLeft = area ? area.left + 6 : 8;
  const maxRight = area ? area.right - 6 : window.innerWidth - 8;
  const top = Math.max(popup.anchor.y + popup.anchor.h + 6, minTop);
  const w = Math.max(MIN_W, Math.min(MAX_W, popup.width, maxRight - minLeft));
  const h = Math.max(MIN_H, Math.min(MAX_H, popup.height, window.innerHeight - top - 8));
  // Right-aligned with the button, kept inside the page area.
  const left = Math.max(minLeft, Math.min(maxRight - w, popup.anchor.x + popup.anchor.w - w));
  return (
    <div className="ext-popup" style={{ left: left - 1, top: top - 1, width: w + 2, height: h + 2 }}>
      <div className="ext-popup-hole" style={{ width: w, height: h }} />
    </div>
  );
}

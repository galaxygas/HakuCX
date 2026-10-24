/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { Icon, Lettermark, Logo } from "../components/Icon";
import { send } from "../ipc/bridge";
import { sidebarItem, type SidebarItem } from "../lib/catalog";
import { clsx } from "../lib/util";
import { noteSidebarUse } from "../panels/CleanerPanel";
import { openInternal, useStore } from "../state/store";

export function Sidebar() {
  const settings = useStore((s) => s.settings);
  const session = useStore((s) => s.session);
  const panel = useStore((s) => s.panel);
  const togglePanel = useStore((s) => s.togglePanel);
  const setDrawer = useStore((s) => s.setDrawer);
  const drawer = useStore((s) => s.drawer);
  const openMenu = useStore((s) => s.openMenu);
  const badges = useStore((s) => s.badges);
  if (!settings) return null;
  const sb = settings.sidebar;
  const gxIcons = settings.appearance.gxMessengerIcons;

  const activate = (item: SidebarItem) => {
    send({ type: "play-sound", sound: "click" });
    noteSidebarUse(item.id);
    if (item.page) openInternal(item.page);
    else togglePanel(item.id);
  };

  const items = sb.items.map(sidebarItem).filter((i): i is SidebarItem => !!i);
  const workspaces = (session?.workspaces ?? []).filter((w) => !sb.hiddenWorkspaces.includes(w.id));

  return (
    <aside className={clsx("sidebar", sb.centerIcons && "centered", settings.appearance.colorizeIcons && "colorize")}>
      <div className="sidebar-scroll">
        {sb.workspacesEnabled && workspaces.length > 1 && (
          <div className="sidebar-workspaces">
            {workspaces.map((w) => (
              <button
                key={w.id}
                className={clsx("sidebar-item", w.id === session?.activeWorkspace && "workspace-active")}
                title={w.name}
                onClick={() => send({ type: "switch-workspace", workspace: w.id })}
              >
                <Icon name={w.icon} size={18} />
              </button>
            ))}
            <span className="sidebar-sep" />
          </div>
        )}
        {items.map((item) => {
          const active = panel === item.id;
          return (
            <button
              key={item.id}
              className={clsx("sidebar-item", active && "active", item.web && "web")}
              title={item.label}
              onClick={() => activate(item)}
              onContextMenu={(e) => {
                e.preventDefault();
                openMenu({ kind: "sidebar", x: e.clientX, y: e.clientY, item: item.id });
              }}
            >
              {item.web ? <Lettermark label={item.label} color={item.brand} size={24} gx={gxIcons} /> : <Icon name={item.icon} size={19} />}
              {sb.notificationBadges && (badges[item.id] ?? 0) > 0 && <span className="badge">{badges[item.id]! > 99 ? "99+" : badges[item.id]}</span>}
            </button>
          );
        })}
      </div>
      <div className="sidebar-bottom">
        <button className={clsx("sidebar-item", drawer === "sidebar-setup" && "active")} title="Sidebar setup" onClick={() => setDrawer(drawer === "sidebar-setup" ? null : "sidebar-setup")}>
          <Icon name="ellipsis" size={19} />
        </button>
        <button className="sidebar-logo" title="Open GX Corner" onClick={() => openInternal("haku://corner/")}>
          <Logo size={20} />
        </button>
      </div>
    </aside>
  );
}

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import { useState } from "react";
import { Toggle } from "../components/controls";
import { Icon, Lettermark } from "../components/Icon";
import { SIDEBAR_GROUPS, SIDEBAR_ITEMS } from "../lib/catalog";
import { patch } from "../lib/util";
import { useStore } from "../state/store";
import { Line, Section } from "./EasySetup";

export function SidebarSetup() {
  const open = useStore((s) => s.drawer === "sidebar-setup");
  const settings = useStore((s) => s.settings);
  const session = useStore((s) => s.session);
  const setDrawer = useStore((s) => s.setDrawer);
  const openModal = useStore((s) => s.openModal);
  const [moreMessengers, setMoreMessengers] = useState(false);
  if (!open || !settings || !session) return null;
  const sb = settings.sidebar;
  const gx = settings.appearance.gxMessengerIcons;

  const toggleItem = (id: string, on: boolean) => {
    const items = on ? [...sb.items.filter((i) => i !== id), id] : sb.items.filter((i) => i !== id);
    patch({ sidebar: { items } });
  };

  return (
    <aside className="drawer sidebar-setup" onMouseDown={(e) => e.stopPropagation()}>
      <header className="drawer-head">
        <h2 className="display accent">Sidebar setup</h2>
        <span className="grow" />
        <button className="icon-btn" title="Close" onClick={() => setDrawer(null)}>
          <Icon name="x" size={16} />
        </button>
      </header>
      <div className="drawer-body">
        <Section title="Workspaces" extra={<Toggle on={sb.workspacesEnabled} onChange={(v) => patch({ sidebar: { workspacesEnabled: v } })} />}>
          {session.workspaces.map((w) => (
            <Line
              key={w.id}
              label={
                <span className="row">
                  <Icon name={w.icon} size={15} /> {w.name}
                  <button className="icon-btn small" title="Edit" onClick={() => openModal({ kind: "workspace-edit", workspace: w.id })}>
                    <Icon name="pencil" size={12} />
                  </button>
                </span>
              }
            >
              <Toggle
                on={!sb.hiddenWorkspaces.includes(w.id)}
                disabled={!sb.workspacesEnabled}
                onChange={(v) => patch({ sidebar: { hiddenWorkspaces: v ? sb.hiddenWorkspaces.filter((x) => x !== w.id) : [...sb.hiddenWorkspaces, w.id] } })}
              />
            </Line>
          ))}
          <button className="link-btn add" onClick={() => openModal({ kind: "workspace-edit", workspace: null })}>
            Add <Icon name="plus" size={13} />
          </button>
        </Section>

        {SIDEBAR_GROUPS.map((group) => {
          let items = SIDEBAR_ITEMS.filter((i) => i.group === group.id);
          const limited = group.id === "messengers" && !moreMessengers && items.length > 3;
          if (limited) items = items.slice(0, 3);
          return (
            <Section key={group.id} title={group.label}>
              {items.map((item) => (
                <Line
                  key={item.id}
                  label={
                    <span className="row">
                      {item.web ? <Lettermark label={item.label} color={item.brand} size={20} gx={gx} /> : <Icon name={item.icon} size={16} />}
                      {item.label}
                    </span>
                  }
                >
                  <Toggle on={sb.items.includes(item.id)} onChange={(v) => toggleItem(item.id, v)} label={item.label} />
                </Line>
              ))}
              {group.id === "messengers" && (
                <button className="link-btn" onClick={() => setMoreMessengers((m) => !m)}>
                  {moreMessengers ? "Show less" : "Show more"} <Icon name={moreMessengers ? "chevron-up" : "chevron-down"} size={13} />
                </button>
              )}
            </Section>
          );
        })}

        <Section title="Settings">
          <Line label="Show sidebar">
            <Toggle on={sb.show} onChange={(v) => patch({ sidebar: { show: v } })} />
          </Line>
          <Line label="Enable notifications for messengers">
            <Toggle on={sb.messengerNotifications} onChange={(v) => patch({ sidebar: { messengerNotifications: v } })} />
          </Line>
          <Line label="Enable notification badges">
            <Toggle on={sb.notificationBadges} onChange={(v) => patch({ sidebar: { notificationBadges: v } })} />
          </Line>
          <Line label="Use GX messenger icons">
            <Toggle on={gx} onChange={(v) => patch({ appearance: { gxMessengerIcons: v } })} />
          </Line>
          <Line label="Center icons">
            <Toggle on={sb.centerIcons} onChange={(v) => patch({ sidebar: { centerIcons: v } })} />
          </Line>
        </Section>
      </div>
    </aside>
  );
}

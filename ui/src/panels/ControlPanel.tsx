/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

import type { ReactNode } from "react";
import { Gauge, Segmented, Select, Slider, Sparkline, Toggle } from "../components/controls";
import { Icon } from "../components/Icon";
import { send } from "../ipc/bridge";
import { formatRate, hostOf, patch } from "../lib/util";
import { useStore } from "../state/store";

const NETWORK_CAPS: { value: string; label: string }[] = [
  { value: "524288", label: "0.5 MB/s - 4 Mbps" },
  { value: "1048576", label: "1 MB/s - 8 Mbps" },
  { value: "5242880", label: "5 MB/s - 40 Mbps" },
  { value: "13107200", label: "12.5 MB/s - 100 Mbps" },
  { value: "26214400", label: "25 MB/s - 200 Mbps" },
  { value: "65536000", label: "62.5 MB/s - 500 Mbps" },
];

function Card({ icon, title, info, on, onToggle, children }: { icon: string; title: string; info: string; on?: boolean; onToggle?: (v: boolean) => void; children: ReactNode }) {
  return (
    <div className="gx-card">
      <div className="gx-card-head">
        <Icon name={icon} size={16} />
        <span className="display gx-card-title">{title}</span>
        <span className="info-dot" title={info}>
          <Icon name="info" size={13} />
        </span>
        <span className="grow" />
        {onToggle && <Toggle on={!!on} onChange={onToggle} label={title} />}
      </div>
      {children}
    </div>
  );
}

export function ControlPanel() {
  const control = useStore((s) => s.settings?.control);
  const multiprocess = useStore((s) => s.settings?.system.multiprocess ?? false);
  const metrics = useStore((s) => s.metrics);
  const cpuHistory = useStore((s) => s.cpuHistory);
  const netHistory = useStore((s) => s.netHistory);
  const session = useStore((s) => s.session);
  if (!control) return null;
  const systemGb = Math.max(1, Math.round((metrics?.systemMemoryMb ?? 16384) / 1024));
  const usedGb = (metrics?.memoryMb ?? 0) / 1024;
  const limitGb = control.ramLimitMb / 1024;

  // Hot tabs: real per-tab usage needs multi-process mode with per-process accounting. Until
  // the engine reports it, tabs are listed by recency so heavy background tabs can be closed.
  const tabs = (session?.tabs ?? [])
    .filter((t) => !t.snoozed)
    .sort((a, b) => a.lastActive - b.lastActive)
    .slice(0, 8);

  return (
    <div className="panel-scroll control-panel">
      <Card
        icon="memory"
        title="RAM limiter"
        info="Keeps the browser's memory use under the limit by snoozing the least recently used background tabs."
        on={control.ramLimiter}
        onToggle={(v) => patch({ control: { ramLimiter: v } })}
      >
        <div className="gauge-row">
          <Gauge value={usedGb} max={systemGb} label={usedGb.toFixed(1)} unit="GB" ticks={["0", String(Math.round(systemGb / 2)), String(systemGb)]} />
        </div>
        <div className="field-label">
          Memory limit (GB) <span className="accent">{limitGb.toFixed(1)}</span>
        </div>
        <Slider value={limitGb} min={0.5} max={systemGb} step={0.5} onCommit={(v) => patch({ control: { ramLimitMb: Math.round(v * 1024) } })} label="Memory limit" disabled={!control.ramLimiter} />
        <div className="scale-row">
          <span>MIN</span>
          <span>MAX</span>
        </div>
        <div className="setting-line">
          <div>
            <div className="display small">Hard limit</div>
            <small className="muted">
              {multiprocess ? "Memory limit will not be exceeded: pages over it crash instead of the browser." : "Needs multi-process mode (Settings > System)."}
            </small>
          </div>
          <Toggle on={control.ramHardLimit} disabled={!multiprocess || !control.ramLimiter} onChange={(v) => patch({ control: { ramHardLimit: v } })} label="Hard limit" />
        </div>
      </Card>

      <Card
        icon="wifi"
        title="Network limiter"
        info="Caps the speed of downloads HakuCX makes. Page traffic is handled by the engine and is not capped."
        on={control.networkLimiter}
        onToggle={(v) => patch({ control: { networkLimiter: v } })}
      >
        <div className="net-readout">
          <div>
            <Icon name="arrow-down" size={14} /> <span className="dim">Download</span>
            <strong>{formatRate(metrics?.downloadBps ?? 0)}</strong>
          </div>
          <div>
            <Icon name="arrow-up" size={14} /> <span className="dim">Upload</span>
            <strong>{formatRate(metrics?.uploadBps ?? 0)}</strong>
          </div>
        </div>
        <Sparkline values={netHistory} />
        <Select
          value={String(control.networkLimitBps)}
          options={NETWORK_CAPS.some((c) => c.value === String(control.networkLimitBps)) ? NETWORK_CAPS : [...NETWORK_CAPS, { value: String(control.networkLimitBps), label: formatRate(control.networkLimitBps) }]}
          onChange={(v) => patch({ control: { networkLimitBps: Number(v) } })}
          disabled={!control.networkLimiter}
        />
      </Card>

      <Card icon="flame" title="Hot tabs killer" info="Find and close the tabs that use the most resources.">
        <Segmented
          value={control.hotTabsSort}
          options={[
            { value: "cpu", label: "CPU" },
            { value: "ram", label: "RAM" },
          ]}
          onChange={(v) => patch({ control: { hotTabsSort: v } })}
        />
        <p className="note">Per-tab usage is not reported by the engine yet; tabs are listed by how long they have been idle.</p>
        <div className="hot-tabs">
          {tabs.map((t) => (
            <div key={t.id} className="hot-tab">
              <Icon name="globe" size={14} />
              <span className="ellipsis grow">{hostOf(t.url) || t.title}</span>
              <button className="icon-btn" title="Close tab" onClick={() => send({ type: "close-tab", tab: t.id })}>
                <Icon name="x" size={14} />
              </button>
            </div>
          ))}
        </div>
      </Card>

      <Card
        icon="cpu"
        title="CPU limiter"
        info="A hard cap on the share of processor time the browser may use, enforced by Windows."
        on={control.cpuLimiter}
        onToggle={(v) => patch({ control: { cpuLimiter: v } })}
      >
        <div className="gauge-row">
          <Gauge value={metrics?.cpu ?? 0} max={100} label={String(Math.round(metrics?.cpu ?? 0))} unit="%" ticks={["0", "50", "100"]} />
        </div>
        <Sparkline values={cpuHistory} max={100} />
        <div className="field-label">
          Processor limit (%) <span className="accent">{control.cpuLimit}</span>
        </div>
        <Slider value={control.cpuLimit} min={10} max={100} step={5} onCommit={(v) => patch({ control: { cpuLimit: v } })} label="Processor limit" disabled={!control.cpuLimiter} />
        <div className="scale-row">
          <span>MIN</span>
          <span>MAX</span>
        </div>
      </Card>

      <div className="setting-line">
        <span>Keep limiters enabled after browser restart</span>
        <Toggle on={control.keepAfterRestart} onChange={(v) => patch({ control: { keepAfterRestart: v } })} />
      </div>
    </div>
  );
}

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Declarative settings rows. Every row reads and writes one typed setting, so the settings
// page, its search, and Easy Setup can never disagree about what an option does.

import type { ReactNode } from "react";
import { Button, Select, Slider, Toggle } from "../../components/controls";
import { Icon } from "../../components/Icon";
import type { Settings } from "../../ipc/generated/Settings";
import { clsx, patch } from "../../lib/util";
import { go } from "../InternalPage";

export type Row =
  | { kind: "toggle"; label: string; description?: string; get: (s: Settings) => boolean; set: (v: boolean) => void; disabled?: (s: Settings) => string | false }
  | { kind: "select"; label: string; description?: string; get: (s: Settings) => string; options: { value: string; label: string }[]; set: (v: string) => void; disabled?: (s: Settings) => string | false; width?: number }
  | { kind: "slider"; label: string; description?: string; get: (s: Settings) => number; min: number; max: number; step?: number; format?: (v: number) => string; set: (v: number) => void; disabled?: (s: Settings) => string | false }
  | { kind: "link"; label: string; description?: string; to: string; icon?: string }
  | { kind: "action"; label: string; description?: string; button: string; run: () => void; danger?: boolean }
  | { kind: "custom"; label: string; description?: string; render: (s: Settings) => ReactNode; full?: boolean }
  | { kind: "unsupported"; label: string; reason: string };

export type Card = { title: string; rows: Row[] };
export type SectionDef = { id: string; label: string; icon: string; group: number; cards: Card[] };

// ------------------------------------------------------------------ builders

type Sec = keyof Settings;

export function toggle<S extends Sec>(section: S, key: keyof Settings[S] & string, label: string, description?: string, disabled?: (s: Settings) => string | false): Row {
  return {
    kind: "toggle",
    label,
    description,
    get: (s) => Boolean((s[section] as Record<string, unknown>)[key]),
    set: (v) => patch({ [section]: { [key]: v } } as never),
    disabled,
  };
}

export function select<S extends Sec>(section: S, key: keyof Settings[S] & string, label: string, options: [string, string][], description?: string, width?: number): Row {
  return {
    kind: "select",
    label,
    description,
    width,
    get: (s) => String((s[section] as Record<string, unknown>)[key]),
    options: options.map(([value, l]) => ({ value, label: l })),
    set: (v) => patch({ [section]: { [key]: v } } as never),
  };
}

export function slider<S extends Sec>(section: S, key: keyof Settings[S] & string, label: string, min: number, max: number, step = 1, format?: (v: number) => string, scale = 1): Row {
  return {
    kind: "slider",
    label,
    // Round to the step so scaled fractions (0.55 x 100) do not show float noise.
    get: (s) => Math.round((Number((s[section] as Record<string, unknown>)[key]) * scale) / step) * step,
    min,
    max,
    step,
    format,
    set: (v) => patch({ [section]: { [key]: v / scale } } as never),
  };
}

export const link = (label: string, to: string, description?: string, icon?: string): Row => ({ kind: "link", label, to, description, icon });
export const action = (label: string, button: string, run: () => void, description?: string, danger?: boolean): Row => ({ kind: "action", label, button, run, description, danger });
export const custom = (label: string, render: (s: Settings) => ReactNode, description?: string, full = false): Row => ({ kind: "custom", label, render, description, full });
export const unsupported = (label: string, reason: string): Row => ({ kind: "unsupported", label, reason });

// ------------------------------------------------------------------ rendering

export function RowView({ row, settings }: { row: Row; settings: Settings }) {
  const disabledReason = "disabled" in row && row.disabled ? row.disabled(settings) : false;
  const label = (
    <div className="setting-line-text">
      <span>{row.label}</span>
      {"description" in row && row.description && <small className="muted">{row.description}</small>}
      {disabledReason && <small className="warn-text">{disabledReason}</small>}
    </div>
  );
  switch (row.kind) {
    case "toggle":
      return (
        <div className="setting-line">
          {label}
          <Toggle on={row.get(settings)} onChange={row.set} disabled={!!disabledReason} label={row.label} />
        </div>
      );
    case "select":
      return (
        <div className="setting-line">
          {label}
          <Select value={row.get(settings)} options={row.options} onChange={row.set} disabled={!!disabledReason} width={row.width ?? 210} />
        </div>
      );
    case "slider": {
      const value = row.get(settings);
      return (
        <div className="setting-line slider-line">
          {label}
          <div className="slider-wrap">
            <Slider value={value} min={row.min} max={row.max} step={row.step} onCommit={row.set} label={row.label} disabled={!!disabledReason} />
            <span className="slider-value">{row.format ? row.format(value) : value}</span>
          </div>
        </div>
      );
    }
    case "link":
      return (
        <button className="setting-line link-line" onClick={() => go(row.to)}>
          {row.icon && <Icon name={row.icon} size={16} />}
          {label}
          <Icon name="chevron-right" size={16} />
        </button>
      );
    case "action":
      return (
        <div className="setting-line">
          {label}
          <Button kind={row.danger ? "danger" : "plain"} onClick={row.run}>
            {row.button}
          </Button>
        </div>
      );
    case "custom":
      return row.full ? (
        <div className="setting-block">
          {row.label && label}
          {row.render(settings)}
        </div>
      ) : (
        <div className="setting-line">
          {label}
          {row.render(settings)}
        </div>
      );
    case "unsupported":
      return (
        <div className={clsx("setting-line", "unsupported")}>
          <div className="setting-line-text">
            <span>{row.label}</span>
            <small className="warn-text">{row.reason}</small>
          </div>
          <Toggle on={false} onChange={() => {}} disabled label={row.label} />
        </div>
      );
  }
}

export function rowText(row: Row): string {
  return `${row.label} ${"description" in row ? (row.description ?? "") : ""}`.toLowerCase();
}

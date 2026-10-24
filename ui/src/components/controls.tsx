/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// GX-style form controls. Native range/select inputs ignore styling in Servo, so these are
// built from plain elements with full keyboard support.

import { useEffect, useRef, useState, type ReactNode } from "react";
import { send } from "../ipc/bridge";
import { clamp, clsx } from "../lib/util";
import { Icon, useThemeColor } from "./Icon";

export function click() {
  send({ type: "play-sound", sound: "click" });
}

export function Toggle({ on, onChange, disabled, label }: { on: boolean; onChange: (on: boolean) => void; disabled?: boolean; label?: string }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      className={clsx("toggle", on && "on")}
      onClick={() => {
        click();
        onChange(!on);
      }}
    >
      <span className="knob" />
    </button>
  );
}

export function Slider({
  value,
  min,
  max,
  step = 1,
  onChange,
  onCommit,
  disabled,
  label,
}: {
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange?: (v: number) => void;
  onCommit: (v: number) => void;
  disabled?: boolean;
  label?: string;
}) {
  const track = useRef<HTMLDivElement>(null);
  const [local, setLocal] = useState(value);
  const dragging = useRef(false);
  useEffect(() => {
    if (!dragging.current) setLocal(value);
  }, [value]);
  const snap = (v: number) => clamp(Math.round((v - min) / step) * step + min, min, max);
  const fromEvent = (clientX: number) => {
    const r = track.current?.getBoundingClientRect();
    if (!r || r.width === 0) return local;
    return snap(min + ((clientX - r.left) / r.width) * (max - min));
  };
  const pct = ((local - min) / (max - min || 1)) * 100;
  const update = (v: number, commit: boolean) => {
    setLocal(v);
    onChange?.(v);
    if (commit) onCommit(v);
  };
  return (
    <div
      ref={track}
      className={clsx("slider", disabled && "disabled")}
      role="slider"
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={local}
      tabIndex={disabled ? -1 : 0}
      onMouseDown={(e) => {
        if (disabled || e.button !== 0) return;
        dragging.current = true;
        update(fromEvent(e.clientX), false);
        const move = (ev: MouseEvent) => update(fromEvent(ev.clientX), false);
        const up = (ev: MouseEvent) => {
          dragging.current = false;
          update(fromEvent(ev.clientX), true);
          window.removeEventListener("mousemove", move);
          window.removeEventListener("mouseup", up);
        };
        window.addEventListener("mousemove", move);
        window.addEventListener("mouseup", up);
      }}
      onKeyDown={(e) => {
        if (disabled) return;
        const big = (max - min) / 10;
        const delta = { ArrowLeft: -step, ArrowDown: -step, ArrowRight: step, ArrowUp: step, PageDown: -big, PageUp: big }[e.key];
        if (delta !== undefined) {
          e.preventDefault();
          update(snap(local + delta), true);
        } else if (e.key === "Home") update(min, true);
        else if (e.key === "End") update(max, true);
      }}
    >
      <div className="slider-track">
        <div className="slider-fill" style={{ width: `${pct}%` }} />
      </div>
      <div className="slider-thumb" style={{ left: `${pct}%` }} />
    </div>
  );
}

export type Option<T extends string> = { value: T; label: string };

export function Select<T extends string>({ value, options, onChange, disabled, width }: { value: T; options: Option<T>[]; onChange: (v: T) => void; disabled?: boolean; width?: number }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);
  const current = options.find((o) => o.value === value);
  return (
    <div className={clsx("select", open && "open", disabled && "disabled")} ref={ref} style={width ? { width } : undefined}>
      <button
        type="button"
        className="select-button"
        disabled={disabled}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={(e) => {
          const i = options.findIndex((o) => o.value === value);
          if (e.key === "ArrowDown" && i < options.length - 1) onChange(options[i + 1]!.value);
          if (e.key === "ArrowUp" && i > 0) onChange(options[i - 1]!.value);
          if (e.key === "Escape") setOpen(false);
        }}
      >
        <span>{current?.label ?? value}</span>
        <Icon name="chevron-down" size={14} />
      </button>
      {open && (
        <div className="select-menu" role="listbox">
          {options.map((o) => (
            <button
              type="button"
              key={o.value}
              role="option"
              aria-selected={o.value === value}
              className={clsx("select-option", o.value === value && "selected")}
              onClick={() => {
                click();
                onChange(o.value);
                setOpen(false);
              }}
            >
              {o.label}
              {o.value === value && <Icon name="check" size={14} />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function Segmented<T extends string>({ value, options, onChange, wide }: { value: T; options: Option<T>[]; onChange: (v: T) => void; wide?: boolean }) {
  return (
    <div className={clsx("segmented", wide && "wide")} role="radiogroup">
      {options.map((o) => (
        <button
          type="button"
          key={o.value}
          role="radio"
          aria-checked={o.value === value}
          className={clsx(o.value === value && "active")}
          onClick={() => {
            click();
            onChange(o.value);
          }}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Radio({ checked, onChange, label, description }: { checked: boolean; onChange: () => void; label: ReactNode; description?: ReactNode }) {
  return (
    <button type="button" role="radio" aria-checked={checked} className={clsx("radio-row", checked && "checked")} onClick={() => { click(); onChange(); }}>
      <span className="radio-dot" />
      <span className="radio-text">
        <span>{label}</span>
        {description && <small>{description}</small>}
      </span>
    </button>
  );
}

export function Checkbox({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: ReactNode }) {
  return (
    <label className="checkbox">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span className={clsx("box", checked && "on")}>{checked && <Icon name="check" size={12} stroke={3} />}</span>
      <span>{label}</span>
    </label>
  );
}

export function Button({ children, onClick, kind = "plain", disabled, title, icon }: { children?: ReactNode; onClick?: () => void; kind?: "plain" | "accent" | "danger" | "ghost"; disabled?: boolean; title?: string; icon?: string }) {
  return (
    <button
      type="button"
      title={title}
      disabled={disabled}
      className={clsx("btn", `btn-${kind}`, !children && "btn-icon")}
      onClick={() => {
        click();
        onClick?.();
      }}
    >
      {icon && <Icon name={icon} size={16} />}
      {children}
    </button>
  );
}

export function IconButton({ icon, title, onClick, active, size = 18, disabled, className }: { icon: string; title: string; onClick?: (e: React.MouseEvent) => void; active?: boolean; size?: number; disabled?: boolean; className?: string }) {
  return (
    <button type="button" className={clsx("icon-btn", active && "active", className)} title={title} aria-label={title} disabled={disabled} onClick={onClick}>
      <Icon name={icon} size={size} />
    </button>
  );
}

/**
 * Opera GX's arc gauge (GX Control). The arcs are SVG with concrete theme colors; the numbers
 * are HTML on top, because Servo's SVG renderer does not see web fonts or CSS variables.
 */
export function Gauge({ value, max, label, unit, ticks }: { value: number; max: number; label: string; unit: string; ticks: string[] }) {
  const accent = useThemeColor("--hx-accent", "#7d46f2");
  const track = useThemeColor("--hx-elev-2", "#1d1d23");
  const frac = clamp(max > 0 ? value / max : 0, 0, 1);
  const r = 46;
  const start = Math.PI * 0.8;
  const sweep = Math.PI * 1.4;
  const point = (t: number): [number, number] => [60 + r * Math.cos(start + sweep * t), 60 + r * Math.sin(start + sweep * t)];
  const [sx, sy] = point(0);
  const [ex, ey] = point(1);
  const [vx, vy] = point(frac);
  const arc = (x: number, y: number, t: number) => `M ${sx} ${sy} A ${r} ${r} 0 ${t * sweep > Math.PI ? 1 : 0} 1 ${x} ${y}`;
  return (
    <div className="gauge">
      <svg viewBox="0 0 120 104" width="100%" height="100%">
        <path d={arc(ex, ey, 1)} stroke={track} strokeWidth="9" fill="none" strokeLinecap="round" />
        {frac > 0.001 && <path d={arc(vx, vy, frac)} stroke={accent} strokeWidth="9" fill="none" strokeLinecap="round" />}
      </svg>
      {ticks.map((t, i) => {
        const [tx, ty] = point(i / (ticks.length - 1));
        const x = 60 + (tx - 60) * 1.24;
        const y = 60 + (ty - 60) * 1.24;
        return (
          <span key={t} className="gauge-tick" style={{ left: `${(x / 120) * 100}%`, top: `${(y / 104) * 100}%` }}>
            {t}
          </span>
        );
      })}
      <div className="gauge-label">
        <strong>{label}</strong>
        <small>{unit}</small>
      </div>
    </div>
  );
}

/** A small line chart for metric history. */
export function Sparkline({ values, max, height = 36 }: { values: number[]; max?: number; height?: number }) {
  const accent = useThemeColor("--hx-accent", "#7d46f2");
  const m = max ?? Math.max(1, ...values);
  const w = 200;
  const pts = values.map((v, i) => `${(i / Math.max(1, values.length - 1)) * w},${height - (v / m) * (height - 2) - 1}`).join(" ");
  return (
    <svg className="sparkline" viewBox={`0 0 ${w} ${height}`} preserveAspectRatio="none" height={height}>
      {values.length > 1 && <polyline points={pts} fill="none" stroke={accent} strokeWidth="1.6" />}
    </svg>
  );
}

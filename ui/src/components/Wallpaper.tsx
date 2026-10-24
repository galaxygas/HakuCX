/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

// Built-in wallpapers are drawn procedurally in SVG from the theme colors, so every theme gets
// a matching wallpaper with no image assets. Custom wallpapers come from the profile.

import { useMemo } from "react";
import { clsx } from "../lib/util";
import { useStore } from "../state/store";

function seeded(seed: number) {
  let s = seed >>> 0 || 1;
  return () => {
    s ^= s << 13;
    s ^= s >>> 17;
    s ^= s << 5;
    return (s >>> 0) / 4294967295;
  };
}

/** Flowing ribbons with accent strands (the GX look). */
function Ribbons({ accent, dark, seed }: { accent: string; dark: boolean; seed: number }) {
  const paths = useMemo(() => {
    const rnd = seeded(seed);
    const out: { d: string; w: number; o: number; accent: boolean }[] = [];
    for (let i = 0; i < 26; i++) {
      const y0 = 120 + rnd() * 760;
      const amp = 80 + rnd() * 220;
      const phase = rnd() * Math.PI * 2;
      const pts: string[] = [];
      for (let x = -100; x <= 2020; x += 80) {
        const y = y0 + Math.sin(x / 380 + phase) * amp + Math.sin(x / 140 + phase * 2) * amp * 0.18;
        pts.push(`${x},${y.toFixed(1)}`);
      }
      out.push({ d: `M${pts.join(" L")}`, w: 0.6 + rnd() * (i % 5 === 0 ? 3.5 : 1.4), o: 0.12 + rnd() * 0.35, accent: i % 3 === 0 });
    }
    return out;
  }, [seed]);
  const base = dark ? "#9a9aa8" : "#ffffff";
  return (
    <svg className="wallpaper-svg" viewBox="0 0 1920 1080" preserveAspectRatio="xMidYMid slice">
      <g className="ribbon-drift">
        {paths.map((p, i) => (
          <path key={i} d={p.d} fill="none" stroke={p.accent ? accent : base} strokeWidth={p.w} strokeOpacity={p.accent ? Math.min(0.9, p.o * 2) : p.o * (dark ? 0.6 : 1)} strokeLinecap="round" />
        ))}
      </g>
    </svg>
  );
}

function Nebula({ accent, seed }: { accent: string; seed: number }) {
  const stars = useMemo(() => {
    const rnd = seeded(seed);
    return Array.from({ length: 160 }, () => ({ x: rnd() * 1920, y: rnd() * 1080, r: rnd() * 1.6 + 0.2, o: rnd() * 0.8 + 0.1 }));
  }, [seed]);
  return (
    <svg className="wallpaper-svg" viewBox="0 0 1920 1080" preserveAspectRatio="xMidYMid slice">
      <defs>
        <radialGradient id="neb1" cx="30%" cy="35%" r="45%">
          <stop offset="0" stopColor={accent} stopOpacity="0.55" />
          <stop offset="1" stopColor={accent} stopOpacity="0" />
        </radialGradient>
        <radialGradient id="neb2" cx="75%" cy="70%" r="40%">
          <stop offset="0" stopColor="#2ec8ff" stopOpacity="0.28" />
          <stop offset="1" stopColor="#2ec8ff" stopOpacity="0" />
        </radialGradient>
      </defs>
      <rect width="1920" height="1080" fill="url(#neb1)" />
      <rect width="1920" height="1080" fill="url(#neb2)" />
      {stars.map((s, i) => (
        <circle key={i} cx={s.x} cy={s.y} r={s.r} fill="#fff" opacity={s.o} />
      ))}
    </svg>
  );
}

function NeonGrid({ accent }: { accent: string }) {
  const lines = [];
  for (let i = 0; i <= 24; i++) {
    const x = (i / 24) * 1920;
    lines.push(<line key={`v${i}`} x1={960 + (x - 960) * 0.15} y1={560} x2={x * 2 - 960} y2={1080} stroke={accent} strokeOpacity="0.35" />);
  }
  for (let j = 0; j < 12; j++) {
    const t = Math.pow(j / 12, 2);
    const y = 560 + t * 520;
    lines.push(<line key={`h${j}`} x1={0} y1={y} x2={1920} y2={y} stroke={accent} strokeOpacity={0.15 + t * 0.4} />);
  }
  return (
    <svg className="wallpaper-svg" viewBox="0 0 1920 1080" preserveAspectRatio="xMidYMid slice">
      <defs>
        <linearGradient id="sun" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor={accent} stopOpacity="0.9" />
          <stop offset="1" stopColor={accent} stopOpacity="0.1" />
        </linearGradient>
      </defs>
      <circle cx="960" cy="540" r="200" fill="url(#sun)" opacity="0.5" />
      {lines}
    </svg>
  );
}

function Petals({ accent, seed }: { accent: string; seed: number }) {
  const petals = useMemo(() => {
    const rnd = seeded(seed);
    return Array.from({ length: 70 }, () => ({ x: rnd() * 1920, y: rnd() * 1080, r: rnd() * 360, s: 6 + rnd() * 16, o: 0.15 + rnd() * 0.5 }));
  }, [seed]);
  return (
    <svg className="wallpaper-svg" viewBox="0 0 1920 1080" preserveAspectRatio="xMidYMid slice">
      {petals.map((p, i) => (
        <ellipse key={i} cx={p.x} cy={p.y} rx={p.s} ry={p.s * 0.55} fill={accent} opacity={p.o} transform={`rotate(${p.r} ${p.x} ${p.y})`} />
      ))}
    </svg>
  );
}

function Waves({ accent, seed, layers = 6 }: { accent: string; seed: number; layers?: number }) {
  const rnd = seeded(seed);
  const shapes = Array.from({ length: layers }, (_, i) => {
    const base = 500 + i * 90;
    const pts: string[] = [];
    const phase = rnd() * 6;
    for (let x = 0; x <= 1920; x += 60) pts.push(`${x},${(base + Math.sin(x / 260 + phase) * (40 + i * 8)).toFixed(1)}`);
    return { d: `M0,1080 L${pts.join(" L")} L1920,1080 Z`, o: 0.06 + i * 0.045 };
  });
  return (
    <svg className="wallpaper-svg" viewBox="0 0 1920 1080" preserveAspectRatio="xMidYMid slice">
      {shapes.map((s, i) => (
        <path key={i} d={s.d} fill={accent} opacity={s.o} />
      ))}
    </svg>
  );
}

export function Wallpaper({ className }: { className?: string }) {
  const appearance = useStore((s) => s.settings?.appearance);
  const palette = useStore((s) => s.palette);
  const saver = useStore((s) => s.power?.saver ?? false);
  if (!appearance || !palette) return null;
  const accent = palette.vars["--hx-accent"] ?? "#7d46f2";
  const dark = palette.dark;
  const animated = appearance.animatedWallpaper && !appearance.reduceAnimations && !saver;
  // Daily wallpapers: a different seed each day.
  const daily = appearance.dailyWallpapers ? Math.floor(Date.now() / 86_400_000) : 0;
  const seed = 1337 + daily;
  let art: React.ReactNode = null;
  if (appearance.showWallpaper) {
    switch (appearance.wallpaper) {
      case "custom":
        art = appearance.customWallpaper ? <img className="wallpaper-img" src={`haku://wallpaper/${appearance.customWallpaper}`} alt="" /> : null;
        break;
      case "haku-nebula":
        art = <Nebula accent={accent} seed={seed} />;
        break;
      case "neon-grid":
        art = <NeonGrid accent={accent} />;
        break;
      case "haku-petals":
        art = <Petals accent={accent} seed={seed} />;
        break;
      case "haku-tide":
      case "haku-depths":
        art = <Waves accent={accent} seed={seed} />;
        break;
      case "haku-ember":
        art = <Waves accent={accent} seed={seed + 7} layers={9} />;
        break;
      case "haku-carbon":
        art = <div className="carbon" />;
        break;
      case "solid":
        art = null;
        break;
      default:
        art = <Ribbons accent={accent} dark={dark} seed={seed} />;
    }
  }
  const custom = appearance.interfaceMode === "custom";
  return (
    <div className={clsx("wallpaper", animated && "animated", className)} style={{ filter: custom && appearance.blur > 0 ? `blur(${appearance.blur}px)` : undefined }}>
      {art}
      <div className="vignette" style={{ opacity: custom ? appearance.vignette : 0.55 }} />
    </div>
  );
}

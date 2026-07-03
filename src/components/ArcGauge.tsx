/**
 * ArcGauge — a 260° instrument arc with a gradient stroke and tick ring.
 * The centerpiece of the system crest; value transitions are tweened by CSS.
 */

import { useId, type ReactNode } from "react";

interface ArcGaugeProps {
  /** 0..1 */
  value: number;
  size?: number;
  strokeWidth?: number;
  /** Center content (numbers, labels). */
  children?: ReactNode;
  /** Override the gradient with a flat color (semantic states). */
  color?: string;
}

const SWEEP = 260; // degrees
const START = 140; // degrees, so the gap faces down

function polar(cx: number, cy: number, r: number, deg: number) {
  const rad = (deg * Math.PI) / 180;
  return { x: cx + r * Math.cos(rad), y: cy + r * Math.sin(rad) };
}

function arcPath(cx: number, cy: number, r: number, startDeg: number, sweepDeg: number) {
  const start = polar(cx, cy, r, startDeg);
  const end = polar(cx, cy, r, startDeg + sweepDeg);
  const large = sweepDeg > 180 ? 1 : 0;
  return `M ${start.x.toFixed(3)} ${start.y.toFixed(3)} A ${r} ${r} 0 ${large} 1 ${end.x.toFixed(3)} ${end.y.toFixed(3)}`;
}

export function ArcGauge({ value, size = 220, strokeWidth = 10, children, color }: ArcGaugeProps) {
  const gid = useId();
  const c = size / 2;
  const r = c - strokeWidth * 1.6;
  const clamped = Math.max(0, Math.min(1, value));

  const track = arcPath(c, c, r, START, SWEEP);
  const trackLen = (SWEEP / 360) * 2 * Math.PI * r;

  // 40 ticks around the sweep, outside the arc.
  const ticks = Array.from({ length: 41 }, (_, i) => {
    const deg = START + (SWEEP * i) / 40;
    const inner = polar(c, c, r + strokeWidth * 0.9, deg);
    const outer = polar(c, c, r + strokeWidth * 0.9 + (i % 5 === 0 ? 5 : 2.5), deg);
    const lit = i / 40 <= clamped;
    return { inner, outer, lit, key: i };
  });

  return (
    <div className="arc-gauge" style={{ width: size, height: size }}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
        <defs>
          <linearGradient id={gid} x1="0%" y1="100%" x2="100%" y2="0%">
            <stop offset="0%" stopColor="var(--brand-1)" />
            <stop offset="55%" stopColor="var(--brand-2)" />
            <stop offset="100%" stopColor="var(--brand-3)" />
          </linearGradient>
        </defs>

        {ticks.map((t) => (
          <line
            key={t.key}
            x1={t.inner.x}
            y1={t.inner.y}
            x2={t.outer.x}
            y2={t.outer.y}
            stroke={t.lit ? "var(--violet-400)" : "var(--ink-ghost)"}
            strokeOpacity={t.lit ? 0.9 : 0.35}
            strokeWidth={1.2}
          />
        ))}

        <path
          d={track}
          stroke="rgba(255,255,255,0.06)"
          strokeWidth={strokeWidth}
          strokeLinecap="round"
          fill="none"
        />
        <path
          d={track}
          stroke={color ?? `url(#${gid})`}
          strokeWidth={strokeWidth}
          strokeLinecap="round"
          fill="none"
          strokeDasharray={trackLen}
          strokeDashoffset={trackLen * (1 - clamped)}
          style={{
            transition: "stroke-dashoffset 900ms var(--ease-out)",
            filter: "drop-shadow(0 0 6px rgba(168,85,247,0.45))",
          }}
        />
      </svg>
      <div className="arc-gauge__center">{children}</div>
    </div>
  );
}

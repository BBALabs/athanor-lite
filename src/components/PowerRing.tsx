/**
 * PowerRing — the hero instrument. A 270° arc of light: gradient stroke from
 * deep violet to filament, a single hot dot riding the tip, no tick marks.
 * The last 10% is the redline zone, lit only when the needle enters it.
 *
 * Pass a pre-tweened value (0..1); the ring renders it directly so the arc,
 * dot, and numeral move as one instrument.
 */

import { useId, type ReactNode } from "react";

interface PowerRingProps {
  /** 0..1, already tweened by the caller. */
  value: number;
  size?: number;
  children?: ReactNode;
}

const START = 135; // degrees; 270° sweep leaves the gap at the bottom
const SWEEP = 270;
const REDLINE = 0.9;

function polar(c: number, r: number, deg: number) {
  const rad = (deg * Math.PI) / 180;
  return { x: c + r * Math.cos(rad), y: c + r * Math.sin(rad) };
}

function arcPath(c: number, r: number, fromDeg: number, sweepDeg: number) {
  const a = polar(c, r, fromDeg);
  const b = polar(c, r, fromDeg + sweepDeg);
  const large = sweepDeg > 180 ? 1 : 0;
  return `M ${a.x.toFixed(3)} ${a.y.toFixed(3)} A ${r} ${r} 0 ${large} 1 ${b.x.toFixed(3)} ${b.y.toFixed(3)}`;
}

export function PowerRing({ value, size = 300, children }: PowerRingProps) {
  const gid = useId();
  const c = size / 2;
  const stroke = 6;
  const r = c - stroke * 2.5;
  const v = Math.max(0, Math.min(1, value));
  const inRedline = v >= REDLINE;

  const track = arcPath(c, r, START, SWEEP);
  const trackLen = (SWEEP / 360) * 2 * Math.PI * r;
  const redline = arcPath(c, r, START + SWEEP * REDLINE, SWEEP * (1 - REDLINE));

  const tip = polar(c, r, START + SWEEP * v);

  return (
    <div className="ring" style={{ width: size, height: size }}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
        <defs>
          <linearGradient id={gid} x1="0%" y1="100%" x2="100%" y2="0%">
            <stop offset="0%" stopColor="var(--lume-deep)" />
            <stop offset="60%" stopColor="var(--lume)" />
            <stop offset="100%" stopColor="var(--lume-hot)" />
          </linearGradient>
          <radialGradient id={`${gid}-face`}>
            <stop offset="50%" stopColor="rgba(76,43,133,0.14)" />
            <stop offset="100%" stopColor="rgba(76,43,133,0.02)" />
          </radialGradient>
        </defs>

        {/* dial face — a breath of depth behind the numeral */}
        <circle cx={c} cy={c} r={r - 12} fill={`url(#${gid}-face)`} />

        {/* ghost track — the instrument exists even when idle */}
        <path d={track} stroke="rgba(244,240,234,0.13)" strokeWidth={2} fill="none" />

        {/* redline zone — silent until entered */}
        <path
          d={redline}
          stroke="var(--bad)"
          strokeWidth={stroke}
          strokeLinecap="round"
          fill="none"
          style={{
            opacity: inRedline ? 0.75 : 0,
            transition: "opacity 600ms var(--glide)",
          }}
        />

        {/* the light */}
        <path
          d={track}
          stroke={`url(#${gid})`}
          strokeWidth={stroke}
          strokeLinecap="round"
          fill="none"
          strokeDasharray={trackLen}
          strokeDashoffset={trackLen * (1 - v)}
        />

        {/* the dot riding the tip */}
        <circle
          cx={tip.x}
          cy={tip.y}
          r={4}
          fill={inRedline ? "var(--bad)" : "var(--lume-hot)"}
          style={{ transition: "fill 600ms var(--glide)" }}
        />
        <circle cx={tip.x} cy={tip.y} r={9} fill={inRedline ? "var(--bad)" : "var(--lume-hot)"} opacity={0.16} />
      </svg>
      <div className="ring__center">{children}</div>
    </div>
  );
}

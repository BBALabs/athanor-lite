/**
 * Sparkline — 2-minute telemetry trace with gradient underfill.
 * Renders from a plain number array; the store owns the ring buffer.
 */

import { useId } from "react";

interface SparklineProps {
  /** Values 0..max, oldest first. */
  data: number[];
  max?: number;
  width?: number;
  height?: number;
  color?: string;
  /** Expected sample count — the line grows in from the right until full. */
  capacity?: number;
}

export function Sparkline({
  data,
  max = 100,
  width = 180,
  height = 44,
  color = "var(--brand-2)",
  capacity = 120,
}: SparklineProps) {
  const gid = useId();
  if (data.length < 2) {
    return (
      <svg width={width} height={height} className="sparkline">
        <line x1={0} y1={height - 1} x2={width} y2={height - 1} stroke="var(--line)" strokeDasharray="3 4" />
      </svg>
    );
  }

  const n = Math.max(capacity, data.length);
  const stepX = width / (n - 1);
  const x0 = width - (data.length - 1) * stepX;
  const pts = data.map((v, i) => {
    const x = x0 + i * stepX;
    const y = height - 3 - (Math.max(0, Math.min(max, v)) / max) * (height - 8);
    return `${x.toFixed(2)},${y.toFixed(2)}`;
  });

  const last = pts[pts.length - 1].split(",");

  return (
    <svg width={width} height={height} className="sparkline">
      <defs>
        <linearGradient id={gid} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={color} stopOpacity="0.28" />
          <stop offset="100%" stopColor={color} stopOpacity="0" />
        </linearGradient>
      </defs>
      <polygon
        points={`${pts.join(" ")} ${width},${height} ${x0},${height}`}
        fill={`url(#${gid})`}
      />
      <polyline
        points={pts.join(" ")}
        fill="none"
        stroke={color}
        strokeWidth={1.6}
        strokeLinejoin="round"
        strokeLinecap="round"
      />
      <circle cx={last[0]} cy={last[1]} r={2.4} fill={color}>
        <animate attributeName="opacity" values="1;0.4;1" dur="2s" repeatCount="indefinite" />
      </circle>
    </svg>
  );
}

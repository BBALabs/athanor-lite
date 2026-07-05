/**
 * Sparkline — a whisper of history behind a live readout. Thin warm line,
 * no fill spectacle, no blinking ornaments; the advancing trace itself is
 * the proof of life.
 */

interface SparklineProps {
  /** Values 0..max, oldest first. */
  data: number[];
  max?: number;
  width?: number;
  height?: number;
  /** Expected sample count — the line grows in from the right until full. */
  capacity?: number;
}

export function Sparkline({ data, max = 100, width = 150, height = 36, capacity = 120 }: SparklineProps) {
  if (data.length < 2) {
    return <svg width={width} height={height} className="sparkline" aria-hidden="true" />;
  }

  const n = Math.max(capacity, data.length);
  const stepX = width / (n - 1);
  const x0 = width - (data.length - 1) * stepX;
  const pts = data.map((v, i) => {
    const x = x0 + i * stepX;
    const y = height - 2 - (Math.max(0, Math.min(max, v)) / max) * (height - 6);
    return `${x.toFixed(2)},${y.toFixed(2)}`;
  });

  return (
    <svg width={width} height={height} className="sparkline" aria-hidden="true">
      <polyline
        points={pts.join(" ")}
        fill="none"
        stroke="rgba(160, 107, 255, 0.35)"
        strokeWidth={1}
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  );
}

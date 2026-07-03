/**
 * SegmentBar — a fuel-gauge style meter built from discrete cells.
 * Reads as capacity, not as a web progress bar.
 */

interface SegmentBarProps {
  /** 0..1 */
  value: number;
  segments?: number;
  /** Optional second value (e.g. projected usage) rendered as outlined cells. */
  ghost?: number;
  height?: number;
}

export function SegmentBar({ value, segments = 28, ghost, height = 14 }: SegmentBarProps) {
  const clamped = Math.max(0, Math.min(1, value));
  const lit = Math.round(clamped * segments);
  const ghostLit = ghost !== undefined ? Math.round(Math.max(0, Math.min(1, ghost)) * segments) : -1;

  const tone = clamped < 0.65 ? "ok" : clamped < 0.85 ? "warn" : "bad";

  return (
    <div
      className={`seg-bar seg-bar--${tone}`}
      style={{ height, gridTemplateColumns: `repeat(${segments}, 1fr)` }}
      role="meter"
      aria-valuenow={Math.round(clamped * 100)}
      aria-valuemin={0}
      aria-valuemax={100}
    >
      {Array.from({ length: segments }, (_, i) => {
        const state = i < lit ? "lit" : i < ghostLit ? "ghost" : "off";
        return <span key={i} className={`seg-bar__cell seg-bar__cell--${state}`} style={{ transitionDelay: `${i * 6}ms` }} />;
      })}
    </div>
  );
}

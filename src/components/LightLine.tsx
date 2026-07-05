/**
 * LightLine — a 2px line of light whose length and warmth encode load,
 * with a faint mirrored reflection glowing onto the dash beneath it.
 * Replaces every bar/meter in the app.
 */

interface LightLineProps {
  /** 0..1 */
  value: number;
  /** Optional pixel width; defaults to filling the container. */
  width?: number;
}

export function LightLine({ value, width }: LightLineProps) {
  const v = Math.max(0, Math.min(1, value));
  // Redline discipline: color speaks only on threshold breach.
  const tone = v >= 0.95 ? "var(--bad)" : v >= 0.85 ? "var(--warn)" : "var(--lume)";
  const tip = v >= 0.95 ? "var(--bad)" : v >= 0.85 ? "var(--warn)" : "var(--lume-warm)";

  return (
    <div
      className="lightline"
      style={width ? { width } : undefined}
      role="meter"
      aria-valuenow={Math.round(v * 100)}
      aria-valuemin={0}
      aria-valuemax={100}
    >
      <div className="lightline__track" />
      <div
        className="lightline__lit"
        style={{
          width: `${(v * 100).toFixed(2)}%`,
          background: `linear-gradient(90deg, var(--lume-deep), ${tone} 70%, ${tip})`,
        }}
      />
      <div
        className="lightline__mirror"
        style={{
          width: `${(v * 100).toFixed(2)}%`,
          background: `linear-gradient(90deg, var(--lume-deep), ${tone} 70%, ${tip})`,
        }}
      />
    </div>
  );
}

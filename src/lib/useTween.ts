/**
 * Value tweening — numbers glide, they never snap.
 * Used for every live readout so 1 Hz telemetry reads as continuous motion.
 */

import { useEffect, useRef, useState } from "react";

/** Luxury glide: fast start, long soft settle. */
function easeOutQuint(t: number): number {
  return 1 - Math.pow(1 - t, 5);
}

export function useTweenedNumber(target: number, durationMs = 900): number {
  const [value, setValue] = useState(target);
  const raf = useRef<number>(0);
  const fromRef = useRef(target);
  const valueRef = useRef(target);

  useEffect(() => {
    if (!Number.isFinite(target)) return;
    const from = valueRef.current;
    fromRef.current = from;
    if (Math.abs(target - from) < 1e-9) return;

    const t0 = performance.now();
    const tick = (now: number) => {
      const t = Math.min(1, (now - t0) / durationMs);
      const v = from + (target - from) * easeOutQuint(t);
      valueRef.current = v;
      setValue(v);
      if (t < 1) raf.current = requestAnimationFrame(tick);
    };
    cancelAnimationFrame(raf.current);
    raf.current = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf.current);
  }, [target, durationMs]);

  return value;
}

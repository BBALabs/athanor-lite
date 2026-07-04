/**
 * Coach — the app teaching itself. Spotlights a real control, says one plain
 * sentence, and gets out of the way. The target stays fully interactive (the
 * dim is four panels *around* it, never over it), so the user learns by doing
 * the actual thing. Esc or "Skip tour" dismisses for good; it never shows twice.
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useStore } from "../state/store";
import { getWalkthrough, type CoachStep } from "../lib/coach";

const CALLOUT_W = 360;
const GAP = 14; // breathing room between spotlight and callout
const PAD = 8; // halo padding around the spotlit element

interface Box {
  top: number;
  left: number;
  width: number;
  height: number;
}

/** Read the live position of the step's target, or null if it isn't on screen. */
function readTarget(step: CoachStep | null): Box | null {
  if (!step?.target) return null;
  const el = document.querySelector(`[data-coach="${step.target}"]`);
  if (!el) return null;
  const r = el.getBoundingClientRect();
  if (r.width < 1 || r.height < 1) return null;
  return { top: r.top, left: r.left, width: r.width, height: r.height };
}

export function Coach() {
  const activeCoach = useStore((s) => s.activeCoach);
  const advanceCoach = useStore((s) => s.advanceCoach);
  const endCoach = useStore((s) => s.endCoach);

  const wt = activeCoach ? getWalkthrough(activeCoach.id) : null;
  const step = wt && activeCoach ? wt.steps[activeCoach.step] ?? null : null;
  const stepIndex = activeCoach?.step ?? 0;
  const total = wt?.steps.length ?? 0;
  const isLast = stepIndex + 1 >= total;

  const [box, setBox] = useState<Box | null>(null);
  const [calloutH, setCalloutH] = useState(150);
  const calloutRef = useRef<HTMLDivElement>(null);
  const boxKey = useRef("");

  // Re-read the target's rect, updating state only when it actually moves.
  const measure = useCallback(() => {
    const next = readTarget(step);
    const key = next ? `${next.top}|${next.left}|${next.width}|${next.height}` : "null";
    if (key !== boxKey.current) {
      boxKey.current = key;
      setBox(next);
    }
  }, [step]);

  // Track the spotlit element. Measure synchronously on step change (so the
  // spotlight lands on the first paint), then keep it aligned through the view's
  // rise animation, scrolling, and resizes. A steady interval — not rAF —
  // because it must keep working even when the window isn't the focused tab.
  useLayoutEffect(() => {
    if (!step) return;
    boxKey.current = ""; // force the first measure to commit
    measure();
    const id = window.setInterval(measure, 200);
    window.addEventListener("scroll", measure, true);
    window.addEventListener("resize", measure);
    return () => {
      window.clearInterval(id);
      window.removeEventListener("scroll", measure, true);
      window.removeEventListener("resize", measure);
    };
  }, [step, measure]);

  // Measure the callout so we can place it without overflowing the viewport.
  useLayoutEffect(() => {
    if (calloutRef.current) setCalloutH(calloutRef.current.offsetHeight);
  }, [step, box]);

  // Keyboard: Enter / → advance, Esc dismisses the whole tour.
  useEffect(() => {
    if (!activeCoach) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        endCoach({ seen: true });
      } else if (e.key === "Enter" || e.key === "ArrowRight") {
        e.preventDefault();
        advanceCoach();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [activeCoach, advanceCoach, endCoach]);

  // Teach-by-doing: if this step invites the action, advance the moment the
  // user actually clicks the spotlit control.
  useEffect(() => {
    if (!step?.advanceOnClick || !step.target) return;
    const onClick = (e: MouseEvent) => {
      const el = document.querySelector(`[data-coach="${step.target}"]`);
      if (el && e.target instanceof Node && el.contains(e.target)) {
        advanceCoach();
      }
    };
    document.addEventListener("click", onClick, true);
    return () => document.removeEventListener("click", onClick, true);
  }, [step, advanceCoach]);

  if (!activeCoach || !wt || !step) return null;

  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const centered = !box || step.placement === "center";

  // Position the callout relative to the spotlight (or dead-center when none).
  let cTop: number;
  let cLeft: number;
  if (centered) {
    cLeft = vw / 2 - CALLOUT_W / 2;
    cTop = vh / 2 - calloutH / 2;
  } else {
    const b = box!;
    const place = step.placement ?? "bottom";
    if (place === "top") {
      cTop = b.top - GAP - calloutH;
      cLeft = b.left + b.width / 2 - CALLOUT_W / 2;
    } else if (place === "left") {
      cLeft = b.left - GAP - CALLOUT_W;
      cTop = b.top + b.height / 2 - calloutH / 2;
    } else if (place === "right") {
      cLeft = b.left + b.width + GAP;
      cTop = b.top + b.height / 2 - calloutH / 2;
    } else {
      cTop = b.top + b.height + GAP;
      cLeft = b.left + b.width / 2 - CALLOUT_W / 2;
    }
  }
  cLeft = Math.max(16, Math.min(cLeft, vw - CALLOUT_W - 16));
  cTop = Math.max(16, Math.min(cTop, vh - calloutH - 16));

  // The spotlight halo rect (padded a touch beyond the element).
  const halo = box
    ? {
        top: box.top - PAD,
        left: box.left - PAD,
        width: box.width + PAD * 2,
        height: box.height + PAD * 2,
      }
    : null;

  return (
    <div className="coach" role="dialog" aria-modal="true" aria-label="Guided walkthrough">
      {/* Dim as four panels AROUND the target so it stays clickable. When
          centered, a single full-screen scrim. */}
      {halo ? (
        <>
          <div className="coach__scrim" style={{ top: 0, left: 0, right: 0, height: Math.max(0, halo.top) }} />
          <div
            className="coach__scrim"
            style={{ top: halo.top, left: 0, width: Math.max(0, halo.left), height: halo.height }}
          />
          <div
            className="coach__scrim"
            style={{ top: halo.top, left: halo.left + halo.width, right: 0, height: halo.height }}
          />
          <div
            className="coach__scrim"
            style={{ top: halo.top + halo.height, left: 0, right: 0, bottom: 0 }}
          />
          <div
            className="coach__halo"
            style={{ top: halo.top, left: halo.left, width: halo.width, height: halo.height }}
          />
        </>
      ) : (
        <div className="coach__scrim coach__scrim--full" style={{ top: 0, left: 0, right: 0, bottom: 0 }} />
      )}

      <div
        ref={calloutRef}
        className="coach__callout"
        style={{ top: cTop, left: cLeft, width: CALLOUT_W }}
      >
        <div className="coach__title t-display">{step.title}</div>
        <p className="coach__body t-quiet">{step.body}</p>
        <div className="coach__foot">
          <div className="coach__dots" aria-hidden="true">
            {wt.steps.map((_, i) => (
              <span key={i} className={`coach__dot${i === stepIndex ? " coach__dot--on" : ""}`} />
            ))}
          </div>
          <div className="coach__actions">
            <button className="btn-quiet" onClick={() => endCoach({ seen: true })}>
              {isLast ? "Dismiss" : "Skip tour"}
            </button>
            <button className="btn-lit" onClick={advanceCoach}>
              {isLast ? "Got it" : "Next"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

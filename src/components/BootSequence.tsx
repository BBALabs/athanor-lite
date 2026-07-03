/**
 * BootSequence — launch overlay. The ticker lines are the *actual* boot
 * steps from the store; the overlay holds for a minimum beat so the
 * reveal reads as intentional, then lifts.
 */

import { useEffect, useState } from "react";
import { useStore } from "../state/store";
import { MarkIcon } from "./Icons";

const MIN_SHOW_MS = 1400;

export function BootSequence() {
  const boot = useStore((s) => s.boot);
  const steps = useStore((s) => s.bootSteps);
  const bootError = useStore((s) => s.bootError);
  const init = useStore((s) => s.init);

  const [minElapsed, setMinElapsed] = useState(false);
  const [lifted, setLifted] = useState(false);

  useEffect(() => {
    void init();
    const t = setTimeout(() => setMinElapsed(true), MIN_SHOW_MS);
    return () => clearTimeout(t);
  }, [init]);

  const done = boot === "ready" && minElapsed;

  useEffect(() => {
    if (!done) return;
    const t = setTimeout(() => setLifted(true), 620);
    return () => clearTimeout(t);
  }, [done]);

  if (lifted) return null;

  return (
    <div className={`boot${done ? " boot--lift" : ""}`}>
      <div className="boot__center">
        <div className="boot__mark">
          <MarkIcon size={40} />
        </div>
        <div className="boot__wordmark">CONDERE</div>
        <div className="boot__tag">local ai, assembled</div>

        <ol className="boot__steps">
          {steps.map((s) => (
            <li key={s.label} className={`boot__step boot__step--${s.state}`}>
              <span className="boot__step-marker">
                {s.state === "done" ? "▪" : s.state === "failed" ? "✕" : s.state === "running" ? "▸" : "·"}
              </span>
              {s.label}
            </li>
          ))}
        </ol>

        {boot === "error" && (
          <div className="boot__error">
            <div className="boot__error-title">Startup fault</div>
            <div className="boot__error-msg k-num">{bootError}</div>
            <button className="boot__retry" onClick={() => window.location.reload()}>
              Retry boot
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

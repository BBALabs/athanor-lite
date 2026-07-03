/**
 * BootSequence — ignition. The spine light sweeps down, the wordmark's
 * tracking settles, one quiet progress line fills as the real boot steps
 * complete. Holds a minimum beat, then the cabin lights come up.
 */

import { useEffect, useMemo, useState } from "react";
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
    const t = setTimeout(() => setLifted(true), 700);
    return () => clearTimeout(t);
  }, [done]);

  const progress = useMemo(() => {
    const total = steps.length;
    const complete = steps.filter((s) => s.state === "done").length;
    return total ? complete / total : 0;
  }, [steps]);

  const current = steps.find((s) => s.state === "running") ?? steps.find((s) => s.state === "failed");

  if (lifted) return null;

  return (
    <div className={`boot${done ? " boot--lift" : ""}`}>
      <div className="boot__sweep" />
      <div className="boot__center">
        <MarkIcon size={34} className="boot__mark" />
        <div className="boot__wordmark">CONDERE</div>

        <div className="boot__progress">
          <div className="boot__progress-track" />
          <div className="boot__progress-lit" style={{ width: `${(progress * 100).toFixed(1)}%` }} />
        </div>
        <div className={`boot__step t-quiet${boot === "error" ? " boot__step--failed" : ""}`}>
          {boot === "error" ? "startup fault" : current ? current.label : "ready"}
        </div>

        {boot === "error" && (
          <div className="boot__error">
            <div className="boot__error-msg t-mono">{bootError}</div>
            <button className="boot__retry" onClick={() => window.location.reload()}>
              Retry
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

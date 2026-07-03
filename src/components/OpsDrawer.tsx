/**
 * The Operations drawer — Activity Monitor for the machine's AI work.
 * Everything running is here; everything here can be stopped with one click.
 * Failures stay visible with the reason and a retry. No ambiguity, ever.
 */

import { useStore } from "../state/store";
import { CloseIcon } from "./Icons";
import { bytesHuman } from "../lib/format";
import type { Operation } from "../lib/types";

const KIND_LABEL: Record<Operation["kind"], string> = {
  download: "download",
  engineFetch: "engine fetch",
  engine: "engine",
  generation: "generation",
  import: "import",
};

function elapsed(startedAt: string): string {
  const s = Math.max(0, (Date.now() - new Date(startedAt).getTime()) / 1000);
  if (s < 60) return `${Math.floor(s)}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${Math.floor(s % 60)}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}

function OpRow({ op }: { op: Operation }) {
  const cancelOperation = useStore((s) => s.cancelOperation);
  const dismissOperation = useStore((s) => s.dismissOperation);
  const retryOperation = useStore((s) => s.retryOperation);

  const pct =
    op.progressCurrent !== null && op.progressTotal
      ? (op.progressCurrent / op.progressTotal) * 100
      : null;

  return (
    <div className={`op op--${op.state}`}>
      <div className="op__head">
        <span className="op__kind t-label">{KIND_LABEL[op.kind]}</span>
        <span className="op__elapsed t-quiet tnum">
          {op.state === "running" ? elapsed(op.startedAt) : op.state}
        </span>
      </div>
      <div className="op__label">{op.label}</div>

      {pct !== null && op.state === "running" && (
        <>
          <div className="lightline">
            <div className="lightline__track" />
            <div
              className="lightline__lit"
              style={{
                width: `${pct.toFixed(1)}%`,
                background:
                  "linear-gradient(90deg, var(--lume-deep), var(--lume) 70%, var(--lume-warm))",
              }}
            />
          </div>
          <div className="op__meta t-quiet tnum">
            {bytesHuman(op.progressCurrent ?? 0)} of {bytesHuman(op.progressTotal ?? 0)} ·{" "}
            {pct.toFixed(0)}%
          </div>
        </>
      )}

      {op.detail && op.state === "running" && pct === null && (
        <div className="op__meta t-quiet">{op.detail}</div>
      )}
      {op.resourceNote && <div className="op__meta t-quiet tnum">{op.resourceNote}</div>}
      {op.error && <div className="op__error t-quiet">{op.error}</div>}

      <div className="op__actions">
        {op.state === "running" && op.cancellable && (
          <button className="btn-quiet op__btn" onClick={() => void cancelOperation(op.id)}>
            Stop
          </button>
        )}
        {op.state !== "running" && op.retry && (
          <button className="btn-quiet op__btn" onClick={() => void retryOperation(op.id)}>
            Retry
          </button>
        )}
        {op.state !== "running" && (
          <button className="btn-quiet op__btn" onClick={() => void dismissOperation(op.id)}>
            Dismiss
          </button>
        )}
      </div>
    </div>
  );
}

export function OpsDrawer() {
  const operations = useStore((s) => s.operations);
  const opsOpen = useStore((s) => s.opsOpen);
  const setOpsOpen = useStore((s) => s.setOpsOpen);

  if (!opsOpen) return null;

  return (
    <aside className="ops-drawer">
      <div className="ops-drawer__head">
        <span className="t-title">Operations</span>
        <button
          className="toast__close"
          onClick={() => setOpsOpen(false)}
          aria-label="Close operations"
        >
          <CloseIcon size={11} />
        </button>
      </div>
      {operations.length === 0 ? (
        <div className="ops-drawer__empty t-quiet">
          Nothing running. Every download, engine, and generation appears here —
          and can be stopped here.
        </div>
      ) : (
        <div className="ops-drawer__list">
          {operations.map((op) => (
            <OpRow key={op.id} op={op} />
          ))}
        </div>
      )}
    </aside>
  );
}

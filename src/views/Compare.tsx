/**
 * Compare — run one prompt through two models and read their answers side by
 * side. A/B testing for model selection. Sequential under the hood (one model
 * resident at a time, so it works on any hardware); each side reports its real
 * measured tokens/second.
 */

import { useEffect, useMemo, useState, type KeyboardEvent } from "react";
import { useStore } from "../state/store";
import { Markdown } from "../components/Markdown";
import type { GenStats, LibraryModel } from "../lib/types";

function Stats({ stats }: { stats: GenStats | null }) {
  if (!stats) return null;
  return (
    <div className="cmp__stats t-quiet tnum">
      {(stats.ttftMs / 1000).toFixed(2)}s to first token · {stats.predictedPerSecond.toFixed(0)} tok/s
      {stats.gpuActive ? "" : " · CPU"}
    </div>
  );
}

function Pane({
  side,
  label,
  running,
  text,
  stats,
  error,
  fastest,
}: {
  side: "a" | "b";
  label: string;
  running: boolean;
  text: string;
  stats: GenStats | null;
  error: string | null;
  fastest: boolean;
}) {
  const empty = !text && !error && !running;
  return (
    <div className={`cmp-pane${fastest ? " cmp-pane--fastest" : ""}`}>
      <div className="cmp-pane__head">
        <span className="t-label">{side === "a" ? "A" : "B"}</span>
        <span className="t-title cmp-pane__name">{label || "—"}</span>
        {fastest && stats && <span className="cmp-pane__win">fastest</span>}
      </div>
      <div className="cmp-pane__body">
        {error ? (
          <p className="cmp__err t-quiet">{error}</p>
        ) : empty ? (
          <p className="t-quiet cmp__idle">Its answer will stream here.</p>
        ) : (
          <>
            <Markdown text={text} />
            {running && !stats && <span className="caret" aria-hidden="true" />}
          </>
        )}
      </div>
      <Stats stats={stats} />
    </div>
  );
}

export function Compare() {
  const { workspaces, activeId } = useStore((s) => s.workspaces);
  const library = useStore((s) => s.library);
  const running = useStore((s) => s.compareRunning);
  const a = useStore((s) => s.compareA);
  const b = useStore((s) => s.compareB);
  const runCompare = useStore((s) => s.runCompare);
  const cancelCompare = useStore((s) => s.cancelCompare);
  const setView = useStore((s) => s.setView);
  const maybeStartCoach = useStore((s) => s.maybeStartCoach);

  const ws = workspaces.find((w) => w.id === activeId) ?? null;
  const models = useMemo(() => library.filter((m) => m.source !== "ollama" || m.entryId), [library]);

  const [modelA, setModelA] = useState("");
  const [modelB, setModelB] = useState("");
  const [prompt, setPrompt] = useState("");

  // Sensible defaults: first two distinct installed models.
  useEffect(() => {
    if (!modelA && models[0]) setModelA(models[0].sha256);
    if (!modelB && models[1]) setModelB(models[1].sha256);
    if (models.length > 0) maybeStartCoach("compare");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [models.length]);

  const nameOf = (sha: string) => models.find((m) => m.sha256 === sha)?.displayName ?? "model";

  const submit = () => {
    if (!prompt.trim() || running || modelA === modelB) return;
    void runCompare(prompt, modelA, nameOf(modelA), modelB, nameOf(modelB));
  };

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      submit();
    }
  };

  const fasterSide =
    a.stats && b.stats
      ? a.stats.predictedPerSecond >= b.stats.predictedPerSecond
        ? "a"
        : "b"
      : null;

  if (!ws) {
    return (
      <div className="compare view">
        <div className="degraded">
          <div className="t-title">No workspace selected</div>
          <button className="btn-lit" onClick={() => setView("workspaces")}>
            Choose a workspace
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="compare view">
      <header className="view-head">
        <div>
          <h1 className="t-display">Compare</h1>
          <span className="view-head__sub t-quiet">
            one prompt, two models, side by side — pick the right one for the job
          </span>
        </div>
      </header>

      {models.length < 2 ? (
        <div className="degraded">
          <p className="t-quiet degraded__note">
            Comparing needs at least two installed models. Get another in Models.
          </p>
          <button className="btn-lit" onClick={() => setView("models")}>
            Open Models
          </button>
        </div>
      ) : (
        <>
          <div className="cmp-setup" data-coach="compare">
            <div className="cmp-pickers">
              <label className="ds-field cmp-picker">
                <span className="t-label">Model A</span>
                <select className="ds-select" value={modelA} onChange={(e) => setModelA(e.target.value)}>
                  {models.map((m: LibraryModel) => (
                    <option key={m.sha256} value={m.sha256}>
                      {m.displayName}
                      {m.quant ? ` · ${m.quant}` : ""}
                    </option>
                  ))}
                </select>
              </label>
              <span className="cmp-vs t-quiet">vs</span>
              <label className="ds-field cmp-picker">
                <span className="t-label">Model B</span>
                <select className="ds-select" value={modelB} onChange={(e) => setModelB(e.target.value)}>
                  {models.map((m: LibraryModel) => (
                    <option key={m.sha256} value={m.sha256}>
                      {m.displayName}
                      {m.quant ? ` · ${m.quant}` : ""}
                    </option>
                  ))}
                </select>
              </label>
            </div>
            {modelA === modelB && <p className="t-quiet cmp-warn">Pick two different models.</p>}
            <div className="cmp-prompt">
              <textarea
                value={prompt}
                onChange={(e) => setPrompt(e.target.value)}
                onKeyDown={onKey}
                placeholder="Ask both models the same thing… (⌘/Ctrl+Enter to run)"
                rows={Math.min(6, Math.max(2, prompt.split("\n").length))}
                disabled={running}
              />
              {running ? (
                <button className="btn-quiet" onClick={() => void cancelCompare()}>
                  Stop
                </button>
              ) : (
                <button className="btn-lit" onClick={submit} disabled={!prompt.trim() || modelA === modelB}>
                  Compare
                </button>
              )}
            </div>
          </div>

          <div className="cmp-panes">
            <Pane
              side="a"
              label={a.name || nameOf(modelA)}
              running={running}
              text={a.text}
              stats={a.stats}
              error={a.error}
              fastest={fasterSide === "a"}
            />
            <Pane
              side="b"
              label={b.name || nameOf(modelB)}
              running={running}
              text={b.text}
              stats={b.stats}
              error={b.error}
              fastest={fasterSide === "b"}
            />
          </div>
        </>
      )}
    </div>
  );
}

/**
 * Speed benchmark — measure real tokens/second for each installed model on this
 * exact machine, and rank them. Every number is measured, never synthetic.
 */

import { useEffect, useState } from "react";
import { useStore } from "../state/store";
import { ipc } from "../lib/ipc";
import type { BenchResult } from "../lib/types";

export function BenchmarkPanel() {
  const library = useStore((s) => s.library);
  const operations = useStore((s) => s.operations);
  const [results, setResults] = useState<BenchResult[]>([]);
  const [model, setModel] = useState<string>("");

  const running = operations.some((o) => o.kind === "benchmark" && o.state === "running");

  useEffect(() => {
    void ipc.listBenchmarks().then(setResults).catch(() => {});
  }, []);

  // When a benchmark op clears, refresh the board.
  useEffect(() => {
    if (!running) void ipc.listBenchmarks().then(setResults).catch(() => {});
  }, [running]);

  if (library.length === 0) return null;

  const run = () => {
    const m = library.find((x) => x.sha256 === model) ?? library[0];
    if (!m) return;
    void ipc
      .runBenchmark(m.sha256, m.displayName)
      .then((r) =>
        setResults((prev) =>
          [r, ...prev.filter((p) => p.modelSha !== r.modelSha)].sort((a, b) => b.genTps - a.genTps),
        ),
      )
      .catch(() => {});
  };

  const best = results[0]?.genTps ?? 0;

  return (
    <section className="bench" data-coach="benchmark">
      <div className="bench__head">
        <span className="t-label">Speed benchmark</span>
        <div className="bench__run">
          <select
            className="ds-select bench__select"
            value={model}
            onChange={(e) => setModel(e.target.value)}
          >
            {library.map((m) => (
              <option key={m.sha256} value={m.sha256}>
                {m.displayName}
                {m.quant ? ` · ${m.quant}` : ""}
              </option>
            ))}
          </select>
          <button className="btn-lit" onClick={run} disabled={running}>
            {running ? "Benchmarking…" : "Run"}
          </button>
        </div>
      </div>
      {results.length > 0 ? (
        <div className="bench__board">
          {results.map((r) => (
            <div key={r.modelSha} className="bench__row">
              <span className="bench__model t-title">{r.modelName}</span>
              <span className="bench__bar" aria-hidden="true">
                <span
                  className="bench__bar-fill"
                  style={{ width: `${best ? Math.round((r.genTps / best) * 100) : 0}%` }}
                />
              </span>
              <span className="bench__tps t-display tnum">
                {r.genTps.toFixed(0)}
                <span className="bench__unit t-quiet"> tok/s</span>
              </span>
              <span className="bench__meta t-quiet tnum">
                {(r.ttftMs / 1000).toFixed(2)}s to first token · {r.promptTps.toFixed(0)} prompt tok/s
                {r.gpuActive ? "" : " · CPU"}
              </span>
            </div>
          ))}
        </div>
      ) : (
        <p className="t-quiet bench__empty">
          Run the suite to measure real tokens/second for a model on your hardware.
        </p>
      )}
    </section>
  );
}

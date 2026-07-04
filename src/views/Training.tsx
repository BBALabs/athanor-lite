/**
 * Tune — the dataset studio. Getting your data into a clean, validated training
 * set is where most people get stuck, so that is what this makes real: drop a
 * JSONL, see exactly what's valid, name it, save it. The training *run* needs a
 * LoRA runtime we don't bundle yet — so we say so plainly (no fake progress),
 * and your prepared datasets wait, ready, for when it lands.
 */

import { useEffect, useState, type DragEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { useStore } from "../state/store";
import { ipc } from "../lib/ipc";
import { IN_TAURI } from "../lib/tauriEnv";
import { relativeTime } from "../lib/format";
import { PlusIcon, TrashIcon } from "../components/Icons";
import type { DatasetMeta, DatasetReport, LibraryModel, TrainerStatus } from "../lib/types";

const FORMAT_LABEL: Record<string, string> = {
  chat: "chat turns",
  instruction: "instruction / output",
  completion: "prompt / completion",
  unknown: "unrecognized",
};

/** The validation report card shown right after an import. */
function ReportCard({ report, name }: { report: DatasetReport; name: string }) {
  return (
    <div className="ds-report">
      <div className="ds-report__head">
        <span className="t-title">{name}</span>
        <span className="t-quiet">{FORMAT_LABEL[report.format] ?? report.format}</span>
      </div>
      <div className="ds-report__stats">
        <div className="ds-stat">
          <span className="ds-stat__n t-display tnum">{report.valid.toLocaleString()}</span>
          <span className="t-quiet">valid examples</span>
        </div>
        <div className="ds-stat">
          <span className="ds-stat__n t-display tnum">{(report.estTokens / 1000).toFixed(0)}k</span>
          <span className="t-quiet">est. tokens</span>
        </div>
        {report.invalid > 0 && (
          <div className="ds-stat ds-stat--warn">
            <span className="ds-stat__n t-display tnum">{report.invalid}</span>
            <span className="t-quiet">skipped</span>
          </div>
        )}
        {report.duplicates > 0 && (
          <div className="ds-stat">
            <span className="ds-stat__n t-display tnum">{report.duplicates}</span>
            <span className="t-quiet">duplicates removed</span>
          </div>
        )}
      </div>
      {report.issues.length > 0 && (
        <ul className="ds-report__issues">
          {report.issues.map((iss, i) => (
            <li key={i} className="t-quiet">
              {iss}
            </li>
          ))}
        </ul>
      )}
      {report.preview.length > 0 && (
        <div className="ds-report__preview">
          {report.preview.map((p, i) => (
            <p key={i} className="t-quiet ds-preview">
              {p}
            </p>
          ))}
        </div>
      )}
    </div>
  );
}

export function Training() {
  const { workspaces, activeId } = useStore((s) => s.workspaces);
  const library = useStore((s) => s.library);
  const setView = useStore((s) => s.setView);
  const maybeStartCoach = useStore((s) => s.maybeStartCoach);

  const ws = workspaces.find((w) => w.id === activeId) ?? null;

  const [datasets, setDatasets] = useState<DatasetMeta[]>([]);
  const [trainer, setTrainer] = useState<TrainerStatus | null>(null);
  const [lastReport, setLastReport] = useState<{ report: DatasetReport; name: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);

  // Training config (real controls; the run is gated on the trainer status).
  const [baseModel, setBaseModel] = useState<string | null>(null);
  const [rank, setRank] = useState(16);
  const [epochs, setEpochs] = useState(3);
  const [datasetId, setDatasetId] = useState<string | null>(null);

  const refresh = () => {
    if (!ws) return;
    void ipc.listDatasets(ws.id).then(setDatasets).catch(() => {});
  };

  useEffect(() => {
    refresh();
    void ipc.getTrainerStatus().then(setTrainer).catch(() => {});
    if (ws) maybeStartCoach("training");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeId]);

  const importPath = async (path: string) => {
    if (!ws) return;
    const base = path.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") ?? "dataset";
    setBusy(true);
    setErr(null);
    try {
      const report = await ipc.importDataset(ws.id, base, path);
      setLastReport({ report, name: base });
      refresh();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const pickFile = async () => {
    if (!IN_TAURI) {
      // Harness: exercise the flow without a real file.
      void importPath("X:/dropped/my-data.jsonl");
      return;
    }
    const picked = await open({ multiple: false, filters: [{ name: "Dataset", extensions: ["jsonl", "json"] }] });
    if (typeof picked === "string") void importPath(picked);
  };

  const onDomDrop = (e: DragEvent) => {
    e.preventDefault();
    setDragging(false);
    if (!IN_TAURI) void importPath("X:/dropped/my-data.jsonl");
  };

  const del = async (id: string) => {
    if (!ws) return;
    try {
      setDatasets(await ipc.deleteDataset(ws.id, id));
      if (datasetId === id) setDatasetId(null);
    } catch {
      /* surfaced elsewhere */
    }
  };

  const trainable = library.filter((m) => m.source !== "ollama");

  if (!ws) {
    return (
      <div className="training view">
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
    <div className="training view">
      <header className="view-head">
        <div>
          <h1 className="t-display">Tune</h1>
          <span className="view-head__sub t-quiet">
            {ws.name} · fine-tune a model on your own data
          </span>
        </div>
      </header>

      {/* ── Dataset studio ─────────────────────────── */}
      <section className="ds-studio">
        <div
          className={`kb-drop${dragging ? " kb-drop--over" : ""}`}
          data-coach="ds-drop"
          onDragOver={(e) => {
            e.preventDefault();
            setDragging(true);
          }}
          onDragLeave={() => setDragging(false)}
          onDrop={onDomDrop}
          onClick={() => void pickFile()}
        >
          <PlusIcon size={20} />
          <div className="kb-drop__title">{busy ? "Validating…" : "Drop a training set"}</div>
          <div className="kb-drop__sub t-quiet">
            or click to browse · JSONL — chat, instruction/output, or prompt/completion · validated
            on your machine
          </div>
        </div>

        {err && <p className="ds-error t-quiet">{err}</p>}
        {lastReport && <ReportCard report={lastReport.report} name={lastReport.name} />}

        {datasets.length > 0 && (
          <div className="ds-list">
            {datasets.map((d) => (
              <div
                key={d.id}
                className={`ds-item${datasetId === d.id ? " ds-item--sel" : ""}`}
                onClick={() => setDatasetId(d.id)}
              >
                <span className="t-title ds-item__name">{d.name}</span>
                <span className="t-quiet tnum">
                  {d.examples.toLocaleString()} examples · {(d.estTokens / 1000).toFixed(0)}k tokens ·{" "}
                  {relativeTime(d.createdAt)}
                </span>
                <button
                  className="ds-item__del"
                  onClick={(e) => {
                    e.stopPropagation();
                    void del(d.id);
                  }}
                  aria-label="Delete dataset"
                >
                  <TrashIcon size={12} />
                </button>
              </div>
            ))}
          </div>
        )}
      </section>

      {/* ── Training config ────────────────────────── */}
      <section className="ds-train" data-coach="ds-train">
        <div className="t-title ds-train__title">Training run</div>
        <div className="ds-config">
          <label className="ds-field">
            <span className="t-quiet">Base model</span>
            <select
              className="ds-select"
              value={baseModel ?? ""}
              onChange={(e) => setBaseModel(e.target.value || null)}
            >
              <option value="">
                {trainable.length ? "Choose a model…" : "No local models — get one in Models"}
              </option>
              {trainable.map((m: LibraryModel) => (
                <option key={m.sha256} value={m.sha256}>
                  {m.displayName}
                  {m.quant ? ` · ${m.quant}` : ""}
                </option>
              ))}
            </select>
          </label>
          <label className="ds-field">
            <span className="t-quiet">Dataset</span>
            <select
              className="ds-select"
              value={datasetId ?? ""}
              onChange={(e) => setDatasetId(e.target.value || null)}
            >
              <option value="">{datasets.length ? "Choose a dataset…" : "Add one above"}</option>
              {datasets.map((d) => (
                <option key={d.id} value={d.id}>
                  {d.name} ({d.examples})
                </option>
              ))}
            </select>
          </label>
          <label className="ds-field">
            <span className="t-quiet">LoRA rank · {rank}</span>
            <input
              type="range"
              min={4}
              max={64}
              step={4}
              value={rank}
              onChange={(e) => setRank(+e.target.value)}
            />
          </label>
          <label className="ds-field">
            <span className="t-quiet">Epochs · {epochs}</span>
            <input
              type="range"
              min={1}
              max={10}
              value={epochs}
              onChange={(e) => setEpochs(+e.target.value)}
            />
          </label>
        </div>

        {/* The honest bit: no fake progress bar. */}
        {trainer && !trainer.available && (
          <div className="ds-trainer-status">
            <p className="t-quiet">{trainer.detail}</p>
          </div>
        )}
        <button
          className="btn-lit ds-train__start"
          disabled={!trainer?.available || !baseModel || !datasetId}
          title={trainer?.available ? "Start training" : "Local training runtime not available yet"}
        >
          {trainer?.available ? "Start training" : "Training runtime not available yet"}
        </button>
      </section>
    </div>
  );
}

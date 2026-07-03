/**
 * Settings — a glass sheet with three quiet sections: performance records
 * (local-first, consent with the literal payload on screen), the local
 * OpenAI-compatible API, and library import.
 */

import { useEffect, useState } from "react";
import { ipc } from "../lib/ipc";
import { useStore } from "../state/store";
import type { ApiInfo, ImportReport, MetricsSettings, OllamaStatus } from "../lib/types";

function Toggle({ on, onChange, label }: { on: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button
      className={`switch${on ? " switch--on" : ""}`}
      role="switch"
      aria-checked={on}
      aria-label={label}
      onClick={() => onChange(!on)}
    >
      <span className="switch__dot" />
    </button>
  );
}

function Copyable({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      className="copyable t-mono"
      title="Copy"
      onClick={() => {
        void navigator.clipboard?.writeText(value).then(() => {
          setCopied(true);
          window.setTimeout(() => setCopied(false), 1400);
        });
      }}
    >
      {value}
      <span className="copyable__hint">{copied ? "copied" : "copy"}</span>
    </button>
  );
}

export function SettingsSheet({ onDone }: { onDone: () => void }) {
  const [metrics, setMetrics] = useState<MetricsSettings | null>(null);
  const [sample, setSample] = useState<string>("");
  const [showSample, setShowSample] = useState(false);
  const [recordCount, setRecordCount] = useState(0);
  const [api, setApi] = useState<ApiInfo | null>(null);
  const [ollama, setOllama] = useState<OllamaStatus | null>(null);
  const [importReport, setImportReport] = useState<ImportReport | null>(null);
  const [importing, setImporting] = useState(false);

  useEffect(() => {
    void ipc.getMetricsSettings().then(setMetrics).catch(() => {});
    void ipc
      .getMetricsSample()
      .then((v) => setSample(JSON.stringify(v, null, 2)))
      .catch(() => {});
    void ipc.getMetricsHistory(500).then((h) => setRecordCount(h.length)).catch(() => {});
    void ipc.getApiInfo().then(setApi).catch(() => {});
    void ipc.getOllamaStatus().then(setOllama).catch(() => {});
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onDone();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onDone]);

  return (
    <div className="sheet-veil" onClick={onDone}>
      <div className="sheet sheet--settings" onClick={(e) => e.stopPropagation()}>
        <div className="t-display">Settings</div>

        {/* ── Performance records ─────────────────── */}
        <section className="setting">
          <div className="setting__head">
            <div>
              <div className="t-title">Performance records</div>
              <p className="t-quiet setting__blurb">
                Every generation writes speed and memory ground truth to a local file only
                you can read ({recordCount} record{recordCount === 1 ? "" : "s"} so far).
                Contributing shares the anonymous form below — never prompts, never chats,
                never your name. Off by default.
              </p>
            </div>
            <Toggle
              on={metrics?.share ?? false}
              label="Contribute anonymous performance records"
              onChange={(v) => {
                void ipc.setMetricsShare(v).then(setMetrics).catch(() => {});
              }}
            />
          </div>
          {metrics?.share && (
            <p className="t-quiet setting__note">
              Contribution uploads begin with a future update — until then, opted-in
              records simply queue locally. You can turn this off at any time.
            </p>
          )}
          <button className="btn-quiet setting__reveal" onClick={() => setShowSample((s) => !s)}>
            {showSample ? "Hide" : "Show"} the exact payload
          </button>
          {showSample && <pre className="setting__payload t-mono">{sample}</pre>}
        </section>

        {/* ── Local API ───────────────────────────── */}
        <section className="setting">
          <div className="setting__head">
            <div>
              <div className="t-title">Local API</div>
              <p className="t-quiet setting__blurb">
                OpenAI-compatible endpoint for Continue, Cursor, n8n, or your scripts.
                Localhost only. The engine serves your active workspace's model
                {api?.running && api.modelName ? ` (running: ${api.modelName})` : ""}.
              </p>
            </div>
            <Toggle
              on={api?.expose ?? false}
              label="Expose local API"
              onChange={(v) => {
                void ipc.setApiExpose(v).then(setApi).catch(() => {});
              }}
            />
          </div>
          {api?.expose && (
            <div className="setting__rows">
              <div className="setting__row">
                <span className="t-quiet">Base URL</span>
                <Copyable value={api.baseUrl} />
              </div>
              <div className="setting__row">
                <span className="t-quiet">API key</span>
                <Copyable value={api.apiKey} />
              </div>
            </div>
          )}
        </section>

        {/* ── Import ──────────────────────────────── */}
        <section className="setting">
          <div className="setting__head">
            <div>
              <div className="t-title">Import from Ollama</div>
              <p className="t-quiet setting__blurb">
                {ollama?.available
                  ? `${ollama.modelCount} model${ollama.modelCount === 1 ? "" : "s"} found in your Ollama store. Importing links them in place — nothing is copied or re-downloaded.`
                  : "No Ollama installation found on this machine."}
              </p>
            </div>
            {ollama?.available && (
              <button
                className="btn-lit"
                disabled={importing}
                onClick={() => {
                  setImporting(true);
                  void ipc
                    .importOllama()
                    .then(async (r) => {
                      setImportReport(r);
                      useStore.setState({ library: await ipc.listLibrary() });
                    })
                    .catch(() => {})
                    .finally(() => setImporting(false));
                }}
              >
                {importing ? "Importing…" : "Import"}
              </button>
            )}
          </div>
          {importReport && (
            <p className="t-quiet setting__note tnum">
              {importReport.imported} imported · {importReport.alreadyInLibrary} already in
              the library
              {importReport.skipped.length > 0
                ? ` · ${importReport.skipped.length} skipped (${importReport.skipped.join("; ")})`
                : ""}
            </p>
          )}
        </section>

        <div className="sheet__actions sheet__actions--end">
          <button className="btn-quiet" onClick={onDone}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

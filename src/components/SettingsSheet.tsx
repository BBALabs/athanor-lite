/**
 * Settings — a glass sheet of quiet sections: appearance, performance records
 * (local-first consent, the literal payload on screen), the local
 * OpenAI-compatible API, library import, your data folder, and updates.
 */

import { useEffect, useState } from "react";
import { ipc } from "../lib/ipc";
import { useStore } from "../state/store";
import { ACCENT_PRESETS } from "../lib/theme";
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
  const accent = useStore((s) => s.accent);
  const setAccent = useStore((s) => s.setAccent);
  const replayCoaches = useStore((s) => s.replayCoaches);
  const maybeStartCoach = useStore((s) => s.maybeStartCoach);

  const [metrics, setMetrics] = useState<MetricsSettings | null>(null);
  const [sample, setSample] = useState<string>("");
  const [showSample, setShowSample] = useState(false);
  const [recordCount, setRecordCount] = useState(0);
  const [api, setApi] = useState<ApiInfo | null>(null);
  const [ollama, setOllama] = useState<OllamaStatus | null>(null);
  const [importReport, setImportReport] = useState<ImportReport | null>(null);
  const [importing, setImporting] = useState(false);
  const [updateBusy, setUpdateBusy] = useState(false);
  const [updateNote, setUpdateNote] = useState<string | null>(null);
  const [dataRoot, setDataRoot] = useState("");
  const [replayed, setReplayed] = useState(false);
  const [rotated, setRotated] = useState(false);

  useEffect(() => {
    void ipc.getMetricsSettings().then(setMetrics).catch(() => {});
    void ipc
      .getMetricsSample()
      .then((v) => setSample(JSON.stringify(v, null, 2)))
      .catch(() => {});
    void ipc.getMetricsHistory(500).then((h) => setRecordCount(h.length)).catch(() => {});
    void ipc.getApiInfo().then(setApi).catch(() => {});
    void ipc.getOllamaStatus().then(setOllama).catch(() => {});
    void ipc.getDataRoot().then(setDataRoot).catch(() => {});
    maybeStartCoach("settings");
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onDone();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onDone, maybeStartCoach]);

  return (
    <div className="sheet-veil" onClick={onDone}>
      <div className="sheet sheet--settings" onClick={(e) => e.stopPropagation()}>
        <div className="t-display">Settings</div>

        {/* ── Appearance ──────────────────────────── */}
        <section className="setting" data-coach="settings-appearance">
          <div className="setting__head">
            <div>
              <div className="t-title">Appearance</div>
              <p className="t-quiet setting__blurb">
                The light the interface glows in. The dark glass stays — only the accent
                changes.
              </p>
            </div>
          </div>
          <div className="pref-accents" role="radiogroup" aria-label="Accent">
            {ACCENT_PRESETS.map((p) => (
              <button
                key={p.id}
                role="radio"
                aria-checked={accent === p.id}
                className={`pref-accent${accent === p.id ? " pref-accent--active" : ""}`}
                style={{ ["--swatch" as string]: p.lume }}
                onClick={() => void setAccent(p.id)}
                title={p.label}
              >
                <span className="pref-accent__disc" />
                <span className="t-quiet">{p.label}</span>
              </button>
            ))}
          </div>
          <div className="setting__row setting__row--between">
            <span className="t-quiet">Guided walkthroughs</span>
            <button
              className="btn-quiet"
              onClick={() => {
                void replayCoaches();
                setReplayed(true);
              }}
            >
              {replayed ? "Tutorials reset" : "Replay tutorials"}
            </button>
          </div>
        </section>

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
              <div className="setting__row setting__row--between">
                <span className="t-quiet">
                  {rotated ? "New key issued — update your clients" : "Compromised? Issue a fresh key."}
                </span>
                <button
                  className="btn-quiet"
                  onClick={() => {
                    void ipc
                      .rotateApiKey()
                      .then((info) => {
                        setApi(info);
                        setRotated(true);
                      })
                      .catch(() => {});
                  }}
                >
                  Regenerate key
                </button>
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

        {/* ── Data ────────────────────────────────── */}
        <section className="setting">
          <div className="setting__head">
            <div>
              <div className="t-title">Your data</div>
              <p className="t-quiet setting__blurb">
                Models, workspaces, chats, and settings all live in one folder — nothing is
                written anywhere else. Back it up or move it with a file manager.
              </p>
            </div>
            <button className="btn-quiet" onClick={() => void ipc.revealDataRoot()}>
              Open folder
            </button>
          </div>
          {dataRoot && (
            <div className="setting__row">
              <span className="t-quiet">Location</span>
              <span className="t-mono setting__path">{dataRoot}</span>
            </div>
          )}
        </section>

        {/* ── Updates ─────────────────────────────── */}
        <section className="setting">
          <div className="setting__head">
            <div>
              <div className="t-title">Updates</div>
              <p className="t-quiet setting__blurb">
                Athanor 0.1.0. Updates are cryptographically signed and never install
                without asking.
              </p>
            </div>
            <button
              className="btn-quiet"
              disabled={updateBusy}
              onClick={() => {
                setUpdateBusy(true);
                void ipc
                  .checkForUpdate()
                  .then((r) =>
                    setUpdateNote(r.available ? `${r.available} available` : r.note),
                  )
                  .catch(() => setUpdateNote("check failed"))
                  .finally(() => setUpdateBusy(false));
              }}
            >
              {updateBusy ? "Checking…" : "Check for updates"}
            </button>
          </div>
          {updateNote && <p className="t-quiet setting__note">{updateNote}</p>}
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

/**
 * Models — the showroom. A vertical ledger of light rows; the machine's
 * recommended model leads as the hero row. Fit verdicts are jewels, not
 * chips. Rows expand in place to the full quant table.
 */

import { useEffect, useMemo, useState, type MouseEvent } from "react";
import { useStore } from "../state/store";
import { ipc } from "../lib/ipc";
import { bytesHuman, ctxHuman } from "../lib/format";
import { LITE } from "../lib/edition";
import { CloseIcon, TrashIcon } from "../components/Icons";
import type {
  CatalogEntry,
  DownloadProgress,
  FitMode,
  OllamaStatus,
  QuantFit,
  QuantOption,
  Role,
} from "../lib/types";

type Filter = "all" | Role;

const FILTERS: { id: Filter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "general", label: "General" },
  { id: "coding", label: "Coding" },
  { id: "reasoning", label: "Reasoning" },
  { id: "embedding", label: "Embedding" },
];

type SortMode = "quality" | "size" | "name";

const SORTS: { id: SortMode; label: string }[] = [
  { id: "quality", label: "Capability" },
  { id: "size", label: "Size" },
  { id: "name", label: "Name" },
];

/** Fit verdicts come from the backend (one source of truth) via a lookup. */
type FitLookup = (entryId: string, quant: string) => QuantFit | undefined;

/** Jewel class per fit mode — the visual grammar of "how well it runs". */
const JEWEL_CLASS: Record<FitMode, string> = {
  gpuFull: "fits",
  gpuTight: "tight",
  partialOffload: "partial",
  cpu: "cpu",
  exceeds: "no",
};

const VERDICT_WORD: Record<FitMode, string> = {
  gpuFull: "fits on GPU",
  gpuTight: "tight",
  partialOffload: "GPU + CPU split",
  cpu: "CPU only",
  exceeds: "exceeds this machine",
};

function fitTitle(fit: QuantFit | undefined): string {
  if (!fit) return "fit unknown";
  const base = `${VERDICT_WORD[fit.fitMode]} · ~${fit.estMemGb.toFixed(1)} GB loaded`;
  if (fit.fitMode === "gpuFull" || fit.fitMode === "gpuTight") {
    return `${base} · up to ${ctxHuman(fit.maxCtx)} context`;
  }
  if (fit.fitMode === "partialOffload" && fit.gpuOffloadPct != null) {
    return `${base} · ~${fit.gpuOffloadPct}% of layers on GPU`;
  }
  return base;
}

function runnable(mode: FitMode): boolean {
  return mode !== "exceeds";
}

function FitJewels({ entry, fitOf }: { entry: CatalogEntry; fitOf: FitLookup }) {
  return (
    <div className="jewels">
      {entry.quants.map((q) => {
        const fit = fitOf(entry.id, q.label);
        const cls = fit ? JEWEL_CLASS[fit.fitMode] : "no";
        return <span key={q.label} className={`jewel jewel--${cls}`} title={fitTitle(fit)} />;
      })}
    </div>
  );
}

/** A two-step delete for an installed model file — reclaims the disk. */
function DeleteQuant({ sha }: { sha: string }) {
  const deleteModel = useStore((s) => s.deleteModel);
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    if (!armed) return;
    const t = setTimeout(() => setArmed(false), 3500);
    return () => clearTimeout(t);
  }, [armed]);
  return (
    <button
      className={`quant-action__delete${armed ? " quant-action__delete--armed" : ""}`}
      onClick={(e) => {
        e.stopPropagation();
        if (armed) void deleteModel(sha);
        else setArmed(true);
      }}
      aria-label={armed ? "Confirm delete from disk" : "Delete from disk"}
      title={armed ? "Click again to delete" : "Delete from disk"}
    >
      {armed ? "delete?" : <TrashIcon size={12} />}
    </button>
  );
}

/** The action cell for one quant: get / progress / installed. */
function QuantAction({ entry, quant, mode }: { entry: CatalogEntry; quant: QuantOption; mode: FitMode }) {
  const library = useStore((s) => s.library);
  const downloads = useStore((s) => s.downloads);
  const startDownload = useStore((s) => s.startDownload);
  const cancelDownload = useStore((s) => s.cancelDownload);

  const sha = quant.files[0]?.sha256;
  if (!sha) return <span className="t-quiet">—</span>;

  const libModel = library.find((m) => m.sha256 === sha);
  const installed = !!libModel;
  const dl: DownloadProgress | undefined = downloads[sha];
  const active = dl && (dl.state === "starting" || dl.state === "downloading" || dl.state === "verifying");

  const stop = (fn: () => void) => (e: MouseEvent) => {
    e.stopPropagation();
    fn();
  };

  if (installed) {
    return (
      <span className="quant-action quant-action--installed">
        <span className="t-quiet">on disk · {bytesHuman(libModel!.sizeBytes)}</span>
        <DeleteQuant sha={sha} />
      </span>
    );
  }
  if (active) {
    const pct = dl.totalBytes ? (dl.receivedBytes / dl.totalBytes) * 100 : 0;
    return (
      <span className="quant-action quant-action--live tnum">
        {dl.state === "verifying" ? (
          "verifying…"
        ) : (
          <>
            {pct.toFixed(0)}% · {bytesHuman(dl.bytesPerSec)}/s
            <button
              className="quant-action__cancel"
              onClick={stop(() => void cancelDownload(sha))}
              aria-label="Cancel download"
              title="Cancel"
            >
              <CloseIcon size={10} />
            </button>
          </>
        )}
      </span>
    );
  }
  if (mode === "exceeds") {
    return <span className="t-quiet quant-action">—</span>;
  }
  return (
    <button
      className="btn-quiet quant-action__get"
      onClick={stop(() => void startDownload(entry.id, quant.label))}
    >
      Get · {quant.fileGb.toFixed(1)} GB
    </button>
  );
}

function QuantTable({ entry, fitOf }: { entry: CatalogEntry; fitOf: FitLookup }) {
  return (
    <div className="quant-table">
      {entry.quants.map((q) => {
        const fit = fitOf(entry.id, q.label);
        const mode: FitMode = fit?.fitMode ?? "exceeds";
        const cls = JEWEL_CLASS[mode];
        const loaded = fit ? `~${fit.estMemGb.toFixed(1)} GB loaded` : "—";
        return (
          <div className="quant-table__row" key={q.label}>
            <span className={`jewel jewel--${cls}`} />
            <span className="t-mono">{q.label}</span>
            <span className="t-quiet tnum">{q.fileGb.toFixed(1)} GB file</span>
            <span className="t-quiet tnum">{loaded}</span>
            <span className={`quant-table__verdict t-quiet quant-table__verdict--${cls}`}>
              {VERDICT_WORD[mode]}
              {mode === "partialOffload" && fit?.gpuOffloadPct != null
                ? ` · ${fit.gpuOffloadPct}% on GPU`
                : ""}
              {(mode === "gpuFull" || mode === "gpuTight") && fit
                ? ` · to ${ctxHuman(fit.maxCtx)} ctx`
                : ""}
            </span>
            <QuantAction entry={entry} quant={q} mode={mode} />
          </div>
        );
      })}
      <div className="quant-table__meta t-quiet">
        {entry.license} · {ctxHuman(entry.contextLength)} context ·{" "}
        <span className="t-mono">{entry.hfRepo}</span>
      </div>
    </div>
  );
}

/** Hero-row primary affordance: get the recommended quant / live progress. */
function HeroAction({ entry, quantLabel }: { entry: CatalogEntry; quantLabel: string }) {
  const library = useStore((s) => s.library);
  const downloads = useStore((s) => s.downloads);
  const startDownload = useStore((s) => s.startDownload);
  const cancelDownload = useStore((s) => s.cancelDownload);
  const liteLaunch = useStore((s) => s.liteLaunch);

  const quant = entry.quants.find((q) => q.label === quantLabel) ?? entry.quants[0];
  const sha = quant?.files[0]?.sha256;
  if (!quant || !sha) return null;

  const installed = library.some((m) => m.sha256 === sha);
  const dl = downloads[sha];
  const active = dl && (dl.state === "starting" || dl.state === "downloading" || dl.state === "verifying");

  if (installed) {
    // Lite has no workspaces — an installed model chats in one press.
    if (LITE) {
      return (
        <button
          className="btn-lit hero-action__get"
          onClick={(e) => {
            e.stopPropagation();
            void liteLaunch(sha);
          }}
        >
          Start chatting
        </button>
      );
    }
    return <span className="hero-action__installed t-quiet">installed · ready for a workspace</span>;
  }
  if (active) {
    const pct = dl.totalBytes ? (dl.receivedBytes / dl.totalBytes) * 100 : 0;
    return (
      <div className="hero-action__progress" onClick={(e) => e.stopPropagation()}>
        <div className="lightline">
          <div className="lightline__track" />
          <div
            className="lightline__lit"
            style={{
              width: `${pct.toFixed(2)}%`,
              background: "linear-gradient(90deg, var(--lume-deep), var(--lume) 70%, var(--lume-warm))",
            }}
          />
        </div>
        <span className="t-quiet tnum">
          {dl.state === "verifying"
            ? "verifying checksum…"
            : `${bytesHuman(dl.receivedBytes)} of ${bytesHuman(dl.totalBytes)} · ${bytesHuman(dl.bytesPerSec)}/s`}
        </span>
        <button className="btn-quiet" onClick={() => void cancelDownload(sha)}>
          Cancel
        </button>
      </div>
    );
  }
  return (
    <button
      className="btn-lit hero-action__get"
      onClick={(e) => {
        e.stopPropagation();
        void startDownload(entry.id, quant.label);
      }}
    >
      Get {quant.label} · {quant.fileGb.toFixed(1)} GB
    </button>
  );
}

function ModelRow({
  entry,
  fitOf,
  hero,
  heroLine,
  heroQuant,
}: {
  entry: CatalogEntry;
  fitOf: FitLookup;
  hero?: boolean;
  heroLine?: string;
  heroQuant?: string;
}) {
  const [open, setOpen] = useState(false);
  const library = useStore((s) => s.library);
  const runs = entry.quants.some((q) => runnable(fitOf(entry.id, q.label)?.fitMode ?? "exceeds"));
  const installed = entry.quants.some((q) =>
    q.files.some((f) => library.some((m) => m.sha256 === f.sha256)),
  );

  return (
    <div
      className={`ledger-row${hero ? " ledger-row--hero" : ""}${open ? " ledger-row--open" : ""}${!runs ? " ledger-row--out" : ""}`}
      onClick={() => setOpen((o) => !o)}
    >
      {hero && <div className="ledger-row__sweep" aria-hidden="true" />}
      <div className="ledger-row__main">
        <div className="ledger-row__id">
          {hero && <span className="t-label">Recommended for this machine</span>}
          <span className={hero ? "t-display" : "t-title"}>{entry.name}</span>
          {hero && heroLine && <span className="ledger-row__blurb t-quiet">{heroLine}</span>}
          {hero && heroQuant && <HeroAction entry={entry} quantLabel={heroQuant} />}
        </div>
        <span className="ledger-row__meta t-quiet">
          {entry.family} · {entry.paramsB < 1 ? `${Math.round(entry.paramsB * 1000)}M` : `${entry.paramsB.toFixed(0)}B`} ·{" "}
          {entry.roles.join(" / ")}
          {installed && <span className="ledger-row__installed"> · on disk</span>}
        </span>
        <FitJewels entry={entry} fitOf={fitOf} />
      </div>
      <div className="ledger-row__detail">
        <div className="ledger-row__detail-inner">
          {!hero && <p className="t-quiet ledger-row__blurb">{entry.blurb}</p>}
          <QuantTable entry={entry} fitOf={fitOf} />
        </div>
      </div>
    </div>
  );
}

/** The switching-cost killer, front and center: adopt an existing Ollama
 *  library in place — zero bytes copied or re-downloaded. */
function OllamaImport() {
  const library = useStore((s) => s.library);
  const [status, setStatus] = useState<OllamaStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);

  useEffect(() => {
    void ipc.getOllamaStatus().then(setStatus).catch(() => {});
  }, []);

  if (!status?.available || status.modelCount === 0) return null;
  const imported = library.filter((m) => m.source === "ollama").length;
  if (result === null && imported >= status.modelCount) return null;

  return (
    <button
      className="btn-quiet models__import"
      disabled={busy}
      onClick={() => {
        setBusy(true);
        void ipc
          .importOllama()
          .then(async (r) => {
            useStore.setState({ library: await ipc.listLibrary() });
            setResult(
              r.imported > 0
                ? `${r.imported} adopted — nothing re-downloaded`
                : "already up to date",
            );
          })
          .catch(() => setResult("import failed — see Operations"))
          .finally(() => setBusy(false));
      }}
    >
      {busy
        ? "Importing…"
        : result ?? `Import from Ollama (${status.modelCount})`}
    </button>
  );
}

export function Models() {
  const catalog = useStore((s) => s.catalog);
  const recs = useStore((s) => s.recommendations);
  const library = useStore((s) => s.library);
  const maybeStartCoach = useStore((s) => s.maybeStartCoach);
  const [filter, setFilter] = useState<Filter>("all");
  const [sort, setSort] = useState<SortMode>("quality");

  useEffect(() => {
    maybeStartCoach("models");
  }, [maybeStartCoach]);

  const { heroEntry, entries } = useMemo(() => {
    if (!catalog) return { heroEntry: null, entries: [] as CatalogEntry[] };
    const list =
      filter === "all"
        ? catalog.entries
        : catalog.entries.filter((e) => e.roles.includes(filter));
    const cmp =
      sort === "size"
        ? (a: CatalogEntry, b: CatalogEntry) => b.paramsB - a.paramsB || b.quality - a.quality
        : sort === "name"
          ? (a: CatalogEntry, b: CatalogEntry) => a.name.localeCompare(b.name)
          : (a: CatalogEntry, b: CatalogEntry) => b.quality - a.quality;
    const sorted = [...list].sort(cmp);
    // The recommended model always leads, regardless of sort.
    const heroId = recs?.best?.entryId;
    const heroEntry = sorted.find((e) => e.id === heroId) ?? null;
    return { heroEntry, entries: sorted.filter((e) => e.id !== heroEntry?.id) };
  }, [catalog, filter, sort, recs]);

  // Total disk used by installed models — the real footprint, from the library.
  const diskGb = useMemo(
    () => library.reduce((sum, m) => sum + m.sizeBytes, 0) / 1e9,
    [library],
  );

  // Fit lookup from the backend's table — the same numbers everywhere.
  const fitMap = useMemo(() => {
    const m = new Map<string, QuantFit>();
    for (const f of recs?.fits ?? []) m.set(`${f.entryId}:${f.quant}`, f);
    return m;
  }, [recs]);

  if (!catalog || !recs) return null;

  const fitOf: FitLookup = (entryId, quant) => fitMap.get(`${entryId}:${quant}`);

  const shown = (heroEntry ? 1 : 0) + entries.length;
  const shownRunnable = [...entries, ...(heroEntry ? [heroEntry] : [])].filter((e) =>
    e.quants.some((q) => runnable(fitOf(e.id, q.label)?.fitMode ?? "exceeds")),
  ).length;

  const budgetLine = recs.multiGpu
    ? `budget ${recs.budgetGb.toFixed(1)} GB across ${recs.gpuCount} GPUs`
    : `budget ${recs.budgetGb.toFixed(1)} GB`;

  // Instrument-voice line for the hero row, composed from real fit data.
  const b = recs.best;
  const heroLine = b
    ? `${b.paramsB.toFixed(0)}B · ${b.quant} · ${
        b.fitMode === "partialOffload"
          ? `GPU+CPU split, ~${b.gpuOffloadPct ?? 0}% on GPU`
          : b.fitMode === "cpu"
            ? "runs on CPU"
            : `${b.headroomGb.toFixed(1)} GB headroom · to ${ctxHuman(b.maxCtx)} context`
      }`
    : undefined;

  return (
    <div className="models view">
      <header className="view-head">
        <h1 className="t-display">Models</h1>
        <span className="view-head__sub t-quiet">
          {filter === "all"
            ? `${shownRunnable} of ${shown} run on this machine · ${budgetLine}`
            : `${shown} ${filter} model${shown === 1 ? "" : "s"} · ${shownRunnable} run on this machine`}
          {library.length > 0 && (
            <span className="models__disk tnum">
              {" · "}
              {diskGb.toFixed(1)} GB on disk across {library.length} model
              {library.length === 1 ? "" : "s"}
            </span>
          )}
        </span>
        <OllamaImport />
        <nav className="filters" data-coach="model-filters">
          {FILTERS.map((f) => (
            <button
              key={f.id}
              className={`filters__btn${filter === f.id ? " filters__btn--active" : ""}`}
              onClick={() => setFilter(f.id)}
            >
              {f.label}
            </button>
          ))}
          <span className="filters__sep" aria-hidden="true" />
          {SORTS.map((s) => (
            <button
              key={s.id}
              className={`filters__btn filters__btn--sort${sort === s.id ? " filters__btn--active" : ""}`}
              onClick={() => setSort(s.id)}
              title={`Sort by ${s.label.toLowerCase()}`}
            >
              {s.label}
            </button>
          ))}
        </nav>
      </header>

      <div className="ledger" key={filter}>
        {heroEntry && (
          <ModelRow
            entry={heroEntry}
            fitOf={fitOf}
            hero
            heroLine={heroLine}
            heroQuant={recs.best?.quant}
          />
        )}
        {entries.map((e) => (
          <ModelRow key={e.id} entry={e} fitOf={fitOf} />
        ))}
      </div>
    </div>
  );
}

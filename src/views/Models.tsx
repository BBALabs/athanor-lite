/**
 * Models — the showroom. A vertical ledger of light rows; the machine's
 * recommended model leads as the hero row. Fit verdicts are jewels, not
 * chips. Rows expand in place to the full quant table.
 */

import { useMemo, useState, type MouseEvent } from "react";
import { useStore } from "../state/store";
import { bytesHuman, ctxHuman } from "../lib/format";
import { CloseIcon } from "../components/Icons";
import type { CatalogEntry, DownloadProgress, QuantOption, Role } from "../lib/types";

type Filter = "all" | Role;

const FILTERS: { id: Filter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "general", label: "General" },
  { id: "coding", label: "Coding" },
  { id: "reasoning", label: "Reasoning" },
  { id: "embedding", label: "Embedding" },
];

type Verdict = "fits" | "tight" | "no";

/**
 * Honest fit: "tight" lives INSIDE the budget (85–100%). Over budget is over
 * budget — the app's core promise is that a verdict is never a euphemism.
 */
function fitVerdict(minMemGb: number, budgetGb: number): Verdict {
  if (minMemGb <= budgetGb * 0.85) return "fits";
  if (minMemGb <= budgetGb) return "tight";
  return "no";
}

const VERDICT_WORD: Record<Verdict, string> = {
  fits: "fits",
  tight: "tight",
  no: "exceeds this machine",
};

function FitJewels({ quants, budgetGb }: { quants: QuantOption[]; budgetGb: number }) {
  return (
    <div className="jewels">
      {quants.map((q) => {
        const v = fitVerdict(q.minMemGb, budgetGb);
        return (
          <span
            key={q.label}
            className={`jewel jewel--${v}`}
            title={`${q.label} — ${VERDICT_WORD[v]} (~${q.minMemGb.toFixed(1)} GB loaded)`}
          />
        );
      })}
    </div>
  );
}

/** The action cell for one quant: get / progress / installed. */
function QuantAction({ entry, quant, verdict }: { entry: CatalogEntry; quant: QuantOption; verdict: Verdict }) {
  const library = useStore((s) => s.library);
  const downloads = useStore((s) => s.downloads);
  const startDownload = useStore((s) => s.startDownload);
  const cancelDownload = useStore((s) => s.cancelDownload);

  const sha = quant.files[0]?.sha256;
  if (!sha) return <span className="t-quiet">—</span>;

  const installed = library.some((m) => m.sha256 === sha);
  const dl: DownloadProgress | undefined = downloads[sha];
  const active = dl && (dl.state === "starting" || dl.state === "downloading" || dl.state === "verifying");

  const stop = (fn: () => void) => (e: MouseEvent) => {
    e.stopPropagation();
    fn();
  };

  if (installed) {
    return <span className="quant-action quant-action--installed t-quiet">on disk</span>;
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
  if (verdict === "no") {
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

function QuantTable({ entry, budgetGb }: { entry: CatalogEntry; budgetGb: number }) {
  return (
    <div className="quant-table">
      {entry.quants.map((q) => {
        const v = fitVerdict(q.minMemGb, budgetGb);
        return (
          <div className="quant-table__row" key={q.label}>
            <span className={`jewel jewel--${v}`} />
            <span className="t-mono">{q.label}</span>
            <span className="t-quiet tnum">{q.fileGb.toFixed(1)} GB file</span>
            <span className="t-quiet tnum">~{q.minMemGb.toFixed(1)} GB loaded</span>
            <span className={`quant-table__verdict t-quiet quant-table__verdict--${v}`}>
              {VERDICT_WORD[v]}
            </span>
            <QuantAction entry={entry} quant={q} verdict={v} />
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

  const quant = entry.quants.find((q) => q.label === quantLabel) ?? entry.quants[0];
  const sha = quant?.files[0]?.sha256;
  if (!quant || !sha) return null;

  const installed = library.some((m) => m.sha256 === sha);
  const dl = downloads[sha];
  const active = dl && (dl.state === "starting" || dl.state === "downloading" || dl.state === "verifying");

  if (installed) {
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
  budgetGb,
  hero,
  heroLine,
  heroQuant,
}: {
  entry: CatalogEntry;
  budgetGb: number;
  hero?: boolean;
  heroLine?: string;
  heroQuant?: string;
}) {
  const [open, setOpen] = useState(false);
  const library = useStore((s) => s.library);
  const runnable = entry.quants.some((q) => fitVerdict(q.minMemGb, budgetGb) !== "no");
  const installed = entry.quants.some((q) =>
    q.files.some((f) => library.some((m) => m.sha256 === f.sha256)),
  );

  return (
    <div
      className={`ledger-row${hero ? " ledger-row--hero" : ""}${open ? " ledger-row--open" : ""}${!runnable ? " ledger-row--out" : ""}`}
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
        <FitJewels quants={entry.quants} budgetGb={budgetGb} />
      </div>
      <div className="ledger-row__detail">
        <div className="ledger-row__detail-inner">
          {!hero && <p className="t-quiet ledger-row__blurb">{entry.blurb}</p>}
          <QuantTable entry={entry} budgetGb={budgetGb} />
        </div>
      </div>
    </div>
  );
}

export function Models() {
  const catalog = useStore((s) => s.catalog);
  const recs = useStore((s) => s.recommendations);
  const [filter, setFilter] = useState<Filter>("all");

  const { heroEntry, entries } = useMemo(() => {
    if (!catalog) return { heroEntry: null, entries: [] as CatalogEntry[] };
    const list =
      filter === "all"
        ? catalog.entries
        : catalog.entries.filter((e) => e.roles.includes(filter));
    const sorted = [...list].sort((a, b) => b.quality - a.quality);
    const heroId = recs?.best?.entryId;
    const heroEntry = sorted.find((e) => e.id === heroId) ?? null;
    return { heroEntry, entries: sorted.filter((e) => e.id !== heroEntry?.id) };
  }, [catalog, filter, recs]);

  if (!catalog || !recs) return null;

  const shown = (heroEntry ? 1 : 0) + entries.length;
  const shownRunnable = [...entries, ...(heroEntry ? [heroEntry] : [])].filter((e) =>
    e.quants.some((q) => q.minMemGb <= recs.budgetGb),
  ).length;

  // Instrument-voice line for the hero row, composed from real fit data.
  const heroLine = recs.best
    ? `${recs.best.paramsB.toFixed(0)}B · strongest pick within the ${recs.budgetGb.toFixed(1)} GB budget · ${recs.best.quant} leaves ${recs.best.headroomGb.toFixed(1)} GB headroom`
    : undefined;

  return (
    <div className="models view">
      <header className="view-head">
        <h1 className="t-display">Models</h1>
        <span className="view-head__sub t-quiet">
          {filter === "all"
            ? `${shownRunnable} of ${shown} run on this machine · budget ${recs.budgetGb.toFixed(1)} GB`
            : `${shown} ${filter} model${shown === 1 ? "" : "s"} · ${shownRunnable} run on this machine`}
        </span>
        <nav className="filters">
          {FILTERS.map((f) => (
            <button
              key={f.id}
              className={`filters__btn${filter === f.id ? " filters__btn--active" : ""}`}
              onClick={() => setFilter(f.id)}
            >
              {f.label}
            </button>
          ))}
        </nav>
      </header>

      <div className="ledger" key={filter}>
        {heroEntry && (
          <ModelRow
            entry={heroEntry}
            budgetGb={recs.budgetGb}
            hero
            heroLine={heroLine}
            heroQuant={recs.best?.quant}
          />
        )}
        {entries.map((e) => (
          <ModelRow key={e.id} entry={e} budgetGb={recs.budgetGb} />
        ))}
      </div>
    </div>
  );
}

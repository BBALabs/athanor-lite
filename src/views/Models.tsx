/**
 * Models — the showroom. A vertical ledger of light rows; the machine's
 * recommended model leads as the hero row. Fit verdicts are jewels, not
 * chips. Rows expand in place to the full quant table.
 */

import { useMemo, useState } from "react";
import { useStore } from "../state/store";
import { ctxHuman } from "../lib/format";
import type { CatalogEntry, QuantOption, Role } from "../lib/types";

type Filter = "all" | Role;

const FILTERS: { id: Filter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "general", label: "General" },
  { id: "coding", label: "Coding" },
  { id: "reasoning", label: "Reasoning" },
  { id: "embedding", label: "Embedding" },
];

type Verdict = "fits" | "tight" | "no";

function fitVerdict(minMemGb: number, budgetGb: number): Verdict {
  if (minMemGb <= budgetGb) return "fits";
  if (minMemGb <= budgetGb * 1.15) return "tight";
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

function ModelRow({
  entry,
  budgetGb,
  hero,
  heroLine,
}: {
  entry: CatalogEntry;
  budgetGb: number;
  hero?: boolean;
  heroLine?: string;
}) {
  const [open, setOpen] = useState(false);
  const runnable = entry.quants.some((q) => fitVerdict(q.minMemGb, budgetGb) !== "no");

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
        </div>
        <span className="ledger-row__meta t-quiet">
          {entry.family} · {entry.paramsB < 1 ? `${Math.round(entry.paramsB * 1000)}M` : `${entry.paramsB.toFixed(0)}B`} ·{" "}
          {entry.roles.join(" / ")}
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
          <ModelRow entry={heroEntry} budgetGb={recs.budgetGb} hero heroLine={heroLine} />
        )}
        {entries.map((e) => (
          <ModelRow key={e.id} entry={e} budgetGb={recs.budgetGb} />
        ))}
      </div>
    </div>
  );
}

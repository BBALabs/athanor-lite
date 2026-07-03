/**
 * Models — the curated catalog, ranked by capability, with every quant
 * fit-checked against THIS machine's budget. Downloads land in M2; the
 * fit verdicts are real today.
 */

import { useMemo, useState } from "react";
import { useStore } from "../state/store";
import { CheckIcon, AlertIcon } from "../components/Icons";
import { ctxHuman } from "../lib/format";
import type { CatalogEntry, Role } from "../lib/types";

type Filter = "all" | Role;

const FILTERS: { id: Filter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "general", label: "General" },
  { id: "coding", label: "Coding" },
  { id: "reasoning", label: "Reasoning" },
  { id: "embedding", label: "Embedding" },
];

function fitVerdict(minMemGb: number, budgetGb: number): "fits" | "tight" | "no" {
  if (minMemGb <= budgetGb) return "fits";
  if (minMemGb <= budgetGb * 1.15) return "tight";
  return "no";
}

function ModelRow({ entry, budgetGb }: { entry: CatalogEntry; budgetGb: number }) {
  const [open, setOpen] = useState(false);
  const bestFit = entry.quants.some((q) => fitVerdict(q.minMemGb, budgetGb) === "fits");
  const anyTight = entry.quants.some((q) => fitVerdict(q.minMemGb, budgetGb) === "tight");

  return (
    <div
      className={`model-row${open ? " model-row--open" : ""}${!bestFit && !anyTight ? " model-row--out" : ""}`}
      onClick={() => setOpen((o) => !o)}
    >
      <div className="model-row__main">
        <div className="model-row__rank k-num">{entry.quality}</div>
        <div className="model-row__id">
          <div className="model-row__name">{entry.name}</div>
          <div className="model-row__family k-num">{entry.hfRepo}</div>
        </div>
        <div className="model-row__tags">
          {entry.roles.map((r) => (
            <span key={r} className={`role-tag role-tag--${r}`}>{r}</span>
          ))}
        </div>
        <span className="k-chip">{entry.paramsB < 1 ? `${Math.round(entry.paramsB * 1000)}M` : `${entry.paramsB.toFixed(0)}B`}</span>
        <span className="k-chip">{ctxHuman(entry.contextLength)} ctx</span>
        <div className="model-row__quants">
          {entry.quants.map((q) => {
            const v = fitVerdict(q.minMemGb, budgetGb);
            return (
              <span key={q.label} className={`quant quant--${v}`} title={`${q.label}: needs ~${q.minMemGb.toFixed(1)} GB (${q.fileGb.toFixed(1)} GB file)`}>
                {v === "fits" ? <CheckIcon size={11} /> : v === "tight" ? <AlertIcon size={11} /> : null}
                {q.label}
              </span>
            );
          })}
        </div>
      </div>
      {open && (
        <div className="model-row__detail">
          <p className="model-row__blurb">{entry.blurb}</p>
          <div className="model-row__detail-chips">
            <span className="k-chip">{entry.license}</span>
            {entry.quants.map((q) => (
              <span key={q.label} className="k-chip">
                {q.label} · {q.fileGb.toFixed(1)} GB file · ~{q.minMemGb.toFixed(1)} GB loaded
              </span>
            ))}
            <span className="k-chip k-chip--soon">one-click install · arriving M2</span>
          </div>
        </div>
      )}
    </div>
  );
}

export function Models() {
  const catalog = useStore((s) => s.catalog);
  const recs = useStore((s) => s.recommendations);
  const [filter, setFilter] = useState<Filter>("all");

  const entries = useMemo(() => {
    if (!catalog) return [];
    const list = filter === "all"
      ? catalog.entries
      : catalog.entries.filter((e) => e.roles.includes(filter));
    return [...list].sort((a, b) => b.quality - a.quality);
  }, [catalog, filter]);

  if (!catalog || !recs) return null;

  const runnable = catalog.entries.filter((e) =>
    e.quants.some((q) => q.minMemGb <= recs.budgetGb),
  ).length;

  return (
    <div className="models">
      <div className="dash__head">
        <div>
          <h1 className="dash__title">Models</h1>
          <div className="dash__sub k-num">
            curated catalog v{catalog.version} · {runnable} of {catalog.entries.length} runnable on this machine
          </div>
        </div>
        <div className="models__filters">
          {FILTERS.map((f) => (
            <button
              key={f.id}
              className={`filter-btn${filter === f.id ? " filter-btn--active" : ""}`}
              onClick={() => setFilter(f.id)}
            >
              {f.label}
            </button>
          ))}
        </div>
      </div>

      <div className="models__legend">
        <span className="quant quant--fits"><CheckIcon size={11} />fits your {recs.budgetGb.toFixed(1)} GB budget</span>
        <span className="quant quant--tight"><AlertIcon size={11} />tight — reduced context</span>
        <span className="quant quant--no">exceeds this machine</span>
      </div>

      <div className="models__list">
        {entries.map((e) => (
          <ModelRow key={e.id} entry={e} budgetGb={recs.budgetGb} />
        ))}
      </div>
    </div>
  );
}

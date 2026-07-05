/**
 * Lite Home — the whole product on one pane. What this machine is, what it
 * can run, one press to be talking to it. Every verdict and number comes
 * from the backend's fit table; nothing here re-derives memory math.
 */

import { useEffect, useMemo, useRef, useState, type MouseEvent } from "react";
import { useStore } from "../state/store";
import { ipc } from "../lib/ipc";
import { BBA_URL } from "../lib/edition";
import { bytesHuman, COMPUTE_CLASS_LABEL, gib } from "../lib/format";
import { CloseIcon, TrashIcon } from "../components/Icons";
import type {
  CatalogEntry,
  FitMode,
  GpuInfo,
  LibraryModel,
  OllamaStatus,
  QuantFit,
} from "../lib/types";

/** Ranking order of fit verdicts — better first. Never invents a verdict. */
const FIT_RANK: Record<FitMode, number> = {
  gpuFull: 0,
  gpuTight: 1,
  partialOffload: 2,
  cpu: 3,
  exceeds: 4,
};

/** Plain words for each verdict — a first-timer reads these, not the codes. */
function fitPlain(mode: FitMode, multiGpu: boolean): string {
  const cards = multiGpu ? "graphics cards" : "graphics card";
  switch (mode) {
    case "gpuFull":
      return `runs fully on your ${cards}`;
    case "gpuTight":
      return `fits your ${cards} — snug`;
    case "partialOffload":
      return `splits between ${cards} and processor`;
    case "cpu":
      return "runs on your processor — slower, still private";
    case "exceeds":
      return "needs more memory than this machine has";
  }
}

const JEWEL: Record<FitMode, string> = {
  gpuFull: "fits",
  gpuTight: "tight",
  partialOffload: "partial",
  cpu: "cpu",
  exceeds: "no",
};

interface RankedModel {
  entry: CatalogEntry;
  quant: string;
  sha: string | null;
  fileGb: number;
  fit: QuantFit;
}

function gpuLine(g: GpuInfo): string {
  const vram = g.vramTotalBytes ? `${gib(g.vramTotalBytes)} GB` : "memory unknown";
  return `${g.name} · ${vram}`;
}

/** Two-step delete, same contract as the full app: first press arms it. */
function DeleteModel({ sha }: { sha: string }) {
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

/**
 * The action cell for one model: download (with live progress), or run.
 * Downloading through here arms auto-launch — done means talking, not a
 * second decision.
 */
function ModelAction({
  m,
  hero,
  onGet,
}: {
  m: RankedModel;
  hero?: boolean;
  onGet: (sha: string) => void;
}) {
  const library = useStore((s) => s.library);
  const downloads = useStore((s) => s.downloads);
  const startDownload = useStore((s) => s.startDownload);
  const cancelDownload = useStore((s) => s.cancelDownload);
  const liteLaunch = useStore((s) => s.liteLaunch);
  const serverStatus = useStore((s) => s.serverStatus);

  const sha = m.sha;
  if (!sha) return null;

  const installed = library.some((l) => l.sha256 === sha);
  const dl = downloads[sha];
  const active =
    dl && (dl.state === "starting" || dl.state === "downloading" || dl.state === "verifying");
  const loaded = serverStatus?.modelSha === sha && serverStatus.phase === "ready";

  const stop = (fn: () => void) => (e: MouseEvent) => {
    e.stopPropagation();
    fn();
  };

  if (active) {
    const pct = dl.totalBytes ? (dl.receivedBytes / dl.totalBytes) * 100 : 0;
    return (
      <div className="lite-action lite-action--live" onClick={(e) => e.stopPropagation()}>
        <div className="lightline">
          <div className="lightline__track" />
          <div
            className="lightline__lit"
            style={{
              width: `${pct.toFixed(2)}%`,
              background:
                "linear-gradient(90deg, var(--lume-deep), var(--lume) 70%, var(--lume-warm))",
            }}
          />
        </div>
        <span className="t-quiet tnum">
          {dl.state === "verifying"
            ? "verifying checksum…"
            : `${pct.toFixed(0)}% · ${bytesHuman(dl.bytesPerSec)}/s`}
        </span>
        <button
          className="quant-action__cancel"
          onClick={stop(() => void cancelDownload(sha))}
          aria-label="Cancel download"
          title="Cancel"
        >
          <CloseIcon size={10} />
        </button>
      </div>
    );
  }
  if (installed) {
    return (
      <span className="lite-action">
        {loaded && <span className="t-quiet">loaded · </span>}
        <button
          className={hero ? "btn-lit" : "btn-quiet"}
          onClick={stop(() => void liteLaunch(sha))}
        >
          Start chatting
        </button>
      </span>
    );
  }
  return (
    <button
      className={`${hero ? "btn-lit" : "btn-quiet"} lite-action`}
      onClick={stop(() => {
        onGet(sha);
        void startDownload(m.entry.id, m.quant);
      })}
      title="Downloads, verifies, and opens the chat when ready"
    >
      Download · {m.fileGb.toFixed(1)} GB
    </button>
  );
}

/** Ollama adoption — the "you already have models" fast path, in place. */
function OllamaAdopt() {
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
    <section className="lite-ollama">
      <div>
        <div className="t-title">Ollama is on this machine</div>
        <p className="t-quiet lite-ollama__note">
          {result ??
            `${status.modelCount} model${status.modelCount === 1 ? "" : "s"} in its library — adopt them in place. Nothing is copied or re-downloaded.`}
        </p>
      </div>
      {!result && (
        <button
          className="btn-quiet"
          disabled={busy}
          onClick={() => {
            setBusy(true);
            void ipc
              .importOllama()
              .then(async (r) => {
                useStore.setState({ library: await ipc.listLibrary() });
                setResult(
                  r.imported > 0
                    ? `${r.imported} adopted — zero bytes downloaded.`
                    : "Already up to date.",
                );
              })
              .catch(() => setResult("Import failed — details in Operations."))
              .finally(() => setBusy(false));
          }}
        >
          {busy ? "Adopting…" : `Adopt ${status.modelCount}`}
        </button>
      )}
    </section>
  );
}

export function LiteHome() {
  const hw = useStore((s) => s.hardware);
  const recs = useStore((s) => s.recommendations);
  const catalog = useStore((s) => s.catalog);
  const library = useStore((s) => s.library);
  const downloads = useStore((s) => s.downloads);
  const liteLaunch = useStore((s) => s.liteLaunch);
  const serverStatus = useStore((s) => s.serverStatus);

  // One-click promise: a download started here launches chat when it lands.
  const [pendingSha, setPendingSha] = useState<string | null>(null);
  const launched = useRef<string | null>(null);
  const pendingDl = pendingSha ? downloads[pendingSha] : undefined;
  useEffect(() => {
    if (!pendingSha || !pendingDl) return;
    if (pendingDl.state === "done" && launched.current !== pendingSha) {
      launched.current = pendingSha;
      setPendingSha(null);
      void liteLaunch(pendingSha);
    } else if (pendingDl.state === "failed" || pendingDl.state === "cancelled") {
      setPendingSha(null);
    }
  }, [pendingSha, pendingDl, liteLaunch]);

  // Backend picks are authoritative; fall back to the fits table for the rest.
  const { ranked, tooBig } = useMemo(() => {
    if (!catalog || !recs) return { ranked: [] as RankedModel[], tooBig: 0 };

    const pickQuant = new Map<string, string>();
    for (const p of [...(recs.best ? [recs.best] : []), ...recs.alternates])
      pickQuant.set(p.entryId, p.quant);
    for (const rp of recs.byRole)
      if (!pickQuant.has(rp.pick.entryId)) pickQuant.set(rp.pick.entryId, rp.pick.quant);

    const fitMap = new Map<string, QuantFit>();
    for (const f of recs.fits) fitMap.set(`${f.entryId}:${f.quant}`, f);

    const ranked: RankedModel[] = [];
    let tooBig = 0;
    for (const entry of catalog.entries) {
      // Chat models only — embedders are plumbing, not something to talk to.
      if (entry.roles.length === 1 && entry.roles[0] === "embedding") continue;

      let quant = pickQuant.get(entry.id) ?? null;
      if (!quant) {
        // Best runnable quant: strongest verdict first, then the bigger file
        // (higher-fidelity quant) within that verdict.
        const options = entry.quants
          .map((q) => ({ q, fit: fitMap.get(`${entry.id}:${q.label}`) }))
          .filter((o): o is { q: (typeof entry.quants)[number]; fit: QuantFit } =>
            o.fit !== undefined && o.fit.fitMode !== "exceeds",
          )
          .sort(
            (a, b) =>
              FIT_RANK[a.fit.fitMode] - FIT_RANK[b.fit.fitMode] || b.q.fileGb - a.q.fileGb,
          );
        quant = options[0]?.q.label ?? null;
      }
      if (!quant) {
        tooBig += 1;
        continue;
      }
      const fit = fitMap.get(`${entry.id}:${quant}`);
      const qo = entry.quants.find((q) => q.label === quant);
      if (!fit || !qo || fit.fitMode === "exceeds") {
        tooBig += 1;
        continue;
      }
      ranked.push({ entry, quant, sha: qo.files[0]?.sha256 ?? null, fileGb: qo.fileGb, fit });
    }
    ranked.sort((a, b) => b.entry.quality - a.entry.quality);
    return { ranked, tooBig };
  }, [catalog, recs]);

  const best = ranked.length > 0 ? ranked[0] : null;
  const rest = ranked.slice(1);

  // ── Degraded: no hardware profile ─────────────────────────────
  if (!hw) {
    return (
      <div className="lite view">
        <section className="degraded">
          <div className="t-title">Reading this machine didn't work</div>
          <p className="t-quiet degraded__note">
            The hardware probe failed, so there's nothing to recommend against yet.
            Details are in the log file.
          </p>
          <button className="btn-lit" onClick={() => void useStore.getState().retryHardware()}>
            Try again
          </button>
        </section>
        <LiteFooter />
      </div>
    );
  }

  const cls = COMPUTE_CLASS_LABEL[hw.computeClass] ?? COMPUTE_CLASS_LABEL.CpuOnly;
  const cpuOnly = recs?.mode === "cpuOnly";
  const multiGpu = recs?.multiGpu ?? false;
  const nvidiaGpus = hw.gpus.filter((g) => g.vendor === "Nvidia");

  const budgetLine = recs
    ? recs.multiGpu
      ? `${recs.gpuCount} GPUs pooled · ${recs.budgetGb.toFixed(1)} GB combined for models`
      : cpuOnly
        ? `${recs.ramBudgetGb.toFixed(1)} GB of system memory available for models`
        : `${recs.budgetGb.toFixed(1)} GB available for models` +
          (recs.vramInUseGb >= 0.5
            ? ` · ${recs.vramInUseGb.toFixed(1)} GB already in use elsewhere`
            : "")
    : null;

  return (
    <div className="lite view">
      {/* ── Zone A — machine and verdict ─────────────────────── */}
      <section className="lite-hero">
        <div className="lite-hero__machine">
          <span className="t-label">Your machine</span>
          {hw.gpus.length > 0 ? (
            hw.gpus.slice(0, 4).map((g, i) => (
              <div key={`${g.name}-${i}`} className="lite-hero__gpu">
                {gpuLine(g)}
              </div>
            ))
          ) : (
            <div className="lite-hero__gpu lite-hero__gpu--none">No dedicated graphics card</div>
          )}
          <div className="t-quiet lite-hero__cpu">
            {hw.cpu.brand} · {hw.cpu.physicalCores ?? "?"} cores · {gib(hw.memory.totalBytes)} GB
            memory
          </div>
          <div className="t-quiet lite-hero__class">
            {cls.title} — {cls.sub}
            {budgetLine ? ` · ${budgetLine}` : ""}
          </div>
          {cpuOnly && hw.gpus.length > 0 && nvidiaGpus.length === 0 && (
            <div className="t-quiet lite-hero__class">
              Model acceleration currently needs an NVIDIA card — everything below runs on the
              processor instead.
            </div>
          )}
        </div>

        <div className="lite-hero__verdict">
          {best && recs ? (
            <>
              <span className="t-label">Best for this machine</span>
              <div className="t-display lite-hero__name">{best.entry.name}</div>
              <div className="t-quiet lite-hero__fit">
                {best.entry.paramsB < 1
                  ? `${Math.round(best.entry.paramsB * 1000)} million`
                  : `${best.entry.paramsB.toFixed(0)} billion`}{" "}
                parameters · {fitPlain(best.fit.fitMode, recs.multiGpu)}
                {best.fit.fitMode === "partialOffload" && best.fit.gpuOffloadPct != null
                  ? ` (~${best.fit.gpuOffloadPct}% on the card)`
                  : ""}{" "}
                · needs {best.fit.estMemGb.toFixed(1)} GB
              </div>
              {recs.best?.note && <p className="t-quiet lite-hero__note">{recs.best.note}</p>}
              <div className="lite-hero__act">
                <ModelAction m={best} hero onGet={setPendingSha} />
                {!library.some((l) => l.sha256 === best.sha) &&
                  !(best.sha && downloads[best.sha]) && (
                    <span className="t-quiet lite-hero__promise">
                      one click — downloads, verifies, opens the chat
                    </span>
                  )}
              </div>
            </>
          ) : (
            <>
              <span className="t-label">Recommendation</span>
              <p className="t-quiet lite-hero__note">
                {recs
                  ? recs.notes.join(" ") || "No model in the catalog fits this machine."
                  : "Recommendations aren't available — the hardware profile came up, but ranking failed. Retry from the machine panel."}
              </p>
            </>
          )}
        </div>
      </section>

      {/* ── Zone B — the ranked ledger ───────────────────────── */}
      {rest.length > 0 && (
        <section className="lite-ledger">
          <span className="t-label">Also runs here — strongest first</span>
          {rest.map((m, i) => (
            <div className="lite-row" key={m.entry.id}>
              <span className="lite-row__rank tnum">{String(i + 2).padStart(2, "0")}</span>
              <span className={`jewel jewel--${JEWEL[m.fit.fitMode]}`} title={fitPlain(m.fit.fitMode, multiGpu)} />
              <div className="lite-row__id">
                <span className="t-title">{m.entry.name}</span>
                <span className="t-quiet">
                  {m.entry.paramsB < 1
                    ? `${Math.round(m.entry.paramsB * 1000)}M`
                    : `${m.entry.paramsB.toFixed(0)}B`}{" "}
                  · {fitPlain(m.fit.fitMode, multiGpu)} · {m.entry.blurb}
                </span>
              </div>
              <ModelAction m={m} onGet={setPendingSha} />
            </div>
          ))}
          {tooBig > 0 && (
            <div className="t-quiet lite-ledger__toobig">
              {tooBig} bigger model{tooBig === 1 ? "" : "s"} in the catalog need
              {tooBig === 1 ? "s" : ""} more memory than this machine has — nothing here will
              let you download something that can't run.
            </div>
          )}
        </section>
      )}

      {/* ── Zone C — already on disk ─────────────────────────── */}
      {library.length > 0 && (
        <section className="lite-library">
          <span className="t-label">On this machine</span>
          {library.map((m: LibraryModel) => {
            const loaded = serverStatus?.modelSha === m.sha256 && serverStatus.phase === "ready";
            return (
              <div className="lite-row" key={m.sha256}>
                <span className="lite-row__rank lite-row__rank--dot" aria-hidden="true">
                  ·
                </span>
                <div className="lite-row__id">
                  <span className="t-title">{m.displayName}</span>
                  <span className="t-quiet tnum">
                    {m.quant ?? "custom"} · {bytesHuman(m.sizeBytes)}
                    {m.source === "ollama" ? " · adopted from Ollama" : ""}
                    {loaded ? " · loaded now" : ""}
                  </span>
                </div>
                <span className="lite-action">
                  <button className="btn-quiet" onClick={() => void liteLaunch(m.sha256)}>
                    Start chatting
                  </button>
                  <DeleteModel sha={m.sha256} />
                </span>
              </div>
            );
          })}
        </section>
      )}

      <OllamaAdopt />
      <LiteFooter />
    </div>
  );
}

function LiteFooter() {
  return (
    <footer className="lite-foot">
      <button
        className="lite-foot__bba"
        onClick={() => void ipc.openLink(BBA_URL).catch(() => {})}
        title={BBA_URL}
      >
        Powered by Black Box Analytics
      </button>
      <span className="t-quiet"> · everything runs on this machine, nothing leaves it</span>
    </footer>
  );
}

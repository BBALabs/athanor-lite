/**
 * Onboarding — install to first conversation in under ten minutes, in plain
 * words. Every screen is skippable; the whole flow never shows a single
 * piece of jargon without the user asking for it.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { ipc } from "../lib/ipc";
import { useStore } from "../state/store";
import { MarkIcon } from "./Icons";
import { bytesHuman, COMPUTE_CLASS_LABEL } from "../lib/format";

type Stage = "welcome" | "choose" | "downloading" | "finishing";

const QUICK_START = { entryId: "llama-3.2-3b-instruct", quant: "Q4_K_M" };

export function Onboarding({ onDone }: { onDone: () => void }) {
  const hardware = useStore((s) => s.hardware);
  const recommendations = useStore((s) => s.recommendations);
  const catalog = useStore((s) => s.catalog);
  const downloads = useStore((s) => s.downloads);
  const startDownload = useStore((s) => s.startDownload);
  const setView = useStore((s) => s.setView);

  const [stage, setStage] = useState<Stage>("welcome");
  const [chosenSha, setChosenSha] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const best = recommendations?.best ?? null;
  const cls = hardware ? COMPUTE_CLASS_LABEL[hardware.computeClass] : null;

  const quickEntry = useMemo(
    () => catalog?.entries.find((e) => e.id === QUICK_START.entryId) ?? null,
    [catalog],
  );
  const quickQuant = quickEntry?.quants.find((q) => q.label === QUICK_START.quant) ?? null;
  const bestEntry = useMemo(
    () => (best ? catalog?.entries.find((e) => e.id === best.entryId) ?? null : null),
    [catalog, best],
  );
  const bestQuant = best ? bestEntry?.quants.find((q) => q.label === best.quant) ?? null : null;

  const dl = chosenSha ? downloads[chosenSha] : undefined;
  const pct = dl && dl.totalBytes ? (dl.receivedBytes / dl.totalBytes) * 100 : 0;

  // Guards the finish path against double-fire (StrictMode, rapid events).
  const finishing = useRef(false);

  const finish = async () => {
    if (finishing.current) return;
    finishing.current = true;
    setStage("finishing");
    try {
      const ws = await ipc.createWorkspace({
        name: "My Chat",
        purpose: "",
        accentHue: 275,
        glyph: "M",
      });
      if (chosenSha) await ipc.setWorkspaceModel(ws.id, chosenSha);
      await ipc.setOnboarded();
      useStore.setState({ workspaces: await ipc.listWorkspaces() });
      setView("chat");
      onDone();
    } catch (e) {
      finishing.current = false;
      setError(e instanceof Error ? e.message : String(e));
      setStage("choose");
    }
  };

  const skip = () => {
    void ipc.setOnboarded().catch(() => {});
    onDone();
  };

  const pick = (entryId: string, quant: string, sha: string | undefined) => {
    if (!sha) return;
    setChosenSha(sha);
    setStage("downloading");
    void startDownload(entryId, quant);
  };

  // Auto-advance when the chosen download settles. An effect, never render-
  // time state changes: finish() has side effects (workspace creation) that
  // must fire exactly once.
  useEffect(() => {
    if (stage !== "downloading" || !dl) return;
    if (dl.state === "done") {
      void finish();
    } else if (dl.state === "failed" || dl.state === "cancelled") {
      setError(dl.error ?? "download did not finish");
      setStage("choose");
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stage, dl?.state]);

  return (
    <div className="onboard">
      <div className="onboard__center">
        <MarkIcon size={30} className="boot__mark" />

        {stage === "welcome" && (
          <>
            <div className="onboard__title t-display">
              Athanor runs AI entirely on your computer.
            </div>
            <p className="onboard__lead t-quiet">
              Nothing you type ever leaves this machine. No account, no cloud, no one
              reading along.
            </p>
            <div className="onboard__actions">
              <button className="btn-lit" onClick={() => setStage("choose")}>
                Set me up
              </button>
              <button className="btn-quiet" onClick={skip}>
                I know what I'm doing
              </button>
            </div>
          </>
        )}

        {stage === "choose" && (
          <>
            <div className="onboard__title t-display">
              {cls ? `Your machine: ${cls.title.toLowerCase()}.` : "Pick your first model."}
            </div>
            <p className="onboard__lead t-quiet">
              {best
                ? `The strongest model this machine can run is ${best.name} — think ChatGPT-class, fully private.`
                : "Start with a small, quick model — you can add stronger ones any time."}
            </p>
            {error && <p className="onboard__error t-quiet">{error}</p>}
            <div className="onboard__choices">
              {quickQuant && (
                <button
                  className="onboard__choice"
                  onClick={() =>
                    pick(QUICK_START.entryId, QUICK_START.quant, quickQuant.files[0]?.sha256)
                  }
                >
                  <span className="t-title">Quick start</span>
                  <span className="t-quiet">
                    {quickEntry?.name} · {bytesHuman(quickQuant.files[0]?.sizeBytes ?? 0)} ·
                    ready in minutes
                  </span>
                </button>
              )}
              {best && bestQuant && best.entryId !== QUICK_START.entryId && (
                <button
                  className="onboard__choice onboard__choice--best"
                  onClick={() => pick(best.entryId, best.quant, bestQuant.files[0]?.sha256)}
                >
                  <span className="t-title">Best for this machine</span>
                  <span className="t-quiet">
                    {best.name} · {bytesHuman(bestQuant.files[0]?.sizeBytes ?? 0)} · the full
                    experience
                  </span>
                </button>
              )}
            </div>
            <button className="btn-quiet onboard__skip" onClick={skip}>
              Skip — I'll explore first
            </button>
          </>
        )}

        {stage === "downloading" && (
          <>
            <div className="onboard__title t-display">
              Fetching your model
              <span className="onboard__pct tnum"> · {pct.toFixed(0)}%</span>
            </div>
            <p className="onboard__lead t-quiet">
              Big file — it's the AI's entire brain.{" "}
              {dl ? `${bytesHuman(dl.receivedBytes)} of ${bytesHuman(dl.totalBytes)} · ${bytesHuman(dl.bytesPerSec)}/s` : ""}
            </p>
            <div className="onboard__bar">
              <div className="lightline">
                <div className="lightline__track" />
                <div
                  className="lightline__lit"
                  style={{
                    width: `${pct.toFixed(1)}%`,
                    background:
                      "linear-gradient(90deg, var(--lume-deep), var(--lume) 70%, var(--lume-warm))",
                  }}
                />
              </div>
            </div>
            <button className="btn-quiet onboard__skip" onClick={skip}>
              Continue in the background
            </button>
          </>
        )}

        {stage === "finishing" && (
          <div className="onboard__title t-display">Preparing your first workspace…</div>
        )}
      </div>
    </div>
  );
}

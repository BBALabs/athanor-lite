/**
 * Library — the models resident on this machine. Start a chat with any of them
 * in one press, or reclaim the disk. Downloading happens in Models; this page
 * is where you manage what has already landed.
 */

import { useEffect, useMemo, useState } from "react";
import { useStore } from "../state/store";
import { bytesHuman } from "../lib/format";
import { TrashIcon } from "../components/Icons";
import type { LibraryModel } from "../lib/types";

/** Two-step delete: the first press arms it, the second removes the file. */
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
      onClick={() => {
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

export function Library() {
  const library = useStore((s) => s.library);
  const liteLaunch = useStore((s) => s.liteLaunch);
  const serverStatus = useStore((s) => s.serverStatus);
  const setView = useStore((s) => s.setView);

  const diskGb = useMemo(
    () => library.reduce((sum, m) => sum + m.sizeBytes, 0) / 1e9,
    [library],
  );

  return (
    <div className="library view">
      <header className="view-head">
        <h1 className="t-display">Library</h1>
        <span className="view-head__sub t-quiet">
          {library.length === 0
            ? "Nothing installed yet"
            : `${library.length} model${library.length === 1 ? "" : "s"} · ${diskGb.toFixed(1)} GB on disk`}
        </span>
      </header>

      {library.length === 0 ? (
        <section className="degraded">
          <div className="t-title">No models on this machine yet</div>
          <p className="t-quiet degraded__note">
            Download one from Models — the recommended pick is already chosen for this
            machine, and everything runs locally once it lands.
          </p>
          <button className="btn-lit" onClick={() => setView("models")}>
            Browse models
          </button>
        </section>
      ) : (
        <section className="lite-library">
          {library.map((m: LibraryModel) => {
            const loaded =
              serverStatus?.modelSha === m.sha256 && serverStatus.phase === "ready";
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
    </div>
  );
}

/**
 * Library — the models resident on this machine, and the ones on their way.
 * Active downloads show live progress at the top; landed models start a chat
 * in one press or reclaim the disk. Browsing and starting downloads happens
 * in Models; this page is where you watch them arrive and manage them.
 */

import { useEffect, useMemo, useState } from "react";
import { useStore } from "../state/store";
import { bytesHuman } from "../lib/format";
import { CloseIcon, TrashIcon } from "../components/Icons";
import type { DownloadProgress, LibraryModel } from "../lib/types";

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

/** One in-flight download: name, live bar, received/total · speed, cancel. */
function DownloadRow({ dl, name }: { dl: DownloadProgress; name: string }) {
  const cancelDownload = useStore((s) => s.cancelDownload);
  const pct = dl.totalBytes ? (dl.receivedBytes / dl.totalBytes) * 100 : 0;
  return (
    <div className="lite-row" key={dl.sha256}>
      <span className="lite-row__rank lite-row__rank--dot" aria-hidden="true">
        ·
      </span>
      <div className="lite-row__id">
        <span className="t-title">{name}</span>
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
            : dl.state === "starting"
              ? "starting…"
              : `${bytesHuman(dl.receivedBytes)} of ${bytesHuman(dl.totalBytes)} · ${bytesHuman(dl.bytesPerSec)}/s`}
          {" · "}
          {dl.quant}
        </span>
      </div>
      <span className="lite-action">
        <button
          className="quant-action__cancel"
          onClick={() => void cancelDownload(dl.sha256)}
          aria-label="Cancel download"
          title="Cancel download"
        >
          <CloseIcon size={10} />
        </button>
      </span>
    </div>
  );
}

export function Library() {
  const library = useStore((s) => s.library);
  const downloads = useStore((s) => s.downloads);
  const catalog = useStore((s) => s.catalog);
  const launchChat = useStore((s) => s.launchChat);
  const serverStatus = useStore((s) => s.serverStatus);
  const setView = useStore((s) => s.setView);

  // In-flight only — done/failed/cancelled rows belong to the installed list
  // and the Operations drawer respectively, not here.
  const active = useMemo(
    () =>
      Object.values(downloads).filter(
        (d) =>
          (d.state === "starting" || d.state === "downloading" || d.state === "verifying") &&
          !library.some((m) => m.sha256 === d.sha256),
      ),
    [downloads, library],
  );

  // Prefer the catalog's display name over the raw file name.
  const nameOf = (dl: DownloadProgress) =>
    catalog?.entries.find((e) => e.id === dl.entryId)?.name ?? dl.fileName;

  const diskGb = useMemo(
    () => library.reduce((sum, m) => sum + m.sizeBytes, 0) / 1e9,
    [library],
  );

  const subParts: string[] = [];
  if (active.length > 0)
    subParts.push(`${active.length} download${active.length === 1 ? "" : "s"} in progress`);
  subParts.push(
    library.length === 0
      ? "nothing installed yet"
      : `${library.length} model${library.length === 1 ? "" : "s"} · ${diskGb.toFixed(1)} GB on disk`,
  );

  return (
    <div className="library view">
      <header className="view-head">
        <h1 className="t-display">Library</h1>
        <span className="view-head__sub t-quiet">{subParts.join(" · ")}</span>
      </header>

      {active.length > 0 && (
        <section className="lite-library">
          <span className="t-label">Downloading now</span>
          {active.map((dl) => (
            <DownloadRow key={dl.sha256} dl={dl} name={nameOf(dl)} />
          ))}
        </section>
      )}

      {library.length === 0 && active.length === 0 ? (
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
        library.length > 0 && (
          <section className="lite-library">
            {active.length > 0 && <span className="t-label">On this machine</span>}
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
                    <button className="btn-quiet" onClick={() => void launchChat(m.sha256)}>
                      Start chatting
                    </button>
                    <DeleteModel sha={m.sha256} />
                  </span>
                </div>
              );
            })}
          </section>
        )
      )}
    </div>
  );
}

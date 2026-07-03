/**
 * Workspaces — the garage. The active workspace is a hero panel of light;
 * the rest wait on a low shelf. Creation happens on a glass sheet that
 * rises from the bottom — boxless fields, curated accents, no rainbow.
 */

import { useEffect, useState, type FormEvent } from "react";
import { useStore } from "../state/store";
import { PlusIcon, TrashIcon } from "../components/Icons";
import { monogram, relativeTime } from "../lib/format";
import type { Workspace } from "../lib/types";

/** Curated accent hues — light, never paint. */
const ACCENTS = [275, 300, 205, 160, 25, 345];

function DeleteControl({ ws }: { ws: Workspace }) {
  const remove = useStore((s) => s.deleteWorkspace);
  const [armed, setArmed] = useState(false);

  useEffect(() => {
    if (!armed) return;
    const t = setTimeout(() => setArmed(false), 3500);
    return () => clearTimeout(t);
  }, [armed]);

  return (
    <button
      className={`ws-delete${armed ? " ws-delete--armed" : ""}`}
      onClick={(e) => {
        e.stopPropagation();
        if (armed) void remove(ws.id);
        else setArmed(true);
      }}
      aria-label={armed ? `Confirm delete ${ws.name}` : `Delete ${ws.name}`}
      title={armed ? "Click again to permanently delete" : "Delete workspace"}
    >
      {armed ? "confirm" : <TrashIcon size={13} />}
    </button>
  );
}

function CreateSheet({ onDone }: { onDone: () => void }) {
  const createWorkspace = useStore((s) => s.createWorkspace);
  const [name, setName] = useState("");
  const [purpose, setPurpose] = useState("");
  const [hue, setHue] = useState(ACCENTS[0]);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onDone();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onDone]);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!name.trim() || busy) return;
    setBusy(true);
    const ws = await createWorkspace({
      name,
      purpose,
      accentHue: hue,
      glyph: monogram(name),
    });
    setBusy(false);
    if (ws) onDone();
  };

  return (
    <div className="sheet-veil" onClick={onDone}>
      <form className="sheet" onClick={(e) => e.stopPropagation()} onSubmit={submit}>
        <input
          className="sheet__name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Name a workspace…"
          maxLength={64}
          autoFocus
        />
        <input
          className="sheet__purpose"
          value={purpose}
          onChange={(e) => setPurpose(e.target.value)}
          placeholder="What is it tuned for?"
          maxLength={200}
        />
        <div className="sheet__row">
          <div className="sheet__accents" role="radiogroup" aria-label="Accent">
            {ACCENTS.map((h) => (
              <button
                type="button"
                key={h}
                role="radio"
                aria-checked={hue === h}
                className={`accent${hue === h ? " accent--active" : ""}`}
                style={{ ["--ws-hue" as string]: h }}
                onClick={() => setHue(h)}
              />
            ))}
          </div>
          <div className="sheet__actions">
            <button type="button" className="btn-quiet" onClick={onDone}>
              Cancel
            </button>
            <button type="submit" className="btn-lit" disabled={!name.trim() || busy}>
              {busy ? "Creating…" : "Create"}
            </button>
          </div>
        </div>
      </form>
    </div>
  );
}

export function Workspaces() {
  const { workspaces, activeId } = useStore((s) => s.workspaces);
  const activate = useStore((s) => s.activateWorkspace);
  const [creating, setCreating] = useState(false);

  const active = workspaces.find((w) => w.id === activeId) ?? null;
  const shelf = workspaces.filter((w) => w.id !== active?.id);

  return (
    <div className="garage view">
      <header className="view-head">
        <h1 className="t-display">Workspaces</h1>
        <span className="view-head__sub t-quiet">
          {workspaces.length === 0
            ? "none yet"
            : `${workspaces.length} workspace${workspaces.length === 1 ? "" : "s"}`}
        </span>
        <button className="btn-quiet view-head__action" onClick={() => setCreating(true)}>
          <PlusIcon size={13} />
          New workspace
        </button>
      </header>

      {active ? (
        <section className="garage__hero" style={{ ["--ws-hue" as string]: active.accentHue }}>
          <div className="garage__hero-sweep" aria-hidden="true" />
          <span className="garage__hero-monogram" aria-hidden="true">
            {monogram(active.name)}
          </span>
          <div className="garage__hero-body">
            <span className="t-label">Active</span>
            <div className="t-display">{active.name}</div>
            {active.purpose && <div className="t-quiet garage__hero-purpose">{active.purpose}</div>}
            <div className="t-quiet garage__hero-meta">opened {relativeTime(active.lastOpenedAt)}</div>
          </div>
          <DeleteControl ws={active} />
        </section>
      ) : (
        <section className="garage__empty" onClick={() => setCreating(true)}>
          <div className="t-display garage__empty-title">Begin with a workspace</div>
          <p className="t-quiet">
            Each workspace is a self-contained stack — its own models, documents, and memory.
          </p>
        </section>
      )}

      {shelf.length > 0 && (
        <section className="garage__shelf">
          {shelf.map((ws) => (
            <button
              key={ws.id}
              className="bay"
              style={{ ["--ws-hue" as string]: ws.accentHue }}
              onClick={() => void activate(ws.id)}
              title={`Switch to ${ws.name}`}
            >
              <span className="bay__monogram" aria-hidden="true">
                {monogram(ws.name)}
              </span>
              <span className="bay__body">
                <span className="t-title bay__name">{ws.name}</span>
                <span className="t-quiet bay__meta">
                  {ws.purpose || `opened ${relativeTime(ws.lastOpenedAt)}`}
                </span>
              </span>
              <DeleteControl ws={ws} />
            </button>
          ))}
        </section>
      )}

      {creating && <CreateSheet onDone={() => setCreating(false)} />}
    </div>
  );
}

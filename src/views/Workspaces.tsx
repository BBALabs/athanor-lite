/**
 * Workspaces — the garage. The active workspace is a hero panel of light;
 * the rest wait on a low shelf. Creation happens on a glass sheet that
 * rises from the bottom — boxless fields, curated accents, no rainbow.
 */

import { useEffect, useState, type FormEvent } from "react";
import { save, open } from "@tauri-apps/plugin-dialog";
import { useStore } from "../state/store";
import { ipc } from "../lib/ipc";
import { IN_TAURI } from "../lib/tauriEnv";
import { PlusIcon, TrashIcon, ExportIcon } from "../components/Icons";
import { monogram, relativeTime } from "../lib/format";
import type { Template, Workspace } from "../lib/types";

/** Export a workspace's shareable config to a file the user chooses. */
function ShareControl({ ws }: { ws: Workspace }) {
  return (
    <button
      className="ws-share"
      onClick={async (e) => {
        e.stopPropagation();
        if (!IN_TAURI) return;
        const name = await ipc.exportWorkspaceFilename(ws.id).catch(() => "workspace.athanor.json");
        const dest = await save({
          defaultPath: name,
          filters: [{ name: "Athanor workspace", extensions: ["json"] }],
        });
        if (dest) void ipc.exportWorkspace(ws.id, dest);
      }}
      aria-label={`Export ${ws.name}`}
      title="Export this workspace's config to share"
    >
      <ExportIcon size={13} />
    </button>
  );
}

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

/** The starting-point gallery — the zero-friction default path. Pick one and
 *  the whole workspace is pre-filled; "Start blank" is the escape hatch. */
function TemplateGallery({
  templates,
  onPick,
  onBlank,
}: {
  templates: Template[];
  onPick: (t: Template) => void;
  onBlank: () => void;
}) {
  return (
    <div className="tpl-gallery">
      <div className="tpl-gallery__head">
        <div className="t-display">Start from a setup</div>
        <p className="t-quiet">
          Each is a ready-made stack — a fitting model, a system prompt, and the right defaults.
          You can change anything later.
        </p>
      </div>
      <div className="tpl-list">
        {templates.map((t) => (
          <button key={t.id} className="tpl" style={{ ["--ws-hue" as string]: t.accentHue }} onClick={() => onPick(t)}>
            <span className="tpl__monogram" aria-hidden="true">
              {t.glyph}
            </span>
            <span className="tpl__body">
              <span className="t-title tpl__name">{t.name}</span>
              <span className="t-quiet tpl__desc">{t.description}</span>
            </span>
            {t.ragEnabled && <span className="tpl__tag t-quiet">uses your documents</span>}
          </button>
        ))}
        <button className="tpl tpl--blank" onClick={onBlank}>
          <span className="tpl__monogram tpl__monogram--blank" aria-hidden="true">
            <PlusIcon size={18} />
          </span>
          <span className="tpl__body">
            <span className="t-title tpl__name">Start blank</span>
            <span className="t-quiet tpl__desc">A clean workspace you configure yourself.</span>
          </span>
        </button>
      </div>
    </div>
  );
}

function CreateSheet({ onDone }: { onDone: () => void }) {
  const createWorkspace = useStore((s) => s.createWorkspace);
  const templates = useStore((s) => s.templates);
  const [phase, setPhase] = useState<"pick" | "detail">(templates.length > 0 ? "pick" : "detail");
  const [template, setTemplate] = useState<Template | null>(null);
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

  const pick = (t: Template) => {
    setTemplate(t);
    setName(t.name);
    setPurpose(t.purpose);
    setHue(t.accentHue);
    setPhase("detail");
  };

  const startBlank = () => {
    setTemplate(null);
    setName("");
    setPurpose("");
    setHue(ACCENTS[0]);
    setPhase("detail");
  };

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!name.trim() || busy) return;
    setBusy(true);
    const ws = await createWorkspace({
      name,
      purpose,
      accentHue: hue,
      glyph: monogram(name),
      templateId: template?.id ?? null,
    });
    setBusy(false);
    if (ws) onDone();
  };

  return (
    <div className="sheet-veil" onClick={onDone}>
      {phase === "pick" ? (
        <div className="sheet sheet--gallery" onClick={(e) => e.stopPropagation()}>
          <TemplateGallery templates={templates} onPick={pick} onBlank={startBlank} />
        </div>
      ) : (
        <form className="sheet" onClick={(e) => e.stopPropagation()} onSubmit={submit}>
          {template && (
            <div className="sheet__from t-quiet">
              from <span className="sheet__from-name">{template.name}</span>
            </div>
          )}
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
              {templates.length > 0 && (
                <button type="button" className="btn-quiet" onClick={() => setPhase("pick")}>
                  Back
                </button>
              )}
              <button type="submit" className="btn-lit" disabled={!name.trim() || busy}>
                {busy ? "Creating…" : "Create"}
              </button>
            </div>
          </div>
        </form>
      )}
    </div>
  );
}

export function Workspaces() {
  const { workspaces, activeId } = useStore((s) => s.workspaces);
  const activate = useStore((s) => s.activateWorkspace);
  const importWorkspaceFile = useStore((s) => s.importWorkspaceFile);
  const maybeStartCoach = useStore((s) => s.maybeStartCoach);
  const [creating, setCreating] = useState(false);

  const doImport = async () => {
    if (!IN_TAURI) return;
    const picked = await open({ multiple: false, filters: [{ name: "Athanor workspace", extensions: ["json"] }] });
    if (typeof picked === "string") void importWorkspaceFile(picked);
  };

  const active = workspaces.find((w) => w.id === activeId) ?? null;
  const shelf = workspaces.filter((w) => w.id !== active?.id);

  // First visit to the garage, point out where new stacks come from.
  useEffect(() => {
    maybeStartCoach("workspaces");
  }, [maybeStartCoach]);

  return (
    <div className="garage view">
      <header className="view-head">
        <h1 className="t-display">Workspaces</h1>
        <span className="view-head__sub t-quiet">
          {workspaces.length === 0
            ? "none yet"
            : `${workspaces.length} workspace${workspaces.length === 1 ? "" : "s"}`}
        </span>
        <div className="view-head__action garage__head-actions">
          {IN_TAURI && (
            <button className="btn-quiet" onClick={() => void doImport()} title="Import a shared workspace file">
              Import
            </button>
          )}
          <button className="btn-quiet" onClick={() => setCreating(true)} data-coach="new-workspace">
            <PlusIcon size={13} />
            New workspace
          </button>
        </div>
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
          <div className="garage__hero-controls">
            <ShareControl ws={active} />
            <DeleteControl ws={active} />
          </div>
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
              <div className="bay__controls">
                <ShareControl ws={ws} />
                <DeleteControl ws={ws} />
              </div>
            </button>
          ))}
        </section>
      )}

      {creating && <CreateSheet onDone={() => setCreating(false)} />}
    </div>
  );
}

/**
 * Workspaces — purpose-built AI stacks, switched like projects in an IDE.
 * M1 scope: create, activate, delete. Models/RAG/memory attach in M2/M3.
 */

import { useState, type FormEvent } from "react";
import { useStore } from "../state/store";
import { PlusIcon, TrashIcon, CheckIcon } from "../components/Icons";
import { relativeTime } from "../lib/format";
import type { Workspace } from "../lib/types";

const GLYPHS = ["◆", "▲", "●", "■", "✦", "⬢", "◗", "▰"];
const DEFAULT_HUE = 275;

function WorkspaceTile({ ws, active }: { ws: Workspace; active: boolean }) {
  const activate = useStore((s) => s.activateWorkspace);
  const remove = useStore((s) => s.deleteWorkspace);
  const [armDelete, setArmDelete] = useState(false);

  return (
    <div
      className={`ws-tile${active ? " ws-tile--active" : ""}`}
      style={{ ["--ws-hue" as string]: ws.accentHue }}
      onClick={() => !active && void activate(ws.id)}
      onMouseLeave={() => setArmDelete(false)}
    >
      <div className="ws-tile__top">
        <span className="ws-tile__glyph">{ws.glyph || ws.name.slice(0, 1).toUpperCase()}</span>
        {active && (
          <span className="ws-tile__active">
            <CheckIcon size={11} /> active
          </span>
        )}
      </div>
      <div className="ws-tile__name">{ws.name}</div>
      <div className="ws-tile__purpose">{ws.purpose || "general purpose stack"}</div>
      <div className="ws-tile__foot">
        <span className="k-num">opened {relativeTime(ws.lastOpenedAt)}</span>
        <button
          className={`ws-tile__delete${armDelete ? " ws-tile__delete--armed" : ""}`}
          onClick={(e) => {
            e.stopPropagation();
            if (armDelete) void remove(ws.id);
            else setArmDelete(true);
          }}
          aria-label={armDelete ? `Confirm delete ${ws.name}` : `Delete ${ws.name}`}
          title={armDelete ? "Click again to permanently delete" : "Delete workspace"}
        >
          {armDelete ? "confirm?" : <TrashIcon size={13} />}
        </button>
      </div>
    </div>
  );
}

function CreateForm({ onDone }: { onDone: () => void }) {
  const createWorkspace = useStore((s) => s.createWorkspace);
  const [name, setName] = useState("");
  const [purpose, setPurpose] = useState("");
  const [glyph, setGlyph] = useState(GLYPHS[0]);
  const [hue, setHue] = useState(DEFAULT_HUE);
  const [busy, setBusy] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!name.trim() || busy) return;
    setBusy(true);
    const ws = await createWorkspace({ name, purpose, accentHue: hue, glyph });
    setBusy(false);
    if (ws) onDone();
  };

  return (
    <form className="ws-create panel" onSubmit={submit}>
      <div className="panel__title">
        <span className="k-label">new workspace</span>
      </div>
      <div className="ws-create__grid">
        <label className="ws-create__field">
          <span className="k-label">name</span>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Game Dev Assistant"
            maxLength={64}
            autoFocus
          />
        </label>
        <label className="ws-create__field">
          <span className="k-label">tuned for</span>
          <input
            value={purpose}
            onChange={(e) => setPurpose(e.target.value)}
            placeholder="Godot scripting, shader help, design docs"
            maxLength={200}
          />
        </label>
        <div className="ws-create__field">
          <span className="k-label">glyph</span>
          <div className="ws-create__glyphs">
            {GLYPHS.map((g) => (
              <button
                type="button"
                key={g}
                className={`glyph-btn${glyph === g ? " glyph-btn--active" : ""}`}
                style={{ ["--ws-hue" as string]: hue }}
                onClick={() => setGlyph(g)}
              >
                {g}
              </button>
            ))}
          </div>
        </div>
        <label className="ws-create__field">
          <span className="k-label">accent</span>
          <div className="ws-create__hue">
            <input
              type="range"
              min={0}
              max={359}
              value={hue}
              onChange={(e) => setHue(Number(e.target.value))}
              className="hue-slider"
            />
            <span className="hue-swatch" style={{ ["--ws-hue" as string]: hue }} />
          </div>
        </label>
      </div>
      <div className="ws-create__actions">
        <button type="button" className="btn-ghost" onClick={onDone}>
          Cancel
        </button>
        <button type="submit" className="btn-primary" disabled={!name.trim() || busy}>
          {busy ? "Creating…" : "Create workspace"}
        </button>
      </div>
    </form>
  );
}

export function Workspaces() {
  const { workspaces, activeId } = useStore((s) => s.workspaces);
  const [creating, setCreating] = useState(false);

  return (
    <div className="ws-view">
      <div className="dash__head">
        <div>
          <h1 className="dash__title">Workspaces</h1>
          <div className="dash__sub k-num">
            {workspaces.length === 0
              ? "purpose-built AI stacks — models, RAG, and memory per job"
              : `${workspaces.length} stack${workspaces.length === 1 ? "" : "s"} · switch like projects in an IDE`}
          </div>
        </div>
      </div>

      {creating && <CreateForm onDone={() => setCreating(false)} />}

      <div className="ws-grid">
        {workspaces.map((ws) => (
          <WorkspaceTile key={ws.id} ws={ws} active={ws.id === activeId} />
        ))}

        {!creating && (
          <button className="ws-tile ws-tile--new" onClick={() => setCreating(true)}>
            <PlusIcon size={22} />
            <span>New workspace</span>
            <span className="ws-tile--new-sub">
              a stack tuned for one job — its own models, documents, memory
            </span>
          </button>
        )}
      </div>
    </div>
  );
}

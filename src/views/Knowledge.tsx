/**
 * Knowledge — a workspace's documents and tools. Drag files in, watch them
 * index (progress lives in the Operations drawer), browse what's indexed,
 * preview the actual chunks, and connect MCP tool servers. This is what makes
 * a workspace more than a chat window.
 */

import { useEffect, useMemo, useState, type DragEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { ipc } from "../lib/ipc";
import { useStore } from "../state/store";
import { IN_TAURI } from "../lib/tauriEnv";
import { bytesHuman, relativeTime } from "../lib/format";
import { PlusIcon, TrashIcon, CloseIcon } from "../components/Icons";
import type { KbDocument, McpServerView, Source } from "../lib/types";

const ACCEPTED = "PDF, Word, Markdown, text, and code files";

function DocRow({ doc }: { doc: KbDocument }) {
  const removeDocument = useStore((s) => s.removeDocument);
  const cancelIndexing = useStore((s) => s.cancelIndexing);
  const operations = useStore((s) => s.operations);
  const [preview, setPreview] = useState<Source[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [armed, setArmed] = useState(false);

  const wsId = useStore((s) => s.workspaces.activeId);
  const indexingOp = operations.find(
    (o) => o.kind === "index" && o.label.includes(doc.name) && o.state === "running",
  );

  const openPreview = async () => {
    if (!wsId || doc.status !== "ready") return;
    if (preview) {
      setPreview(null);
      return;
    }
    setLoading(true);
    try {
      setPreview(await ipc.previewChunks(wsId, doc.id));
    } catch {
      setPreview([]);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className={`kb-doc kb-doc--${doc.status}`}>
      <div className="kb-doc__main" onClick={() => void openPreview()}>
        <div className="kb-doc__id">
          <span className={`kb-doc__dot kb-doc__dot--${doc.status}`} />
          <div>
            <div className="kb-doc__name">{doc.name}</div>
            <div className="kb-doc__meta t-quiet tnum">
              {doc.status === "ready" && `${doc.chunkCount} chunks · ${bytesHuman(doc.bytes)}`}
              {doc.status === "indexing" &&
                (indexingOp?.progressTotal
                  ? `indexing · ${indexingOp.progressCurrent ?? 0}/${indexingOp.progressTotal} chunks`
                  : "indexing…")}
              {doc.status === "failed" && (doc.error ?? "failed")}
              {doc.status === "ready" && ` · added ${relativeTime(doc.addedAt)}`}
            </div>
          </div>
        </div>
        <div className="kb-doc__actions">
          {doc.status === "indexing" ? (
            <button
              className="btn-quiet kb-doc__btn"
              onClick={(e) => {
                e.stopPropagation();
                void cancelIndexing(doc.id);
              }}
            >
              Stop
            </button>
          ) : (
            <button
              className={`ws-delete${armed ? " ws-delete--armed" : ""}`}
              onClick={(e) => {
                e.stopPropagation();
                if (armed) void removeDocument(doc.id);
                else setArmed(true);
              }}
              onMouseLeave={() => setArmed(false)}
              aria-label={armed ? "Confirm remove" : "Remove document"}
            >
              {armed ? "remove?" : <TrashIcon size={13} />}
            </button>
          )}
        </div>
      </div>
      {preview && (
        <div className="kb-doc__chunks">
          {loading && <div className="t-quiet">loading chunks…</div>}
          {preview.map((c) => (
            <div key={c.chunkIndex} className="kb-chunk">
              <span className="kb-chunk__idx t-mono">#{c.chunkIndex}</span>
              <p className="kb-chunk__text">{c.excerpt}</p>
            </div>
          ))}
          {!loading && preview.length === 0 && (
            <div className="t-quiet">no chunks to preview</div>
          )}
        </div>
      )}
    </div>
  );
}

function McpRow({ server }: { server: McpServerView }) {
  const connect = useStore((s) => s.connectMcpServer);
  const disconnect = useStore((s) => s.disconnectMcpServer);
  const remove = useStore((s) => s.removeMcpServer);
  const operations = useStore((s) => s.operations);
  const connecting = operations.some(
    (o) => o.kind === "mcp" && o.label.includes(server.config.name) && o.state === "running",
  );
  const [armed, setArmed] = useState(false);

  return (
    <div className="mcp-row">
      <div className="mcp-row__head">
        <div>
          <div className="mcp-row__name">
            {server.config.name}
            {server.connected && <span className="mcp-row__on"> · connected</span>}
          </div>
          <div className="mcp-row__cmd t-mono">
            {server.config.command} {server.config.args.join(" ")}
          </div>
        </div>
        <div className="mcp-row__actions">
          {server.connected ? (
            <button className="btn-quiet kb-doc__btn" onClick={() => void disconnect(server.config.id)}>
              Disconnect
            </button>
          ) : (
            <button
              className="btn-lit kb-doc__btn"
              disabled={connecting}
              onClick={() => void connect(server.config.id)}
            >
              {connecting ? "Connecting…" : "Connect"}
            </button>
          )}
          <button
            className={`ws-delete${armed ? " ws-delete--armed" : ""}`}
            onClick={() => (armed ? void remove(server.config.id) : setArmed(true))}
            onMouseLeave={() => setArmed(false)}
            aria-label="Remove server"
          >
            {armed ? "remove?" : <TrashIcon size={13} />}
          </button>
        </div>
      </div>
      {server.error && <div className="mcp-row__error t-quiet">{server.error}</div>}
      {server.tools.length > 0 && (
        <div className="mcp-row__tools">
          {server.tools.map((t) => (
            <span key={t.name} className="mcp-tool" title={t.description ?? undefined}>
              {t.name}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

function AddMcpForm({ onDone }: { onDone: () => void }) {
  const save = useStore((s) => s.saveMcpServer);
  const [name, setName] = useState("");
  const [command, setCommand] = useState("");
  const [args, setArgs] = useState("");

  return (
    <form
      className="mcp-form"
      onSubmit={(e) => {
        e.preventDefault();
        if (!name.trim() || !command.trim()) return;
        void save({
          id: "",
          name: name.trim(),
          command: command.trim(),
          args: args.split(/\s+/).filter(Boolean),
          env: {},
        }).then(onDone);
      }}
    >
      <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Server name" autoFocus />
      <input value={command} onChange={(e) => setCommand(e.target.value)} placeholder="Command (e.g. npx)" />
      <input
        value={args}
        onChange={(e) => setArgs(e.target.value)}
        placeholder="Arguments (e.g. -y @modelcontextprotocol/server-everything)"
      />
      <div className="mcp-form__actions">
        <button type="button" className="btn-quiet" onClick={onDone}>
          Cancel
        </button>
        <button type="submit" className="btn-lit" disabled={!name.trim() || !command.trim()}>
          Add server
        </button>
      </div>
    </form>
  );
}

export function Knowledge() {
  const { workspaces, activeId } = useStore((s) => s.workspaces);
  const knowledge = useStore((s) => s.knowledge);
  const mcpServers = useStore((s) => s.mcpServers);
  const addDocuments = useStore((s) => s.addDocuments);
  const setRetrievalEnabled = useStore((s) => s.setRetrievalEnabled);
  const loadKnowledge = useStore((s) => s.loadKnowledge);
  const setView = useStore((s) => s.setView);
  const [dragging, setDragging] = useState(false);
  const [addingMcp, setAddingMcp] = useState(false);

  const ws = workspaces.find((w) => w.id === activeId) ?? null;

  useEffect(() => {
    void loadKnowledge();
  }, [activeId, loadKnowledge]);

  // Tauri delivers OS file drops as a window event, not the DOM drop event.
  useEffect(() => {
    if (!IN_TAURI) return;
    const un = listen<{ paths: string[] }>("tauri://drag-drop", (e) => {
      setDragging(false);
      if (e.payload.paths?.length) void addDocuments(e.payload.paths);
    });
    const unOver = listen("tauri://drag-enter", () => setDragging(true));
    const unLeave = listen("tauri://drag-leave", () => setDragging(false));
    return () => {
      void un.then((f) => f());
      void unOver.then((f) => f());
      void unLeave.then((f) => f());
    };
  }, [addDocuments]);

  const pickFiles = async () => {
    if (!IN_TAURI) return;
    const picked = await open({
      multiple: true,
      filters: [
        {
          name: "Documents",
          extensions: ["pdf", "docx", "txt", "md", "markdown", "rs", "py", "js", "ts", "json", "csv", "html"],
        },
      ],
    });
    const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
    if (paths.length) void addDocuments(paths as string[]);
  };

  // Harness affordance for browser design work.
  const onDomDrop = (e: DragEvent) => {
    e.preventDefault();
    setDragging(false);
    if (!IN_TAURI) {
      const names = Array.from(e.dataTransfer.files).map((f) => `X:/dropped/${f.name}`);
      if (names.length) void addDocuments(names);
    }
  };

  const docs = knowledge?.documents ?? [];
  const readyCount = useMemo(() => docs.filter((d) => d.status === "ready").length, [docs]);

  if (!ws) {
    return (
      <div className="knowledge view">
        <div className="degraded">
          <div className="t-title">No workspace selected</div>
          <button className="btn-lit" onClick={() => setView("workspaces")}>
            Choose a workspace
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="knowledge view">
      <header className="view-head">
        <div>
          <h1 className="t-display">Knowledge</h1>
          <span className="view-head__sub t-quiet">
            {ws.name} · {readyCount} document{readyCount === 1 ? "" : "s"} ·{" "}
            {knowledge?.chunkTotal ?? 0} chunks indexed
          </span>
        </div>
        {knowledge && docs.length > 0 && (
          <label className="kb-retrieval">
            <span className="t-quiet">retrieval</span>
            <button
              className={`switch${knowledge.retrievalEnabled ? " switch--on" : ""}`}
              role="switch"
              aria-checked={knowledge.retrievalEnabled}
              onClick={() => void setRetrievalEnabled(!knowledge.retrievalEnabled)}
            >
              <span className="switch__dot" />
            </button>
          </label>
        )}
      </header>

      <div
        className={`kb-drop${dragging ? " kb-drop--over" : ""}`}
        onDragOver={(e) => {
          e.preventDefault();
          setDragging(true);
        }}
        onDragLeave={() => setDragging(false)}
        onDrop={onDomDrop}
        onClick={() => void pickFiles()}
      >
        <PlusIcon size={20} />
        <div className="kb-drop__title">Drop documents here</div>
        <div className="kb-drop__sub t-quiet">
          or click to browse · {ACCEPTED} · everything is embedded and stays on this machine
        </div>
      </div>

      {docs.length > 0 && (
        <section className="kb-list">
          {docs.map((d) => (
            <DocRow key={d.id} doc={d} />
          ))}
        </section>
      )}

      {/* ── Tools (MCP) ─────────────────────────────── */}
      <section className="kb-mcp">
        <div className="kb-mcp__head">
          <div className="t-title">Connected tools</div>
          <button className="btn-quiet" onClick={() => setAddingMcp((v) => !v)}>
            {addingMcp ? <CloseIcon size={13} /> : <PlusIcon size={13} />}
            {addingMcp ? " Close" : " Add MCP server"}
          </button>
        </div>
        <p className="t-quiet kb-mcp__blurb">
          Connect external tools and data via the Model Context Protocol. Servers run as
          sandboxed processes — visible and stoppable in Operations, and they never
          outlive the app.
        </p>
        {addingMcp && <AddMcpForm onDone={() => setAddingMcp(false)} />}
        {mcpServers.length === 0 && !addingMcp && (
          <div className="t-quiet kb-mcp__empty">No tool servers yet.</div>
        )}
        {mcpServers.map((s) => (
          <McpRow key={s.config.id} server={s} />
        ))}
      </section>
    </div>
  );
}

/**
 * Chat — the room where the machine speaks. The assistant's voice is the room
 * itself (plain ink on glass); the user's turns sit right-aligned on quiet
 * surfaces. Engine states narrate honestly: fetching, loading, ready.
 */

import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useStore } from "../state/store";
import { Markdown } from "../components/Markdown";
import { PlusIcon, TrashIcon } from "../components/Icons";
import { bytesHuman, monogram, relativeTime } from "../lib/format";
import { KnowledgeIcon } from "../components/Icons";
import type { ChatMessage, LibraryModel, Source } from "../lib/types";

/** Retrieved sources shown under a reply — transparency for what was pulled. */
function Sources({ sources }: { sources: Source[] }) {
  const [open, setOpen] = useState(false);
  if (sources.length === 0) return null;
  return (
    <div className="sources">
      <button className="sources__toggle t-quiet" onClick={() => setOpen((o) => !o)}>
        <KnowledgeIcon size={13} />
        drew on {sources.length} source{sources.length === 1 ? "" : "s"}
        <span className="sources__names">
          {" · "}
          {[...new Set(sources.map((s) => s.docName))].join(", ")}
        </span>
      </button>
      {open && (
        <div className="sources__list">
          {sources.map((s, i) => (
            <div key={i} className="source">
              <div className="source__head t-quiet">
                <span className="source__doc">{s.docName}</span>
                <span className="source__meta t-mono">
                  #{s.chunkIndex} · {(s.score * 100).toFixed(0)}% match
                </span>
              </div>
              <p className="source__excerpt">{s.excerpt}…</p>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

/** "Consulting the knowledge base" while retrieval runs, before tokens. */
function RetrievalStrip({ sources }: { sources: Source[] }) {
  if (sources.length === 0) return null;
  return (
    <div className="retrieval-strip t-quiet">
      <KnowledgeIcon size={13} />
      consulting {[...new Set(sources.map((s) => s.docName))].join(", ")}
    </div>
  );
}

function EngineStrip() {
  const runtimeState = useStore((s) => s.runtimeState);
  const serverStatus = useStore((s) => s.serverStatus);

  if (runtimeState && (runtimeState.phase === "downloading" || runtimeState.phase === "extracting")) {
    const pct = runtimeState.totalBytes
      ? (runtimeState.receivedBytes / runtimeState.totalBytes) * 100
      : 0;
    return (
      <div className="engine-strip">
        <span className="t-quiet">
          {runtimeState.phase === "downloading"
            ? `fetching the inference engine · ${pct.toFixed(0)}% of ${bytesHuman(runtimeState.totalBytes)}`
            : runtimeState.detail}
        </span>
        <div className="lightline">
          <div className="lightline__track" />
          <div
            className="lightline__lit"
            style={{
              width: `${pct.toFixed(1)}%`,
              background: "linear-gradient(90deg, var(--lume-deep), var(--lume) 70%, var(--lume-warm))",
            }}
          />
        </div>
      </div>
    );
  }
  if (serverStatus && (serverStatus.phase === "starting" || serverStatus.phase === "loading")) {
    return (
      <div className="engine-strip">
        <span className="engine-strip__pulse" />
        <span className="t-quiet">{serverStatus.detail}…</span>
      </div>
    );
  }
  return null;
}

function StatsLine({ msg }: { msg: ChatMessage }) {
  const s = msg.stats;
  if (!s) return null;
  return (
    <div className="msg__stats t-quiet tnum">
      {(s.ttftMs / 1000).toFixed(1)}s to first token · {s.predictedPerSecond.toFixed(1)} tok/s ·{" "}
      {s.contextUsed.toLocaleString()} ctx{s.gpuActive ? "" : " · CPU"}
      {s.cancelled ? " · stopped" : ""}
    </div>
  );
}

function ModelChooser() {
  const library = useStore((s) => s.library);
  const choose = useStore((s) => s.chooseWorkspaceModel);
  const setView = useStore((s) => s.setView);

  return (
    <div className="chooser">
      <div className="t-title">Choose this workspace's model</div>
      {library.length === 0 ? (
        <>
          <p className="t-quiet chooser__note">
            Nothing installed yet. Get a model from the catalog — the recommended one is
            already picked out for this machine.
          </p>
          <button className="btn-lit" onClick={() => setView("models")}>
            Open Models
          </button>
        </>
      ) : (
        <div className="chooser__list">
          {library.map((m: LibraryModel) => (
            <button key={m.sha256} className="chooser__item" onClick={() => void choose(m.sha256)}>
              <span className="t-title chooser__name">{m.displayName}</span>
              <span className="t-quiet tnum">
                {m.quant ?? "custom"} · {bytesHuman(m.sizeBytes)}
                {m.source === "ollama" ? " · imported from Ollama" : ""}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

export function Chat() {
  const { workspaces, activeId } = useStore((s) => s.workspaces);
  const conversations = useStore((s) => s.conversations);
  const activeConv = useStore((s) => s.activeConv);
  const streamText = useStore((s) => s.streamText);
  const generating = useStore((s) => s.generating);
  const liveSources = useStore((s) => s.liveSources);
  const library = useStore((s) => s.library);
  const sendMessage = useStore((s) => s.sendMessage);
  const stopGeneration = useStore((s) => s.stopGeneration);
  const openConversation = useStore((s) => s.openConversation);
  const newSession = useStore((s) => s.newSession);
  const removeConversation = useStore((s) => s.removeConversation);
  const setView = useStore((s) => s.setView);

  const [draft, setDraft] = useState("");
  const [armedDelete, setArmedDelete] = useState<string | null>(null);
  const threadRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  const ws = workspaces.find((w) => w.id === activeId) ?? null;
  const model = ws?.activeModel ? library.find((m) => m.sha256 === ws.activeModel) ?? null : null;

  useEffect(() => {
    const el = threadRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [activeConv?.messages.length, streamText]);

  const submit = () => {
    const text = draft.trim();
    if (!text || generating) return;
    setDraft("");
    void sendMessage(text);
    inputRef.current?.focus();
  };

  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  };

  if (!ws) {
    return (
      <div className="chat view">
        <div className="degraded">
          <div className="t-title">No workspace yet</div>
          <p className="t-quiet degraded__note">
            A workspace is a self-contained stack — its own model, sessions, and purpose.
            Create one to start talking.
          </p>
          <button className="btn-lit" onClick={() => setView("workspaces")}>
            Create a workspace
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="chat view">
      <div className="chat__layout">
        {/* ── Sessions rail ─────────────────────────── */}
        <aside className="sessions">
          <div className="sessions__head">
            <span
              className="statusbar__monogram"
              style={{ ["--ws-hue" as string]: ws.accentHue }}
            >
              {monogram(ws.name)}
            </span>
            <span className="sessions__ws t-quiet">{ws.name}</span>
            <button
              className="sessions__new"
              onClick={newSession}
              title="New session"
              aria-label="New session"
            >
              <PlusIcon size={14} />
            </button>
          </div>
          <div className="sessions__list">
            {conversations.map((c) => (
              <div
                key={c.id}
                className={`session${activeConv?.id === c.id ? " session--active" : ""}`}
                onClick={() => void openConversation(c.id)}
                onMouseLeave={() => setArmedDelete(null)}
              >
                <span className="session__title">{c.title}</span>
                <span className="session__meta t-quiet">{relativeTime(c.updatedAt)}</span>
                <button
                  className={`ws-delete session__delete${armedDelete === c.id ? " ws-delete--armed" : ""}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    if (armedDelete === c.id) void removeConversation(c.id);
                    else setArmedDelete(c.id);
                  }}
                  aria-label={armedDelete === c.id ? "Confirm delete" : "Delete session"}
                >
                  {armedDelete === c.id ? "sure?" : <TrashIcon size={12} />}
                </button>
              </div>
            ))}
            {conversations.length === 0 && (
              <div className="sessions__empty t-quiet">no sessions yet</div>
            )}
          </div>
          {model && (
            <div className="sessions__model t-quiet" title={model.fileName}>
              {model.displayName}
              {model.quant ? ` · ${model.quant}` : ""}
            </div>
          )}
        </aside>

        {/* ── The room ──────────────────────────────── */}
        <section className="room">
          {!ws.activeModel ? (
            <ModelChooser />
          ) : (
            <>
              <div className="thread" ref={threadRef}>
                {!activeConv && !generating && (
                  <div className="thread__empty">
                    <div className="t-display thread__empty-title">
                      {ws.purpose ? ws.purpose : "Ask anything"}
                    </div>
                    <p className="t-quiet">
                      Runs entirely on this machine. Nothing you type ever leaves it.
                    </p>
                  </div>
                )}
                {activeConv?.messages.map((m, i) => (
                  <div key={i} className={`msg msg--${m.role}`}>
                    {m.role === "assistant" ? <Markdown text={m.content} /> : m.content}
                    {m.role === "assistant" && m.sources?.length > 0 && <Sources sources={m.sources} />}
                    {m.role === "assistant" && <StatsLine msg={m} />}
                  </div>
                ))}
                {generating && (
                  <div className="msg msg--assistant">
                    <RetrievalStrip sources={liveSources} />
                    {streamText ? <Markdown text={streamText} /> : null}
                    <span className="caret" aria-hidden="true" />
                  </div>
                )}
              </div>

              <EngineStrip />

              <div className="composer">
                <textarea
                  ref={inputRef}
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  onKeyDown={onKey}
                  placeholder="Ask anything — this never leaves your computer"
                  rows={Math.min(6, Math.max(1, draft.split("\n").length))}
                  disabled={generating}
                />
                {generating ? (
                  <button className="btn-quiet composer__stop" onClick={() => void stopGeneration()}>
                    Stop
                  </button>
                ) : (
                  <button
                    className="btn-lit composer__send"
                    onClick={submit}
                    disabled={!draft.trim()}
                    aria-label="Send"
                  >
                    ↵
                  </button>
                )}
              </div>
            </>
          )}
        </section>
      </div>
    </div>
  );
}

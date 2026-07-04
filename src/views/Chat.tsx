/**
 * Chat — the room where the machine speaks. The assistant's voice is the room
 * itself (plain ink on glass); the user's turns sit right-aligned on quiet
 * surfaces. Engine states narrate honestly: fetching, loading, ready.
 */

import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { useStore, useLatestSample } from "../state/store";
import { IN_TAURI } from "../lib/tauriEnv";
import { Markdown } from "../components/Markdown";
import { PlusIcon, TrashIcon, SearchIcon, ExportIcon, CloseIcon } from "../components/Icons";
import { PromptLibrary } from "../components/PromptLibrary";
import { bytesHuman, monogram, relativeTime } from "../lib/format";
import { KnowledgeIcon } from "../components/Icons";
import type {
  ChatMessage,
  ConversationMeta,
  LibraryModel,
  SearchHit,
  Source,
  ToolStep,
} from "../lib/types";

/** Pretty-print tool JSON args; fall back to the raw string if unparseable. */
function fmtArgs(raw: string): string {
  if (!raw || raw.trim() === "" || raw.trim() === "{}") return "";
  try {
    return JSON.stringify(JSON.parse(raw));
  } catch {
    return raw;
  }
}

/** One autonomous tool call — expandable to show arguments and the result. */
function ToolStepRow({ step, live }: { step: ToolStep; live?: boolean }) {
  const [open, setOpen] = useState(false);
  const args = fmtArgs(step.arguments);
  return (
    <div className={`toolstep${step.ok ? "" : " toolstep--fail"}${live ? " toolstep--live" : ""}`}>
      <button className="toolstep__head" onClick={() => setOpen((o) => !o)}>
        <span className={`toolstep__dot${step.ok ? "" : " toolstep__dot--fail"}`} aria-hidden="true" />
        <span className="toolstep__tool t-mono">{step.tool}</span>
        {args && <span className="toolstep__args t-mono">{args}</span>}
        <span className="toolstep__chev t-quiet">{open ? "−" : "+"}</span>
      </button>
      {open && (
        <div className="toolstep__body">
          {args && (
            <div className="toolstep__field">
              <span className="toolstep__label t-quiet">arguments</span>
              <pre className="toolstep__pre t-mono">{args}</pre>
            </div>
          )}
          <div className="toolstep__field">
            <span className="toolstep__label t-quiet">{step.ok ? "result" : "error"}</span>
            <pre className="toolstep__pre t-mono">{step.result || "(empty)"}</pre>
          </div>
        </div>
      )}
    </div>
  );
}

/** The full set of tool calls made during one turn. */
function ToolSteps({ steps, live }: { steps: ToolStep[]; live?: boolean }) {
  if (steps.length === 0) return null;
  return (
    <div className="toolsteps">
      {steps.map((s, i) => (
        <ToolStepRow key={i} step={s} live={live} />
      ))}
    </div>
  );
}

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

const GIB = 1024 ** 3;

/** Live inference HUD — real GPU/VRAM/temp from telemetry, plus an estimated
 *  rate while tokens stream (the exact measured tok/s lands in the stats line). */
function InferenceHUD() {
  const generating = useStore((s) => s.generating);
  const streamText = useStore((s) => s.streamText);
  const sample = useLatestSample();
  const startRef = useRef<number | null>(null);
  const [, setTick] = useState(0);

  useEffect(() => {
    if (!generating) {
      startRef.current = null;
      return;
    }
    if (startRef.current == null) startRef.current = Date.now();
    const t = window.setInterval(() => setTick((x) => x + 1), 250);
    return () => window.clearInterval(t);
  }, [generating]);

  if (!generating) return null;
  const elapsed = startRef.current ? (Date.now() - startRef.current) / 1000 : 0;
  const words = streamText ? streamText.split(/\s+/).filter(Boolean).length : 0;
  const tps = elapsed > 0.3 ? (words * 1.3) / elapsed : 0; // rough: ~1.3 tokens/word
  const gpu = sample?.gpus[0];

  return (
    <div className="inf-hud">
      <span className="inf-hud__cell">
        <span className="inf-hud__n tnum">{tps.toFixed(0)}</span>
        <span className="t-quiet"> ~tok/s</span>
      </span>
      {gpu && (
        <span className="inf-hud__cell">
          <span className="inf-hud__n tnum">{gpu.utilizationPct ?? 0}</span>
          <span className="t-quiet">% GPU</span>
        </span>
      )}
      {gpu && (
        <span className="inf-hud__cell">
          <span className="inf-hud__n tnum">{(gpu.vramUsedBytes / GIB).toFixed(1)}</span>
          <span className="t-quiet"> GB VRAM</span>
        </span>
      )}
      {gpu?.temperatureC != null && (
        <span className="inf-hud__cell">
          <span className="inf-hud__n tnum">{gpu.temperatureC}</span>
          <span className="t-quiet">° C</span>
        </span>
      )}
      <span className="inf-hud__cell">
        <span className="inf-hud__n tnum">{elapsed.toFixed(1)}</span>
        <span className="t-quiet"> s</span>
      </span>
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

/** A sanitized default filename for an exported conversation. */
function exportName(title: string): string {
  const clean = title.replace(/[^\w \-]+/g, "").trim().slice(0, 50);
  return `${clean || "conversation"}.md`;
}

/** One session in the rail — open on click, rename on double-click, export/delete on hover. */
function SessionRow({
  c,
  active,
  onOpen,
}: {
  c: ConversationMeta;
  active: boolean;
  onOpen: () => void;
}) {
  const removeConversation = useStore((s) => s.removeConversation);
  const renameConversation = useStore((s) => s.renameConversation);
  const exportConversation = useStore((s) => s.exportConversation);
  const [armedDelete, setArmedDelete] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [text, setText] = useState(c.title);

  const doExport = async (e: MouseEvent) => {
    e.stopPropagation();
    if (!IN_TAURI) return;
    const dest = await save({
      defaultPath: exportName(c.title),
      filters: [{ name: "Markdown", extensions: ["md"] }],
    });
    if (dest) void exportConversation(c.id, "markdown", dest);
  };

  const commitRename = () => {
    setRenaming(false);
    const t = text.trim();
    if (t && t !== c.title) void renameConversation(c.id, t);
  };

  return (
    <div
      className={`session${active ? " session--active" : ""}`}
      onClick={() => !renaming && onOpen()}
      onMouseLeave={() => setArmedDelete(false)}
    >
      {renaming ? (
        <input
          className="session__rename"
          value={text}
          autoFocus
          maxLength={80}
          onClick={(e) => e.stopPropagation()}
          onChange={(e) => setText(e.target.value)}
          onBlur={commitRename}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              commitRename();
            } else if (e.key === "Escape") {
              setRenaming(false);
              setText(c.title);
            }
          }}
        />
      ) : (
        <span
          className="session__title"
          title="Double-click to rename"
          onDoubleClick={(e) => {
            e.stopPropagation();
            setText(c.title);
            setRenaming(true);
          }}
        >
          {c.title}
        </span>
      )}
      <span className="session__meta t-quiet">{relativeTime(c.updatedAt)}</span>
      <div className="session__actions">
        {IN_TAURI && (
          <button
            className="session__act"
            onClick={doExport}
            aria-label="Export conversation"
            title="Export as Markdown"
          >
            <ExportIcon size={12} />
          </button>
        )}
        <button
          className={`ws-delete session__delete${armedDelete ? " ws-delete--armed" : ""}`}
          onClick={(e) => {
            e.stopPropagation();
            if (armedDelete) void removeConversation(c.id);
            else setArmedDelete(true);
          }}
          aria-label={armedDelete ? "Confirm delete" : "Delete session"}
        >
          {armedDelete ? "sure?" : <TrashIcon size={12} />}
        </button>
      </div>
    </div>
  );
}

/** Search results replace the session list while the search box has a query. */
function SearchResults({
  hits,
  query,
  onOpen,
}: {
  hits: SearchHit[];
  query: string;
  onOpen: (id: string) => void;
}) {
  if (hits.length === 0) {
    return <div className="sessions__empty t-quiet">no matches for “{query}”</div>;
  }
  return (
    <div className="search-results">
      {hits.map((h) => (
        <button key={h.id} className="search-hit" onClick={() => onOpen(h.id)}>
          <span className="search-hit__title">{h.title}</span>
          <span className="search-hit__meta t-quiet">
            {relativeTime(h.updatedAt)}
            {h.matches.length > 0 && ` · ${h.matches.length} match${h.matches.length === 1 ? "" : "es"}`}
          </span>
          {h.matches[0] && <span className="search-hit__snippet t-quiet">{h.matches[0].snippet}</span>}
        </button>
      ))}
    </div>
  );
}

/** A single message with hover actions: copy, edit (user), regenerate (last
 *  assistant), and branch. User messages edit inline and resend. */
function ChatBubble({
  m,
  index,
  isLastAssistant,
}: {
  m: ChatMessage;
  index: number;
  isLastAssistant: boolean;
}) {
  const editMessage = useStore((s) => s.editMessage);
  const regenerate = useStore((s) => s.regenerateReply);
  const fork = useStore((s) => s.forkAt);
  const generating = useStore((s) => s.generating);
  const [editing, setEditing] = useState(false);
  const [text, setText] = useState(m.content);
  const [copied, setCopied] = useState(false);

  const copy = () =>
    void navigator.clipboard?.writeText(m.content).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1200);
    });

  if (m.role === "user" && editing) {
    return (
      <div className="msg msg--user msg--editing">
        <textarea
          className="msg__edit"
          value={text}
          autoFocus
          rows={Math.min(10, Math.max(2, text.split("\n").length))}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              setEditing(false);
              void editMessage(index, text);
            } else if (e.key === "Escape") {
              setEditing(false);
              setText(m.content);
            }
          }}
        />
        <div className="msg__edit-actions">
          <button className="btn-quiet" onClick={() => { setEditing(false); setText(m.content); }}>
            Cancel
          </button>
          <button className="btn-lit" onClick={() => { setEditing(false); void editMessage(index, text); }}>
            Save &amp; resend
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className={`msg msg--${m.role}`}>
      {m.role === "assistant" && m.toolSteps?.length > 0 && <ToolSteps steps={m.toolSteps} />}
      {m.role === "assistant" ? <Markdown text={m.content} /> : m.content}
      {m.role === "assistant" && m.sources?.length > 0 && <Sources sources={m.sources} />}
      {m.role === "assistant" && <StatsLine msg={m} />}
      <div className="msg__actions">
        <button className="msg__act" onClick={copy}>
          {copied ? "copied" : "copy"}
        </button>
        {m.role === "user" && !generating && (
          <button className="msg__act" onClick={() => { setText(m.content); setEditing(true); }}>
            edit
          </button>
        )}
        {isLastAssistant && !generating && (
          <button className="msg__act" onClick={() => void regenerate()}>
            regenerate
          </button>
        )}
        <button className="msg__act" onClick={() => void fork(index)} title="Branch a new conversation from here">
          branch
        </button>
      </div>
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
  const liveToolSteps = useStore((s) => s.liveToolSteps);
  const library = useStore((s) => s.library);
  const sendMessage = useStore((s) => s.sendMessage);
  const stopGeneration = useStore((s) => s.stopGeneration);
  const openConversation = useStore((s) => s.openConversation);
  const newSession = useStore((s) => s.newSession);
  const setView = useStore((s) => s.setView);
  const searchHits = useStore((s) => s.searchHits);
  const runSearch = useStore((s) => s.searchConversations);
  const clearSearch = useStore((s) => s.clearSearch);
  const maybeStartCoach = useStore((s) => s.maybeStartCoach);
  const applySystemPrompt = useStore((s) => s.applySystemPrompt);

  const [draft, setDraft] = useState("");
  const [showPrompts, setShowPrompts] = useState(false);
  const [query, setQuery] = useState("");
  const threadRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  const ws = workspaces.find((w) => w.id === activeId) ?? null;
  const model = ws?.activeModel ? library.find((m) => m.sha256 === ws.activeModel) ?? null : null;

  useEffect(() => {
    const el = threadRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [activeConv?.messages.length, streamText]);

  // Debounced search — a big workspace shouldn't scan on every keystroke.
  useEffect(() => {
    const q = query.trim();
    if (!q) {
      clearSearch();
      return;
    }
    const t = setTimeout(() => void runSearch(q), 180);
    return () => clearTimeout(t);
  }, [query, runSearch, clearSearch]);

  // Clear the search when switching workspaces.
  useEffect(() => {
    setQuery("");
    clearSearch();
  }, [activeId, clearSearch]);

  // Once a workspace has a little history, point out that it's all searchable.
  useEffect(() => {
    if (conversations.length >= 3) maybeStartCoach("conversations");
  }, [conversations.length, maybeStartCoach]);

  // With a model in place, point out the prompt library (persona) control.
  useEffect(() => {
    if (ws?.activeModel) maybeStartCoach("prompts");
  }, [ws?.activeModel, maybeStartCoach]);

  // Once a real exchange exists, point out the per-message actions.
  useEffect(() => {
    if ((activeConv?.messages.length ?? 0) >= 2) maybeStartCoach("chatactions");
  }, [activeConv?.messages.length, maybeStartCoach]);

  const openFromSearch = (id: string) => {
    setQuery("");
    clearSearch();
    void openConversation(id);
  };

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
          {conversations.length > 0 && (
            <div className="sessions__search" data-coach="conv-search">
              <SearchIcon size={13} />
              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search conversations…"
                aria-label="Search conversations"
              />
              {query && (
                <button
                  className="sessions__search-clear"
                  onClick={() => setQuery("")}
                  aria-label="Clear search"
                >
                  <CloseIcon size={11} />
                </button>
              )}
            </div>
          )}
          <div className="sessions__list">
            {searchHits !== null ? (
              <SearchResults hits={searchHits} query={query.trim()} onOpen={openFromSearch} />
            ) : conversations.length === 0 ? (
              <div className="sessions__empty t-quiet">no sessions yet</div>
            ) : (
              conversations.map((c) => (
                <SessionRow
                  key={c.id}
                  c={c}
                  active={activeConv?.id === c.id}
                  onOpen={() => void openConversation(c.id)}
                />
              ))
            )}
          </div>
          <button
            className={`sessions__persona${ws.systemPrompt ? " sessions__persona--on" : ""}`}
            data-coach="prompt-lib"
            onClick={() => setShowPrompts(true)}
            title="Choose a system prompt"
          >
            <span className="sessions__persona-dot" />
            {ws.systemPrompt ? "System prompt active" : "Prompt library"}
          </button>
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
              <div className="thread" ref={threadRef} data-coach="thread">
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
                  <ChatBubble
                    key={i}
                    m={m}
                    index={i}
                    isLastAssistant={m.role === "assistant" && i === activeConv.messages.length - 1}
                  />
                ))}
                {generating && (
                  <div className="msg msg--assistant">
                    <RetrievalStrip sources={liveSources} />
                    <ToolSteps steps={liveToolSteps} live />
                    {streamText ? <Markdown text={streamText} /> : null}
                    <span className="caret" aria-hidden="true" />
                  </div>
                )}
              </div>

              <EngineStrip />
              <InferenceHUD />

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
      {showPrompts && (
        <PromptLibrary
          active={ws.systemPrompt ?? null}
          onApply={(body) => void applySystemPrompt(body && body.trim() ? body : null)}
          onDone={() => setShowPrompts(false)}
        />
      )}
    </div>
  );
}

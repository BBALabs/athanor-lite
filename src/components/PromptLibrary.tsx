/**
 * Prompt library — curated system prompts plus your own, searchable, applied to
 * the active workspace in one click. The dark glass stays; a prompt just changes
 * what the assistant is told it is.
 */

import { useEffect, useMemo, useState } from "react";
import { ipc } from "../lib/ipc";
import { SearchIcon, PlusIcon, TrashIcon, CloseIcon } from "./Icons";
import type { Prompt, UserPrompt } from "../lib/types";

interface Draft {
  id: string | null;
  title: string;
  category: string;
  body: string;
}

const EMPTY: Draft = { id: null, title: "", category: "", body: "" };

export function PromptLibrary({
  active,
  onApply,
  onDone,
}: {
  active: string | null;
  onApply: (body: string | null) => void;
  onDone: () => void;
}) {
  const [curated, setCurated] = useState<Prompt[]>([]);
  const [mine, setMine] = useState<UserPrompt[]>([]);
  const [query, setQuery] = useState("");
  const [draft, setDraft] = useState<Draft | null>(null);

  useEffect(() => {
    void ipc.getCuratedPrompts().then((s) => setCurated(s.prompts)).catch(() => {});
    void ipc.listUserPrompts().then(setMine).catch(() => {});
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") (draft ? setDraft(null) : onDone());
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onDone, draft]);

  const q = query.trim().toLowerCase();
  const match = (p: { title: string; category: string; body: string }) =>
    !q ||
    p.title.toLowerCase().includes(q) ||
    p.category.toLowerCase().includes(q) ||
    p.body.toLowerCase().includes(q);

  const filteredMine = useMemo(() => mine.filter(match), [mine, q]);
  const byCategory = useMemo(() => {
    const groups = new Map<string, Prompt[]>();
    for (const p of curated.filter(match)) {
      if (!groups.has(p.category)) groups.set(p.category, []);
      groups.get(p.category)!.push(p);
    }
    return [...groups.entries()];
  }, [curated, q]);

  const apply = (body: string) => {
    onApply(body);
    onDone();
  };

  const saveDraft = async () => {
    if (!draft || !draft.title.trim() || !draft.body.trim()) return;
    setMine(await ipc.saveUserPrompt(draft.id, draft.title, draft.category, draft.body));
    setDraft(null);
  };

  const isActive = (body: string) => active != null && active.trim() === body.trim();

  const Row = ({ p, own }: { p: Prompt | UserPrompt; own?: boolean }) => (
    <div className={`prompt${isActive(p.body) ? " prompt--active" : ""}`}>
      <button className="prompt__main" onClick={() => apply(p.body)}>
        <span className="prompt__head">
          <span className="t-title prompt__title">{p.title}</span>
          <span className="t-quiet prompt__cat">{p.category}</span>
          {isActive(p.body) && <span className="prompt__on">in use</span>}
        </span>
        <span className="t-quiet prompt__body">{p.body}</span>
      </button>
      {own && (
        <div className="prompt__own-actions">
          <button
            className="prompt__act"
            onClick={() => setDraft({ id: p.id, title: p.title, category: p.category, body: p.body })}
            aria-label="Edit prompt"
          >
            edit
          </button>
          <button
            className="prompt__act"
            onClick={() => void ipc.deleteUserPrompt(p.id).then(setMine)}
            aria-label="Delete prompt"
          >
            <TrashIcon size={12} />
          </button>
        </div>
      )}
    </div>
  );

  return (
    <div className="sheet-veil" onClick={onDone}>
      <div className="sheet sheet--gallery prompt-sheet" onClick={(e) => e.stopPropagation()}>
        {draft ? (
          <div className="prompt-editor">
            <div className="t-display">{draft.id ? "Edit prompt" : "New prompt"}</div>
            <input
              className="sheet__name"
              placeholder="Name it…"
              value={draft.title}
              maxLength={80}
              autoFocus
              onChange={(e) => setDraft({ ...draft, title: e.target.value })}
            />
            <input
              className="ds-select prompt-editor__cat"
              placeholder="Category (e.g. Coding)"
              value={draft.category}
              maxLength={32}
              onChange={(e) => setDraft({ ...draft, category: e.target.value })}
            />
            <textarea
              className="prompt-editor__body"
              placeholder="You are…"
              value={draft.body}
              rows={8}
              maxLength={4000}
              onChange={(e) => setDraft({ ...draft, body: e.target.value })}
            />
            <div className="sheet__actions sheet__actions--end">
              <button className="btn-quiet" onClick={() => setDraft(null)}>
                Cancel
              </button>
              <button className="btn-lit" onClick={() => void saveDraft()} disabled={!draft.title.trim() || !draft.body.trim()}>
                Save prompt
              </button>
            </div>
          </div>
        ) : (
          <>
            <div className="prompt-lib__head">
              <div className="t-display">Prompt library</div>
              <div className="prompt-lib__tools">
                <div className="sessions__search prompt-lib__search">
                  <SearchIcon size={13} />
                  <input
                    value={query}
                    onChange={(e) => setQuery(e.target.value)}
                    placeholder="Search prompts…"
                    aria-label="Search prompts"
                  />
                  {query && (
                    <button className="sessions__search-clear" onClick={() => setQuery("")} aria-label="Clear">
                      <CloseIcon size={11} />
                    </button>
                  )}
                </div>
                <button className="btn-quiet" onClick={() => setDraft({ ...EMPTY })}>
                  <PlusIcon size={13} />
                  New
                </button>
              </div>
            </div>

            <div className="prompt-list">
              {active && (
                <button className="prompt-clear t-quiet" onClick={() => apply("")}>
                  Clear the active prompt — go back to the workspace's default
                </button>
              )}
              {filteredMine.length > 0 && (
                <>
                  <div className="t-label prompt-group">Yours</div>
                  {filteredMine.map((p) => (
                    <Row key={p.id} p={p} own />
                  ))}
                </>
              )}
              {byCategory.map(([cat, prompts]) => (
                <div key={cat}>
                  <div className="t-label prompt-group">{cat}</div>
                  {prompts.map((p) => (
                    <Row key={p.id} p={p} />
                  ))}
                </div>
              ))}
              {filteredMine.length === 0 && byCategory.length === 0 && (
                <div className="sessions__empty t-quiet">no prompts match “{query}”</div>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

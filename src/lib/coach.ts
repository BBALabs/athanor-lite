/**
 * Contextual walkthroughs — the app teaching itself by doing, not by reading.
 * Each walkthrough is a short sequence of steps that spotlight a *real* control
 * and say one plain sentence about it. They fire the first time a user reaches
 * a feature, are always skippable, and never show twice (seen-set persisted in
 * the data root). Steps point at live elements via a `data-coach` attribute so
 * the tour survives markup refactors that don't touch those anchors.
 */

export type CoachPlacement = "top" | "bottom" | "left" | "right" | "center";

export interface CoachStep {
  /** `data-coach` value of the element to spotlight. Omit for a centered card. */
  target?: string;
  title: string;
  body: string;
  /** Where the callout sits relative to the target. Defaults to "bottom". */
  placement?: CoachPlacement;
  /** Teach-by-doing: also advance when the user actually clicks the target. */
  advanceOnClick?: boolean;
}

export interface Walkthrough {
  id: string;
  steps: CoachStep[];
}

/**
 * The registry. Keyed by id; a view calls `maybeStartCoach(id)` on first entry.
 * Anchors are `data-coach="…"` attributes on the real controls.
 */
export const WALKTHROUGHS: Record<string, Walkthrough> = {
  workspaces: {
    id: "workspaces",
    steps: [
      {
        title: "Workspaces are your AI setups",
        body: "Each one is a self-contained stack — its own model, documents, memory, and tools — that you switch between like projects in an editor.",
        placement: "center",
      },
      {
        target: "new-workspace",
        title: "Start from a ready-made setup",
        body: "Pick a starting point — Code Assistant, Document Reviewer, and more — and it arrives pre-configured with a fitting model and the right defaults.",
        placement: "bottom",
        advanceOnClick: true,
      },
    ],
  },
  training: {
    id: "training",
    steps: [
      {
        title: "Fine-tune on your own data",
        body: "Getting your data clean is the hard part — so start there. Drop a JSONL file and Athanor validates every example on your machine before saving it.",
        placement: "center",
      },
      {
        target: "ds-drop",
        title: "Drop a training set",
        body: "Chat turns, instruction/output, or prompt/completion — any of the common shapes. You'll see exactly what's valid, what was skipped, and roughly how many tokens it is.",
        placement: "bottom",
        advanceOnClick: true,
      },
      {
        target: "ds-train",
        title: "Set up the run",
        body: "Pick a base model, a dataset, and the LoRA settings. The training run itself needs a runtime we don't bundle yet — we'll tell you straight, and your prepared data will be ready.",
        placement: "top",
      },
    ],
  },
  settings: {
    id: "settings",
    steps: [
      {
        target: "settings-appearance",
        title: "Make it yours",
        body: "Recolor the accent, replay any walkthrough, manage the local API key, and open your data folder — it all lives here. The dark glass never changes.",
        placement: "right",
      },
    ],
  },
  models: {
    id: "models",
    steps: [
      {
        title: "Every model, matched to your machine",
        body: "The jewels beside each model show how well each version runs here — filled violet fits comfortably, amber is tight, dim won't fit. Click any row for the full breakdown.",
        placement: "center",
      },
      {
        target: "model-filters",
        title: "Filter and sort to find yours",
        body: "Narrow by what you need — coding, reasoning — and sort by capability, size, or name. Downloaded models show their size and can be deleted to reclaim disk.",
        placement: "bottom",
      },
    ],
  },
  conversations: {
    id: "conversations",
    steps: [
      {
        target: "conv-search",
        title: "Find anything you've discussed",
        body: "Search runs across every conversation in this workspace — titles and message text — and jumps you straight to the match. Double-click a session to rename it, or use the export icon to save one as Markdown.",
        placement: "right",
      },
    ],
  },
  knowledge: {
    id: "knowledge",
    steps: [
      {
        title: "Give this workspace a memory",
        body: "Add your own documents and this assistant will answer from them — grounded in your files, still fully on-device. Takes about a minute.",
        placement: "center",
      },
      {
        target: "kb-drop",
        title: "Add your first document",
        body: "Drop a PDF, Word doc, or text file here — or click to browse. Athanor reads it on your machine and never uploads a byte.",
        placement: "bottom",
        advanceOnClick: true,
      },
      {
        target: "kb-retrieval",
        title: "Retrieval stays in your control",
        body: "With this on, every chat pulls the most relevant passages from these documents — and shows you exactly which ones it used.",
        placement: "left",
      },
    ],
  },
};

export function getWalkthrough(id: string): Walkthrough | null {
  return WALKTHROUGHS[id] ?? null;
}

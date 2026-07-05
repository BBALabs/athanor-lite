import { create } from "zustand";
import { ipc } from "../lib/ipc";
import { applyAccent } from "../lib/theme";
import {
  isAthanorError,
  type Catalog,
  type Conversation,
  type ConversationMeta,
  type DownloadProgress,
  type HardwareReport,
  type LibraryModel,
  type Operation,
  type RecommendationSet,
  type RuntimeState,
  type ServerStatus,
  type SearchHit,
  type TelemetrySample,
  type WorkspaceList,
} from "../lib/types";

export type View = "chat" | "dashboard" | "models" | "library";

const PENDING_CONV = "pending";
type BootPhase = "booting" | "ready" | "error";
type Subsystem = "hardware" | "catalog" | "recommendations" | "workspaces" | "telemetry";

/** One entry in the boot ticker — real steps, real completion. */
interface BootStep {
  label: string;
  state: "pending" | "running" | "done" | "failed";
}

const TELEMETRY_WINDOW = 120; // samples ≈ 2 minutes at 1 Hz

interface AthanorStore {
  boot: BootPhase;
  bootSteps: BootStep[];
  bootError: string | null;
  /** Subsystems that failed to initialize — the app runs degraded, not dead. */
  degraded: Subsystem[];

  view: View;
  hardware: HardwareReport | null;
  recommendations: RecommendationSet | null;
  catalog: Catalog | null;
  telemetry: TelemetrySample[];
  workspaces: WorkspaceList;
  library: LibraryModel[];
  /** Live download progress, keyed by artifact sha256. */
  downloads: Record<string, DownloadProgress>;
  /** Non-fatal, user-visible operation failure (workspace ops etc.). */
  lastOpError: string | null;

  // Chat
  conversations: ConversationMeta[];
  /** Live results for the sessions-rail search, or null when not searching. */
  searchHits: SearchHit[] | null;
  activeConv: Conversation | null;
  /** Assistant text accumulating during a live generation. */
  streamText: string | null;
  generating: boolean;
  runtimeState: RuntimeState | null;
  serverStatus: ServerStatus | null;
  /** Everything running (or failed and awaiting attention), newest first. */
  operations: Operation[];
  opsOpen: boolean;

  /** App-wide accent family id (persisted preference). */
  accent: string;

  setView: (v: View) => void;
  /**
   * One-click "run this model": makes sure a chat workspace exists,
   * binds the model to it, pre-warms the engine, and lands in Chat.
   */
  launchChat: (sha256: string) => Promise<void>;
  setOpsOpen: (open: boolean) => void;
  cancelOperation: (id: string) => Promise<void>;
  dismissOperation: (id: string) => Promise<void>;
  retryOperation: (id: string) => Promise<void>;
  init: () => Promise<void>;
  retryHardware: () => Promise<void>;
  startDownload: (entryId: string, quant: string) => Promise<void>;
  cancelDownload: (sha256: string) => Promise<void>;
  deleteModel: (sha256: string) => Promise<void>;
  loadConversations: () => Promise<void>;
  openConversation: (id: string) => Promise<void>;
  newSession: () => void;
  sendMessage: (text: string) => Promise<void>;
  stopGeneration: () => Promise<void>;
  removeConversation: (id: string) => Promise<void>;
  renameConversation: (id: string, title: string) => Promise<void>;
  regenerateReply: () => Promise<void>;
  editMessage: (index: number, content: string) => Promise<void>;
  forkAt: (index: number) => Promise<void>;
  exportConversation: (id: string, format: "markdown" | "json", dest: string) => Promise<void>;
  searchConversations: (query: string) => Promise<void>;
  clearSearch: () => void;
  chooseWorkspaceModel: (sha256: string | null) => Promise<void>;
  clearOpError: () => void;

  // Preferences
  setAccent: (id: string) => Promise<void>;
}

function errText(e: unknown): string {
  if (isAthanorError(e)) return `${e.code}: ${e.message}`;
  return e instanceof Error ? e.message : String(e);
}

let telemetryBound = false;
let initStarted = false; // React StrictMode double-fires effects in dev; boot once.

export const useStore = create<AthanorStore>((set, get) => ({
  boot: "booting",
  bootSteps: [
    { label: "shell online", state: "done" },
    { label: "probing hardware", state: "pending" },
    { label: "loading model catalog", state: "pending" },
    { label: "computing recommendations", state: "pending" },
    { label: "mounting workspaces", state: "pending" },
    { label: "telemetry stream", state: "pending" },
  ],
  bootError: null,
  degraded: [],

  view: "dashboard",
  hardware: null,
  recommendations: null,
  catalog: null,
  telemetry: [],
  workspaces: { workspaces: [], activeId: null, damaged: [] },
  library: [],
  downloads: {},
  lastOpError: null,
  conversations: [],
  searchHits: null,
  activeConv: null,
  streamText: null,
  generating: false,
  runtimeState: null,
  serverStatus: null,
  operations: [],
  opsOpen: false,
  accent: "violet",

  setView: (view) => set({ view }),

  launchChat: async (sha256) => {
    try {
      const s = get();
      let ws =
        s.workspaces.workspaces.find((w) => w.id === s.workspaces.activeId) ??
        s.workspaces.workspaces[0] ??
        null;
      if (!ws) {
        ws = await ipc.createWorkspace({ name: "My Chat", purpose: "", accentHue: 275, glyph: "M" });
      }
      await ipc.setWorkspaceModel(ws.id, sha256);
      if (get().workspaces.activeId !== ws.id) await ipc.activateWorkspace(ws.id);
      set({
        workspaces: await ipc.listWorkspaces(),
        view: "chat",
        activeConv: null,
        streamText: null,
      });
      await get().loadConversations();
      // Pre-warm the engine so the first reply starts fast. Quiet on purpose:
      // a duplicate-start is harmless, and a real failure surfaces in the
      // Operations drawer and again on the first send.
      void ipc.startEngine(ws.id).catch(() => {});
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },
  setOpsOpen: (opsOpen) => set({ opsOpen }),

  cancelOperation: async (id) => {
    try {
      await ipc.cancelOperation(id);
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  dismissOperation: async (id) => {
    try {
      await ipc.dismissOperation(id);
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  retryOperation: async (id) => {
    try {
      await ipc.retryOperation(id);
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  init: async () => {
    if (initStarted) return;
    initStarted = true;

    const step = (label: string, state: BootStep["state"]) =>
      set((s) => ({
        bootSteps: s.bootSteps.map((b) => (b.label === label ? { ...b, state } : b)),
      }));
    const degrade = (sub: Subsystem, e: unknown) => {
      console.error(`boot: ${sub} unavailable`, e);
      set((s) => ({ degraded: [...s.degraded, sub] }));
    };

    // Every subsystem initializes independently. A broken WMI service must not
    // stop the user from browsing the catalog; a torn workspace dir must not
    // hide the hardware dashboard. The fatal screen is for fatal states only.
    step("probing hardware", "running");
    try {
      const hardware = await ipc.detectHardware();
      set({ hardware });
      step("probing hardware", "done");
    } catch (e) {
      degrade("hardware", e);
      step("probing hardware", "failed");
    }

    step("loading model catalog", "running");
    try {
      set({ catalog: await ipc.getModelCatalog() });
      step("loading model catalog", "done");
    } catch (e) {
      degrade("catalog", e);
      step("loading model catalog", "failed");
    }

    step("computing recommendations", "running");
    const hw = get().hardware;
    if (hw) {
      try {
        set({ recommendations: await ipc.getRecommendations(hw) });
        step("computing recommendations", "done");
      } catch (e) {
        degrade("recommendations", e);
        step("computing recommendations", "failed");
      }
    } else {
      // No hardware profile — nothing to recommend against.
      step("computing recommendations", "failed");
    }

    step("mounting workspaces", "running");
    try {
      set({ workspaces: await ipc.listWorkspaces() });
      step("mounting workspaces", "done");
    } catch (e) {
      degrade("workspaces", e);
      step("mounting workspaces", "failed");
    }

    step("telemetry stream", "running");
    try {
      if (!telemetryBound) {
        telemetryBound = true;
        await ipc.onTelemetry((sample) =>
          set((s) => ({
            telemetry: [...s.telemetry.slice(-(TELEMETRY_WINDOW - 1)), sample],
          })),
        );
      }
      step("telemetry stream", "done");
    } catch (e) {
      degrade("telemetry", e);
      step("telemetry stream", "failed");
    }

    // Model library + download progress stream (quiet — no boot step; a fresh
    // install legitimately has an empty library).
    try {
      set({ library: await ipc.listLibrary() });
      await ipc.onDownloadProgress((p) => {
        set((s) => ({ downloads: { ...s.downloads, [p.sha256]: p } }));
        if (p.state === "done") {
          void ipc
            .listLibrary()
            .then((library) => set({ library }))
            .catch(() => {});
        }
      });
    } catch (e) {
      console.error("library unavailable", e);
    }

    // Chat + engine event streams.
    try {
      await ipc.onChatDelta((d) => {
        set((s) => {
          if (!s.generating) return {};
          let activeConv = s.activeConv;
          // Adopt the real id for a conversation created by this send.
          if (activeConv && activeConv.id === PENDING_CONV) {
            activeConv = { ...activeConv, id: d.conversationId };
          }
          if (!activeConv || activeConv.id !== d.conversationId) return {};
          return { activeConv, streamText: (s.streamText ?? "") + d.delta };
        });
      });
      await ipc.onChatDone((d) => {
        if (d.error) set({ lastOpError: d.error });
      });
      await ipc.onRuntimeState((runtimeState) => set({ runtimeState }));
      await ipc.onServerStatus((serverStatus) => set({ serverStatus }));
      await ipc.onOpsChanged((operations) => set({ operations }));
      set({ operations: await ipc.listOperations() });
      const ws = get().workspaces;
      if (ws.activeId) {
        await get().loadConversations();
      }
    } catch (e) {
      console.error("chat streams unavailable", e);
    }

    // App preferences (accent) — apply before first paint of any view.
    try {
      const prefs = await ipc.getPreferences();
      set({ accent: prefs.accent });
      applyAccent(prefs.accent);
    } catch {
      /* default violet stands */
    }

    const { degraded } = get();
    // Truly fatal only when nothing at all came up (e.g. the IPC bridge is gone).
    if (degraded.length >= 5) {
      set({
        boot: "error",
        bootError: "no subsystem could initialize — see the log file for details",
      });
    } else {
      set({ boot: "ready" });
    }
  },

  retryHardware: async () => {
    try {
      const hardware = await ipc.detectHardware();
      const recommendations = await ipc.getRecommendations(hardware);
      set((s) => ({
        hardware,
        recommendations,
        degraded: s.degraded.filter((d) => d !== "hardware" && d !== "recommendations"),
      }));
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  startDownload: async (entryId, quant) => {
    try {
      await ipc.startDownload(entryId, quant);
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  cancelDownload: async (sha256) => {
    try {
      await ipc.cancelDownload(sha256);
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  deleteModel: async (sha256) => {
    try {
      const library = await ipc.deleteModel(sha256);
      set((s) => {
        const downloads = { ...s.downloads };
        delete downloads[sha256];
        return { library, downloads };
      });
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  loadConversations: async () => {
    const wsId = get().workspaces.activeId;
    if (!wsId) {
      set({ conversations: [], activeConv: null });
      return;
    }
    try {
      set({ conversations: await ipc.listConversations(wsId) });
    } catch (e) {
      console.error("conversations unavailable", e);
      set({ conversations: [] });
    }
  },

  openConversation: async (id) => {
    const wsId = get().workspaces.activeId;
    if (!wsId) return;
    try {
      set({ activeConv: await ipc.getConversation(wsId, id), streamText: null });
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  newSession: () => set({ activeConv: null, streamText: null }),

  sendMessage: async (text) => {
    const s = get();
    const wsId = s.workspaces.activeId;
    if (!wsId || s.generating || !text.trim()) return;

    const now = new Date().toISOString();
    const base: Conversation = s.activeConv ?? {
      schema: 1,
      id: PENDING_CONV,
      workspaceId: wsId,
      title: text.trim().slice(0, 48),
      modelSha: null,
      createdAt: now,
      updatedAt: now,
      messages: [],
    };
    const optimistic: Conversation = {
      ...base,
      messages: [
        ...base.messages,
        { role: "user", content: text, ts: now, stats: null },
      ],
      updatedAt: now,
    };
    set({ activeConv: optimistic, generating: true, streamText: "" });

    const convId = base.id === PENDING_CONV ? null : base.id;
    try {
      const realId = await ipc.chatSend(wsId, convId, text);
      const [conv, conversations] = await Promise.all([
        ipc.getConversation(wsId, realId),
        ipc.listConversations(wsId),
      ]);
      set({ activeConv: conv, conversations, generating: false, streamText: null });
    } catch (e) {
      set({ generating: false, streamText: null, lastOpError: errText(e) });
      // Reload from disk — the user turn was persisted before generation.
      const active = get().activeConv;
      if (active && active.id !== PENDING_CONV) {
        try {
          set({ activeConv: await ipc.getConversation(wsId, active.id) });
        } catch {
          /* keep optimistic state */
        }
      }
    }
  },

  stopGeneration: async () => {
    const conv = get().activeConv;
    if (conv) {
      try {
        await ipc.cancelGeneration(conv.id);
      } catch (e) {
        set({ lastOpError: errText(e) });
      }
    }
  },

  removeConversation: async (id) => {
    const wsId = get().workspaces.activeId;
    if (!wsId) return;
    try {
      const conversations = await ipc.deleteConversation(wsId, id);
      set((s) => ({
        conversations,
        activeConv: s.activeConv?.id === id ? null : s.activeConv,
      }));
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  regenerateReply: async () => {
    const s = get();
    const conv = s.activeConv;
    const wsId = s.workspaces.activeId;
    if (!conv || !wsId || conv.id === PENDING_CONV || s.generating) return;
    const msgs = [...conv.messages];
    while (msgs.length && msgs[msgs.length - 1].role === "assistant") msgs.pop();
    set({
      activeConv: { ...conv, messages: msgs },
      generating: true,
      streamText: "",
    });
    try {
      await ipc.regenerateReply(wsId, conv.id);
      const [c, list] = await Promise.all([
        ipc.getConversation(wsId, conv.id),
        ipc.listConversations(wsId),
      ]);
      set({ activeConv: c, conversations: list, generating: false, streamText: null });
    } catch (e) {
      set({ generating: false, streamText: null, lastOpError: errText(e) });
      try {
        set({ activeConv: await ipc.getConversation(wsId, conv.id) });
      } catch {
        /* keep optimistic */
      }
    }
  },

  editMessage: async (index, content) => {
    const s = get();
    const conv = s.activeConv;
    const wsId = s.workspaces.activeId;
    if (!conv || !wsId || conv.id === PENDING_CONV || s.generating || !content.trim()) return;
    const msgs = conv.messages.slice(0, index + 1);
    msgs[index] = { ...msgs[index], content };
    set({
      activeConv: { ...conv, messages: msgs },
      generating: true,
      streamText: "",
    });
    try {
      await ipc.editAndResend(wsId, conv.id, index, content);
      const [c, list] = await Promise.all([
        ipc.getConversation(wsId, conv.id),
        ipc.listConversations(wsId),
      ]);
      set({ activeConv: c, conversations: list, generating: false, streamText: null });
    } catch (e) {
      set({ generating: false, streamText: null, lastOpError: errText(e) });
      try {
        set({ activeConv: await ipc.getConversation(wsId, conv.id) });
      } catch {
        /* keep optimistic */
      }
    }
  },

  forkAt: async (index) => {
    const s = get();
    const conv = s.activeConv;
    const wsId = s.workspaces.activeId;
    if (!conv || !wsId || conv.id === PENDING_CONV) return;
    try {
      const newId = await ipc.forkConversation(wsId, conv.id, index);
      const [c, list] = await Promise.all([
        ipc.getConversation(wsId, newId),
        ipc.listConversations(wsId),
      ]);
      set({ activeConv: c, conversations: list });
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  renameConversation: async (id, title) => {
    const wsId = get().workspaces.activeId;
    if (!wsId) return;
    try {
      const conversations = await ipc.renameConversation(wsId, id, title);
      set((s) => ({
        conversations,
        activeConv:
          s.activeConv?.id === id
            ? { ...s.activeConv, title: title.trim().slice(0, 80) || "Untitled" }
            : s.activeConv,
      }));
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  exportConversation: async (id, format, dest) => {
    const wsId = get().workspaces.activeId;
    if (!wsId) return;
    try {
      await ipc.exportConversation(wsId, id, format, dest);
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  searchConversations: async (query) => {
    const wsId = get().workspaces.activeId;
    const q = query.trim();
    if (!wsId || !q) {
      set({ searchHits: null }); // empty query exits search mode
      return;
    }
    // Enter search mode immediately (non-null) so results can render.
    set((s) => ({ searchHits: s.searchHits ?? [] }));
    try {
      const hits = await ipc.searchConversations(wsId, q);
      // Ignore a stale response if the box was cleared while it was in flight.
      if (get().searchHits !== null) set({ searchHits: hits });
    } catch (e) {
      set({ lastOpError: errText(e), searchHits: [] });
    }
  },

  clearSearch: () => set({ searchHits: null }),

  chooseWorkspaceModel: async (sha256) => {
    const wsId = get().workspaces.activeId;
    if (!wsId) return;
    try {
      await ipc.setWorkspaceModel(wsId, sha256);
      set({ workspaces: await ipc.listWorkspaces() });
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  clearOpError: () => set({ lastOpError: null }),

  setAccent: async (id) => {
    applyAccent(id); // paint immediately — zero perceived latency
    set({ accent: id });
    try {
      await ipc.setAccent(id);
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },
}));

/** Latest telemetry sample, or null before the stream warms up. */
export function useLatestSample(): TelemetrySample | null {
  return useStore((s) => (s.telemetry.length ? s.telemetry[s.telemetry.length - 1] : null));
}

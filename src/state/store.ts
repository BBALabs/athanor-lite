import { create } from "zustand";
import { ipc } from "../lib/ipc";
import {
  isAthanorError,
  type Catalog,
  type Conversation,
  type ConversationMeta,
  type DownloadProgress,
  type HardwareReport,
  type KnowledgeBase,
  type LibraryModel,
  type McpServerView,
  type Operation,
  type RecommendationSet,
  type RuntimeState,
  type ServerStatus,
  type Source,
  type TelemetrySample,
  type ToolStep,
  type Workspace,
  type WorkspaceList,
} from "../lib/types";

export type View = "chat" | "knowledge" | "dashboard" | "models" | "workspaces";

const PENDING_CONV = "pending";
export type BootPhase = "booting" | "ready" | "error";
export type Subsystem = "hardware" | "catalog" | "recommendations" | "workspaces" | "telemetry";

/** One entry in the boot ticker — real steps, real completion. */
export interface BootStep {
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
  activeConv: Conversation | null;
  /** Assistant text accumulating during a live generation. */
  streamText: string | null;
  generating: boolean;
  runtimeState: RuntimeState | null;
  serverStatus: ServerStatus | null;
  onboardingNeeded: boolean;
  /** Everything running (or failed and awaiting attention), newest first. */
  operations: Operation[];
  opsOpen: boolean;

  // Knowledge base + MCP (per active workspace)
  knowledge: KnowledgeBase | null;
  mcpServers: McpServerView[];
  /** Live retrieval sources for the in-flight generation, before it finishes. */
  liveSources: Source[];
  /** Tool calls the model made during the in-flight turn, in call order. */
  liveToolSteps: ToolStep[];

  setView: (v: View) => void;
  dismissOnboarding: () => void;
  setOpsOpen: (open: boolean) => void;
  loadKnowledge: () => Promise<void>;
  addDocuments: (paths: string[]) => Promise<void>;
  removeDocument: (docId: string) => Promise<void>;
  cancelIndexing: (docId: string) => Promise<void>;
  setRetrievalEnabled: (enabled: boolean) => Promise<void>;
  loadMcpServers: () => Promise<void>;
  saveMcpServer: (config: McpServerView["config"]) => Promise<void>;
  connectMcpServer: (serverId: string) => Promise<void>;
  disconnectMcpServer: (serverId: string) => Promise<void>;
  removeMcpServer: (serverId: string) => Promise<void>;
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
  chooseWorkspaceModel: (sha256: string | null) => Promise<void>;
  createWorkspace: (args: {
    name: string;
    purpose: string;
    accentHue: number;
    glyph: string;
  }) => Promise<Workspace | null>;
  activateWorkspace: (id: string) => Promise<void>;
  deleteWorkspace: (id: string) => Promise<void>;
  clearOpError: () => void;
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
  activeConv: null,
  streamText: null,
  generating: false,
  runtimeState: null,
  serverStatus: null,
  onboardingNeeded: false,
  operations: [],
  opsOpen: false,
  knowledge: null,
  mcpServers: [],
  liveSources: [],
  liveToolSteps: [],

  setView: (view) => set({ view }),
  dismissOnboarding: () => set({ onboardingNeeded: false }),
  setOpsOpen: (opsOpen) => set({ opsOpen }),

  loadKnowledge: async () => {
    const wsId = get().workspaces.activeId;
    if (!wsId) {
      set({ knowledge: null });
      return;
    }
    try {
      set({ knowledge: await ipc.getKnowledgeBase(wsId) });
    } catch (e) {
      console.error("knowledge base unavailable", e);
    }
  },

  addDocuments: async (paths) => {
    const wsId = get().workspaces.activeId;
    if (!wsId || paths.length === 0) return;
    try {
      // Fire and forget — progress arrives via the operations registry; the
      // knowledge base refreshes as each document's op completes.
      void ipc.addDocuments(wsId, paths);
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  removeDocument: async (docId) => {
    const wsId = get().workspaces.activeId;
    if (!wsId) return;
    try {
      set({ knowledge: await ipc.removeDocument(wsId, docId) });
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  cancelIndexing: async (docId) => {
    const wsId = get().workspaces.activeId;
    if (!wsId) return;
    try {
      await ipc.cancelIndexing(wsId, docId);
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  setRetrievalEnabled: async (enabled) => {
    const wsId = get().workspaces.activeId;
    if (!wsId) return;
    try {
      set({ knowledge: await ipc.setRetrievalEnabled(wsId, enabled) });
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  loadMcpServers: async () => {
    const wsId = get().workspaces.activeId;
    if (!wsId) {
      set({ mcpServers: [] });
      return;
    }
    try {
      set({ mcpServers: await ipc.listMcpServers(wsId) });
    } catch (e) {
      console.error("mcp servers unavailable", e);
    }
  },

  saveMcpServer: async (config) => {
    const wsId = get().workspaces.activeId;
    if (!wsId) return;
    try {
      set({ mcpServers: await ipc.saveMcpServer(wsId, config) });
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  connectMcpServer: async (serverId) => {
    const wsId = get().workspaces.activeId;
    if (!wsId) return;
    try {
      await ipc.connectMcpServer(wsId, serverId);
      set({ mcpServers: await ipc.listMcpServers(wsId) });
    } catch (e) {
      set({ lastOpError: errText(e) });
      void get().loadMcpServers();
    }
  },

  disconnectMcpServer: async (serverId) => {
    const wsId = get().workspaces.activeId;
    if (!wsId) return;
    try {
      await ipc.disconnectMcpServer(serverId);
      set({ mcpServers: await ipc.listMcpServers(wsId) });
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  removeMcpServer: async (serverId) => {
    const wsId = get().workspaces.activeId;
    if (!wsId) return;
    try {
      set({ mcpServers: await ipc.removeMcpServer(wsId, serverId) });
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

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
        set({ liveSources: [], liveToolSteps: [] });
      });
      await ipc.onChatRetrieval((r) => {
        // Only for the active conversation's in-flight turn.
        if (get().activeConv?.id === r.conversationId || get().generating) {
          set({ liveSources: r.sources });
        }
      });
      await ipc.onChatTool((t) => {
        // Append each autonomous tool call as it happens, in order.
        if (!get().generating) return;
        set((s) => ({ liveToolSteps: [...s.liveToolSteps, t.step] }));
      });
      await ipc.onRuntimeState((runtimeState) => set({ runtimeState }));
      await ipc.onServerStatus((serverStatus) => set({ serverStatus }));
      let prevOps: Operation[] = [];
      await ipc.onOpsChanged((operations) => {
        // When an index/import op clears, the knowledge base changed — refresh.
        const hadIndex = prevOps.some((o) => o.kind === "index" || o.kind === "import");
        const hasIndex = operations.some((o) => o.kind === "index" || o.kind === "import");
        if (hadIndex && !hasIndex) void get().loadKnowledge();
        prevOps = operations;
        set({ operations });
      });
      set({ operations: await ipc.listOperations() });
      const ws = get().workspaces;
      if (ws.activeId) {
        await get().loadConversations();
        void get().loadKnowledge();
        void get().loadMcpServers();
      }
    } catch (e) {
      console.error("chat streams unavailable", e);
    }

    try {
      const needed = await ipc.onboardingNeeded();
      const s = get();
      // Only greet a genuinely fresh install — existing state means the user
      // already knows the app.
      set({
        onboardingNeeded:
          needed && s.workspaces.workspaces.length === 0 && s.library.length === 0,
      });
      if (!needed && s.workspaces.workspaces.length > 0) {
        set({ view: "chat" });
      }
    } catch {
      /* onboarding is optional sugar */
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
        { role: "user", content: text, ts: now, stats: null, sources: [], toolSteps: [] },
      ],
      updatedAt: now,
    };
    set({ activeConv: optimistic, generating: true, streamText: "", liveSources: [], liveToolSteps: [] });

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

  createWorkspace: async (args) => {
    try {
      const ws = await ipc.createWorkspace(args);
      set({ workspaces: await ipc.listWorkspaces() });
      await get().loadConversations();
      return ws;
    } catch (e) {
      set({ lastOpError: errText(e) });
      return null;
    }
  },

  activateWorkspace: async (id) => {
    // Optimistic — switching must feel instant; reconcile after.
    const prior = get().workspaces;
    set((s) => ({
      workspaces: { ...s.workspaces, activeId: id },
      activeConv: null,
      streamText: null,
      conversations: [],
    }));
    try {
      await ipc.activateWorkspace(id);
      set({ workspaces: await ipc.listWorkspaces() });
      await get().loadConversations();
    } catch (e) {
      // Double failure (reconcile also failed) must not escape or corrupt state.
      try {
        set({ workspaces: await ipc.listWorkspaces() });
      } catch {
        set({ workspaces: prior });
      }
      set({ lastOpError: errText(e) });
    }
  },

  deleteWorkspace: async (id) => {
    try {
      const workspaces = await ipc.deleteWorkspace(id);
      set({ workspaces });
    } catch (e) {
      set({ lastOpError: errText(e) });
    }
  },

  clearOpError: () => set({ lastOpError: null }),
}));

/** Latest telemetry sample, or null before the stream warms up. */
export function useLatestSample(): TelemetrySample | null {
  return useStore((s) => (s.telemetry.length ? s.telemetry[s.telemetry.length - 1] : null));
}

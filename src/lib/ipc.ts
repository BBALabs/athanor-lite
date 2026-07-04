/**
 * Typed wrappers over the Tauri IPC surface — the only place `invoke` appears.
 * In a plain browser tab (UI design work) the design harness stands in for the
 * Rust core; inside the desktop app the harness is unreachable.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { IN_TAURI } from "./tauriEnv";
import { harnessIpc } from "./designHarness";
import type {
  ApiInfo,
  Catalog,
  ChatDelta,
  ChatDone,
  ChatRetrieval,
  ChatToolEvent,
  CoachState,
  Conversation,
  Preferences,
  SearchHit,
  ConversationMeta,
  KnowledgeBase,
  McpServerConfig,
  McpServerView,
  Source,
  DownloadProgress,
  HardwareReport,
  ImportReport,
  LibraryModel,
  MetricsRecord,
  MetricsSettings,
  OllamaStatus,
  Operation,
  RecommendationSet,
  RuntimeState,
  ServerStatus,
  TelemetrySample,
  TemplateSet,
  Workspace,
  WorkspaceList,
} from "./types";

const tauriIpc = {
  detectHardware: () => invoke<HardwareReport>("detect_hardware"),

  getRecommendations: (report: HardwareReport) =>
    invoke<RecommendationSet>("get_recommendations", { report }),

  getModelCatalog: () => invoke<Catalog>("get_model_catalog"),

  listWorkspaces: () => invoke<WorkspaceList>("list_workspaces"),

  createWorkspace: (args: {
    name: string;
    purpose: string;
    accentHue: number;
    glyph: string;
    templateId?: string | null;
  }) => invoke<Workspace>("create_workspace", args),

  getTemplates: () => invoke<TemplateSet>("get_templates"),

  activateWorkspace: (id: string) =>
    invoke<Workspace>("activate_workspace", { id }),

  setWorkspaceModel: (id: string, sha256: string | null) =>
    invoke<Workspace>("set_workspace_model", { id, sha256 }),

  deleteWorkspace: (id: string) =>
    invoke<WorkspaceList>("delete_workspace", { id }),

  onTelemetry: (handler: (s: TelemetrySample) => void): Promise<UnlistenFn> =>
    listen<TelemetrySample>("telemetry://sample", (e) => handler(e.payload)),

  startDownload: (entryId: string, quant: string) =>
    invoke<void>("start_download", { entryId, quant }),

  cancelDownload: (sha256: string) => invoke<void>("cancel_download", { sha256 }),

  listLibrary: () => invoke<LibraryModel[]>("list_library"),

  deleteModel: (sha256: string) => invoke<LibraryModel[]>("delete_model", { sha256 }),

  onDownloadProgress: (handler: (p: DownloadProgress) => void): Promise<UnlistenFn> =>
    listen<DownloadProgress>("download://progress", (e) => handler(e.payload)),

  chatSend: (workspaceId: string, conversationId: string | null, message: string) =>
    invoke<string>("chat_send", { workspaceId, conversationId, message }),

  cancelGeneration: (conversationId: string) =>
    invoke<void>("cancel_generation", { conversationId }),

  listConversations: (workspaceId: string) =>
    invoke<ConversationMeta[]>("list_conversations", { workspaceId }),

  getConversation: (workspaceId: string, conversationId: string) =>
    invoke<Conversation>("get_conversation", { workspaceId, conversationId }),

  deleteConversation: (workspaceId: string, conversationId: string) =>
    invoke<ConversationMeta[]>("delete_conversation", { workspaceId, conversationId }),

  renameConversation: (workspaceId: string, conversationId: string, title: string) =>
    invoke<ConversationMeta[]>("rename_conversation", { workspaceId, conversationId, title }),

  searchConversations: (workspaceId: string, query: string) =>
    invoke<SearchHit[]>("search_conversations", { workspaceId, query }),

  exportConversation: (
    workspaceId: string,
    conversationId: string,
    format: "markdown" | "json",
    dest: string,
  ) => invoke<void>("export_conversation", { workspaceId, conversationId, format, dest }),

  stopEngine: () => invoke<void>("stop_engine"),

  getMetricsSettings: () => invoke<MetricsSettings>("get_metrics_settings"),
  setMetricsShare: (share: boolean) => invoke<MetricsSettings>("set_metrics_share", { share }),
  getMetricsHistory: (limit: number) => invoke<MetricsRecord[]>("get_metrics_history", { limit }),
  getMetricsSample: () => invoke<unknown>("get_metrics_sample"),

  listOperations: () => invoke<Operation[]>("list_operations"),
  cancelOperation: (id: string) => invoke<void>("cancel_operation", { id }),
  dismissOperation: (id: string) => invoke<void>("dismiss_operation", { id }),
  retryOperation: (id: string) => invoke<void>("retry_operation", { id }),
  onOpsChanged: (handler: (ops: Operation[]) => void): Promise<UnlistenFn> =>
    listen<Operation[]>("ops://changed", (e) => handler(e.payload)),

  getOllamaStatus: () => invoke<OllamaStatus>("get_ollama_status"),
  importOllama: () => invoke<ImportReport>("import_ollama"),
  getApiInfo: () => invoke<ApiInfo>("get_api_info"),
  setApiExpose: (expose: boolean) => invoke<ApiInfo>("set_api_expose", { expose }),
  startEngine: (workspaceId: string) => invoke<void>("start_engine", { workspaceId }),
  onboardingNeeded: () => invoke<boolean>("onboarding_needed"),
  setOnboarded: () => invoke<void>("set_onboarded"),
  getCoachState: () => invoke<CoachState>("get_coach_state"),
  coachMarkSeen: (id: string) => invoke<CoachState>("coach_mark_seen", { id }),
  coachReset: () => invoke<CoachState>("coach_reset"),
  getPreferences: () => invoke<Preferences>("get_preferences"),
  setAccent: (accent: string) => invoke<Preferences>("set_accent", { accent }),
  getDataRoot: () => invoke<string>("get_data_root"),
  revealDataRoot: () => invoke<void>("reveal_data_root"),
  rotateApiKey: () => invoke<ApiInfo>("rotate_api_key"),
  checkForUpdate: () =>
    invoke<{ currentVersion: string; available: string | null; note: string }>(
      "check_for_update",
    ),

  // ── Knowledge base (RAG) ──
  getKnowledgeBase: (workspaceId: string) =>
    invoke<KnowledgeBase>("get_knowledge_base", { workspaceId }),
  addDocuments: (workspaceId: string, paths: string[]) =>
    invoke<void>("add_documents", { workspaceId, paths }),
  cancelIndexing: (workspaceId: string, docId: string) =>
    invoke<void>("cancel_indexing", { workspaceId, docId }),
  removeDocument: (workspaceId: string, docId: string) =>
    invoke<KnowledgeBase>("remove_document", { workspaceId, docId }),
  setRetrievalEnabled: (workspaceId: string, enabled: boolean) =>
    invoke<KnowledgeBase>("set_retrieval_enabled", { workspaceId, enabled }),
  previewChunks: (workspaceId: string, docId: string) =>
    invoke<Source[]>("preview_chunks", { workspaceId, docId }),
  stopEmbedder: () => invoke<void>("stop_embedder"),

  // ── MCP ──
  listMcpServers: (workspaceId: string) =>
    invoke<McpServerView[]>("list_mcp_servers", { workspaceId }),
  saveMcpServer: (workspaceId: string, config: McpServerConfig) =>
    invoke<McpServerView[]>("save_mcp_server", { workspaceId, config }),
  removeMcpServer: (workspaceId: string, serverId: string) =>
    invoke<McpServerView[]>("remove_mcp_server", { workspaceId, serverId }),
  connectMcpServer: (workspaceId: string, serverId: string) =>
    invoke<McpServerView>("connect_mcp_server", { workspaceId, serverId }),
  disconnectMcpServer: (serverId: string) =>
    invoke<void>("disconnect_mcp_server", { serverId }),

  onChatRetrieval: (handler: (r: ChatRetrieval) => void): Promise<UnlistenFn> =>
    listen<ChatRetrieval>("chat://retrieval", (e) => handler(e.payload)),

  onChatTool: (handler: (t: ChatToolEvent) => void): Promise<UnlistenFn> =>
    listen<ChatToolEvent>("chat://tool", (e) => handler(e.payload)),

  onChatDelta: (handler: (d: ChatDelta) => void): Promise<UnlistenFn> =>
    listen<ChatDelta>("chat://delta", (e) => handler(e.payload)),

  onChatDone: (handler: (d: ChatDone) => void): Promise<UnlistenFn> =>
    listen<ChatDone>("chat://done", (e) => handler(e.payload)),

  onRuntimeState: (handler: (s: RuntimeState) => void): Promise<UnlistenFn> =>
    listen<RuntimeState>("runtime://state", (e) => handler(e.payload)),

  onServerStatus: (handler: (s: ServerStatus) => void): Promise<UnlistenFn> =>
    listen<ServerStatus>("runtime://server", (e) => handler(e.payload)),
};

type Ipc = typeof tauriIpc;

// The harness implements a superset (extra internal fields); cast through
// unknown since the two object literals only need to agree on the Ipc surface.
export const ipc: Ipc = IN_TAURI ? tauriIpc : (harnessIpc as unknown as Ipc);

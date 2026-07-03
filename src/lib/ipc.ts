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
  Catalog,
  ChatDelta,
  ChatDone,
  Conversation,
  ConversationMeta,
  DownloadProgress,
  HardwareReport,
  LibraryModel,
  MetricsRecord,
  MetricsSettings,
  RecommendationSet,
  RuntimeState,
  ServerStatus,
  TelemetrySample,
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
  }) => invoke<Workspace>("create_workspace", args),

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

  stopEngine: () => invoke<void>("stop_engine"),

  getMetricsSettings: () => invoke<MetricsSettings>("get_metrics_settings"),
  setMetricsShare: (share: boolean) => invoke<MetricsSettings>("set_metrics_share", { share }),
  getMetricsHistory: (limit: number) => invoke<MetricsRecord[]>("get_metrics_history", { limit }),
  getMetricsSample: () => invoke<unknown>("get_metrics_sample"),

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

export const ipc: Ipc = IN_TAURI ? tauriIpc : (harnessIpc as Ipc);

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
  DownloadProgress,
  HardwareReport,
  LibraryModel,
  RecommendationSet,
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
};

type Ipc = typeof tauriIpc;

export const ipc: Ipc = IN_TAURI ? tauriIpc : (harnessIpc as Ipc);

/** Typed wrappers over the Tauri IPC surface — the only place `invoke` appears. */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Catalog,
  HardwareReport,
  RecommendationSet,
  TelemetrySample,
  Workspace,
  WorkspaceList,
} from "./types";

export const ipc = {
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

  deleteWorkspace: (id: string) =>
    invoke<WorkspaceList>("delete_workspace", { id }),

  onTelemetry: (handler: (s: TelemetrySample) => void): Promise<UnlistenFn> =>
    listen<TelemetrySample>("telemetry://sample", (e) => handler(e.payload)),
};

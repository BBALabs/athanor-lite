import { create } from "zustand";
import { ipc } from "../lib/ipc";
import {
  isCondereError,
  type Catalog,
  type HardwareReport,
  type RecommendationSet,
  type TelemetrySample,
  type Workspace,
  type WorkspaceList,
} from "../lib/types";

export type View = "dashboard" | "models" | "workspaces";
export type BootPhase = "booting" | "ready" | "error";

/** One entry in the boot ticker — real steps, real completion. */
export interface BootStep {
  label: string;
  state: "pending" | "running" | "done" | "failed";
}

const TELEMETRY_WINDOW = 120; // samples ≈ 2 minutes at 1 Hz

interface CondereStore {
  boot: BootPhase;
  bootSteps: BootStep[];
  bootError: string | null;

  view: View;
  hardware: HardwareReport | null;
  recommendations: RecommendationSet | null;
  catalog: Catalog | null;
  telemetry: TelemetrySample[];
  workspaces: WorkspaceList;
  /** Non-fatal, user-visible operation failure (workspace ops etc.). */
  lastOpError: string | null;

  setView: (v: View) => void;
  init: () => Promise<void>;
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
  if (isCondereError(e)) return `${e.code}: ${e.message}`;
  return e instanceof Error ? e.message : String(e);
}

let telemetryBound = false;
let initStarted = false; // React StrictMode double-fires effects in dev; boot once.

export const useStore = create<CondereStore>((set) => ({
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

  view: "dashboard",
  hardware: null,
  recommendations: null,
  catalog: null,
  telemetry: [],
  workspaces: { workspaces: [], activeId: null },
  lastOpError: null,

  setView: (view) => set({ view }),

  init: async () => {
    if (initStarted) return;
    initStarted = true;
    const step = (label: string, state: BootStep["state"]) =>
      set((s) => ({
        bootSteps: s.bootSteps.map((b) => (b.label === label ? { ...b, state } : b)),
      }));

    try {
      step("probing hardware", "running");
      const hardware = await ipc.detectHardware();
      set({ hardware });
      step("probing hardware", "done");

      step("loading model catalog", "running");
      const catalog = await ipc.getModelCatalog();
      set({ catalog });
      step("loading model catalog", "done");

      step("computing recommendations", "running");
      const recommendations = await ipc.getRecommendations(hardware);
      set({ recommendations });
      step("computing recommendations", "done");

      step("mounting workspaces", "running");
      const workspaces = await ipc.listWorkspaces();
      set({ workspaces });
      step("mounting workspaces", "done");

      step("telemetry stream", "running");
      if (!telemetryBound) {
        telemetryBound = true;
        await ipc.onTelemetry((sample) =>
          set((s) => ({
            telemetry: [...s.telemetry.slice(-(TELEMETRY_WINDOW - 1)), sample],
          })),
        );
      }
      step("telemetry stream", "done");

      set({ boot: "ready" });
    } catch (e) {
      console.error("boot failed", e);
      set((s) => ({
        boot: "error",
        bootError: errText(e),
        bootSteps: s.bootSteps.map((b) =>
          b.state === "running" ? { ...b, state: "failed" } : b,
        ),
      }));
    }
  },

  createWorkspace: async (args) => {
    try {
      const ws = await ipc.createWorkspace(args);
      set({ workspaces: await ipc.listWorkspaces() });
      return ws;
    } catch (e) {
      set({ lastOpError: errText(e) });
      return null;
    }
  },

  activateWorkspace: async (id) => {
    // Optimistic — switching must feel instant; reconcile after.
    set((s) => ({ workspaces: { ...s.workspaces, activeId: id } }));
    try {
      await ipc.activateWorkspace(id);
      set({ workspaces: await ipc.listWorkspaces() });
    } catch (e) {
      set({ workspaces: await ipc.listWorkspaces(), lastOpError: errText(e) });
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

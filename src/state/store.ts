import { create } from "zustand";
import { ipc } from "../lib/ipc";
import {
  isAthanorError,
  type Catalog,
  type HardwareReport,
  type RecommendationSet,
  type TelemetrySample,
  type Workspace,
  type WorkspaceList,
} from "../lib/types";

export type View = "dashboard" | "models" | "workspaces";
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
  /** Non-fatal, user-visible operation failure (workspace ops etc.). */
  lastOpError: string | null;

  setView: (v: View) => void;
  init: () => Promise<void>;
  retryHardware: () => Promise<void>;
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
  lastOpError: null,

  setView: (view) => set({ view }),

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
    const prior = get().workspaces;
    set((s) => ({ workspaces: { ...s.workspaces, activeId: id } }));
    try {
      await ipc.activateWorkspace(id);
      set({ workspaces: await ipc.listWorkspaces() });
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

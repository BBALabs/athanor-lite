/**
 * Design harness — a browser-only stand-in for the Rust core, so the UI can be
 * developed and reviewed in a plain browser tab (`npm run dev`, no Tauri).
 *
 * Inside the desktop app `IN_TAURI` is always true and none of this module's
 * behavior is reachable. The catalog is imported from the same catalog.json the
 * Rust core embeds — one source of truth; only the fit math is mirrored here.
 */

import rawCatalog from "../../src-tauri/src/models/catalog.json";
import { IN_TAURI } from "./tauriEnv";
import type {
  Catalog,
  DownloadProgress,
  HardwareReport,
  LibraryModel,
  Pick,
  RecommendationSet,
  Role,
  TelemetrySample,
  Workspace,
  WorkspaceList,
} from "./types";

const catalog = rawCatalog as Catalog;

// The browser tab must never pass for the real app. (This module is imported
// inside Tauri too — the retitle applies only to the browser harness.)
if (!IN_TAURI && typeof document !== "undefined") {
  document.title = "Athanor — design harness (synthetic data)";
}

const GIB = 1024 ** 3;

/** A representative workstation profile for design review. */
const HW: HardwareReport = {
  cpu: {
    brand: "AMD Ryzen Threadripper PRO 7965WX 24-Cores",
    physicalCores: 24,
    logicalCores: 48,
    baseFrequencyMhz: 4200,
    arch: "x86_64",
  },
  memory: { totalBytes: 80 * GIB, availableBytes: 51 * GIB },
  gpus: [
    {
      name: "NVIDIA RTX 6000 Ada Generation",
      vendor: "Nvidia",
      vramTotalBytes: 48 * GIB,
      vramUsedBytes: 9.2 * GIB,
      driverVersion: "580.88",
      cudaVersion: "13.0",
      architecture: "Ada Lovelace",
      computeCapability: "8.9",
      temperatureC: 52,
      utilizationPct: 14,
      source: "nvml",
    },
  ],
  disks: [
    { name: "OS", mount: "C:\\", totalBytes: 1863 * GIB, availableBytes: 512 * GIB, kind: "Ssd" },
    { name: "Models", mount: "D:\\", totalBytes: 3726 * GIB, availableBytes: 2980 * GIB, kind: "Ssd" },
  ],
  os: { name: "Windows", version: "11 (26200)", hostname: "forge", arch: "x86_64" },
  computeClass: "VramWorkstation",
  detectedAt: new Date().toISOString(),
};

/* Mirror of the Rust budget/fit rules (recommend.rs) for harness data only. */
const VRAM_USABLE_FRACTION = 0.95;
const RUNTIME_RESERVE_GB = 0.5;

function budgetGb(): number {
  const vram = (HW.gpus[0].vramTotalBytes ?? 0) / GIB;
  return vram * VRAM_USABLE_FRACTION - RUNTIME_RESERVE_GB;
}

function pickFor(entryId: string, budget: number): Pick | null {
  const e = catalog.entries.find((x) => x.id === entryId);
  if (!e) return null;
  const quant = [...e.quants]
    .filter((q) => q.minMemGb <= budget)
    .sort((a, b) => b.minMemGb - a.minMemGb)[0];
  if (!quant) return null;
  const headroomGb = budget - quant.minMemGb;
  return {
    entryId: e.id,
    name: e.name,
    family: e.family,
    paramsB: e.paramsB,
    roles: e.roles,
    quality: e.quality,
    blurb: e.blurb,
    quant: quant.label,
    fileGb: quant.fileGb,
    estMemGb: quant.minMemGb,
    headroomGb,
    headroomPct: (headroomGb / budget) * 100,
    note:
      headroomGb >= 8
        ? `Fits with ${headroomGb.toFixed(0)} GB to spare — room for an embedding model and long context alongside.`
        : `Comfortable fit with ${headroomGb.toFixed(1)} GB of headroom at 8K context.`,
  };
}

function recommendations(): RecommendationSet {
  const budget = budgetGb();
  const chat = catalog.entries
    .filter((e) => !e.roles.includes("embedding"))
    .map((e) => pickFor(e.id, budget))
    .filter((p): p is Pick => p !== null)
    .sort((a, b) => b.quality - a.quality);

  const byRole = (["general", "coding", "reasoning", "embedding"] as Role[]).flatMap((role) => {
    const best = catalog.entries
      .filter((e) => e.roles.includes(role))
      .map((e) => pickFor(e.id, budget))
      .filter((p): p is Pick => p !== null)
      .sort((a, b) => b.quality - a.quality)[0];
    return best ? [{ role, pick: best }] : [];
  });

  return {
    mode: "gpuFull",
    computeClass: HW.computeClass,
    budgetGb: budget,
    best: chat[0] ?? null,
    alternates: chat.slice(1, 4),
    byRole,
    notes: [
      `Budget: ${budget.toFixed(1)} GB usable of 48 GB VRAM (95% usable minus 0.5 GB runtime reserve).`,
    ],
  };
}

let workspaces: Workspace[] = [
  {
    schema: 1,
    id: "harness-1",
    name: "Game Dev Assistant",
    purpose: "Godot scripting, shader help, design docs",
    accentHue: 275,
    glyph: "◆",
    createdAt: new Date(Date.now() - 86400000 * 12).toISOString(),
    lastOpenedAt: new Date(Date.now() - 3600000 * 2).toISOString(),
    modelRefs: [],
    activeModel: null,
  },
  {
    schema: 1,
    id: "harness-2",
    name: "Legal Doc Review",
    purpose: "contract analysis over the Meridian file set",
    accentHue: 160,
    glyph: "▲",
    createdAt: new Date(Date.now() - 86400000 * 5).toISOString(),
    lastOpenedAt: new Date(Date.now() - 86400000).toISOString(),
    modelRefs: [],
    activeModel: null,
  },
];
let activeId: string | null = "harness-1";

function list(): WorkspaceList {
  return { workspaces: [...workspaces], activeId, damaged: [] };
}

/* Simulated download machinery — visual behavior only. */
let harnessLibrary: LibraryModel[] = [];
const harnessTimers: Record<string, number> = {};
let onProgress: ((p: DownloadProgress) => void) | null = null;

function findQuant(entryId: string, quantLabel: string) {
  const entry = catalog.entries.find((e) => e.id === entryId);
  const quant = entry?.quants.find((q) => q.label === quantLabel);
  if (!entry || !quant || !quant.files[0]) throw { code: "DOWNLOAD", message: "unknown model" };
  return { entry, quant, file: quant.files[0] };
}

export const harnessIpc = {
  detectHardware: async () => ({ ...HW, detectedAt: new Date().toISOString() }),
  getRecommendations: async () => recommendations(),
  getModelCatalog: async () => catalog,
  listWorkspaces: async () => list(),

  listLibrary: async () => [...harnessLibrary],

  startDownload: async (entryId: string, quantLabel: string) => {
    const { entry, quant, file } = findQuant(entryId, quantLabel);
    if (harnessTimers[file.sha256] !== undefined) return;
    let received = 0;
    const emit = (state: DownloadProgress["state"]) =>
      onProgress?.({
        sha256: file.sha256,
        entryId: entry.id,
        quant: quant.label,
        fileName: file.name,
        receivedBytes: received,
        totalBytes: file.sizeBytes,
        bytesPerSec: 88 * 1024 * 1024,
        state,
        error: null,
      });
    emit("starting");
    harnessTimers[file.sha256] = window.setInterval(() => {
      received = Math.min(file.sizeBytes, received + file.sizeBytes * 0.04);
      if (received >= file.sizeBytes) {
        window.clearInterval(harnessTimers[file.sha256]);
        delete harnessTimers[file.sha256];
        emit("verifying");
        window.setTimeout(() => {
          harnessLibrary = [
            {
              schema: 1,
              sha256: file.sha256,
              fileName: file.name,
              path: `X:/harness/models/${file.sha256}/${file.name}`,
              sizeBytes: file.sizeBytes,
              displayName: entry.name,
              entryId: entry.id,
              quant: quant.label,
              source: "huggingface",
              addedAt: new Date().toISOString(),
            },
            ...harnessLibrary,
          ];
          emit("done");
        }, 900);
      } else {
        emit("downloading");
      }
    }, 350);
  },

  cancelDownload: async (sha256: string) => {
    if (harnessTimers[sha256] !== undefined) {
      window.clearInterval(harnessTimers[sha256]);
      delete harnessTimers[sha256];
      onProgress?.({
        sha256,
        entryId: "",
        quant: "",
        fileName: "",
        receivedBytes: 0,
        totalBytes: 0,
        bytesPerSec: 0,
        state: "cancelled",
        error: null,
      });
    }
  },

  deleteModel: async (sha256: string) => {
    harnessLibrary = harnessLibrary.filter((m) => m.sha256 !== sha256);
    return [...harnessLibrary];
  },

  onDownloadProgress: async (handler: (p: DownloadProgress) => void) => {
    onProgress = handler;
    return () => {
      onProgress = null;
    };
  },

  createWorkspace: async (args: {
    name: string;
    purpose: string;
    accentHue: number;
    glyph: string;
  }): Promise<Workspace> => {
    const now = new Date().toISOString();
    const ws: Workspace = {
      schema: 1,
      id: `harness-${Date.now()}`,
      name: args.name.trim(),
      purpose: args.purpose.trim(),
      accentHue: args.accentHue % 360,
      glyph: args.glyph,
      createdAt: now,
      lastOpenedAt: now,
      modelRefs: [],
      activeModel: null,
    };
    workspaces = [ws, ...workspaces];
    activeId = ws.id;
    return ws;
  },

  activateWorkspace: async (id: string): Promise<Workspace> => {
    const ws = workspaces.find((w) => w.id === id);
    if (!ws) throw { code: "WORKSPACE", message: `workspace ${id} not found` };
    ws.lastOpenedAt = new Date().toISOString();
    activeId = id;
    return ws;
  },

  setWorkspaceModel: async (id: string, sha256: string | null): Promise<Workspace> => {
    const ws = workspaces.find((w) => w.id === id);
    if (!ws) throw { code: "WORKSPACE", message: `workspace ${id} not found` };
    ws.activeModel = sha256;
    return ws;
  },

  deleteWorkspace: async (id: string): Promise<WorkspaceList> => {
    workspaces = workspaces.filter((w) => w.id !== id);
    if (activeId === id) activeId = workspaces[0]?.id ?? null;
    return list();
  },

  onTelemetry: async (handler: (s: TelemetrySample) => void) => {
    let cpu = 12;
    let vramUsed = 9.2 * GIB;
    let memUsed = 29 * GIB;
    const timer = window.setInterval(() => {
      cpu = Math.max(2, Math.min(96, cpu + (Math.random() - 0.48) * 9));
      vramUsed = Math.max(2 * GIB, Math.min(46 * GIB, vramUsed + (Math.random() - 0.5) * 0.6 * GIB));
      memUsed = Math.max(16 * GIB, Math.min(72 * GIB, memUsed + (Math.random() - 0.5) * 1.2 * GIB));
      handler({
        tsMs: Date.now(),
        cpuUsagePct: cpu,
        memTotalBytes: HW.memory.totalBytes,
        memUsedBytes: memUsed,
        gpus: [
          {
            index: 0,
            name: HW.gpus[0].name,
            vramTotalBytes: HW.gpus[0].vramTotalBytes ?? 0,
            vramUsedBytes: vramUsed,
            // Realistic idle-to-busy band; the warn hue must mean something.
            utilizationPct: Math.round(Math.max(6, Math.min(68, cpu * 0.9 + (Math.random() - 0.5) * 14))),
            temperatureC: Math.round(46 + cpu / 6),
          },
        ],
      });
    }, 1000);
    return () => window.clearInterval(timer);
  },
};

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
  ChatDelta,
  ChatDone,
  Conversation,
  DownloadProgress,
  HardwareReport,
  LibraryModel,
  Operation,
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
  document.title = "Athanor Lite — design harness (synthetic data)";
}

const GIB = 1024 ** 3;

/**
 * Hardware scenario for design review, switched by URL hash:
 * `#cpu` = no dedicated GPU, `#dualgpu` = 2×24 GB pooled.
 * Default is the single-GPU workstation.
 */
type HwScenario = "default" | "cpu" | "dualgpu";
const HW_SCENARIO: HwScenario =
  typeof window === "undefined"
    ? "default"
    : window.location.hash === "#cpu"
      ? "cpu"
      : window.location.hash === "#dualgpu"
        ? "dualgpu"
        : "default";

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

if (HW_SCENARIO === "cpu") {
  HW.cpu.brand = "Intel Core i5-12400";
  HW.cpu.physicalCores = 6;
  HW.cpu.logicalCores = 12;
  HW.memory = { totalBytes: 16 * GIB, availableBytes: 9 * GIB };
  HW.gpus = [];
  HW.computeClass = "CpuOnly";
} else if (HW_SCENARIO === "dualgpu") {
  const card = (i: number) => ({
    name: "NVIDIA GeForce RTX 3090",
    vendor: "Nvidia" as const,
    vramTotalBytes: 24 * GIB,
    vramUsedBytes: (i === 0 ? 1.8 : 0.3) * GIB,
    driverVersion: "580.88",
    cudaVersion: "13.0",
    architecture: "Ampere",
    computeCapability: "8.6",
    temperatureC: 41 + i * 3,
    utilizationPct: 4,
    source: "nvml",
  });
  HW.gpus = [card(0), card(1)];
  HW.computeClass = "VramWorkstation";
}

/* Mirror of the Rust budget/fit rules (recommend.rs) for harness data only. */
const VRAM_USABLE_FRACTION = 0.95;
const RUNTIME_RESERVE_GB = 0.5;

function budgetGb(): number {
  // CPU-only machines budget against half of system RAM, like the backend.
  if (HW.gpus.length === 0) return HW.memory.totalBytes / GIB / 2;
  const vram = HW.gpus.reduce((sum, g) => sum + (g.vramTotalBytes ?? 0) / GIB, 0);
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
  const cpuOnly = HW.gpus.length === 0;
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
    fitMode: cpuOnly ? "cpu" : headroomGb >= budget * 0.15 ? "gpuFull" : "gpuTight",
    gpuOffloadPct: null,
    maxCtx: cpuOnly ? 0 : e.contextLength,
    note: cpuOnly
      ? `Runs on the processor — expect a reading pace, fully private.`
      : headroomGb >= 8
        ? `Runs fully on the GPU with ${headroomGb.toFixed(1)} GB to spare — full context.`
        : `Fits on the GPU with ${headroomGb.toFixed(1)} GB of headroom.`,
  };
}

function recommendations(): RecommendationSet {
  const budget = budgetGb();
  const chat = catalog.entries
    .map((e) => pickFor(e.id, budget))
    .filter((p): p is Pick => p !== null)
    .sort((a, b) => b.quality - a.quality);

  const byRole = (["general", "coding", "reasoning"] as Role[]).flatMap((role) => {
    const best = catalog.entries
      .filter((e) => e.roles.includes(role))
      .map((e) => pickFor(e.id, budget))
      .filter((p): p is Pick => p !== null)
      .sort((a, b) => b.quality - a.quality)[0];
    return best ? [{ role, pick: best }] : [];
  });

  // Fit verdict for every quant (mirrors the Rust decomposition well enough
  // for design review; the desktop app uses the real backend numbers).
  const OVERHEAD = 0.5;
  const fits = catalog.entries.flatMap((e) =>
    e.quants.map((q) => {
      const kvPerTok = Math.max(0, q.minMemGb - q.fileGb - OVERHEAD) / 8192;
      const est = q.fileGb + OVERHEAD + kvPerTok * 8192;
      let fitMode: Pick["fitMode"];
      if (HW.gpus.length === 0) fitMode = est <= budget ? "cpu" : "exceeds";
      else if (est <= budget * 0.85) fitMode = "gpuFull";
      else if (est <= budget) fitMode = "gpuTight";
      else if (est <= HW.memory.totalBytes / 1024 ** 3 / 2) fitMode = "partialOffload";
      else fitMode = "exceeds";
      const maxCtx =
        fitMode === "gpuFull" || fitMode === "gpuTight"
          ? kvPerTok > 0
            ? Math.min(e.contextLength, Math.floor((budget - q.fileGb - OVERHEAD) / kvPerTok))
            : e.contextLength
          : 0;
      return {
        entryId: e.id,
        quant: q.label,
        fitMode,
        estMemGb: est,
        gpuOffloadPct: fitMode === "partialOffload" ? 55 : null,
        maxCtx,
      };
    }),
  );

  const totalVramGb = HW.gpus.reduce((s, g) => s + (g.vramTotalBytes ?? 0) / GIB, 0);
  return {
    mode: HW.gpus.length === 0 ? "cpuOnly" : "gpuFull",
    computeClass: HW.computeClass,
    budgetGb: budget,
    ramBudgetGb: HW.memory.totalBytes / 1024 ** 3 / 2,
    gpuCount: HW.gpus.length,
    multiGpu: HW.gpus.length > 1,
    vramInUseGb: HW.gpus.reduce((s, g) => s + (g.vramUsedBytes ?? 0) / GIB, 0),
    defaultCtx: 8192,
    best: chat[0] ?? null,
    alternates: chat.slice(1, 4),
    byRole,
    fits,
    notes:
      HW.gpus.length === 0
        ? [
            `No dedicated GPU — models run on the processor from ${budget.toFixed(1)} GB of system memory.`,
          ]
        : [
            `Budget: ${budget.toFixed(1)} GB usable of ${totalVramGb.toFixed(0)} GB VRAM (95% usable minus VRAM in use).`,
            `Fit shown at 8K context; each pick lists the largest context it can hold.`,
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
    modelRefs: ["6c1a2b41161032677be168d354123594c0e6e67d2b9227c84f296ad037c728ff"],
    activeModel: "6c1a2b41161032677be168d354123594c0e6e67d2b9227c84f296ad037c728ff",
  },
];
let activeId: string | null = "harness-1";

function list(): WorkspaceList {
  return { workspaces: [...workspaces], activeId, damaged: [] };
}

/* Simulated download machinery — visual behavior only. One model ships
   pre-installed and active so the chat room can be designed in a browser
   without walking the install flow. */
let harnessLibrary: LibraryModel[] = [
  {
    schema: 1,
    sha256: "6c1a2b41161032677be168d354123594c0e6e67d2b9227c84f296ad037c728ff",
    fileName: "llama-3.2-3b-instruct-Q4_K_M.gguf",
    path: "X:/harness/models/llama-3.2-3b/llama-3.2-3b-instruct-Q4_K_M.gguf",
    sizeBytes: 2_019_377_000,
    displayName: "Llama 3.2 3B Instruct",
    entryId: "llama-3.2-3b-instruct",
    quant: "Q4_K_M",
    source: "huggingface",
    addedAt: new Date(Date.now() - 86400000 * 3).toISOString(),
  },
  {
    schema: 1,
    sha256: "harness-qwen-coder-7b",
    fileName: "qwen2.5-coder-7b-instruct-Q4_K_M.gguf",
    path: "X:/harness/models/qwen2.5-coder-7b/qwen2.5-coder-7b-instruct-Q4_K_M.gguf",
    sizeBytes: 4_680_000_000,
    displayName: "Qwen2.5 Coder 7B",
    entryId: "qwen2.5-coder-7b",
    quant: "Q4_K_M",
    source: "huggingface",
    addedAt: new Date(Date.now() - 86400000 * 2).toISOString(),
  },
];
const harnessTimers: Record<string, number> = {};
let onProgress: ((p: DownloadProgress) => void) | null = null;
/* A few seeded sessions so the chat rail — history, search, rename — can be
   designed in a browser without walking the (throttled) canned stream. */
const harnessConvs: Conversation[] = [
  {
    schema: 1,
    id: "harness-conv-1",
    workspaceId: "harness-1",
    title: "Shader compilation errors",
    modelSha: "6c1a2b41161032677be168d354123594c0e6e67d2b9227c84f296ad037c728ff",
    createdAt: new Date(Date.now() - 86400000).toISOString(),
    updatedAt: new Date(Date.now() - 3600000 * 3).toISOString(),
    messages: [
      { role: "user", content: "Why does my Godot fragment shader fail with 'sampler2D expected'?", ts: new Date(Date.now() - 3600000 * 3).toISOString(), stats: null },
      { role: "assistant", content: "That error means a uniform declared as `sampler2D` is being read like a plain value. Bind the texture and sample it with `texture(tex, UV)` instead.", ts: new Date(Date.now() - 3600000 * 3).toISOString(), stats: null },
    ],
  },
  {
    schema: 1,
    id: "harness-conv-2",
    workspaceId: "harness-1",
    title: "Zebra migration patterns",
    modelSha: "6c1a2b41161032677be168d354123594c0e6e67d2b9227c84f296ad037c728ff",
    createdAt: new Date(Date.now() - 86400000 * 2).toISOString(),
    updatedAt: new Date(Date.now() - 86400000).toISOString(),
    messages: [
      { role: "user", content: "Summarize zebra migration patterns across the Serengeti.", ts: new Date(Date.now() - 86400000).toISOString(), stats: null },
      { role: "assistant", content: "Plains zebra follow the rains in a clockwise loop, tracking fresh grazing between the southern plains and the western corridor.", ts: new Date(Date.now() - 86400000).toISOString(), stats: null },
    ],
  },
  {
    schema: 1,
    id: "harness-conv-3",
    workspaceId: "harness-1",
    title: "Tilemap autoloading",
    modelSha: "6c1a2b41161032677be168d354123594c0e6e67d2b9227c84f296ad037c728ff",
    createdAt: new Date(Date.now() - 86400000 * 3).toISOString(),
    updatedAt: new Date(Date.now() - 86400000 * 2).toISOString(),
    messages: [
      { role: "user", content: "How do I autoload a tilemap resource in Godot 4?", ts: new Date(Date.now() - 86400000 * 2).toISOString(), stats: null },
      { role: "assistant", content: "Register a script in Project Settings → Autoload, then `preload()` the .tres tilemap in its `_ready()`.", ts: new Date(Date.now() - 86400000 * 2).toISOString(), stats: null },
    ],
  },
];
let onChatDeltaHandler: ((d: ChatDelta) => void) | null = null;
let onChatDoneHandler: ((d: ChatDone) => void) | null = null;
let onOpsHandler: ((ops: Operation[]) => void) | null = null;
let harnessAccent = "violet";
// eslint-disable-next-line @typescript-eslint/no-explicit-any
let harnessBenchmarks: any[] = [
  { schema: 1, modelSha: "seed-a", modelName: "Qwen2.5 Coder 7B", ttftMs: 62, genTps: 138.2, promptTps: 2410, gpuActive: true, prompts: 4, ranAt: new Date(Date.now() - 86400000).toISOString() },
];

const harnessLiveOps: Record<string, Operation> = {};

function harnessOps(): Operation[] {
  return Object.values(harnessLiveOps);
}

function harnessOpUpdate(op: Operation | null, id: string) {
  if (op) harnessLiveOps[id] = op;
  else delete harnessLiveOps[id];
  onOpsHandler?.(harnessOps());
}

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
    const opId = `dl:${file.sha256}`;
    const emit = (state: DownloadProgress["state"]) => {
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
      const finished = state === "done" || state === "cancelled" || state === "failed";
      harnessOpUpdate(
        finished
          ? null
          : {
              id: opId,
              kind: "download",
              state: "running",
              label: `Download · ${file.name}`,
              detail: "",
              progressCurrent: received,
              progressTotal: file.sizeBytes,
              resourceNote: null,
              startedAt: new Date().toISOString(),
              error: null,
              cancellable: true,
              retry: { kind: "download", entryId: entry.id, quant: quant.label },
            },
        opId,
      );
    };
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

  /* Chat — canned streaming so the room can be designed in a browser. */
  chatSend: async (workspaceId: string, conversationId: string | null, message: string) => {
    const id = conversationId ?? `conv-${Date.now()}`;
    let conv = harnessConvs.find((c) => c.id === id);
    if (!conv) {
      conv = {
        schema: 1,
        id,
        workspaceId,
        title: message.slice(0, 48),
        modelSha: null,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        messages: [],
      };
      harnessConvs.unshift(conv);
    }
    conv.messages.push({ role: "user", content: message, ts: new Date().toISOString(), stats: null });

    await new Promise((r) => setTimeout(r, 320));
    const reply =
      "This is the **design harness** — a canned reply so the room can be styled.\n\n```rust\nfn ignition() {\n    println!(\"the machine speaks\");\n}\n```\nEverything here would stream token by token from llama.cpp in the desktop app.";
    const words = reply.split(/(?<=\s)/);
    let acc = "";
    for (const w of words) {
      await new Promise((r) => setTimeout(r, 26));
      acc += w;
      onChatDeltaHandler?.({ workspaceId, conversationId: id, delta: w });
    }
    const stats = {
      ttftMs: 380,
      promptN: 42,
      predictedN: words.length,
      promptPerSecond: 900,
      predictedPerSecond: 38.4,
      contextUsed: 42 + words.length,
      gpuActive: true,
      cancelled: false,
    };
    conv.messages.push({ role: "assistant", content: acc, ts: new Date().toISOString(), stats });
    conv.updatedAt = new Date().toISOString();
    onChatDoneHandler?.({ workspaceId, conversationId: id, content: acc, stats, error: null });
    return id;
  },
  cancelGeneration: async () => {},
  listConversations: async (workspaceId: string) =>
    harnessConvs
      .filter((c) => c.workspaceId === workspaceId)
      .map((c) => ({ id: c.id, title: c.title, updatedAt: c.updatedAt, messageCount: c.messages.length })),
  getConversation: async (_workspaceId: string, conversationId: string) => {
    const c = harnessConvs.find((x) => x.id === conversationId);
    if (!c) throw { code: "CHAT", message: "conversation not found" };
    return JSON.parse(JSON.stringify(c)) as Conversation;
  },
  deleteConversation: async (workspaceId: string, conversationId: string) => {
    const i = harnessConvs.findIndex((c) => c.id === conversationId);
    if (i >= 0) harnessConvs.splice(i, 1);
    return harnessIpc.listConversations(workspaceId);
  },
  renameConversation: async (workspaceId: string, conversationId: string, title: string) => {
    const c = harnessConvs.find((x) => x.id === conversationId);
    if (c) c.title = title.trim().slice(0, 80) || "Untitled";
    return harnessIpc.listConversations(workspaceId);
  },
  regenerateReply: async (_workspaceId: string, conversationId: string) => {
    const c = harnessConvs.find((x) => x.id === conversationId);
    if (c) {
      while (c.messages.length && c.messages[c.messages.length - 1].role === "assistant") c.messages.pop();
      c.messages.push({ role: "assistant", content: "(regenerated) A fresh take on the same question.", ts: new Date().toISOString(), stats: null });
      c.updatedAt = new Date().toISOString();
    }
    return conversationId;
  },
  editAndResend: async (_workspaceId: string, conversationId: string, messageIndex: number, content: string) => {
    const c = harnessConvs.find((x) => x.id === conversationId);
    if (c) {
      c.messages = c.messages.slice(0, messageIndex + 1);
      c.messages[messageIndex] = { role: "user", content, ts: new Date().toISOString(), stats: null };
      c.messages.push({ role: "assistant", content: "(edited) Responding to your revised message.", ts: new Date().toISOString(), stats: null });
      c.updatedAt = new Date().toISOString();
    }
    return conversationId;
  },
  forkConversation: async (_workspaceId: string, conversationId: string, upto: number) => {
    const c = harnessConvs.find((x) => x.id === conversationId);
    const id = `conv-fork-${Date.now()}`;
    if (c) {
      harnessConvs.unshift({ ...c, id, title: `↳ ${c.title}`, messages: c.messages.slice(0, upto + 1).map((m) => ({ ...m })), updatedAt: new Date().toISOString() });
    }
    return id;
  },
  searchConversations: async (workspaceId: string, query: string) => {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    return harnessConvs
      .filter((c) => c.workspaceId === workspaceId)
      .map((c) => ({
        id: c.id,
        title: c.title,
        updatedAt: c.updatedAt,
        messageCount: c.messages.length,
        matches: c.messages
          .map((m, messageIndex) => ({ m, messageIndex }))
          .filter(({ m }) => m.content.toLowerCase().includes(q))
          .slice(0, 4)
          .map(({ m, messageIndex }) => ({ messageIndex, role: m.role, snippet: m.content.slice(0, 90) })),
      }))
      .filter((h) => h.matches.length > 0 || h.title.toLowerCase().includes(q));
  },
  exportConversation: async () => {},
  getMetricsSettings: async () => ({ schema: 1, share: false }),
  setMetricsShare: async (share: boolean) => ({ schema: 1, share }),
  getMetricsHistory: async () => [],
  getMetricsSample: async () => ({ note: "design harness — synthetic" }),
  onChatDelta: async (handler: (d: ChatDelta) => void) => {
    onChatDeltaHandler = handler;
    return () => {
      onChatDeltaHandler = null;
    };
  },
  onChatDone: async (handler: (d: ChatDone) => void) => {
    onChatDoneHandler = handler;
    return () => {
      onChatDoneHandler = null;
    };
  },
  onRuntimeState: async (_handler: (s: unknown) => void) => () => {},
  onServerStatus: async (_handler: (s: unknown) => void) => () => {},

  listOperations: async () => harnessOps(),
  cancelOperation: async (id: string) => {
    if (id.startsWith("dl:")) await harnessIpc.cancelDownload(id.slice(3));
  },
  dismissOperation: async () => {},
  retryOperation: async () => {},
  onOpsChanged: async (handler: (ops: Operation[]) => void) => {
    onOpsHandler = handler;
    return () => {
      onOpsHandler = null;
    };
  },

  getOllamaStatus: async () => ({ available: true, root: "X:/harness/.ollama", modelCount: 2 }),
  importOllama: async () => ({ found: 2, imported: 2, alreadyInLibrary: 0, skipped: [] }),
  getApiInfo: async () => ({
    expose: false,
    running: false,
    baseUrl: "http://127.0.0.1:11435/v1",
    apiKey: "harness-key-0000",
    modelName: null,
  }),
  setApiExpose: async (expose: boolean) => ({
    expose,
    running: false,
    baseUrl: "http://127.0.0.1:11435/v1",
    apiKey: "harness-key-0000",
    modelName: null,
  }),
  startEngine: async () => {},
  getPreferences: async () => ({ schema: 1, accent: harnessAccent }),
  setAccent: async (accent: string) => {
    harnessAccent = accent;
    return { schema: 1, accent: harnessAccent };
  },
  getDataRoot: async () => "C:\\Users\\you\\AppData\\Roaming\\com.bba.athanor",
  revealDataRoot: async () => {},
  isPortable: async () => false,
  openLink: async (url: string) => {
    window.open(url, "_blank", "noopener,noreferrer");
  },
  runBenchmark: async (modelSha: string, modelName: string) => {
    const r = { schema: 1, modelSha, modelName, ttftMs: 41, genTps: 214.6, promptTps: 3120.4, gpuActive: true, prompts: 4, ranAt: new Date().toISOString() };
    harnessBenchmarks = [r, ...harnessBenchmarks.filter((b) => b.modelSha !== modelSha)];
    return r;
  },
  listBenchmarks: async () => [...harnessBenchmarks].sort((a, b) => b.genTps - a.genTps),
  rotateApiKey: async () => ({
    expose: true,
    running: true,
    baseUrl: "http://127.0.0.1:11435/v1",
    apiKey: `harness-${Math.abs((Date.now() % 1e9) | 0).toString(16)}`,
    modelName: "Llama 3.2 3B Instruct",
  }),
  checkForUpdate: async () => ({
    currentVersion: "0.1.0",
    available: null,
    note: "design harness — updates are a desktop concern",
  }),

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
        memUsedBytes: Math.min(memUsed, HW.memory.totalBytes * 0.9),
        gpus: HW.gpus.map((g, index) => ({
          index,
          name: g.name,
          vramTotalBytes: g.vramTotalBytes ?? 0,
          vramUsedBytes: index === 0 ? vramUsed : (g.vramUsedBytes ?? 0),
          // Realistic idle-to-busy band; the warn hue must mean something.
          utilizationPct: Math.round(Math.max(6, Math.min(68, cpu * 0.9 + (Math.random() - 0.5) * 14))),
          temperatureC: Math.round(46 + cpu / 6),
        })),
      });
    }, 1000);
    return () => window.clearInterval(timer);
  },
};

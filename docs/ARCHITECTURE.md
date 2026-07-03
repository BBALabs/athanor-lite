# Athanor — System Architecture

**Product:** Athanor — local AI, assembled.
**Owner:** Black Box Analytics (Tony Winslow)
**Status:** M1 in progress (shell, hardware intelligence, recommendation engine, workspace core)
**Last updated:** 2026-07-03

---

## 1. Product thesis

Athanor makes running local AI models dead simple for any skill level. The differentiator is
**purpose-built workspaces**: a user creates an AI stack tuned for a specific job (game-dev
assistant, legal doc reviewer, code helper), each with its own models, RAG corpus, memory
graph, and fine-tunes — and switches between them like switching projects in an IDE.

Everything the app does flows from three promises:

1. **It knows your machine.** Hardware is assessed on launch and every recommendation,
   download, and runtime decision is derived from that profile. The user is never allowed
   to walk into an out-of-memory wall.
2. **It never breaks your system.** All runtimes, models, and indexes live inside Athanor's
   own sandboxed directory tree. No global Python, no CUDA installs, no PATH edits, no
   conflicts with Docker or existing tooling.
3. **It shows you what's happening.** Every long-running operation streams progress; every
   subsystem has a visible health state.

---

## 2. Technology stack

| Layer | Choice | Rationale |
|---|---|---|
| Desktop shell | **Tauri 2.0** (Rust core + WebView2) | ~10 MB footprint, native process control, memory-safe backend, first-class sidecar management |
| Frontend | **React 19 + TypeScript + Vite** | Fast iteration, typed IPC surface |
| State | **Zustand** | Minimal, no boilerplate, fits event-streamed telemetry |
| Inference | **llama.cpp** (`llama-server` sidecar, M2) | Process isolation, GGUF ecosystem, best-in-class quantized CPU/GPU inference |
| Vector store | **LanceDB** (embedded, per-workspace, M3) | Zero-server, columnar, versioned; one directory per workspace |
| Structured data | **SQLite** (per-workspace, M3) | Chat history, memory graph edges, job journal |
| GPU telemetry | **NVML** (primary) + **WMI/registry** (fallback) | Accurate VRAM totals + live utilization on NVIDIA; correct totals elsewhere |
| Styling | Hand-built design system (pure CSS custom properties) | Brand requirement: no generic component libraries |

There is intentionally **no bundled Python**. Inference, embedding, and (later) fine-tuning
run through native sidecar binaries that Athanor downloads, verifies, and version-pins itself.

---

## 3. Process architecture

```
┌─────────────────────────────────────────────────────────────────┐
│ Athanor (Tauri core process — Rust)                             │
│                                                                 │
│  ┌───────────────┐  ┌────────────────┐  ┌───────────────────┐  │
│  │ Hardware      │  │ Model Manager  │  │ Workspace Manager │  │
│  │ Service       │  │  catalog       │  │  manifests        │  │
│  │  detection    │  │  recommender   │  │  active state     │  │
│  │  telemetry 1Hz│  │  downloads(M2) │  │  isolation rules  │  │
│  └───────┬───────┘  └────────┬───────┘  └─────────┬─────────┘  │
│          │  typed commands + event channels (IPC) │            │
│  ────────┴───────────────────┴───────────────────┴──────────   │
│  ┌───────────────────────────────────────────────────────────┐ │
│  │ WebView2 — React UI (dashboard, models, workspaces, chat) │ │
│  └───────────────────────────────────────────────────────────┘ │
│                                                                 │
│  Sidecars (M2+): llama-server (one per active workspace),      │
│  managed lifecycle: spawn → health-poll → drain → kill          │
└─────────────────────────────────────────────────────────────────┘
```

### IPC contract

Commands are typed end-to-end: Rust `serde` structs ↔ TypeScript interfaces in
`src/lib/types.ts` (kept in lockstep by convention; codegen via `specta` is a candidate for M2).

| Command | Direction | Payload |
|---|---|---|
| `detect_hardware` | invoke → | `HardwareReport` (CPU, RAM, GPUs, disks, OS) |
| `get_recommendations` | invoke → | `RecommendationSet` derived from a `HardwareReport` |
| `get_model_catalog` | invoke → | full curated `CatalogEntry[]` |
| `list_workspaces` / `create_workspace` / `activate_workspace` / `delete_workspace` | invoke → | `Workspace` manifests |
| `telemetry://sample` | event ← | 1 Hz `TelemetrySample` (CPU %, RAM, per-GPU VRAM/util/temp) |
| `download://progress` (M2) | event ← | per-file byte progress, verify state |
| `runtime://state` (M2) | event ← | sidecar lifecycle transitions |

All commands return `Result<T, AthanorError>`; errors carry a stable `code` the UI maps to
designed failure states (never raw strings in the UI).

---

## 4. Hardware intelligence

**Detection (`detect_hardware`)** runs at launch and on demand:

- **CPU** — brand, physical/logical cores, base frequency (`sysinfo`).
- **RAM** — total/available bytes.
- **GPU** — three-stage strategy:
  1. **NVML** (NVIDIA): exact VRAM total/used, driver + CUDA version, utilization, temperature.
  2. **WMI** `Win32_VideoController`: vendor/name/driver for every adapter.
  3. **Registry** (`HardwareInformation.qwMemorySize`): accurate VRAM totals for non-NVIDIA
     adapters (WMI's `AdapterRAM` is a u32 and caps at 4 GB — never trust it alone).
- **Disks** — per-volume capacity/free, so downloads can be placed and pre-flighted.
- **OS** — name, version, hostname, arch.

**Telemetry** is a dedicated sampler thread in the Rust core (never the UI): 1 Hz, emits
`telemetry://sample` events. The frontend keeps a 120-sample ring buffer per metric and renders
sparklines/gauges from it. Sampling cost is bounded (single `sysinfo` refresh + NVML queries).

**Compute capability score** — the profile is distilled to a `ComputeClass`
(`CpuOnly | VramLow(<6GB) | VramMid(6–15) | VramHigh(16–31) | VramWorkstation(32+)`) used by
the recommender and later by the runtime configurator (context length, GPU layer split).

## 5. Model catalog & recommendation engine

The catalog is **curated, not scraped**: a reviewed list of GGUF models embedded in the binary
(`models/catalog.json`, ~15 entries), each with:

- identity: family, name, parameter count, HF repo, license, context window
- role tags: `general | coding | reasoning | embedding`
- a hand-tuned `quality` ordinal (relative capability within the catalog)
- per-quant footprint: file size + estimated memory floor (weights + KV cache @ 8K ctx)

**Recommendation algorithm** (pure function, unit-tested):

1. Compute the memory budget: largest single GPU VRAM × 0.95 − 0.5 GB runtime reserve
   (constants live in `models/recommend.rs` — this document defers to code);
   CPU-only machines get RAM × 0.5 and a "CPU inference" flag with tempered expectations.
2. For every catalog entry, pick the best quant that fits the budget (prefer Q4_K_M+; refuse
   quants below Q3 as quality-misleading).
3. **Best single model** = highest quality entry that fits. **Alternates** = next three.
4. **Per-role picks** = best fitting entry per role tag, so a workspace wizard can propose
   "your machine's best coding stack" instantly.
5. Every pick carries fit metadata: projected VRAM at 8K context, headroom %, and a plain-
   English note ("fits with 31 GB to spare — room for an embedding model alongside").

## 6. Workspace system

A workspace is a directory — fully self-contained, portable, deletable:

```
%APPDATA%/com.bba.athanor/
├── state.json                  # active workspace id, app-level prefs
├── logs/athanor.log            # rotating structured log
├── models/                     # shared, content-addressed GGUF store (M2)
│   └── <sha256>/<file>.gguf    # workspaces reference models; never copy them
└── workspaces/
    └── <id>/
        ├── workspace.json      # manifest: name, purpose, accent, model refs, created/opened
        ├── rag/                # LanceDB tables (M3)
        ├── memory/             # memory graph SQLite (M3)
        └── chats.db            # conversation history (M2)
```

Isolation rules: workspaces reference shared model blobs by hash (no duplication); every
runtime process is spawned with its working directory inside the workspace; deleting a
workspace deletes exactly its directory and releases (refcounted) model references.
Nothing is ever written outside the app data root.

## 7. Frontend architecture

```
src/
├── styles/        tokens.css (design tokens) · base.css (reset/type) · fx.css (motion)
├── lib/           ipc.ts (typed invoke/subscribe) · types.ts · format.ts
├── state/         store.ts (Zustand: hardware, telemetry rings, recs, workspaces, view)
├── components/    Titlebar · NavRail · BootSequence · ArcGauge · SegmentBar ·
│                  Sparkline · StatusPill · Icons (hand-drawn set) · Wordmark
└── views/         Dashboard · Models · Workspaces
```

### Design system (BBA brand)

- **Palette:** near-black violet field (`#0A0612` range), layered translucent panels,
  primary gradient `#6B21A8 → #A855F7 → #E879F9`, semantic greens/ambers/reds for health.
- **Type:** Space Grotesk (display), Inter (UI), JetBrains Mono (all numerics, tabular).
- **Language:** a *machine console*, not a web dashboard — asymmetric layout, hairline
  borders, arc gauges with gradient strokes, segmented VRAM bars, live sparklines, a boot
  sequence on launch, status pills with heartbeat dots. No stock component library anywhere.
- **Motion:** every state change acknowledges itself (staggered panel reveals, value
  tweening, pulse on event). Animations are transform/opacity only — no layout thrash.

## 8. Reliability, security, logging

- **Sandboxing:** Tauri capability system pinned to the minimum (window controls, events,
  core commands). CSP locked to `'self'`. No remote content, no eval.
- **Errors:** `thiserror`-typed on the Rust side with stable codes; every UI failure state is
  designed (retry affordances, diagnostics copy button) — no raw panics reaching the user.
- **Logging:** `tauri-plugin-log` → rotating file in `logs/` + stdout in dev. Subsystem-tagged
  (`hw`, `catalog`, `ws`), and surfaced in-app in the diagnostics drawer (M2).
- **Downloads (M2):** resumable, SHA256-verified against the catalog, pre-flighted against
  disk free space, content-addressed storage.

## 9. Milestones

| | Scope | Status |
|---|---|---|
| **M1** | Shell, design system, hardware detection + live telemetry, recommendation engine, workspace create/switch, system dashboard | **this milestone** |
| **M2** | Model downloads (resumable/verified), llama-server sidecar lifecycle, chat interface with streaming | next |
| **M3** | RAG pipeline (LanceDB + embedding sidecar), document ingestion UX, memory graph | |
| **M4** | Fine-tuning workflows (LoRA via sidecar), workspace export/import | |
| **M5** | Auto-update, crash reporting, installer polish, onboarding tour | |

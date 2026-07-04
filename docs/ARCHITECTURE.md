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

1. Compute the memory budget honestly. The recommender **sums every NVIDIA GPU** (VRAM ×
   0.95 each) and **subtracts VRAM already in use** by other processes — so a dual-card
   workstation is scoped for tensor-split, and a machine already running something isn't
   promised memory it doesn't have. CPU-only machines get RAM × 0.5 and a "CPU inference"
   flag with tempered expectations. Constants live in `models/recommend.rs`.
2. **Fit is context-aware.** Each catalog footprint is decomposed into weights + KV-cache
   (per-token, derived from the 8K reference floor) + overhead, so memory is projected at
   the *actual* context length rather than a fixed 8K. Every quant gets one of five verdicts:
   `GpuFull` (fits with comfortable headroom), `GpuTight` (fits, thin headroom),
   `PartialOffload` (near-fit → CPU+GPU split, with the GPU-offload % the runtime should use),
   `Cpu`, or `Exceeds`. **"Tight" is never over budget** — an OOM crash is `Exceeds`, not tight.
3. For every entry, pick the best quant that is *runnable* (prefer Q4_K_M+; refuse sub-Q3 as
   quality-misleading). **Best single model** = highest quality that fits; **Alternates** = next three.
4. **Per-role picks** = best fitting entry per role tag, so a workspace wizard can propose
   "your machine's best coding stack" instantly.
5. The backend emits a `fits` table — one verdict per catalog quant — that is the **single
   source of truth** the UI reads; the frontend never re-derives fit. Every number a power
   user sees (budget, in-use VRAM, GPU count, projected memory, max context) is trustworthy.

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

**Templates** (`models/templates.json`, embedded like the catalog) make a new workspace a
working stack in one click instead of a blank form — the zero-friction default create path.
Each names a model *role* (never a specific id, so it can't break as the catalog evolves), a
crafted system-prompt/purpose, an accent, RAG intent, and plain-language tool suggestions
(never auto-installed — the user decides). A startup test binds every template's role to the
catalog. On create, if the user already has a model of that role installed, the recommender
resolves the best one and the workspace is ready to chat on arrival; otherwise the model
chooser guides them. Five ship: Code Assistant, Document Reviewer, Creative Writer, Research
Assistant, Math Tutor (plus Blank).

**Guided walkthroughs** (`uistate` module + `src/components/Coach.tsx`): the app teaches
itself by doing. A reusable coach spotlights a *real* control (dimming four panels around it
so it stays interactive), says one plain sentence, and advances when the user performs the
action. The seen-set persists to `coach.json`; each walkthrough fires once, is always
skippable, and "replay the tutorials" resets them. Feature views call `maybeStartCoach(id)`
on first entry; steps anchor to `data-coach` attributes.

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

## 7b. Process control — a core design principle

Every operation that outlives a click (download, engine fetch, model load,
generation, import, and every future one: indexing, fine-tuning) obeys five
rules, enforced by the **operations registry** (`ops/mod.rs`):

1. **Visible:** it registers on start and appears in the Operations drawer
   with progress, elapsed time, and resource notes. The status line always
   shows the running count. No hidden work, ever.
2. **Stoppable:** one click stops it, through one mechanism (the registry's
   cancel flags; the engine stops by termination). Cancel is honored at the
   next chunk/token boundary.
3. **Duplicate-proof:** `Ops::begin` refuses a second running operation with
   the same id — double-clicks, racing callers, and re-entrant flows all hit
   the same guard. Engine bring-up is additionally serialized behind a spawn
   lock (queued waiters re-check and reuse instead of double-spawning).
4. **Recoverable:** failures stay visible with what/why and a retry spec
   where one exists (downloads resume from their partial). Success leaves no
   residue.
5. **Orphan-free:** every child process is assigned to the app's Windows Job
   Object (`KILL_ON_JOB_CLOSE`) — children die with the app even on a hard
   kill (verified: `taskkill /F` leaves zero llama-server processes). A
   startup sweep additionally terminates anything still executing from our
   `runtimes/` directory after a crash of a pre-job-object build.

Multi-step flows (onboarding today; RAG/fine-tune wizards later) must be
skippable and back-out-able at every step, and any long step they trigger
goes through the registry like everything else.

## 7c. Knowledge (RAG) and tools (MCP) — M3

**RAG.** Each workspace has a knowledge base at `workspaces/<id>/rag/`: a
schema-versioned `knowledge.json` manifest (the document list, status, counts —
source of truth for the UI) and a per-workspace **LanceDB** database (`rag/lance`,
table `chunks`) holding vectors + text. Pipeline: extract text
(txt/md/code as UTF-8, PDF via `pdf-extract`, DOCX by parsing `word/document.xml`)
→ paragraph-aware chunk with overlap → embed → store. Indexing is a registered,
cancellable operation with per-chunk progress.

Embeddings come from a **dedicated `llama-server`** running nomic-embed-text-v1.5
in `--embeddings --pooling mean` mode on its own port, coexisting with the chat
engine (the embed model is ~0.3 GB). It reuses the entire runtime substrate: the
pinned llama.cpp build, the job object (no orphans), the operations registry,
and serialized bring-up. The nomic task prefixes are mandatory and applied here:
`search_document:` on stored chunks, `search_query:` on the query — omitting them
silently wrecks retrieval. Vectors are L2-normalized by the server, so LanceDB's
L2 nearest-neighbor ranking equals cosine ordering; a cosine similarity score
(`1 − L2²/2`) is reported for display.

At chat time (`retrieve`): embed the latest user turn, pull top-k chunks above a
similarity floor, inject them as a system message, and **surface the sources** —
which documents, which chunk indices, what score — as a first-class output. The
UI shows retrieval running before tokens and lists the sources under the reply.
Retrieval failure never blocks chatting; retrieval can be toggled per workspace.

**MCP.** Workspaces connect to external tools over the Model Context Protocol
(`mcp/servers.json` per workspace). Transport is newline-delimited compact
JSON-RPC 2.0 on the server's stdio (stderr is logs only); handshake is
`initialize` (protocol 2025-11-25) → `notifications/initialized` → `tools/list`,
with `tools/call` available. Each server is a child process under the same
guarantees as the engine: job-object bound (dies with the app), registered in
the operations registry, duplicate-connection-proof, killed on exit.

**The agentic loop.** Connected MCP tools are converted to OpenAI function
definitions and sent with each generation (`tool_choice: auto`). The chat backend
runs a bounded multi-round loop: stream a turn while accumulating both content
deltas and tool-call fragments (assembled by streamed `index`); when the model
emits tool calls, execute each against the owning server via `tools/call`, append
the results as `role: "tool"` messages, and loop — up to a `MAX_TOOL_ROUNDS`
backstop. Two robustness measures make this reliable across models: **schema-aware
argument coercion** (models routinely stringify numeric/boolean args — `{"a":"40217"}`
where the tool wants a number — so arguments are healed against the tool's declared
input schema before the call), and **self-correcting feedback** (an unknown tool name,
common with small models, returns the list of valid names so the model recovers next
round instead of looping). Every call is surfaced as a first-class `ToolStep`
(server, tool, the actually-sent arguments, result, ok) — streamed live to the UI as
`chat://tool` events and persisted on the assistant message, so the user sees exactly
what was called, with what, and what came back. Verified end-to-end on-device: the
model autonomously calls a sum tool with coerced arguments and folds the result into
its answer.

## 8. Reliability, security, logging

- **Sandboxing:** Tauri capability system pinned to the minimum (window controls, events,
  core commands). CSP locked to `'self'`. No remote content, no eval.
- **Errors:** `thiserror`-typed on the Rust side with stable codes; every UI failure state is
  designed (retry affordances, diagnostics copy button) — no raw panics reaching the user.
- **Logging:** `tauri-plugin-log` → rotating file in `logs/` + stdout in dev. Subsystem-tagged
  (`hw`, `catalog`, `ws`), and surfaced in-app in the diagnostics drawer (M2).
- **Downloads (M2):** resumable, SHA256-verified against the catalog, pre-flighted against
  disk free space, content-addressed storage.

## 8b. Cross-platform & portability

The app is written to compile and run on Windows, macOS, and Linux; platform-specific code is
isolated behind `#[cfg]` so no path is Windows-only by accident:

- **Paths & data root:** everything derives from one `data_root()`; paths use `std::path`
  (never hardcoded separators). Portable mode (§6) redirects the whole root to a folder beside
  the executable — no registry, no user profile.
- **Process lifetime:** the engine and MCP servers are bound to a Windows **Job Object**
  (kill-on-crash). On Unix that binding is a no-op, and the cross-platform **orphan sweep** at
  next launch is the safety net (it scans and kills any leftover engine by path).
- **Child processes:** the server binary name is `#[cfg]`-selected (`llama-server.exe` vs
  `llama-server`); `CREATE_NO_WINDOW` is Windows-only; MCP servers launch via `cmd /c` on
  Windows (for `.cmd` shims) and directly on Unix.
- **GPU detection:** NVML-first (Windows + Linux); WMI/registry supplementation is
  Windows-gated; macOS has no NVIDIA path and degrades to CPU cleanly.
- **Platform deps** (`windows`, `wmi`, `winreg`) are `[target.'cfg(windows)'.dependencies]`, so
  they never break a Unix build.

**Honest boundary:** the *prebuilt llama.cpp runtime* is bundled for Windows only today —
`ensure_runtime` returns a clear "in progress" error on other platforms rather than fetching
Windows binaries. The rest of the app (hardware, workspaces, RAG, MCP, settings, portable
mode) is platform-neutral. **Windows is verified end-to-end on-device; macOS/Linux builds are
correct-by-construction and pending verification in per-platform CI** — which, with real
llama.cpp macOS-arm64 / Linux release assets wired into the asset table, closes the gap.

## 9. Milestones

| | Scope | Status |
|---|---|---|
| **M1** | Shell, design system, hardware detection + live telemetry, recommendation engine, workspace create/switch, system dashboard | **this milestone** |
| **M2** | Model downloads (resumable/verified), llama-server sidecar lifecycle, chat interface with streaming | next |
| **M3** | RAG pipeline (LanceDB + embedding sidecar), document ingestion UX, memory graph | |
| **M4** | Fine-tuning workflows (LoRA via sidecar), workspace export/import | |
| **M5** | Auto-update, crash reporting, installer polish, onboarding tour | |

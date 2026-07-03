# Athanor

**Local AI, assembled.** A Black Box Analytics product.

Athanor makes running local AI models dead simple for any skill level. Its core idea is
**purpose-built workspaces**: AI stacks tuned for a specific job — game-dev assistant,
legal doc reviewer, code helper — each with its own models, RAG corpus, and memory,
switched like projects in an IDE.

The app assesses your hardware on launch, recommends the best models your machine can
actually run, and keeps everything sandboxed in its own directory tree — no Python
installs, no CUDA setup, no conflicts with Docker or existing tooling.

## Status — M1

- ✅ Hardware intelligence: CPU/RAM/disk detection, NVML GPU probe with WMI + registry
  fallback, live 1 Hz telemetry
- ✅ Curated model catalog (19 entries) + recommendation engine, fit-checked per machine
- ✅ Workspace create / switch / delete (self-contained directories)
- ✅ System dashboard, models browser, workspace manager — full custom design system
- 🔜 M2: one-click model downloads, llama.cpp runtime, chat
- 🔜 M3: RAG (LanceDB) + memory graph per workspace

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the full system design and roadmap.

## Development

Prereqs: Node 20+, Rust (stable, MSVC toolchain on Windows), WebView2 (ships with Windows 11).

```sh
npm install
npm run tauri dev     # full desktop app (Rust core + UI)
npm run dev           # UI only, in a browser, with the design harness standing in
                      # for the Rust core (synthetic hardware/telemetry)
```

Tests and checks:

```sh
cd src-tauri
cargo test            # backend unit tests (recommender, catalog, classification)
cargo clippy          # lints
cd ..
npm run build         # tsc + production bundle
```

## Structure

```
src-tauri/            Rust core: hardware, models/recommender, workspaces, IPC
src/                  React UI: views, components, design system (styles/)
docs/ARCHITECTURE.md  System design, IPC contract, milestones
```

© Black Box Analytics. All rights reserved.

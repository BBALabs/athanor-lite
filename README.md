# Athanor

**Local AI, assembled.** A Black Box Analytics product.

Athanor makes running local AI models dead simple for any skill level. Its core idea is
**purpose-built workspaces**: AI stacks tuned for a specific job — game-dev assistant,
legal doc reviewer, code helper — each with its own models, RAG corpus, and memory,
switched like projects in an IDE.

The app assesses your hardware on launch, recommends the best models your machine can
actually run, and keeps everything sandboxed in its own directory tree — no Python
installs, no CUDA setup, no conflicts with Docker or existing tooling.

## Status — M2 (v1 feature-complete core)

- ✅ Hardware intelligence: CPU/RAM/disk detection, NVML GPU probe with WMI + registry
  fallback, architecture identification (Blackwell+), live 1 Hz local monitor
- ✅ Curated catalog (19 entries) with verified per-quant file manifests (exact sizes,
  LFS sha256) + recommendation engine, fit-checked per machine
- ✅ Resumable, checksum-verified model downloads into a content-addressed library
- ✅ Managed llama.cpp runtime (pinned build, architecture-aware CUDA 12/13/CPU) and
  streaming chat with measured TTFT / tok/s on every reply
- ✅ Per-workspace conversations; workspace purpose as standing instruction
- ✅ Ollama import-in-place (hard links, zero re-download)
- ✅ Local OpenAI-compatible API (stable port + bearer key, localhost-only)
- ✅ Local-first performance records; sharing opt-in (default OFF) with the literal
  payload on screen
- ✅ Guided onboarding: install → private chat in minutes; data-safety hardening
  (atomic writes, schema versioning, single-instance, trash-based deletes)
- 🔜 M3: RAG (LanceDB) + memory graph per workspace, MCP client
- 🔜 App signing + auto-updater (needs a code-signing certificate)

Dev self-tests (real network + GPU): `ATHANOR_SELFTEST=chat npm run tauri dev` and
`ATHANOR_SELFTEST=import npm run tauri dev`.

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

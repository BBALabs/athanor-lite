# Condere M1 — power-user audit (Tony persona: 1yr Ollama/Open WebUI, 40 models/~300GB, 2x P40 + laptop, scripts against OpenAI-compat API)

## 1. Dealbreakers — why I'd close it in 5 minutes today

- **It can't run a model.** No inference, no chat, no downloads (M2). M1 is a hardware dashboard with a shopping list. Beautiful, but I already have `nvidia-smi` and a brain.
- **It can't see my 300GB.** The model store is a content-addressed `models/<sha256>/` dir planned for M2 (ARCHITECTURE.md §6) with zero mention of importing existing GGUFs or Ollama blobs — which are *already sha256-addressed* in `~/.ollama/models/blobs`. Re-downloading 300GB I own is an instant uninstall. No import = no power users, period.
- **No custom models.** Catalog is "curated, not scraped," 19 entries embedded in the binary (`catalog.json`). No "paste an HF repo," no "add local GGUF." My daily drivers include finetunes that will never be in anyone's curated list.
- **No API server, no headless.** ARCHITECTURE.md never mentions exposing the llama-server sidecar's OpenAI-compatible endpoint, and it's a Tauri GUI with Windows-only WMI/registry probing (`gpu.rs` `#[cfg(windows)]`). My inference box is a headless Ubuntu server behind a Cloudflare tunnel. If my scripts and n8n flows can't hit `http://host:port/v1/chat/completions`, this is a toy.
- **Multi-GPU is explicitly ignored.** `HardwareReport::max_gpu_vram_gb()` takes the *largest single GPU* — the comment admits "Multi-GPU splits are an M2+ concern" (`hardware/mod.rs:102`). My 2x P40 = 48GB usable via `--tensor-split`; Condere sees 24GB and tells me Llama-3.3-70B "exceeds this machine." It does not.

## 2. Where the recommendation math is naive vs. hand-tuned reality

All in `recommend.rs` + `catalog.json`:

- **Binary GpuFull|CpuOnly — no partial offload.** The single biggest omission. The entire art of `num_gpu`/`-ngl` is putting 25 of 80 layers on the card. A 12GB GPU + 64GB RAM runs 70B Q4 at reading speed; Condere's model of the world says impossible. There is no layer-split math anywhere.
- **KV cache frozen at 8K, hand-entered.** `minMemGb` is a static per-quant number "@ 8K ctx" (ARCHITECTURE.md §5). No scaling with requested context (KV grows linearly with n_ctx; a 131K-ctx model marked "fits" is only true at 8K), no GQA head-count awareness, and **no KV-cache quantization** — `q8_0` cache halves KV memory and I run it everywhere; the data model can't even express it.
- **MoE treated as dense.** GPT-OSS-20B and Qwen3-Coder-30B-A3B are in the catalog, but there's no expert-offload concept (`--n-cpu-moe` / offloading `ffn_*_exps` to CPU) — the exact trick that makes 30B-A3B scream on an 8GB card and makes GPT-OSS-120B viable on 64GB RAM + any GPU. That's the most important scheduling development in local inference this year and it's invisible here.
- **Speed is never modeled — only fit.** A P40 (Pascal, cc 6.1, no tensor cores, anemic FP16) and a 4090 with the same VRAM get identical picks and the same cheery "comfortable fit" note. The app *detects* architecture beautifully (`architecture_for()` in `gpu.rs`) and then the recommender ignores it. Also nothing about flash-attn availability, row-split, or memory bandwidth on the CPU path.
- **No draft/speculative decoding.** No companion-draft-model concept in catalog or picks.
- **Constants are shaky and self-contradictory.** Code: `VRAM*0.95 − 0.5GB` (`recommend.rs:11-13`); ARCHITECTURE.md §5 says `×0.90 − 1.0GB`. Docs and code disagree on the product's core equation. And 0.95/−0.5 is optimistic on the display GPU: Windows DWM + the WebView2 app itself eat 1–2GB, and llama.cpp's CUDA compute buffers alone exceed 0.5GB at long context.
- **UI "tight" verdict is actually "will OOM."** `fitVerdict` in `Models.tsx:24-28` labels anything up to **15% over budget** as "tight" instead of "no." Over budget is over budget; on top of the 0.95 fraction that's a designed-in out-of-memory wall — the exact thing promise #1 in ARCHITECTURE.md says can never happen.
- **Catalog gaps for its own users:** no vision models (Qwen2.5-VL — the thing normies actually want), no Q5/Q6/IQ/imatrix quants on most entries (usually a lone Q4_K_M), no whisper/TTS roles, catalog says "~15 entries" but ships 19.

## 3. What it has that Ollama genuinely lacks — and I'd envy

- **Pre-flight fit truth.** Ollama's answer to "will this fit?" is *pull 40GB and find out*, then silently spill to CPU and gaslight you about why it's slow. Condere's per-quant fit table with headroom GB/% *before* download (recommend.rs `Pick`, Models.tsx quant table) attacks Ollama's single worst behavior.
- **Honest VRAM detection.** The NVML → WMI → registry `qwMemorySize` chain with the documented u32 `AdapterRAM` 4GB-cap trap, plus Blackwell identification from compute capability instead of a name table (`gpu.rs`) — this is more careful than anything in the Ollama codebase's Windows path.
- **Integrated 1Hz telemetry** (`telemetry.rs`) — replaces my permanent tmux `watch nvidia-smi` pane.
- **Workspaces as portable directories** — per-project models + RAG + memory + chats in one deletable dir, referencing a shared content-addressed blob store with refcounts (ARCHITECTURE.md §6). Nothing in Ollama/Open WebUI/LM Studio has this shape; it's the one genuinely original idea in the product.
- **A curated quality ordinal.** Ollama's library page tells a newbie nothing; "best coding model your machine can hold, with the receipts" is real value — for first-timers.

## 4. The 3 features that would make me switch tomorrow

1. **Library import + open catalog.** Scan `~/.ollama/models/blobs` and arbitrary GGUF folders, hash-match into the content-addressed store (zero re-download), and accept any HF repo. This is nearly free given their sha256 design and it's the entire power-user acquisition funnel.
2. **Per-workspace OpenAI-compatible API + headless mode.** llama-server already *is* the endpoint — expose it (port + token toggle) and give me a CLI/daemon mode on Linux. Then Condere becomes my server's control plane instead of another Windows tray toy.
3. **A real runtime configurator that matches llama.cpp reality:** layer-offload slider with live VRAM projection, context-length-scaled KV math, KV-quant options, tensor-split across both P40s, MoE expert offload, optional draft model. The recommendation engine is a pure, unit-tested function (`recommend.rs`) — the architecture can absorb this; the current math just doesn't try.

## 5. What I'd tell my Discord

"New Tauri app called Condere — think LM Studio with an actual design budget. The hardware detection is legit (it correctly reads VRAM on Windows where everyone else trusts WMI's lying 4GB cap, IDs Blackwell by compute capability) and it tells you which quant fits *before* you download, which is the thing Ollama should have done years ago. The workspace idea — self-contained project dirs with their own models/RAG/memory — is genuinely good. BUT: it's M1. No chat, no downloads, no API, can't import your existing models, thinks your dual-GPU rig is a single card, and its fit math doesn't know partial offload, KV quant, or MoE expert offload exist — so every number it shows a power user is somewhere between conservative and wrong. Verdict: gorgeous skeleton, aimed at first-timers for now. Watch it at M2/M3; if it ships library import + an exposed llama-server endpoint, it gets interesting. If not, it's a very pretty nvidia-smi."

Files cited: `C:/Users/tonyw/claude-workspace/projects/condere/docs/ARCHITECTURE.md`, `src-tauri/src/models/recommend.rs`, `src-tauri/src/models/catalog.json`, `src-tauri/src/hardware/mod.rs`, `src-tauri/src/hardware/gpu.rs`, `src-tauri/src/hardware/telemetry.rs`, `src/views/Models.tsx`, `src/views/Dashboard.tsx`, `src/views/Workspaces.tsx`.
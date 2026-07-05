# Athanor Lite

**See exactly what AI your machine can run — then run it. Free, local, private.**

Athanor Lite is a Windows desktop app that scans your hardware, tells you which
open-source AI models actually fit your GPU (with honest memory math, not
guesswork), and gets you from "which model?" to a private, streaming chat in one
click. Everything runs on your machine. Nothing you type ever leaves it.

Built by [Black Box Analytics](https://bbasecure.com).

---

## Features

**Hardware dashboard** — a live instrument cluster for your machine: GPU
identification (architecture, driver, CUDA), VRAM power ring, CPU / memory /
GPU / disk meters at 1 Hz, and a built-in speed benchmark that measures real
tokens-per-second on your hardware.

**Model browser with honest recommendations** — a curated catalog of verified
models (exact file sizes, SHA-256 checksums). Every quant of every model gets a
fit verdict computed from *your* GPU: fits fully, fits tight, needs a GPU+CPU
split, CPU-only, or exceeds this machine. The best pick for your hardware leads
the page. Downloads are resumable, checksum-verified, and pre-flighted against
free disk space.

**Library** — everything installed in one place: watch downloads land with live
progress, then start chatting with any model in one press, or reclaim the disk
with a two-step delete.

**Adopt your Ollama library** — already using Ollama? Athanor Lite finds your
models and adopts them in place with hard links. Zero bytes copied, zero bytes
re-downloaded.

**Private by construction** — models run locally through a managed
[llama.cpp](https://github.com/ggml-org/llama.cpp) runtime. No account, no
cloud, no telemetry unless you explicitly opt in (and the app shows you the
literal payload before you do).

## Screenshots

*Screenshots coming soon.*

## Prerequisites

- **Windows 10/11** (64-bit)
- **WebView2 Runtime** — ships with Windows 11; Windows 10 users can get it from
  [Microsoft](https://developer.microsoft.com/microsoft-edge/webview2/)
- **[Ollama](https://ollama.com)** *(optional)* — only needed if you want to
  adopt an existing Ollama model library in place
- An NVIDIA GPU is recommended for accelerated inference; without one, models
  run on the CPU (slower, still private)

## Building from source

Toolchain:

- [Node.js](https://nodejs.org) 20.19+ (or 22+)
- [Rust](https://rustup.rs) (stable, MSVC toolchain)

Build and run:

```sh
npm install
npm run tauri dev      # run the desktop app in development
npm run tauri build    # produce a release build
```

UI-only development in a browser (synthetic hardware data, no Rust toolchain
needed):

```sh
npm run dev
```

Backend tests:

```sh
cd src-tauri
cargo test
```

## The full version

Athanor Lite is the free edition of **Athanor** — purpose-built AI workspaces
with document knowledge (RAG), tool connections (MCP), multi-model compare,
fine-tuning, and more.

**[bbasecure.com/athanor](https://bbasecure.com/athanor)**

## License

© 2026 Tony Winslow / Black Box Analytics. Source is provided for building,
evaluation, and personal use — see [LICENSE](LICENSE) for terms.

## Credits

Designed and built by **Tony Winslow** — [Black Box Analytics](https://bbasecure.com).

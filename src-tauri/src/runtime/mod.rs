//! The inference runtime: a managed llama.cpp `llama-server` installation and
//! the lifecycle of the server process that serves the active workspace.
//!
//! The runtime is downloaded like a model — pinned build, progress events,
//! extracted into `runtimes/<tag>-<backend>/`. Release zips are flat archives;
//! llama-server.exe is a stub that loads a DLL forest, so the extracted
//! directory is the unit of installation. The CUDA build needs the companion
//! cudart zip extracted into the same directory (verified: the build zip does
//! NOT bundle the CUDA runtime DLLs).
//!
//! Silent-CPU-fallback trap: with dynamic backends, a broken ggml-cuda.dll
//! does not kill the process — it quietly runs on CPU. We watch the server's
//! stderr for the CUDA backend-load line and report `gpu_active` honestly.

pub mod api;
pub mod guard;
pub mod server;

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{AthanorError, Result};
use crate::hardware::gpu;
use crate::ops::{OpKind, Ops};
use crate::workspaces;

pub const EVENT_STATE: &str = "runtime://state";

/// Pinned llama.cpp release (verified against the GitHub API 2026-07-03).
/// CUDA 12.4 is chosen over 13.3 for architecture breadth (Pascal+).
pub const LLAMA_TAG: &str = "b9867";

/// The server binary's name — `.exe` on Windows, bare elsewhere.
#[cfg(windows)]
pub const LLAMA_BINARY: &str = "llama-server.exe";
#[cfg(not(windows))]
pub const LLAMA_BINARY: &str = "llama-server";

struct RuntimeAsset {
    url: &'static str,
    /// Expected size from the release API — a sanity check, not a hash
    /// (GitHub release assets carry no content hash).
    size_bytes: u64,
}

const CUDA12_BUILD: RuntimeAsset = RuntimeAsset {
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b9867/llama-b9867-bin-win-cuda-12.4-x64.zip",
    size_bytes: 266_142_585,
};
const CUDA12_RUNTIME: RuntimeAsset = RuntimeAsset {
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b9867/cudart-llama-bin-win-cuda-12.4-x64.zip",
    size_bytes: 391_443_627,
};
const CUDA13_BUILD: RuntimeAsset = RuntimeAsset {
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b9867/llama-b9867-bin-win-cuda-13.3-x64.zip",
    size_bytes: 161_400_603,
};
const CUDA13_RUNTIME: RuntimeAsset = RuntimeAsset {
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b9867/cudart-llama-bin-win-cuda-13.3-x64.zip",
    size_bytes: 390_970_417,
};
const CPU_BUILD: RuntimeAsset = RuntimeAsset {
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b9867/llama-b9867-bin-win-cpu-x64.zip",
    size_bytes: 17_486_019,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Backend {
    /// CUDA 12.4 build — carries kernels back to Pascal (P40-class cards).
    Cuda12,
    /// CUDA 13.x build — carries kernels for the newest architectures
    /// (Blackwell sm_120); drops pre-Turing.
    Cuda13,
    Cpu,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeState {
    pub phase: String, // "checking"|"downloading"|"extracting"|"ready"|"error"
    pub backend: Backend,
    pub tag: String,
    pub received_bytes: u64,
    pub total_bytes: u64,
    pub detail: String,
}

fn emit_state(app: &AppHandle, s: &RuntimeState) {
    let _ = app.emit(EVENT_STATE, s);
}

/// Backend by compute capability: modern cards (Turing+, CC >= 7.5) get the
/// CUDA 13 build — required for Blackwell (CC 12.x), whose kernels do not
/// exist in the 12.4 build (the cause of a silent CPU fallback we hit on the
/// RTX PRO 6000). Pascal/Volta-era cards get the 12.4 build, which still
/// carries their kernels.
pub fn pick_backend() -> Backend {
    let Some(nvml) = gpu::nvml() else {
        return Backend::Cpu;
    };
    let cc = nvml
        .device_by_index(0)
        .ok()
        .and_then(|d| d.cuda_compute_capability().ok());
    match cc {
        Some(cc) if cc.major > 7 || (cc.major == 7 && cc.minor >= 5) => Backend::Cuda13,
        Some(_) => Backend::Cuda12,
        None => Backend::Cuda12,
    }
}

pub fn runtime_dir(app: &AppHandle, backend: Backend) -> Result<PathBuf> {
    let name = match backend {
        Backend::Cuda12 => format!("{LLAMA_TAG}-cuda12"),
        Backend::Cuda13 => format!("{LLAMA_TAG}-cuda13"),
        Backend::Cpu => format!("{LLAMA_TAG}-cpu"),
    };
    Ok(workspaces::data_root(app)?.join("runtimes").join(name))
}

/// Download + extract the runtime if missing. Blocking; call from a blocking
/// task. Emits `runtime://state` throughout; registered in the operations
/// registry as a cancellable fetch.
pub fn ensure_runtime(app: &AppHandle, backend: Backend) -> Result<PathBuf> {
    let dir = runtime_dir(app, backend)?;
    let exe = dir.join(LLAMA_BINARY);
    if exe.exists() {
        return Ok(exe);
    }

    // The prebuilt llama.cpp runtime we bundle is Windows-only today. The rest
    // of the app (hardware, workspaces, RAG, settings, portable mode) is
    // platform-neutral; this is the one honest boundary until macOS/Linux
    // release assets are wired and verified on those platforms in CI. The two
    // cfg blocks are mutually exclusive tail expressions — exactly one survives
    // compilation and returns the function's value.
    #[cfg(not(windows))]
    {
        Err(AthanorError::Runtime(format!(
            "the managed inference engine is currently bundled for Windows only \
             ({} support is in progress)",
            std::env::consts::OS
        )))
    }
    #[cfg(windows)]
    {
        let ops = app.state::<Ops>();
        let cancel = ops
            .begin(
                app,
                "engine-fetch",
                OpKind::EngineFetch,
                &format!("Inference engine {LLAMA_TAG}"),
                true,
                None,
            )
            .ok_or_else(|| AthanorError::Runtime("engine fetch already running".into()))?;

        let result = fetch_runtime(app, backend, &dir, &cancel);
        match &result {
            Ok(_) => ops.done(app, "engine-fetch"),
            Err(e) if e.to_string().contains("cancelled") => ops.cancelled(app, "engine-fetch"),
            Err(e) => ops.failed(app, "engine-fetch", &e.to_string()),
        }
        result
    }
}

fn fetch_runtime(
    app: &AppHandle,
    backend: Backend,
    dir: &std::path::Path,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<PathBuf> {
    use std::sync::atomic::Ordering;
    let ops = app.state::<Ops>();

    let assets: &[&RuntimeAsset] = match backend {
        Backend::Cuda12 => &[&CUDA12_BUILD, &CUDA12_RUNTIME],
        Backend::Cuda13 => &[&CUDA13_BUILD, &CUDA13_RUNTIME],
        Backend::Cpu => &[&CPU_BUILD],
    };
    let total: u64 = assets.iter().map(|a| a.size_bytes).sum();
    let mut state = RuntimeState {
        phase: "downloading".into(),
        backend,
        tag: LLAMA_TAG.into(),
        received_bytes: 0,
        total_bytes: total,
        detail: "fetching the inference engine".into(),
    };
    emit_state(app, &state);

    let staging = dir.with_extension("staging");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;

    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("athanor/{}", env!("CARGO_PKG_VERSION")))
        .timeout(None)
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| AthanorError::Runtime(e.to_string()))?;

    let mut done_bytes: u64 = 0;
    for asset in assets {
        let zip_path = staging.join(
            asset
                .url
                .rsplit('/')
                .next()
                .unwrap_or("asset.zip"),
        );
        let mut resp = client
            .get(asset.url)
            .send()
            .map_err(|e| AthanorError::Runtime(format!("engine download failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(AthanorError::Runtime(format!(
                "engine download failed: HTTP {}",
                resp.status()
            )));
        }
        let mut file = fs::File::create(&zip_path)?;
        let mut buf = vec![0u8; 1024 * 1024];
        let mut last = std::time::Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                drop(file);
                let _ = fs::remove_dir_all(&staging);
                return Err(AthanorError::Runtime("cancelled".into()));
            }
            let n = resp
                .read(&mut buf)
                .map_err(|e| AthanorError::Runtime(format!("engine download failed: {e}")))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            done_bytes += n as u64;
            if last.elapsed().as_millis() >= 300 {
                state.received_bytes = done_bytes;
                emit_state(app, &state);
                ops.progress(app, "engine-fetch", done_bytes, total, "fetching the inference engine");
                last = std::time::Instant::now();
            }
        }
        file.sync_all()?;

        state.phase = "extracting".into();
        state.detail = format!("unpacking {}", zip_path.file_name().unwrap_or_default().to_string_lossy());
        emit_state(app, &state);
        ops.detail(app, "engine-fetch", &state.detail);
        let f = fs::File::open(&zip_path)?;
        let mut archive =
            zip::ZipArchive::new(f).map_err(|e| AthanorError::Runtime(format!("bad archive: {e}")))?;
        archive
            .extract(&staging)
            .map_err(|e| AthanorError::Runtime(format!("extract failed: {e}")))?;
        fs::remove_file(&zip_path).ok();
        state.phase = "downloading".into();
    }

    if !staging.join(LLAMA_BINARY).exists() {
        let _ = fs::remove_dir_all(&staging);
        return Err(AthanorError::Runtime(format!(
            "extracted runtime is missing {LLAMA_BINARY}"
        )));
    }

    // Atomic-ish install: staging -> final. A crash leaves staging, retried next time.
    if dir.exists() {
        let _ = fs::remove_dir_all(dir);
    }
    fs::rename(&staging, dir)?;

    state.phase = "ready".into();
    state.received_bytes = total;
    state.detail = "engine installed".into();
    emit_state(app, &state);
    log::info!(target: "rt", "runtime {LLAMA_TAG} ({backend:?}) installed at {dir:?}");
    Ok(dir.join(LLAMA_BINARY))
}

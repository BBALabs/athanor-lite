//! The inference runtime: a managed llama.cpp `llama-server` installation and
//! the lifecycle of the server process that serves the active workspace.
//!
//! The runtime is downloaded like a model — pinned build, progress events,
//! extracted into `runtimes/<tag>-<backend>/`. Windows release archives are
//! ZIP files; macOS and Linux releases are tar.gz archives. The Windows CUDA
//! builds need a companion cudart zip extracted into the same directory
//! (verified: the build zip does NOT bundle the CUDA runtime DLLs).
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

// macOS builds — Metal acceleration is baked in, one build per architecture.
const MACOS_ARM64_BUILD: RuntimeAsset = RuntimeAsset {
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b9867/llama-b9867-bin-macos-arm64.tar.gz",
    size_bytes: 11_134_835,
};
const MACOS_X64_BUILD: RuntimeAsset = RuntimeAsset {
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b9867/llama-b9867-bin-macos-x64.tar.gz",
    size_bytes: 11_450_643,
};

// Linux builds — CPU and Vulkan (GPU-accelerated, works with NVIDIA and AMD).
const LINUX_X64_BUILD: RuntimeAsset = RuntimeAsset {
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b9867/llama-b9867-bin-ubuntu-x64.tar.gz",
    size_bytes: 15_862_965,
};
const LINUX_ARM64_BUILD: RuntimeAsset = RuntimeAsset {
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b9867/llama-b9867-bin-ubuntu-arm64.tar.gz",
    size_bytes: 12_864_189,
};
const LINUX_VULKAN_X64_BUILD: RuntimeAsset = RuntimeAsset {
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b9867/llama-b9867-bin-ubuntu-vulkan-x64.tar.gz",
    size_bytes: 31_212_832,
};
const LINUX_VULKAN_ARM64_BUILD: RuntimeAsset = RuntimeAsset {
    url: "https://github.com/ggml-org/llama.cpp/releases/download/b9867/llama-b9867-bin-ubuntu-vulkan-arm64.tar.gz",
    size_bytes: 25_511_976,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Backend {
    /// CUDA 12.4 build — carries kernels back to Pascal (P40-class cards).
    Cuda12,
    /// CUDA 13.x build — carries kernels for the newest architectures
    /// (Blackwell sm_120); drops pre-Turing.
    Cuda13,
    /// Windows CPU-only fallback.
    Cpu,
    /// macOS — Metal acceleration is baked into the build (arm64 or x64 selected by arch).
    Metal,
    /// Linux with GPU — Vulkan backend supports both NVIDIA and AMD.
    Vulkan,
    /// Linux CPU-only fallback.
    LinuxCpu,
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

/// Select the best backend for the current machine.
///
/// - macOS: always Metal (baked into the release build).
/// - Linux: Vulkan if any GPU is detected via NVML, otherwise CPU.
/// - Windows: CUDA 13 for Turing+ cards, CUDA 12 for Pascal/Volta, CPU if no NVIDIA GPU.
pub fn pick_backend() -> Backend {
    #[cfg(target_os = "macos")]
    return Backend::Metal;

    #[cfg(target_os = "linux")]
    {
        return if gpu::nvml().is_some() {
            Backend::Vulkan
        } else {
            Backend::LinuxCpu
        };
    }

    #[cfg(windows)]
    {
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
}

pub fn runtime_dir(app: &AppHandle, backend: Backend) -> Result<PathBuf> {
    let name = match backend {
        Backend::Cuda12 => format!("{LLAMA_TAG}-cuda12"),
        Backend::Cuda13 => format!("{LLAMA_TAG}-cuda13"),
        Backend::Cpu => format!("{LLAMA_TAG}-cpu"),
        Backend::Metal => format!("{LLAMA_TAG}-metal"),
        Backend::Vulkan => format!("{LLAMA_TAG}-vulkan"),
        Backend::LinuxCpu => format!("{LLAMA_TAG}-linux-cpu"),
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

/// Select the download assets for the current platform and backend.
fn platform_assets(backend: Backend) -> Vec<&'static RuntimeAsset> {
    #[cfg(windows)]
    return match backend {
        Backend::Cuda12 => vec![&CUDA12_BUILD, &CUDA12_RUNTIME],
        Backend::Cuda13 => vec![&CUDA13_BUILD, &CUDA13_RUNTIME],
        _ => vec![&CPU_BUILD],
    };

    #[cfg(target_os = "macos")]
    {
        let _ = backend;
        return if cfg!(target_arch = "aarch64") {
            vec![&MACOS_ARM64_BUILD]
        } else {
            vec![&MACOS_X64_BUILD]
        };
    }

    #[cfg(target_os = "linux")]
    return match backend {
        Backend::Vulkan => {
            if cfg!(target_arch = "aarch64") {
                vec![&LINUX_VULKAN_ARM64_BUILD]
            } else {
                vec![&LINUX_VULKAN_X64_BUILD]
            }
        }
        _ => {
            if cfg!(target_arch = "aarch64") {
                vec![&LINUX_ARM64_BUILD]
            } else {
                vec![&LINUX_X64_BUILD]
            }
        }
    };

    #[allow(unreachable_code)]
    vec![&CPU_BUILD]
}

/// Extract a downloaded archive into `dest`.
/// Windows archives are ZIP; macOS and Linux archives are tar.gz.
fn extract_archive(archive_path: &std::path::Path, dest: &std::path::Path) -> Result<()> {
    #[cfg(windows)]
    {
        let f = fs::File::open(archive_path)?;
        let mut archive =
            zip::ZipArchive::new(f).map_err(|e| AthanorError::Runtime(format!("bad archive: {e}")))?;
        archive
            .extract(dest)
            .map_err(|e| AthanorError::Runtime(format!("extract failed: {e}")))?;
    }

    #[cfg(unix)]
    {
        let status = std::process::Command::new("tar")
            .args([
                "xzf",
                archive_path.to_str().unwrap_or_default(),
                "-C",
                dest.to_str().unwrap_or_default(),
            ])
            .status()
            .map_err(|e| AthanorError::Runtime(format!("tar not found: {e}")))?;
        if !status.success() {
            return Err(AthanorError::Runtime(format!(
                "tar extraction failed (exit {})",
                status
            )));
        }
        // Ensure the server binary is executable after extraction.
        let binary = dest.join(LLAMA_BINARY);
        if binary.exists() {
            let _ = std::process::Command::new("chmod")
                .args(["+x", binary.to_str().unwrap_or_default()])
                .status();
        }
    }

    Ok(())
}

fn fetch_runtime(
    app: &AppHandle,
    backend: Backend,
    dir: &std::path::Path,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<PathBuf> {
    use std::sync::atomic::Ordering;
    let ops = app.state::<Ops>();

    let assets = platform_assets(backend);
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
        let archive_path = staging.join(
            asset
                .url
                .rsplit('/')
                .next()
                .unwrap_or("asset.bin"),
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
        let mut file = fs::File::create(&archive_path)?;
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
        state.detail = format!("unpacking {}", archive_path.file_name().unwrap_or_default().to_string_lossy());
        emit_state(app, &state);
        ops.detail(app, "engine-fetch", &state.detail);
        extract_archive(&archive_path, &staging)?;
        fs::remove_file(&archive_path).ok();
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

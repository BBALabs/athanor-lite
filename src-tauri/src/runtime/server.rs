//! llama-server process lifecycle: one server at a time, serving the active
//! workspace's model. Spawn → health-poll (503 while loading, 200 ready) →
//! serve → drain/kill on model switch or app exit.
//!
//! Process-control guarantees: bring-up is serialized (no double-spawn),
//! every child joins the app's job object (no orphans, even on hard kill),
//! and the running engine is a visible, stoppable row in the operations
//! registry at all times.

use std::io::BufRead;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{AthanorError, Result};
use crate::hardware::{gpu, GIB};
use crate::ops::{OpKind, Ops};

use super::{ensure_runtime, guard, pick_backend, Backend};

pub const EVENT_SERVER: &str = "runtime://server";

/// Context window the server is launched with. Matches the recommendation
/// engine's fit math (memory floors are computed at 8K).
pub const CTX_SIZE: u32 = 8192;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub phase: String, // "starting"|"loading"|"ready"|"stopped"|"error"
    pub model_sha: Option<String>,
    pub model_name: Option<String>,
    pub port: Option<u16>,
    pub backend: Option<Backend>,
    pub gpu_active: bool,
    /// VRAM the model load consumed (bytes), measured via NVML delta.
    pub vram_at_load_bytes: Option<u64>,
    pub detail: String,
}

impl ServerStatus {
    fn stopped() -> Self {
        ServerStatus {
            phase: "stopped".into(),
            model_sha: None,
            model_name: None,
            port: None,
            backend: None,
            gpu_active: false,
            vram_at_load_bytes: None,
            detail: String::new(),
        }
    }
}

pub struct ActiveServer {
    pub child: Child,
    pub port: u16,
    pub model_sha: String,
    pub model_name: String,
    pub gpu_active: Arc<AtomicBool>,
    pub vram_at_load_bytes: Option<u64>,
    pub api_key: String,
}

/// Managed state: the single active server. `spawn` serializes engine
/// bring-up so concurrent callers can never race two servers onto the GPU —
/// the duplicate-process guarantee for inference.
#[derive(Default)]
pub struct Llm {
    active: Mutex<Option<ActiveServer>>,
    spawn: Mutex<()>,
}

impl Llm {
    pub fn lock(&self) -> std::sync::MutexGuard<'_, Option<ActiveServer>> {
        self.active.lock().unwrap_or_else(|p| p.into_inner())
    }
    fn spawn_guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.spawn.lock().unwrap_or_else(|p| p.into_inner())
    }
}

fn free_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| AthanorError::Runtime(format!("no free port: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| AthanorError::Runtime(e.to_string()))?
        .port();
    Ok(port)
}

/// VRAM held by a specific process on device 0. Under Windows WDDM, NVML
/// usually cannot attribute per-process memory — callers must fall back to
/// the tight-window device delta.
fn process_vram(pid: u32) -> Option<u64> {
    use nvml_wrapper::enums::device::UsedGpuMemory;
    let nvml = gpu::nvml()?;
    let dev = nvml.device_by_index(0).ok()?;
    let mut procs = dev.running_compute_processes().unwrap_or_default();
    procs.extend(dev.running_graphics_processes().unwrap_or_default());
    procs.iter().find(|p| p.pid == pid).and_then(|p| match p.used_gpu_memory {
        UsedGpuMemory::Used(bytes) if bytes > 0 => Some(bytes),
        _ => None,
    })
}

fn device_vram_used() -> Option<u64> {
    let nvml = gpu::nvml()?;
    let dev = nvml.device_by_index(0).ok()?;
    Some(dev.memory_info().ok()?.used)
}

fn emit_status(app: &AppHandle, s: &ServerStatus) {
    let _ = app.emit(EVENT_SERVER, s);
}

pub fn stop(app: &AppHandle, llm: &Llm) {
    let mut guard = llm.lock();
    if let Some(mut active) = guard.take() {
        log::info!(target: "rt", "stopping llama-server on port {}", active.port);
        let _ = active.child.kill();
        let _ = active.child.wait();
    }
    drop(guard);
    if let Some(ops) = app.try_state::<Ops>() {
        ops.done(app, "engine");
    }
    emit_status(app, &ServerStatus::stopped());
}

/// Ensure a server is running for `model_sha`. Reuses a live server already
/// holding that model; otherwise replaces the active server. Returns the port.
/// Serialized: concurrent callers queue and re-check instead of double-spawning.
pub fn ensure(app: &AppHandle, llm: &Llm, model_sha: &str) -> Result<u16> {
    let already_live = |llm: &Llm| -> Option<u16> {
        let mut guard = llm.lock();
        if let Some(active) = guard.as_mut() {
            if active.model_sha == model_sha {
                if let Ok(None) = active.child.try_wait() {
                    return Some(active.port);
                }
                log::warn!(target: "rt", "llama-server exited unexpectedly; restarting");
            }
        }
        None
    };

    // Fast path without the spawn lock…
    if let Some(port) = already_live(llm) {
        return Ok(port);
    }
    // …then serialize bring-up and re-check (a queued waiter usually finds
    // the engine its predecessor just started).
    let _spawning = llm.spawn_guard();
    if let Some(port) = already_live(llm) {
        return Ok(port);
    }

    // Anything else live gets stopped first (one engine at a time).
    stop(app, llm);

    let library = crate::downloads::list_library(app)?;
    let model = library
        .iter()
        .find(|m| m.sha256 == model_sha)
        .ok_or_else(|| AthanorError::Runtime("model not in library".into()))?
        .clone();

    // The engine is a first-class, visible, stoppable operation.
    let ops = app.state::<Ops>();
    let _ = ops.begin(
        app,
        "engine",
        OpKind::Engine,
        &format!("Engine · {}", model.display_name),
        true,
        None,
    );

    let result = bring_up(app, llm, &model);
    if let Err(e) = &result {
        ops.failed(app, "engine", &e.to_string());
        let mut status = ServerStatus::stopped();
        status.phase = "error".into();
        status.detail = e.to_string();
        emit_status(app, &status);
    }
    result
}

fn bring_up(app: &AppHandle, llm: &Llm, model: &crate::downloads::LibraryModel) -> Result<u16> {
    let ops = app.state::<Ops>();
    let backend = pick_backend();
    let mut status = ServerStatus {
        phase: "starting".into(),
        model_sha: Some(model.sha256.clone()),
        model_name: Some(model.display_name.clone()),
        port: None,
        backend: Some(backend),
        gpu_active: false,
        vram_at_load_bytes: None,
        detail: "preparing the engine".into(),
    };
    emit_status(app, &status);
    ops.detail(app, "engine", "preparing the engine");

    let exe = ensure_runtime(app, backend)?;

    // Exposed mode uses a stable port + persistent key so external tools
    // survive engine restarts; private mode stays ephemeral.
    let api = super::api::get_settings(app)?;
    let (port, api_key) = if api.expose {
        (api.port, api.api_key.clone())
    } else {
        (free_port()?, uuid::Uuid::new_v4().to_string())
    };

    // Sampled immediately before spawn (runtime install is already done) so
    // the load-delta window is seconds, not minutes.
    let vram_before = device_vram_used();

    let mut cmd = Command::new(&exe);
    cmd.current_dir(exe.parent().expect("exe has a parent dir"))
        .args([
            "-m",
            &model.path,
            // External clients (/v1/models) see a clean name, not a path.
            "-a",
            &model.display_name,
            "-c",
            &CTX_SIZE.to_string(),
            "-ngl",
            "auto",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--api-key",
            &api_key,
            "--no-webui",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| AthanorError::Runtime(format!("failed to start llama-server: {e}")))?;

    // No orphans: the child dies with the app, even on a hard kill.
    guard::adopt(&child);

    // Forward both output streams to the log — the engine's own report is the
    // only diagnosis channel a user can send us.
    let gpu_active = Arc::new(AtomicBool::new(false));
    for (name, stream) in [
        ("llm-stderr", child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>)),
        ("llm-stdout", child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>)),
    ] {
        if let Some(stream) = stream {
            std::thread::Builder::new()
                .name(name.into())
                .spawn(move || {
                    let reader = std::io::BufReader::new(stream);
                    for line in reader.lines().map_while(|l| l.ok()) {
                        log::info!(target: "llm", "{line}");
                    }
                })
                .ok();
        }
    }

    // Health poll: 503 while loading, 200 when ready. Timeout scales with
    // model size (cold NVMe reads of 40GB take a while).
    status.phase = "loading".into();
    status.port = Some(port);
    status.detail = format!("loading {} into memory", model.display_name);
    emit_status(app, &status);
    ops.detail(app, "engine", &status.detail);

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| AthanorError::Runtime(e.to_string()))?;
    let health_url = format!("http://127.0.0.1:{port}/health");
    let budget = Duration::from_secs(45 + (model.size_bytes / GIB as u64) * 12);
    let started = Instant::now();

    loop {
        if let Ok(Some(code)) = child.try_wait() {
            return Err(AthanorError::Runtime(format!(
                "llama-server exited during load (code {code:?}) — see the log for the engine's own report"
            )));
        }
        if started.elapsed() > budget {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AthanorError::Runtime(format!(
                "model load timed out after {}s",
                budget.as_secs()
            )));
        }
        match client.get(&health_url).send() {
            Ok(resp) if resp.status().as_u16() == 200 => break,
            _ => std::thread::sleep(Duration::from_millis(400)),
        }
    }

    // Ground truth, best available signal first: per-process VRAM (exact, but
    // unavailable under WDDM), else the device delta across the seconds-wide
    // load window, thresholded at half the model's weights. A silent CPU
    // fallback shows neither.
    let vram_at_load = process_vram(child.id()).or_else(|| {
        match (vram_before, device_vram_used()) {
            (Some(before), Some(after)) if after > before => Some(after - before),
            _ => None,
        }
    });
    gpu_active.store(
        vram_at_load.map(|b| b >= model.size_bytes / 2).unwrap_or(false),
        Ordering::Relaxed,
    );

    status.phase = "ready".into();
    status.gpu_active = gpu_active.load(Ordering::Relaxed);
    status.vram_at_load_bytes = vram_at_load;
    status.detail = if status.gpu_active {
        "engine ready · GPU".into()
    } else {
        "engine ready · CPU".into()
    };
    emit_status(app, &status);
    ops.detail(app, "engine", "serving");
    ops.resource(
        app,
        "engine",
        &format!(
            "{} · port {port} · {}",
            vram_at_load
                .map(|b| format!("{:.1} GB VRAM", b as f64 / GIB))
                .unwrap_or_else(|| "no VRAM attributed".into()),
            if status.gpu_active { "GPU" } else { "CPU" }
        ),
    );
    log::info!(
        target: "rt",
        "llama-server ready: {} on port {port} (gpu={}, vram_delta={:?})",
        model.display_name,
        status.gpu_active,
        vram_at_load
    );

    let mut guard = llm.lock();
    *guard = Some(ActiveServer {
        child,
        port,
        model_sha: model.sha256.clone(),
        model_name: model.display_name.clone(),
        gpu_active,
        vram_at_load_bytes: vram_at_load,
        api_key,
    });
    Ok(port)
}

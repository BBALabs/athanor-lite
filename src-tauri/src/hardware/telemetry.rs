//! Live system telemetry: a dedicated sampler thread in the Rust core emits one
//! `telemetry://sample` event per second. The UI never polls — it subscribes.

use std::time::Duration;

use serde::Serialize;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
use tauri::{AppHandle, Emitter};

use super::gpu;

pub const EVENT_SAMPLE: &str = "telemetry://sample";
const SAMPLE_INTERVAL: Duration = Duration::from_millis(1000);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuTelemetry {
    pub index: u32,
    pub name: String,
    pub vram_total_bytes: u64,
    pub vram_used_bytes: u64,
    pub utilization_pct: u32,
    pub temperature_c: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetrySample {
    pub ts_ms: i64,
    pub cpu_usage_pct: f32,
    pub mem_total_bytes: u64,
    pub mem_used_bytes: u64,
    pub gpus: Vec<GpuTelemetry>,
}

fn sample_gpus() -> Vec<GpuTelemetry> {
    let Some(nvml) = gpu::nvml() else {
        return Vec::new();
    };
    let count = nvml.device_count().unwrap_or(0);
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        let Ok(dev) = nvml.device_by_index(i) else {
            continue;
        };
        let Ok(mem) = dev.memory_info() else {
            continue;
        };
        out.push(GpuTelemetry {
            index: i,
            name: dev.name().unwrap_or_else(|_| format!("GPU {i}")),
            vram_total_bytes: mem.total,
            vram_used_bytes: mem.used,
            utilization_pct: dev.utilization_rates().map(|u| u.gpu).unwrap_or(0),
            temperature_c: dev
                .temperature(nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu)
                .unwrap_or(0),
        });
    }
    out
}

/// Blocking sampler loop — run it on its own named thread.
pub fn run(app: AppHandle) {
    let mut sys = System::new_with_specifics(
        RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
            .with_memory(MemoryRefreshKind::nothing().with_ram()),
    );

    log::info!(target: "hw", "telemetry sampler online ({SAMPLE_INTERVAL:?} cadence)");

    loop {
        std::thread::sleep(SAMPLE_INTERVAL);
        sys.refresh_cpu_usage();
        sys.refresh_memory();

        let sample = TelemetrySample {
            ts_ms: chrono::Utc::now().timestamp_millis(),
            cpu_usage_pct: sys.global_cpu_usage(),
            mem_total_bytes: sys.total_memory(),
            mem_used_bytes: sys.used_memory(),
            gpus: sample_gpus(),
        };

        if let Err(e) = app.emit(EVENT_SAMPLE, &sample) {
            // Shutting down — the webview is gone; exit quietly.
            log::debug!(target: "hw", "telemetry emit failed ({e}); sampler stopping");
            break;
        }
    }
}

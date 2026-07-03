//! Local-first performance measurement.
//!
//! Every inference session writes ground truth to a local, user-inspectable
//! journal (`metrics/local.jsonl`) — the user's own performance history.
//! SHARING is a separate, explicit decision: default OFF, consent shown with
//! the literal payload, and the shared form strips identity (no hostname, no
//! serials, day-precision time). Uploads are not implemented yet; when opted
//! in, records queue locally and the settings surface says exactly that.
//!
//! Never recorded, in any form: prompts, completions, document names, chat
//! metadata.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::chat::GenStats;
use crate::error::Result;
use crate::hardware::GIB;
use crate::runtime::LLAMA_TAG;
use crate::workspaces::{self, write_atomic};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSettings {
    #[serde(default = "workspaces::schema_version_default")]
    pub schema: u32,
    /// Contribute anonymous performance records. Default: false, always.
    #[serde(default)]
    pub share: bool,
}

impl Default for MetricsSettings {
    fn default() -> Self {
        MetricsSettings { schema: 1, share: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HwKey {
    pub gpu: Option<String>,
    pub vram_gb: Option<f64>,
    pub driver: Option<String>,
    pub cpu: String,
    pub ram_gb: f64,
    pub os: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsRecord {
    pub schema: u32,
    /// Full precision locally; truncated to day in the shared payload.
    pub ts: String,
    pub event: String, // "generation" | "failure"
    pub hw: HwKey,
    pub llama_build: String,
    pub app_version: String,
    pub model_sha: String,
    pub ctx: u32,
    pub gpu_active: bool,
    pub ttft_ms: Option<u64>,
    pub prompt_n: Option<u32>,
    pub prompt_per_second: Option<f64>,
    pub predicted_n: Option<u32>,
    pub predicted_per_second: Option<f64>,
    pub vram_at_load_bytes: Option<u64>,
    pub error_kind: Option<String>,
}

fn metrics_dir(app: &AppHandle) -> Result<PathBuf> {
    let dir = workspaces::data_root(app)?.join("metrics");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn journal_path(app: &AppHandle) -> Result<PathBuf> {
    Ok(metrics_dir(app)?.join("local.jsonl"))
}

fn settings_path(app: &AppHandle) -> Result<PathBuf> {
    Ok(metrics_dir(app)?.join("settings.json"))
}

pub fn get_settings(app: &AppHandle) -> MetricsSettings {
    settings_path(app)
        .ok()
        .filter(|p| p.exists())
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn set_share(app: &AppHandle, share: bool) -> Result<MetricsSettings> {
    let settings = MetricsSettings { schema: 1, share };
    write_atomic(
        &settings_path(app)?,
        serde_json::to_string_pretty(&settings)?.as_bytes(),
    )?;
    log::info!(target: "metrics", "share set to {share}");
    Ok(settings)
}

fn hw_key(app: &AppHandle) -> HwKey {
    let _ = app; // reserved: future per-install salt lives in app data
    // Cheap re-probe of the identity fields only.
    let gpus = crate::hardware::gpu::detect_gpus();
    let primary = gpus.first();
    let mut sys = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::nothing()
            .with_cpu(sysinfo::CpuRefreshKind::nothing())
            .with_memory(sysinfo::MemoryRefreshKind::nothing().with_ram()),
    );
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    HwKey {
        gpu: primary.map(|g| g.name.clone()),
        vram_gb: primary
            .and_then(|g| g.vram_total_bytes)
            .map(|b| (b as f64 / GIB * 10.0).round() / 10.0),
        driver: primary.and_then(|g| g.driver_version.clone()),
        cpu: sys
            .cpus()
            .first()
            .map(|c| c.brand().trim().to_string())
            .unwrap_or_default(),
        ram_gb: (sys.total_memory() as f64 / GIB * 10.0).round() / 10.0,
        os: format!(
            "{} {}",
            sysinfo::System::name().unwrap_or_default(),
            sysinfo::System::os_version().unwrap_or_default()
        ),
    }
}

fn append(app: &AppHandle, record: &MetricsRecord) {
    let write = || -> Result<()> {
        let mut line = serde_json::to_string(record)?;
        line.push('\n');
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(journal_path(app)?)?;
        f.write_all(line.as_bytes())?;
        Ok(())
    };
    if let Err(e) = write() {
        log::warn!(target: "metrics", "journal append failed: {e}");
    }
}

pub fn record_generation(
    app: &AppHandle,
    model_sha: &str,
    stats: &GenStats,
    vram_at_load: Option<u64>,
    ctx: u32,
) {
    append(
        app,
        &MetricsRecord {
            schema: 1,
            ts: chrono::Utc::now().to_rfc3339(),
            event: "generation".into(),
            hw: hw_key(app),
            llama_build: LLAMA_TAG.into(),
            app_version: env!("CARGO_PKG_VERSION").into(),
            model_sha: model_sha.into(),
            ctx,
            gpu_active: stats.gpu_active,
            ttft_ms: Some(stats.ttft_ms),
            prompt_n: Some(stats.prompt_n),
            prompt_per_second: Some(stats.prompt_per_second),
            predicted_n: Some(stats.predicted_n),
            predicted_per_second: Some(stats.predicted_per_second),
            vram_at_load_bytes: vram_at_load,
            error_kind: None,
        },
    );
}

pub fn record_failure(app: &AppHandle, model_sha: &str, error: &str) {
    // Failures are the most valuable rows — they are what "never hit an OOM
    // wall" learns from. Only the error CLASS is recorded, never free text
    // beyond our own error message.
    append(
        app,
        &MetricsRecord {
            schema: 1,
            ts: chrono::Utc::now().to_rfc3339(),
            event: "failure".into(),
            hw: hw_key(app),
            llama_build: LLAMA_TAG.into(),
            app_version: env!("CARGO_PKG_VERSION").into(),
            model_sha: model_sha.into(),
            ctx: 0,
            gpu_active: false,
            ttft_ms: None,
            prompt_n: None,
            prompt_per_second: None,
            predicted_n: None,
            predicted_per_second: None,
            vram_at_load_bytes: None,
            error_kind: Some(error.chars().take(120).collect()),
        },
    );
}

/// Most recent records, newest first — the user's own performance history.
pub fn history(app: &AppHandle, limit: usize) -> Result<Vec<MetricsRecord>> {
    let path = journal_path(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path)?;
    let mut records: Vec<MetricsRecord> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    records.reverse();
    records.truncate(limit);
    Ok(records)
}

/// The literal JSON that WOULD be contributed if sharing were on — identity
/// stripped, day-precision time. Shown verbatim on the consent surface.
pub fn sample_shared_payload(app: &AppHandle) -> Result<serde_json::Value> {
    let latest = history(app, 1)?.into_iter().next();
    let record = latest.unwrap_or(MetricsRecord {
        schema: 1,
        ts: chrono::Utc::now().to_rfc3339(),
        event: "generation".into(),
        hw: hw_key(app),
        llama_build: LLAMA_TAG.into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        model_sha: "example".into(),
        ctx: 8192,
        gpu_active: true,
        ttft_ms: Some(412),
        prompt_n: Some(58),
        prompt_per_second: Some(812.4),
        predicted_n: Some(256),
        predicted_per_second: Some(43.7),
        vram_at_load_bytes: Some(21_474_836_480),
        error_kind: None,
    });
    let mut v = serde_json::to_value(&record)?;
    if let Some(obj) = v.as_object_mut() {
        // Day precision only in the shared form.
        let day = record.ts.chars().take(10).collect::<String>();
        obj.insert("ts".into(), serde_json::Value::String(day));
    }
    Ok(v)
}

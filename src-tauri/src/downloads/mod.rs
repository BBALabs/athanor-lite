//! Model downloads and the content-addressed library.
//!
//! Layout under the app data root:
//! ```text
//! models/
//! ├── .partial/<sha256>.part      # resumable in-flight downloads
//! └── <sha256>/
//!     ├── <file>.gguf             # the verified artifact
//!     └── metadata.json           # LibraryModel (schema-versioned)
//! ```
//! Guarantees: every byte is hashed while streaming and verified against the
//! catalog's Hugging Face LFS sha256 before an artifact enters the library;
//! interrupted downloads resume from the partial (re-hashed on resume); a
//! failed write can never corrupt an existing artifact (finalize is a rename).

pub mod ollama;

use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};

use crate::error::{AthanorError, Result};
use crate::models::{catalog, QuantFile};
use crate::ops::{OpKind, Ops, RetrySpec};
use crate::workspaces::{self, write_atomic};

pub const EVENT_PROGRESS: &str = "download://progress";
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);
const DISK_MARGIN_BYTES: u64 = 500 * 1024 * 1024;

pub fn op_id(sha256: &str) -> String {
    format!("dl:{sha256}")
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum DownloadState {
    Starting,
    Downloading,
    Verifying,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub sha256: String,
    pub entry_id: String,
    pub quant: String,
    pub file_name: String,
    pub received_bytes: u64,
    pub total_bytes: u64,
    pub bytes_per_sec: u64,
    pub state: DownloadState,
    pub error: Option<String>,
}

/// A model on disk. `metadata.json` in its content-addressed directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryModel {
    #[serde(default = "workspaces::schema_version_default")]
    pub schema: u32,
    pub sha256: String,
    pub file_name: String,
    /// Absolute path to the primary GGUF (may live outside the store for
    /// imported-in-place models).
    pub path: String,
    pub size_bytes: u64,
    pub display_name: String,
    /// Catalog linkage when the model came from our catalog.
    #[serde(default)]
    pub entry_id: Option<String>,
    #[serde(default)]
    pub quant: Option<String>,
    /// "huggingface" | "ollama" | "file"
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub added_at: String,
}

// ── Paths ─────────────────────────────────────────────────────

pub fn models_root(app: &AppHandle) -> Result<PathBuf> {
    let dir = workspaces::data_root(app)?.join("models");
    fs::create_dir_all(dir.join(".partial"))?;
    Ok(dir)
}

fn model_dir(app: &AppHandle, sha256: &str) -> Result<PathBuf> {
    if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AthanorError::Download(format!("invalid sha256: {sha256}")));
    }
    Ok(models_root(app)?.join(sha256))
}

fn partial_path(app: &AppHandle, sha256: &str) -> Result<PathBuf> {
    Ok(models_root(app)?.join(".partial").join(format!("{sha256}.part")))
}

// ── Library ───────────────────────────────────────────────────

pub fn list_library(app: &AppHandle) -> Result<Vec<LibraryModel>> {
    let root = models_root(app)?;
    let mut out = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let meta = entry.path().join("metadata.json");
        if !meta.exists() {
            continue;
        }
        match fs::read_to_string(&meta)
            .map_err(AthanorError::from)
            .and_then(|s| serde_json::from_str::<LibraryModel>(&s).map_err(AthanorError::from))
        {
            Ok(mut m) => {
                // Heal: artifact must actually exist (user may have hand-deleted it).
                if PathBuf::from(&m.path).exists() {
                    out.push(m);
                } else {
                    // Try the canonical in-store location before declaring it gone.
                    let canonical = entry.path().join(&m.file_name);
                    if canonical.exists() {
                        m.path = canonical.to_string_lossy().to_string();
                        out.push(m);
                    } else {
                        log::warn!(target: "dl", "library entry {} has no artifact on disk", m.sha256);
                    }
                }
            }
            Err(e) => log::warn!(target: "dl", "unreadable library metadata {meta:?}: {e}"),
        }
    }
    out.sort_by(|a, b| b.added_at.cmp(&a.added_at));
    Ok(out)
}

pub fn add_library_entry(app: &AppHandle, model: &LibraryModel) -> Result<()> {
    let dir = model_dir(app, &model.sha256)?;
    fs::create_dir_all(&dir)?;
    write_atomic(
        &dir.join("metadata.json"),
        serde_json::to_string_pretty(model)?.as_bytes(),
    )
}

pub fn delete_model(app: &AppHandle, sha256: &str) -> Result<Vec<LibraryModel>> {
    let dir = model_dir(app, sha256)?;
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
        log::info!(target: "dl", "removed model {sha256}");
    }
    list_library(app)
}

// ── Disk preflight ────────────────────────────────────────────

fn free_space_for(path: &std::path::Path) -> Option<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|d| path.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

// ── The download job ──────────────────────────────────────────

struct JobSpec {
    entry_id: String,
    quant: String,
    hf_repo: String,
    file: QuantFile,
}

fn resolve_spec(entry_id: &str, quant_label: &str) -> Result<JobSpec> {
    let cat = catalog()?;
    let entry = cat
        .entries
        .iter()
        .find(|e| e.id == entry_id)
        .ok_or_else(|| AthanorError::Download(format!("unknown model: {entry_id}")))?;
    let quant = entry
        .quants
        .iter()
        .find(|q| q.label == quant_label)
        .ok_or_else(|| AthanorError::Download(format!("unknown quant: {quant_label}")))?;
    match quant.files.as_slice() {
        [file] => Ok(JobSpec {
            entry_id: entry.id.clone(),
            quant: quant.label.clone(),
            hf_repo: entry.hf_repo.clone(),
            file: file.clone(),
        }),
        [] => Err(AthanorError::Download(format!(
            "{entry_id} {quant_label} has no download metadata"
        ))),
        _ => Err(AthanorError::Download(
            "split-file models are not supported yet".into(),
        )),
    }
}

fn emit(app: &AppHandle, p: &DownloadProgress) {
    let _ = app.emit(EVENT_PROGRESS, p);
}

/// Begin (or resume) a download. Returns immediately; progress arrives as
/// `download://progress` events keyed by sha256.
pub fn start(app: AppHandle, entry_id: &str, quant: &str) -> Result<()> {
    let spec = resolve_spec(entry_id, quant)?;
    let sha = spec.file.sha256.clone();

    // Already in the library?
    let final_dir = model_dir(&app, &sha)?;
    if final_dir.join("metadata.json").exists() {
        emit(
            &app,
            &DownloadProgress {
                sha256: sha,
                entry_id: spec.entry_id,
                quant: spec.quant,
                file_name: spec.file.name,
                received_bytes: spec.file.size_bytes,
                total_bytes: spec.file.size_bytes,
                bytes_per_sec: 0,
                state: DownloadState::Done,
                error: None,
            },
        );
        return Ok(());
    }

    // Duplicate guard + cancel authority live in the operations registry.
    let ops = app.state::<Ops>();
    let cancel = ops
        .begin(
            &app,
            &op_id(&sha),
            OpKind::Download,
            &format!("Download · {}", spec.file.name),
            true,
            Some(RetrySpec::Download {
                entry_id: spec.entry_id.clone(),
                quant: spec.quant.clone(),
            }),
        )
        .ok_or_else(|| AthanorError::Download("download already running".into()))?;

    tauri::async_runtime::spawn_blocking(move || {
        let sha = spec.file.sha256.clone();
        let id = op_id(&sha);
        let result = run_job(&app, &spec, &cancel);
        let ops = app.state::<Ops>();
        match &result {
            Ok(()) => ops.done(&app, &id),
            Err(e) => {
                log::warn!(target: "dl", "download {sha} ended: {e}");
                let cancelled = matches!(e, AthanorError::Download(ref m) if m == "cancelled");
                if cancelled {
                    ops.cancelled(&app, &id);
                } else {
                    ops.failed(&app, &id, &e.to_string());
                }
                emit(
                    &app,
                    &DownloadProgress {
                        sha256: sha,
                        entry_id: spec.entry_id.clone(),
                        quant: spec.quant.clone(),
                        file_name: spec.file.name.clone(),
                        received_bytes: 0,
                        total_bytes: spec.file.size_bytes,
                        bytes_per_sec: 0,
                        state: if cancelled {
                            DownloadState::Cancelled
                        } else {
                            DownloadState::Failed
                        },
                        error: Some(e.to_string()),
                    },
                );
            }
        }
    });

    Ok(())
}

/// Blocking install-if-missing. Used by the dev self-test and the onboarding
/// fast path; the evented `start` is the interactive route.
pub fn ensure_installed(app: &AppHandle, entry_id: &str, quant: &str) -> Result<LibraryModel> {
    let spec = resolve_spec(entry_id, quant)?;
    let sha = spec.file.sha256.clone();
    if let Some(m) = list_library(app)?.into_iter().find(|m| m.sha256 == sha) {
        return Ok(m);
    }
    run_job(app, &spec, &AtomicBool::new(false))?;
    list_library(app)?
        .into_iter()
        .find(|m| m.sha256 == sha)
        .ok_or_else(|| AthanorError::Download("install did not register in the library".into()))
}

fn run_job(app: &AppHandle, spec: &JobSpec, cancel: &AtomicBool) -> Result<()> {
    let part = partial_path(app, &spec.file.sha256)?;
    let dir = model_dir(app, &spec.file.sha256)?;
    let app2 = app.clone();
    let id = op_id(&spec.file.sha256);
    let final_path = run_job_core(&part, &dir, spec, cancel, move |p| {
        emit(&app2, p);
        if let Some(ops) = app2.try_state::<Ops>() {
            ops.progress(&app2, &id, p.received_bytes, p.total_bytes, "");
        }
    })?;

    let display_name = catalog()?
        .entries
        .iter()
        .find(|e| e.id == spec.entry_id)
        .map(|e| e.name.clone())
        .unwrap_or_else(|| spec.file.name.clone());

    add_library_entry(
        app,
        &LibraryModel {
            schema: 1,
            sha256: spec.file.sha256.clone(),
            file_name: spec.file.name.clone(),
            path: final_path.to_string_lossy().to_string(),
            size_bytes: spec.file.size_bytes,
            display_name,
            entry_id: Some(spec.entry_id.clone()),
            quant: Some(spec.quant.clone()),
            source: "huggingface".into(),
            added_at: chrono::Utc::now().to_rfc3339(),
        },
    )?;
    log::info!(target: "dl", "installed {} ({})", spec.file.name, spec.file.sha256);

    // The library write above is part of the job; emit Done only when the
    // model is genuinely usable.
    emit(
        app,
        &DownloadProgress {
            sha256: spec.file.sha256.clone(),
            entry_id: spec.entry_id.clone(),
            quant: spec.quant.clone(),
            file_name: spec.file.name.clone(),
            received_bytes: spec.file.size_bytes,
            total_bytes: spec.file.size_bytes,
            bytes_per_sec: 0,
            state: DownloadState::Done,
            error: None,
        },
    );
    Ok(())
}

/// The whole transfer pipeline, decoupled from Tauri so it is testable end to
/// end: resume-aware ranged GET, streaming SHA256, verification, and atomic
/// finalize into `final_dir`. Returns the final artifact path.
fn run_job_core(
    part: &std::path::Path,
    final_dir: &std::path::Path,
    spec: &JobSpec,
    cancel: &AtomicBool,
    on_progress: impl Fn(&DownloadProgress),
) -> Result<PathBuf> {
    let total = spec.file.size_bytes;
    let mut progress = DownloadProgress {
        sha256: spec.file.sha256.clone(),
        entry_id: spec.entry_id.clone(),
        quant: spec.quant.clone(),
        file_name: spec.file.name.clone(),
        received_bytes: 0,
        total_bytes: total,
        bytes_per_sec: 0,
        state: DownloadState::Starting,
        error: None,
    };
    on_progress(&progress);

    // Resume support: re-hash what we already have so verification stays valid.
    let mut hasher = Sha256::new();
    let mut received: u64 = 0;
    if part.exists() {
        let mut existing = fs::File::open(part)?;
        let mut buf = vec![0u8; 4 * 1024 * 1024];
        loop {
            let n = existing.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            received += n as u64;
        }
        if received > total {
            // Corrupt partial (bigger than the artifact) — start over.
            drop(existing);
            fs::remove_file(part)?;
            hasher = Sha256::new();
            received = 0;
        }
        log::info!(target: "dl", "resuming {} from {received} bytes", spec.file.name);
    }

    // Disk preflight on the remaining bytes.
    if let Some(free) = free_space_for(part) {
        let needed = (total - received).saturating_add(DISK_MARGIN_BYTES);
        if free < needed {
            return Err(AthanorError::Download(format!(
                "not enough disk space: need {:.1} GB free, have {:.1} GB",
                needed as f64 / 1e9,
                free as f64 / 1e9
            )));
        }
    }

    let url = format!(
        "https://huggingface.co/{}/resolve/main/{}?download=true",
        spec.hf_repo, spec.file.name
    );

    let client = reqwest::blocking::Client::builder()
        .user_agent(format!("athanor/{}", env!("CARGO_PKG_VERSION")))
        .timeout(None) // per-read timeouts handled by the OS; large files take hours
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| AthanorError::Download(e.to_string()))?;

    let mut req = client.get(&url);
    if received > 0 && received < total {
        req = req.header(reqwest::header::RANGE, format!("bytes={received}-"));
    }

    let mut file;
    let mut resp_opt = None;
    if received == total && total > 0 {
        // Partial is already complete — straight to verification.
        file = fs::OpenOptions::new().append(true).open(part)?;
    } else {
        let resp = req.send().map_err(|e| AthanorError::Download(e.to_string()))?;
        let status = resp.status();
        match status.as_u16() {
            206 => {
                file = fs::OpenOptions::new().append(true).open(part)?;
            }
            200 => {
                // Server ignored the range — start over.
                if received > 0 {
                    hasher = Sha256::new();
                    received = 0;
                }
                file = fs::File::create(part)?;
            }
            416 => {
                return Err(AthanorError::Download(
                    "server rejected resume range; delete the partial and retry".into(),
                ));
            }
            _ => {
                return Err(AthanorError::Download(format!(
                    "HTTP {status} from Hugging Face"
                )));
            }
        }
        resp_opt = Some(resp);
    }

    if let Some(mut resp) = resp_opt {
        progress.state = DownloadState::Downloading;
        let mut buf = vec![0u8; 1024 * 1024];
        let mut last_emit = Instant::now();
        let mut last_bytes = received;

        loop {
            if cancel.load(Ordering::Relaxed) {
                file.sync_all().ok();
                return Err(AthanorError::Download("cancelled".into()));
            }
            let n = resp
                .read(&mut buf)
                .map_err(|e| AthanorError::Download(format!("network read failed: {e}")))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            hasher.update(&buf[..n]);
            received += n as u64;

            let dt = last_emit.elapsed();
            if dt >= PROGRESS_INTERVAL {
                progress.state = DownloadState::Downloading;
                progress.received_bytes = received;
                progress.bytes_per_sec =
                    ((received - last_bytes) as f64 / dt.as_secs_f64()) as u64;
                on_progress(&progress);
                last_emit = Instant::now();
                last_bytes = received;
            }
        }
        file.sync_all()?;
    }

    if received != total {
        return Err(AthanorError::Download(format!(
            "incomplete: got {received} of {total} bytes (connection ended early — retry to resume)"
        )));
    }

    progress.state = DownloadState::Verifying;
    progress.received_bytes = received;
    progress.bytes_per_sec = 0;
    on_progress(&progress);

    let digest = hex::encode(hasher.finalize());
    if digest != spec.file.sha256 {
        fs::remove_file(part).ok();
        return Err(AthanorError::Download(
            "checksum mismatch — the download was corrupted and has been discarded; try again"
                .into(),
        ));
    }

    // Finalize: rename into the content-addressed store.
    fs::create_dir_all(final_dir)?;
    let final_path = final_dir.join(&spec.file.name);
    fs::rename(part, &final_path)?;
    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    /// Real end-to-end transfer test against Hugging Face: downloads the
    /// smallest catalog artifact (nomic-embed, ~274 MB), cancels mid-flight,
    /// resumes from the partial, verifies the checksum, and finalizes.
    /// Run: cargo test download_resume_verify_real -- --ignored --nocapture
    #[test]
    #[ignore]
    fn download_resume_verify_real() {
        let spec = resolve_spec("nomic-embed-text-v1.5", "F16").expect("catalog spec");
        let tmp = std::env::temp_dir().join(format!("athanor-dl-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();
        let part = tmp.join("artifact.part");
        let final_dir = tmp.join("store");

        // Pass 1: cancel once we're past 15% — proves cancel + partial survival.
        let cancel = AtomicBool::new(false);
        let emitted = AtomicU32::new(0);
        let res = run_job_core(&part, &final_dir, &spec, &cancel, |p| {
            emitted.fetch_add(1, Ordering::Relaxed);
            if p.received_bytes > p.total_bytes / 7 {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        assert!(matches!(res, Err(AthanorError::Download(ref m)) if m == "cancelled"));
        let partial_len = fs::metadata(&part).expect("partial must survive cancel").len();
        assert!(partial_len > 0, "cancel must leave resumable bytes");
        println!("cancelled at {partial_len} bytes; {} progress events", emitted.load(Ordering::Relaxed));

        // Pass 2: resume to completion and verify.
        let cancel = AtomicBool::new(false);
        let saw_resume_offset = AtomicU32::new(0);
        let final_path = run_job_core(&part, &final_dir, &spec, &cancel, |p| {
            if p.state == DownloadState::Downloading
                && p.received_bytes >= partial_len
                && saw_resume_offset.load(Ordering::Relaxed) == 0
            {
                saw_resume_offset.store(1, Ordering::Relaxed);
            }
        })
        .expect("resumed download must complete and verify");

        assert!(final_path.exists());
        assert_eq!(fs::metadata(&final_path).unwrap().len(), spec.file.size_bytes);
        assert!(!part.exists(), "partial must be consumed by finalize");
        println!("verified artifact at {final_path:?}");
        fs::remove_dir_all(&tmp).ok();
    }
}

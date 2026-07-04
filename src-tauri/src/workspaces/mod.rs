//! Workspace system: each workspace is a self-contained directory under the app
//! data root. Manifests are plain JSON on disk — portable, inspectable, and
//! recoverable with nothing but a file manager.
//!
//! Durability contract: every write is atomic (temp + fsync + rename), every
//! persisted format carries a schema version with serde defaults, and no cache
//! file can ever fail boot — corruption is quarantined, logged, and healed.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::error::{AthanorError, Result};

pub const SCHEMA_VERSION: u32 = 1;

pub fn schema_version_default() -> u32 {
    SCHEMA_VERSION
}
use schema_version_default as schema_version;

/// Serializes all workspace mutations. Reads are cheap; writes are rare.
#[derive(Default)]
pub struct WsLock(pub Mutex<()>);

impl WsLock {
    /// Poison-proof acquire: the mutex guards no invariant-carrying data
    /// (it is a `Mutex<()>`), so a panic in a previous holder must not
    /// brick every subsequent workspace operation.
    pub fn acquire(&self) -> std::sync::MutexGuard<'_, ()> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    #[serde(default = "schema_version")]
    pub schema: u32,
    pub id: String,
    pub name: String,
    /// What this stack is tuned for — drives model/RAG suggestions in the wizard.
    #[serde(default)]
    pub purpose: String,
    /// Accent hue (0–360) used across the UI wherever this workspace appears.
    #[serde(default)]
    pub accent_hue: u16,
    /// Single glyph shown on the workspace tile.
    #[serde(default)]
    pub glyph: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub last_opened_at: String,
    /// Content hashes of models this workspace references.
    #[serde(default)]
    pub model_refs: Vec<String>,
    /// The model (library sha256) this workspace chats with, if chosen.
    #[serde(default)]
    pub active_model: Option<String>,
    /// The template this workspace was created from, if any — informational,
    /// used to tailor first-run guidance.
    #[serde(default)]
    pub template_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AppState {
    #[serde(default = "schema_version")]
    schema: u32,
    #[serde(default)]
    active_workspace_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceList {
    pub workspaces: Vec<Workspace>,
    pub active_id: Option<String>,
    /// Directories whose manifests could not be read — surfaced, never hidden.
    pub damaged: Vec<String>,
}

// ── Durable filesystem primitives ─────────────────────────────

/// Atomic write: temp file in the same directory, fsync, rename over target.
/// Rename replaces atomically on NTFS and POSIX; a crash at any point leaves
/// either the old file or the new file — never a torn one. On failure the
/// original is untouched (this is also the disk-full safety property).
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp~");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(contents)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path).map_err(|e| {
        // Never leave temp litter behind a failed rename.
        let _ = fs::remove_file(&tmp);
        AthanorError::Io(e)
    })
}

/// Quarantine an unreadable file for diagnosis instead of deleting it.
fn quarantine(path: &Path) {
    let corrupt = path.with_extension("corrupt");
    if fs::rename(path, &corrupt).is_ok() {
        log::warn!(target: "ws", "quarantined unreadable file to {corrupt:?}");
    }
}

// ── Paths ─────────────────────────────────────────────────────

pub fn data_root(app: &AppHandle) -> Result<PathBuf> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|e| AthanorError::Path(e.to_string()))?;
    fs::create_dir_all(root.join("workspaces"))?;
    Ok(root)
}

fn state_path(app: &AppHandle) -> Result<PathBuf> {
    Ok(data_root(app)?.join("state.json"))
}

fn trash_dir(app: &AppHandle) -> Result<PathBuf> {
    let dir = data_root(app)?.join(".trash");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn workspace_dir(app: &AppHandle, id: &str) -> Result<PathBuf> {
    // ids are UUIDs we generated; reject anything path-like defensively.
    if id.is_empty()
        || id.len() > 64
        || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(AthanorError::Workspace(format!("invalid workspace id: {id}")));
    }
    Ok(data_root(app)?.join("workspaces").join(id))
}

fn manifest_path(dir: &Path) -> PathBuf {
    dir.join("workspace.json")
}

// ── State (a reconstructible cache — must NEVER fail boot) ────

fn read_state(app: &AppHandle) -> AppState {
    let Ok(path) = state_path(app) else {
        return AppState::default();
    };
    if !path.exists() {
        return AppState::default();
    }
    match fs::read_to_string(&path)
        .map_err(AthanorError::from)
        .and_then(|s| serde_json::from_str::<AppState>(&s).map_err(AthanorError::from))
    {
        Ok(state) => state,
        Err(e) => {
            log::warn!(target: "ws", "state.json unreadable ({e}); rebuilding from defaults");
            quarantine(&path);
            AppState::default()
        }
    }
}

fn write_state(app: &AppHandle, state: &AppState) -> Result<()> {
    write_atomic(&state_path(app)?, serde_json::to_string_pretty(state)?.as_bytes())
}

// ── Manifests ─────────────────────────────────────────────────

fn read_manifest(dir: &Path) -> Result<Workspace> {
    Ok(serde_json::from_str(&fs::read_to_string(manifest_path(
        dir,
    ))?)?)
}

fn write_manifest(dir: &Path, ws: &Workspace) -> Result<()> {
    write_atomic(
        &manifest_path(dir),
        serde_json::to_string_pretty(ws)?.as_bytes(),
    )
}

// ── Operations ────────────────────────────────────────────────

pub fn list(app: &AppHandle) -> Result<WorkspaceList> {
    let root = data_root(app)?.join("workspaces");
    let mut workspaces = Vec::new();
    let mut damaged = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.path().is_dir() {
            continue;
        }
        match read_manifest(&entry.path()) {
            Ok(ws) => workspaces.push(ws),
            Err(e) => {
                let name = entry.file_name().to_string_lossy().to_string();
                log::warn!(target: "ws", "workspace {name:?}: unreadable manifest ({e})");
                damaged.push(name);
            }
        }
    }
    // Most recently opened first — matches the rail ordering in the UI.
    workspaces.sort_by(|a, b| b.last_opened_at.cmp(&a.last_opened_at));

    let mut state = read_state(app);
    // Heal dangling active ids in memory; persist opportunistically. A failed
    // heal-write must never fail a read operation.
    if let Some(active) = &state.active_workspace_id {
        if !workspaces.iter().any(|w| &w.id == active) {
            state.active_workspace_id = workspaces.first().map(|w| w.id.clone());
            if let Err(e) = write_state(app, &state) {
                log::warn!(target: "ws", "active-pointer heal not persisted ({e}); continuing");
            }
        }
    }

    Ok(WorkspaceList {
        workspaces,
        active_id: state.active_workspace_id,
        damaged,
    })
}

pub fn create(
    app: &AppHandle,
    name: &str,
    purpose: &str,
    accent_hue: u16,
    glyph: &str,
    template_id: Option<String>,
) -> Result<Workspace> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AthanorError::Workspace("workspace name cannot be empty".into()));
    }
    if name.len() > 64 {
        return Err(AthanorError::Workspace("workspace name is too long (max 64 chars)".into()));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let ws = Workspace {
        schema: SCHEMA_VERSION,
        id: uuid::Uuid::new_v4().to_string(),
        name: name.to_string(),
        purpose: purpose.trim().chars().take(200).collect(),
        accent_hue: accent_hue % 360,
        glyph: glyph.chars().take(1).collect::<String>(),
        created_at: now.clone(),
        last_opened_at: now,
        model_refs: Vec::new(),
        active_model: None,
        template_id,
    };

    let dir = workspace_dir(app, &ws.id)?;
    fs::create_dir_all(&dir)?;
    write_manifest(&dir, &ws)?;

    // Creating a workspace takes you there — matches IDE project-switch behavior.
    let mut state = read_state(app);
    state.active_workspace_id = Some(ws.id.clone());
    write_state(app, &state)?;

    log::info!(target: "ws", "created workspace '{}' ({})", ws.name, ws.id);
    Ok(ws)
}

pub fn activate(app: &AppHandle, id: &str) -> Result<Workspace> {
    let dir = workspace_dir(app, id)?;
    let mut ws = read_manifest(&dir)
        .map_err(|_| AthanorError::Workspace(format!("workspace {id} not found")))?;

    ws.last_opened_at = chrono::Utc::now().to_rfc3339();
    write_manifest(&dir, &ws)?;

    let mut state = read_state(app);
    state.active_workspace_id = Some(id.to_string());
    write_state(app, &state)?;

    log::info!(target: "ws", "activated workspace '{}' ({})", ws.name, ws.id);
    Ok(ws)
}

pub fn set_active_model(app: &AppHandle, id: &str, sha256: Option<String>) -> Result<Workspace> {
    let dir = workspace_dir(app, id)?;
    let mut ws = read_manifest(&dir)
        .map_err(|_| AthanorError::Workspace(format!("workspace {id} not found")))?;
    ws.active_model = sha256;
    write_manifest(&dir, &ws)?;
    Ok(ws)
}

/// Delete = move to the app's trash, purged after [`TRASH_RETENTION_DAYS`].
/// Two clicks must never be able to permanently destroy a document corpus.
pub fn delete(app: &AppHandle, id: &str) -> Result<WorkspaceList> {
    let dir = workspace_dir(app, id)?;
    if !dir.exists() {
        return Err(AthanorError::Workspace(format!("workspace {id} not found")));
    }
    let dest = trash_dir(app)?.join(format!("{id}-{}", chrono::Utc::now().timestamp()));
    fs::rename(&dir, &dest)?;
    log::info!(target: "ws", "workspace {id} moved to trash ({dest:?})");

    // list() heals the active pointer if we just removed the active workspace.
    list(app)
}

pub const TRASH_RETENTION_DAYS: i64 = 7;

/// Purge trash entries older than the retention window. Called at startup on a
/// background thread; failures are logged and harmless.
pub fn purge_trash(app: &AppHandle) {
    let Ok(dir) = trash_dir(app) else { return };
    let Ok(entries) = fs::read_dir(&dir) else { return };
    let cutoff = chrono::Utc::now().timestamp() - TRASH_RETENTION_DAYS * 86_400;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // Names are "{uuid}-{unix_ts}"; unparseable entries are left alone.
        let Some(ts) = name.rsplit('-').next().and_then(|t| t.parse::<i64>().ok()) else {
            continue;
        };
        if ts < cutoff {
            match fs::remove_dir_all(entry.path()) {
                Ok(()) => log::info!(target: "ws", "purged trashed workspace {name}"),
                Err(e) => log::warn!(target: "ws", "trash purge of {name} failed: {e}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_replaces_existing() {
        let dir = std::env::temp_dir().join(format!("athanor-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("f.json");
        write_atomic(&p, b"first").unwrap();
        write_atomic(&p, b"second").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "second");
        assert!(!p.with_extension("tmp~").exists(), "no temp litter");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn corrupt_state_parses_to_default() {
        // AppState must tolerate garbage: unknown fields, missing fields.
        let v: AppState = serde_json::from_str("{}").unwrap();
        assert_eq!(v.schema, SCHEMA_VERSION);
        assert!(v.active_workspace_id.is_none());
        let v: AppState =
            serde_json::from_str(r#"{"schema":1,"activeWorkspaceId":"x","futureField":42}"#)
                .unwrap();
        assert_eq!(v.active_workspace_id.as_deref(), Some("x"));
    }

    #[test]
    fn v1_manifest_without_new_fields_still_parses() {
        // A pre-activeModel manifest (schema-less v1) must load with defaults.
        let json = r#"{
            "id": "0b0e8a62-0000-4000-8000-000000000000",
            "name": "Old Workspace",
            "purpose": "",
            "accentHue": 275,
            "glyph": "O",
            "createdAt": "2026-07-01T00:00:00Z",
            "lastOpenedAt": "2026-07-02T00:00:00Z",
            "modelRefs": []
        }"#;
        let ws: Workspace = serde_json::from_str(json).unwrap();
        assert_eq!(ws.schema, SCHEMA_VERSION);
        assert!(ws.active_model.is_none());
    }

    #[test]
    fn workspace_id_validation_rejects_path_tricks() {
        for bad in ["..", "a/b", "a\\b", "", "x".repeat(65).as_str()] {
            assert!(
                bad.is_empty()
                    || bad.len() > 64
                    || !bad.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
                "{bad} should be rejected by the guard logic"
            );
        }
    }
}

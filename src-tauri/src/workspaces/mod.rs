//! Workspace system: each workspace is a self-contained directory under the app
//! data root. Manifests are plain JSON on disk — portable, inspectable, and
//! recoverable with nothing but a file manager.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::error::{CondereError, Result};

/// Serializes all workspace mutations. Reads are cheap; writes are rare.
#[derive(Default)]
pub struct WsLock(pub Mutex<()>);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub name: String,
    /// What this stack is tuned for — drives model/RAG suggestions in the wizard.
    pub purpose: String,
    /// Accent hue (0–360) used across the UI wherever this workspace appears.
    pub accent_hue: u16,
    /// Single glyph shown on the workspace tile.
    pub glyph: String,
    pub created_at: String,
    pub last_opened_at: String,
    /// Content hashes of models this workspace references (M2+).
    pub model_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct AppState {
    active_workspace_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceList {
    pub workspaces: Vec<Workspace>,
    pub active_id: Option<String>,
}

fn data_root(app: &AppHandle) -> Result<PathBuf> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|e| CondereError::Path(e.to_string()))?;
    fs::create_dir_all(root.join("workspaces"))?;
    Ok(root)
}

fn state_path(app: &AppHandle) -> Result<PathBuf> {
    Ok(data_root(app)?.join("state.json"))
}

fn read_state(app: &AppHandle) -> Result<AppState> {
    let path = state_path(app)?;
    if !path.exists() {
        return Ok(AppState::default());
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

fn write_state(app: &AppHandle, state: &AppState) -> Result<()> {
    fs::write(state_path(app)?, serde_json::to_string_pretty(state)?)?;
    Ok(())
}

fn workspace_dir(app: &AppHandle, id: &str) -> Result<PathBuf> {
    // ids are UUIDs we generated; reject anything path-like defensively.
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(CondereError::Workspace(format!("invalid workspace id: {id}")));
    }
    Ok(data_root(app)?.join("workspaces").join(id))
}

fn manifest_path(dir: &std::path::Path) -> PathBuf {
    dir.join("workspace.json")
}

fn read_manifest(dir: &std::path::Path) -> Result<Workspace> {
    Ok(serde_json::from_str(&fs::read_to_string(manifest_path(
        dir,
    ))?)?)
}

fn write_manifest(dir: &std::path::Path, ws: &Workspace) -> Result<()> {
    fs::write(manifest_path(dir), serde_json::to_string_pretty(ws)?)?;
    Ok(())
}

pub fn list(app: &AppHandle) -> Result<WorkspaceList> {
    let root = data_root(app)?.join("workspaces");
    let mut workspaces = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.path().is_dir() {
            continue;
        }
        match read_manifest(&entry.path()) {
            Ok(ws) => workspaces.push(ws),
            Err(e) => log::warn!(
                target: "ws",
                "skipping {:?}: unreadable manifest ({e})",
                entry.file_name()
            ),
        }
    }
    // Most recently opened first — matches the rail ordering in the UI.
    workspaces.sort_by(|a, b| b.last_opened_at.cmp(&a.last_opened_at));

    let mut state = read_state(app)?;
    // Heal dangling active ids (e.g. workspace dir deleted by hand).
    if let Some(active) = &state.active_workspace_id {
        if !workspaces.iter().any(|w| &w.id == active) {
            state.active_workspace_id = workspaces.first().map(|w| w.id.clone());
            write_state(app, &state)?;
        }
    }

    Ok(WorkspaceList {
        workspaces,
        active_id: state.active_workspace_id,
    })
}

pub fn create(app: &AppHandle, name: &str, purpose: &str, accent_hue: u16, glyph: &str) -> Result<Workspace> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CondereError::Workspace("workspace name cannot be empty".into()));
    }
    if name.len() > 64 {
        return Err(CondereError::Workspace("workspace name is too long (max 64 chars)".into()));
    }

    let now = chrono::Utc::now().to_rfc3339();
    let ws = Workspace {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.to_string(),
        purpose: purpose.trim().chars().take(200).collect(),
        accent_hue: accent_hue % 360,
        glyph: glyph.chars().take(1).collect::<String>(),
        created_at: now.clone(),
        last_opened_at: now,
        model_refs: Vec::new(),
    };

    let dir = workspace_dir(app, &ws.id)?;
    fs::create_dir_all(&dir)?;
    write_manifest(&dir, &ws)?;

    // Creating a workspace takes you there — matches IDE project-switch behavior.
    let mut state = read_state(app)?;
    state.active_workspace_id = Some(ws.id.clone());
    write_state(app, &state)?;

    log::info!(target: "ws", "created workspace '{}' ({})", ws.name, ws.id);
    Ok(ws)
}

pub fn activate(app: &AppHandle, id: &str) -> Result<Workspace> {
    let dir = workspace_dir(app, id)?;
    let mut ws = read_manifest(&dir)
        .map_err(|_| CondereError::Workspace(format!("workspace {id} not found")))?;

    ws.last_opened_at = chrono::Utc::now().to_rfc3339();
    write_manifest(&dir, &ws)?;

    let mut state = read_state(app)?;
    state.active_workspace_id = Some(id.to_string());
    write_state(app, &state)?;

    log::info!(target: "ws", "activated workspace '{}' ({})", ws.name, ws.id);
    Ok(ws)
}

pub fn delete(app: &AppHandle, id: &str) -> Result<WorkspaceList> {
    let dir = workspace_dir(app, id)?;
    if !dir.exists() {
        return Err(CondereError::Workspace(format!("workspace {id} not found")));
    }
    // Deleting a workspace deletes exactly its directory — the isolation contract.
    fs::remove_dir_all(&dir)?;
    log::info!(target: "ws", "deleted workspace {id}");

    // list() heals the active pointer if we just removed the active workspace.
    list(app)
}

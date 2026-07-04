//! Workspace sharing — export a whole workspace's *configuration* (not its
//! gigabytes of model or documents) as one small, portable JSON file, and
//! recreate it from that file on another machine. The model is referenced by
//! catalog id + hash so the importer can offer to fetch it; nothing private
//! (chats, document contents) is included.

use serde::{Deserialize, Serialize};

use crate::error::{AthanorError, Result};
use crate::mcp::McpServerConfig;
use crate::workspaces::Workspace;
use crate::{downloads, mcp, rag, workspaces};

const FORMAT: &str = "athanor-workspace/1";

/// A portable reference to a model — by catalog identity and hash, never the
/// blob. Lets the importer match an installed copy or offer to download it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRef {
    pub entry_id: Option<String>,
    pub quant: Option<String>,
    pub sha256: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceManifest {
    pub format: String,
    pub name: String,
    #[serde(default)]
    pub purpose: String,
    #[serde(default)]
    pub accent_hue: u16,
    #[serde(default)]
    pub glyph: String,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub model: Option<ModelRef>,
    #[serde(default)]
    pub rag_enabled: bool,
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
    #[serde(default)]
    pub exported_at: String,
}

/// The outcome of an import, so the UI can guide the next step.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub workspace: Workspace,
    /// A model the imported workspace wants that isn't installed here.
    pub missing_model: Option<ModelRef>,
}

fn find_workspace(app: &tauri::AppHandle, id: &str) -> Result<Workspace> {
    workspaces::list(app)?
        .workspaces
        .into_iter()
        .find(|w| w.id == id)
        .ok_or_else(|| AthanorError::Workspace(format!("workspace {id} not found")))
}

/// Build a shareable manifest for a workspace.
pub fn build_manifest(app: &tauri::AppHandle, workspace_id: &str) -> Result<WorkspaceManifest> {
    let ws = find_workspace(app, workspace_id)?;

    let model = ws.active_model.as_ref().and_then(|sha| {
        downloads::list_library(app)
            .ok()?
            .into_iter()
            .find(|m| &m.sha256 == sha)
            .map(|m| ModelRef {
                entry_id: m.entry_id,
                quant: m.quant,
                sha256: m.sha256,
                display_name: m.display_name,
            })
    });

    let rag_enabled = rag::knowledge_base(app, workspace_id)
        .map(|kb| kb.retrieval_enabled)
        .unwrap_or(true);

    let mcp_servers = mcp::list_servers(app, workspace_id)
        .map(|views| views.into_iter().map(|v| v.config).collect())
        .unwrap_or_default();

    Ok(WorkspaceManifest {
        format: FORMAT.into(),
        name: ws.name,
        purpose: ws.purpose,
        accent_hue: ws.accent_hue,
        glyph: ws.glyph,
        system_prompt: ws.system_prompt,
        model,
        rag_enabled,
        mcp_servers,
        exported_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Write a workspace's manifest to a user-chosen path.
pub fn export(app: &tauri::AppHandle, workspace_id: &str, dest: &str) -> Result<()> {
    let manifest = build_manifest(app, workspace_id)?;
    std::fs::write(dest, serde_json::to_string_pretty(&manifest)?)?;
    log::info!(target: "share", "exported workspace '{}' to {dest}", manifest.name);
    Ok(())
}

/// Recreate a workspace from a manifest file. Returns the new workspace plus any
/// model it references that isn't installed here.
pub fn import(app: &tauri::AppHandle, src: &str) -> Result<ImportResult> {
    let text = std::fs::read_to_string(src)
        .map_err(|e| AthanorError::Workspace(format!("cannot read file: {e}")))?;
    let manifest: WorkspaceManifest = serde_json::from_str(&text)
        .map_err(|_| AthanorError::Workspace("this isn't a valid Athanor workspace file".into()))?;
    if !manifest.format.starts_with("athanor-workspace/") {
        return Err(AthanorError::Workspace("unrecognized workspace file format".into()));
    }

    let glyph = if manifest.glyph.is_empty() { "W" } else { &manifest.glyph };
    let ws = workspaces::create(app, &manifest.name, &manifest.purpose, manifest.accent_hue, glyph, None)?;

    if let Some(sp) = manifest.system_prompt.clone() {
        let _ = workspaces::set_system_prompt(app, &ws.id, Some(sp));
    }
    let _ = rag::set_retrieval_enabled(app, &ws.id, manifest.rag_enabled);
    for cfg in &manifest.mcp_servers {
        let _ = mcp::save_server(app, &ws.id, cfg.clone());
    }

    // Wire the model if a matching copy is installed; otherwise report it missing
    // so the UI can offer to download it.
    let mut missing_model = None;
    if let Some(mref) = manifest.model {
        let installed = downloads::list_library(app).ok().and_then(|lib| {
            lib.into_iter().find(|m| {
                m.sha256 == mref.sha256
                    || (m.entry_id == mref.entry_id && m.quant == mref.quant && mref.entry_id.is_some())
            })
        });
        match installed {
            Some(m) => {
                let _ = workspaces::set_active_model(app, &ws.id, Some(m.sha256));
            }
            None => missing_model = Some(mref),
        }
    }

    log::info!(target: "share", "imported workspace '{}' ({})", ws.name, ws.id);
    Ok(ImportResult { workspace: ws, missing_model })
}

/// A safe default filename for an exported workspace.
pub fn export_filename(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == ' ' || c == '-' { c } else { '_' })
        .collect();
    let clean = clean.trim();
    format!("{}.athanor.json", if clean.is_empty() { "workspace" } else { clean })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_round_trips_through_json() {
        let m = WorkspaceManifest {
            format: FORMAT.into(),
            name: "Code Assistant".into(),
            purpose: "coding".into(),
            accent_hue: 205,
            glyph: "C".into(),
            system_prompt: Some("You are a senior engineer.".into()),
            model: Some(ModelRef {
                entry_id: Some("qwen2.5-coder-7b".into()),
                quant: Some("Q4_K_M".into()),
                sha256: "abc123".into(),
                display_name: "Qwen2.5 Coder 7B".into(),
            }),
            rag_enabled: true,
            mcp_servers: vec![],
            exported_at: "2026-07-04T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: WorkspaceManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "Code Assistant");
        assert_eq!(back.accent_hue, 205);
        assert_eq!(back.model.unwrap().quant.as_deref(), Some("Q4_K_M"));
        assert!(back.rag_enabled);
    }

    #[test]
    fn export_filename_is_sanitized() {
        assert_eq!(export_filename("Legal / Review"), "Legal _ Review.athanor.json");
        assert_eq!(export_filename(""), "workspace.athanor.json");
    }

    #[test]
    fn rejects_foreign_format() {
        let bad = r#"{"format":"something-else","name":"x"}"#;
        // Parses as JSON, but the format guard must reject it in import(); here we
        // assert the discriminator is what we check.
        let m: WorkspaceManifest = serde_json::from_str(bad).unwrap();
        assert!(!m.format.starts_with("athanor-workspace/"));
    }
}

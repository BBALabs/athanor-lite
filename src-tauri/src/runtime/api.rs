//! Local OpenAI-compatible API exposure.
//!
//! llama-server already speaks `/v1/chat/completions`; exposing it is a
//! settings surface, not a second server. When enabled, the engine launches
//! on a stable port with a persistent bearer key so Continue/Cursor/n8n and
//! scripts can point at `http://127.0.0.1:<port>/v1` and survive restarts.
//! Localhost-only by design in v1 — no LAN binding until there is a real
//! authorization story.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::error::Result;
use crate::workspaces::{self, write_atomic};

use super::server::Llm;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiSettings {
    #[serde(default = "workspaces::schema_version_default")]
    pub schema: u32,
    #[serde(default)]
    pub expose: bool,
    #[serde(default = "default_port")]
    pub port: u16,
    /// Persistent bearer key, generated on first read.
    #[serde(default)]
    pub api_key: String,
}

fn default_port() -> u16 {
    11435
}

impl Default for ApiSettings {
    fn default() -> Self {
        ApiSettings {
            schema: 1,
            expose: false,
            port: default_port(),
            api_key: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiInfo {
    pub expose: bool,
    pub running: bool,
    pub base_url: String,
    pub api_key: String,
    pub model_name: Option<String>,
}

fn path(app: &AppHandle) -> Result<PathBuf> {
    Ok(workspaces::data_root(app)?.join("api.json"))
}

pub fn get_settings(app: &AppHandle) -> Result<ApiSettings> {
    let p = path(app)?;
    let mut settings: ApiSettings = if p.exists() {
        fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        ApiSettings::default()
    };
    if settings.api_key.is_empty() {
        settings.api_key = uuid::Uuid::new_v4().to_string();
        write_atomic(&p, serde_json::to_string_pretty(&settings)?.as_bytes())?;
    }
    Ok(settings)
}

pub fn set_expose(app: &AppHandle, expose: bool) -> Result<ApiSettings> {
    let mut settings = get_settings(app)?;
    settings.expose = expose;
    write_atomic(&path(app)?, serde_json::to_string_pretty(&settings)?.as_bytes())?;
    log::info!(target: "api", "local API expose set to {expose}");
    Ok(settings)
}

/// Issue a fresh API key, invalidating the old one. Existing clients that were
/// holding the previous key must be updated with the new one.
pub fn rotate_key(app: &AppHandle, llm: &Llm) -> Result<ApiInfo> {
    let mut settings = get_settings(app)?;
    settings.api_key = uuid::Uuid::new_v4().to_string();
    write_atomic(&path(app)?, serde_json::to_string_pretty(&settings)?.as_bytes())?;
    log::info!(target: "api", "local API key rotated");
    info(app, llm)
}

pub fn info(app: &AppHandle, llm: &Llm) -> Result<ApiInfo> {
    let settings = get_settings(app)?;
    let guard = llm.lock();
    let running = guard.as_ref().map(|a| a.port);
    let model_name = guard.as_ref().map(|a| a.model_name.clone());
    let port = if settings.expose {
        settings.port
    } else {
        running.unwrap_or(settings.port)
    };
    Ok(ApiInfo {
        expose: settings.expose,
        running: running.is_some(),
        base_url: format!("http://127.0.0.1:{port}/v1"),
        api_key: settings.api_key,
        model_name,
    })
}

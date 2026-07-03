//! Conversations and the streaming generation proxy.
//!
//! The frontend never talks to llama-server directly: Rust proxies the SSE
//! stream, which keeps the CSP closed, lets us measure ground truth (TTFT,
//! decode rate, context use) at the source, and survives UI reloads.
//! Conversations are schema-versioned JSON files inside their workspace —
//! portable and inspectable like everything else on disk.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::error::{AthanorError, Result};
use crate::metrics;
use crate::runtime::server::{self, Llm, CTX_SIZE};
use crate::workspaces::{self, write_atomic};

pub const EVENT_DELTA: &str = "chat://delta";
pub const EVENT_DONE: &str = "chat://done";

/// In-flight generation cancel flags, keyed by conversation id.
#[derive(Default)]
pub struct ChatCancels(pub Mutex<HashMap<String, Arc<AtomicBool>>>);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenStats {
    pub ttft_ms: u64,
    pub prompt_n: u32,
    pub predicted_n: u32,
    pub prompt_per_second: f64,
    pub predicted_per_second: f64,
    pub context_used: u32,
    pub gpu_active: bool,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub ts: String,
    #[serde(default)]
    pub stats: Option<GenStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    #[serde(default = "workspaces::schema_version_default")]
    pub schema: u32,
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    #[serde(default)]
    pub model_sha: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMeta {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub message_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Delta {
    pub workspace_id: String,
    pub conversation_id: String,
    pub delta: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Done {
    pub workspace_id: String,
    pub conversation_id: String,
    pub content: String,
    pub stats: Option<GenStats>,
    pub error: Option<String>,
}

// ── Storage ───────────────────────────────────────────────────

fn chats_dir(app: &AppHandle, workspace_id: &str) -> Result<PathBuf> {
    // Reuses workspace id validation via the workspaces module path helpers.
    let dir = workspaces::data_root(app)?
        .join("workspaces")
        .join(workspace_id)
        .join("chats");
    if !dir.parent().map(|p| p.exists()).unwrap_or(false) {
        return Err(AthanorError::Chat(format!(
            "workspace {workspace_id} not found"
        )));
    }
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn conv_path(app: &AppHandle, workspace_id: &str, conv_id: &str) -> Result<PathBuf> {
    if conv_id.is_empty() || !conv_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(AthanorError::Chat(format!("invalid conversation id: {conv_id}")));
    }
    Ok(chats_dir(app, workspace_id)?.join(format!("{conv_id}.json")))
}

fn save(app: &AppHandle, conv: &Conversation) -> Result<()> {
    let path = conv_path(app, &conv.workspace_id, &conv.id)?;
    write_atomic(&path, serde_json::to_string_pretty(conv)?.as_bytes())
}

pub fn load(app: &AppHandle, workspace_id: &str, conv_id: &str) -> Result<Conversation> {
    let path = conv_path(app, workspace_id, conv_id)?;
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

pub fn list(app: &AppHandle, workspace_id: &str) -> Result<Vec<ConversationMeta>> {
    let dir = chats_dir(app, workspace_id)?;
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.path().extension().map(|e| e != "json").unwrap_or(true) {
            continue;
        }
        match fs::read_to_string(entry.path())
            .map_err(AthanorError::from)
            .and_then(|s| serde_json::from_str::<Conversation>(&s).map_err(AthanorError::from))
        {
            Ok(c) => out.push(ConversationMeta {
                id: c.id,
                title: c.title,
                updated_at: c.updated_at,
                message_count: c.messages.len(),
            }),
            Err(e) => log::warn!(target: "chat", "unreadable conversation {:?}: {e}", entry.path()),
        }
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(out)
}

pub fn delete(app: &AppHandle, workspace_id: &str, conv_id: &str) -> Result<Vec<ConversationMeta>> {
    let path = conv_path(app, workspace_id, conv_id)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    list(app, workspace_id)
}

fn title_from(message: &str) -> String {
    let t: String = message.trim().chars().take(48).collect();
    if t.is_empty() { "New session".into() } else { t }
}

// ── Generation ────────────────────────────────────────────────

#[derive(Deserialize)]
struct SseDelta {
    choices: Vec<SseChoice>,
    #[serde(default)]
    timings: Option<SseTimings>,
}

#[derive(Deserialize)]
struct SseChoice {
    #[serde(default)]
    delta: SseContent,
}

#[derive(Deserialize, Default)]
struct SseContent {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize, Clone, Copy)]
struct SseTimings {
    #[serde(default)]
    cache_n: u32,
    #[serde(default)]
    prompt_n: u32,
    #[serde(default)]
    prompt_per_second: f64,
    #[serde(default)]
    predicted_n: u32,
    #[serde(default)]
    predicted_per_second: f64,
}

/// Run one generation turn. Blocking (call via spawn_blocking). Appends the
/// user message durably before generating; streams deltas as events; appends
/// the assistant message (with measured stats) when done.
#[allow(clippy::too_many_arguments)]
pub fn send(
    app: &AppHandle,
    llm: &Llm,
    cancels: &ChatCancels,
    workspace_id: &str,
    conversation_id: Option<String>,
    message: String,
) -> Result<String> {
    let ws_list = workspaces::list(app)?;
    let ws = ws_list
        .workspaces
        .iter()
        .find(|w| w.id == workspace_id)
        .ok_or_else(|| AthanorError::Chat("workspace not found".into()))?;
    let model_sha = ws
        .active_model
        .clone()
        .ok_or_else(|| AthanorError::Chat("no model selected for this workspace".into()))?;

    // Load or create the conversation; persist the user turn before anything
    // can fail so no typed message is ever lost.
    let now = chrono::Utc::now().to_rfc3339();
    let mut conv = match &conversation_id {
        Some(id) => load(app, workspace_id, id)?,
        None => Conversation {
            schema: 1,
            id: uuid::Uuid::new_v4().to_string(),
            workspace_id: workspace_id.to_string(),
            title: title_from(&message),
            model_sha: Some(model_sha.clone()),
            created_at: now.clone(),
            updated_at: now.clone(),
            messages: Vec::new(),
        },
    };
    conv.model_sha = Some(model_sha.clone());
    conv.messages.push(ChatMessage {
        role: "user".into(),
        content: message,
        ts: now.clone(),
        stats: None,
    });
    conv.updated_at = now;
    save(app, &conv)?;
    let conv_id = conv.id.clone();

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut map = cancels.0.lock().unwrap_or_else(|p| p.into_inner());
        map.insert(conv_id.clone(), cancel.clone());
    }
    let result = generate(app, llm, &mut conv, &cancel);
    {
        let mut map = cancels.0.lock().unwrap_or_else(|p| p.into_inner());
        map.remove(&conv_id);
    }

    match result {
        Ok(()) => Ok(conv_id),
        Err(e) => {
            let _ = app.emit(
                EVENT_DONE,
                &Done {
                    workspace_id: conv.workspace_id.clone(),
                    conversation_id: conv_id.clone(),
                    content: String::new(),
                    stats: None,
                    error: Some(e.to_string()),
                },
            );
            metrics::record_failure(app, &model_sha, &e.to_string());
            Err(e)
        }
    }
}

fn generate(app: &AppHandle, llm: &Llm, conv: &mut Conversation, cancel: &AtomicBool) -> Result<()> {
    let model_sha = conv.model_sha.clone().expect("set by caller");
    let port = server::ensure(app, llm, &model_sha)?;
    let (api_key, gpu_active, vram_at_load) = {
        let guard = llm.lock();
        let active = guard.as_ref().ok_or_else(|| AthanorError::Chat("engine stopped".into()))?;
        (
            active.api_key.clone(),
            active.gpu_active.load(Ordering::Relaxed),
            active.vram_at_load_bytes,
        )
    };

    // Workspace purpose becomes the standing instruction — the whole point of
    // per-job workspaces.
    let ws_list = workspaces::list(app)?;
    let purpose = ws_list
        .workspaces
        .iter()
        .find(|w| w.id == conv.workspace_id)
        .map(|w| w.purpose.clone())
        .unwrap_or_default();
    let mut api_messages = Vec::new();
    if !purpose.trim().is_empty() {
        api_messages.push(serde_json::json!({
            "role": "system",
            "content": format!("You are a focused assistant for this workspace. Its purpose: {purpose}")
        }));
    }
    for m in &conv.messages {
        api_messages.push(serde_json::json!({ "role": m.role, "content": m.content }));
    }

    let body = serde_json::json!({
        "messages": api_messages,
        "stream": true,
        "cache_prompt": true,
    });

    let client = reqwest::blocking::Client::builder()
        .timeout(None)
        .build()
        .map_err(|e| AthanorError::Chat(e.to_string()))?;

    let started = Instant::now();
    let resp = client
        .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .bearer_auth(&api_key)
        .json(&body)
        .send()
        .map_err(|e| AthanorError::Chat(format!("generation request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(AthanorError::Chat(format!(
            "engine returned HTTP {}",
            resp.status()
        )));
    }

    let mut content = String::new();
    let mut ttft_ms: Option<u64> = None;
    let mut timings: Option<SseTimings> = None;
    let reader = BufReader::new(resp);

    for line in reader.lines() {
        if cancel.load(Ordering::Relaxed) {
            break; // dropping the reader closes the connection; server aborts
        }
        let line = line.map_err(|e| AthanorError::Chat(format!("stream read failed: {e}")))?;
        let Some(payload) = line.strip_prefix("data: ") else {
            continue;
        };
        if payload.trim() == "[DONE]" {
            break;
        }
        let Ok(chunk) = serde_json::from_str::<SseDelta>(payload) else {
            continue;
        };
        if let Some(t) = chunk.timings {
            timings = Some(t);
        }
        if let Some(delta) = chunk.choices.first().and_then(|c| c.delta.content.as_ref()) {
            if !delta.is_empty() {
                if ttft_ms.is_none() {
                    ttft_ms = Some(started.elapsed().as_millis() as u64);
                }
                content.push_str(delta);
                let _ = app.emit(
                    EVENT_DELTA,
                    &Delta {
                        workspace_id: conv.workspace_id.clone(),
                        conversation_id: conv.id.clone(),
                        delta: delta.clone(),
                    },
                );
            }
        }
    }

    let was_cancelled = cancel.load(Ordering::Relaxed);
    let stats = timings.map(|t| GenStats {
        ttft_ms: ttft_ms.unwrap_or(0),
        prompt_n: t.prompt_n,
        predicted_n: t.predicted_n,
        prompt_per_second: t.prompt_per_second,
        predicted_per_second: t.predicted_per_second,
        context_used: t.cache_n + t.prompt_n + t.predicted_n,
        gpu_active,
        cancelled: was_cancelled,
    });

    let now = chrono::Utc::now().to_rfc3339();
    conv.messages.push(ChatMessage {
        role: "assistant".into(),
        content: content.clone(),
        ts: now.clone(),
        stats: stats.clone(),
    });
    conv.updated_at = now;
    save(app, conv)?;

    let _ = app.emit(
        EVENT_DONE,
        &Done {
            workspace_id: conv.workspace_id.clone(),
            conversation_id: conv.id.clone(),
            content,
            stats: stats.clone(),
            error: None,
        },
    );

    if let Some(s) = &stats {
        metrics::record_generation(app, &model_sha, s, vram_at_load, CTX_SIZE);
    }
    Ok(())
}

pub fn cancel(cancels: &ChatCancels, conversation_id: &str) {
    let map = cancels.0.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(flag) = map.get(conversation_id) {
        flag.store(true, Ordering::Relaxed);
    }
}

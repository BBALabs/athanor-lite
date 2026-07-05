//! Conversations and the streaming generation proxy.
//!
//! The frontend never talks to llama-server directly: Rust proxies the SSE
//! stream, which keeps the CSP closed, lets us measure ground truth (TTFT,
//! decode rate, context use) at the source, and survives UI reloads.
//! Conversations are schema-versioned JSON files inside their workspace —
//! portable and inspectable like everything else on disk.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::error::{AthanorError, Result};
use crate::metrics;
use crate::ops::{OpKind, Ops};
use crate::runtime::server::{self, Llm, CTX_SIZE};
use crate::workspaces::{self, write_atomic};

pub const EVENT_DELTA: &str = "chat://delta";
pub const EVENT_DONE: &str = "chat://done";

pub fn op_id(conversation_id: &str) -> String {
    format!("gen:{conversation_id}")
}

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

/// Give a conversation a name of the user's choosing (past the auto-title).
pub fn rename(
    app: &AppHandle,
    workspace_id: &str,
    conv_id: &str,
    title: &str,
) -> Result<Vec<ConversationMeta>> {
    let mut conv = load(app, workspace_id, conv_id)?;
    let t: String = title.trim().chars().take(80).collect();
    conv.title = if t.is_empty() { "Untitled".into() } else { t };
    save(app, &conv)?;
    list(app, workspace_id)
}

// ── Search ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    pub message_index: usize,
    pub role: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub message_count: usize,
    /// Up to a few matched messages, newest-relevant first.
    pub matches: Vec<SearchMatch>,
}

const MAX_MATCHES_PER_CONV: usize = 4;

/// A one-line preview centered on the first case-insensitive hit, whitespace
/// collapsed. Char-based throughout, so it never splits a multibyte boundary.
fn snippet(content: &str, needle_lower: &str) -> String {
    const PAD: usize = 40;
    let hay: Vec<char> = content.chars().collect();
    let needle: Vec<char> = needle_lower.chars().collect();
    let pos = if needle.is_empty() || needle.len() > hay.len() {
        None
    } else {
        (0..=hay.len() - needle.len()).find(|&i| {
            hay[i..i + needle.len()]
                .iter()
                .zip(&needle)
                .all(|(c, n)| c.to_lowercase().next() == n.to_lowercase().next())
        })
    };
    let (start, end) = match pos {
        Some(p) => (p.saturating_sub(PAD), (p + needle.len() + PAD).min(hay.len())),
        None => (0, hay.len().min(80)),
    };
    let mut s = String::new();
    if start > 0 {
        s.push('…');
    }
    s.extend(&hay[start..end]);
    if end < hay.len() {
        s.push('…');
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Index-free full-text search across one workspace's conversations. Scans the
/// per-conversation JSON at query time — no index to build, stale, or corrupt.
/// Case-insensitive substring over titles and message content.
pub fn search(app: &AppHandle, workspace_id: &str, query: &str) -> Result<Vec<SearchHit>> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let dir = chats_dir(app, workspace_id)?;
    let mut hits = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.path().extension().map(|e| e != "json").unwrap_or(true) {
            continue;
        }
        // A file that vanishes or won't parse mid-scan is skipped, never fatal.
        let Ok(text) = fs::read_to_string(entry.path()) else { continue };
        let Ok(conv) = serde_json::from_str::<Conversation>(&text) else { continue };

        let title_hit = conv.title.to_lowercase().contains(&q);
        let mut matches = Vec::new();
        for (i, m) in conv.messages.iter().enumerate() {
            if m.content.to_lowercase().contains(&q) {
                matches.push(SearchMatch {
                    message_index: i,
                    role: m.role.clone(),
                    snippet: snippet(&m.content, &q),
                });
                if matches.len() >= MAX_MATCHES_PER_CONV {
                    break;
                }
            }
        }
        if title_hit || !matches.is_empty() {
            hits.push(SearchHit {
                id: conv.id,
                title: conv.title,
                updated_at: conv.updated_at,
                message_count: conv.messages.len(),
                matches,
            });
        }
    }
    hits.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(hits)
}

// ── Export ────────────────────────────────────────────────────

/// Render a conversation as readable Markdown — turns as headings, stats as a
/// quiet blockquote.
fn to_markdown(conv: &Conversation) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let _ = writeln!(s, "# {}\n", conv.title);
    let _ = writeln!(
        s,
        "> {} message{} · updated {}\n",
        conv.messages.len(),
        if conv.messages.len() == 1 { "" } else { "s" },
        conv.updated_at
    );
    for m in &conv.messages {
        let who = match m.role.as_str() {
            "user" => "You",
            "assistant" => "Assistant",
            other => other,
        };
        let _ = writeln!(s, "## {who}\n\n{}\n", m.content.trim());
        if let Some(st) = &m.stats {
            let _ = writeln!(
                s,
                "> {:.1}s to first token · {:.1} tok/s · {} ctx{}\n",
                st.ttft_ms as f64 / 1000.0,
                st.predicted_per_second,
                st.context_used,
                if st.gpu_active { "" } else { " · CPU" }
            );
        }
    }
    s
}

/// Write a conversation to a user-chosen path as `markdown` or `json`.
pub fn export(
    app: &AppHandle,
    workspace_id: &str,
    conv_id: &str,
    format: &str,
    dest: &str,
) -> Result<()> {
    let conv = load(app, workspace_id, conv_id)?;
    let content = match format {
        "json" => serde_json::to_string_pretty(&conv)?,
        _ => to_markdown(&conv),
    };
    fs::write(dest, content)?;
    log::info!(target: "chat", "exported conversation {conv_id} as {format} to {dest}");
    Ok(())
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
/// the assistant message (with measured stats) when done. Registered in the
/// operations registry — one generation per conversation, cancellable there.
pub fn send(
    app: &AppHandle,
    llm: &Llm,
    ops: &Ops,
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
        content: message.clone(),
        ts: now.clone(),
        stats: None,
    });
    conv.updated_at = now;
    save(app, &conv)?;

    run_generation(app, llm, ops, conv, &model_sha)
}

/// Run the generation op over a conversation whose turns are already in place
/// (used by send, regenerate, and edit-and-resend). Registers in the ops
/// registry, streams, and reports failures on the done channel.
fn run_generation(
    app: &AppHandle,
    llm: &Llm,
    ops: &Ops,
    mut conv: Conversation,
    model_sha: &str,
) -> Result<String> {
    let conv_id = conv.id.clone();
    let cancel = ops
        .begin(
            app,
            &op_id(&conv_id),
            OpKind::Generation,
            &format!("Generating · {}", conv.title),
            true,
            None,
        )
        .ok_or_else(|| AthanorError::Chat("a reply is already being generated here".into()))?;

    let result = generate(app, llm, &mut conv, &cancel);
    match &result {
        Ok(()) if cancel.load(Ordering::Relaxed) => ops.cancelled(app, &op_id(&conv_id)),
        Ok(()) => ops.done(app, &op_id(&conv_id)),
        Err(e) => ops.failed(app, &op_id(&conv_id), &e.to_string()),
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
            metrics::record_failure(app, model_sha, &e.to_string());
            Err(e)
        }
    }
}

fn active_model(app: &AppHandle, workspace_id: &str) -> Result<String> {
    workspaces::list(app)?
        .workspaces
        .iter()
        .find(|w| w.id == workspace_id)
        .and_then(|w| w.active_model.clone())
        .ok_or_else(|| AthanorError::Chat("no model selected for this workspace".into()))
}

/// Regenerate the last assistant reply: drop trailing assistant turn(s) and run
/// generation again over the same user turn.
pub fn regenerate(
    app: &AppHandle,
    llm: &Llm,
    ops: &Ops,
    workspace_id: &str,
    conversation_id: &str,
) -> Result<String> {
    let model_sha = active_model(app, workspace_id)?;
    let mut conv = load(app, workspace_id, conversation_id)?;
    while conv.messages.last().map(|m| m.role == "assistant").unwrap_or(false) {
        conv.messages.pop();
    }
    if conv.messages.last().map(|m| m.role != "user").unwrap_or(true) {
        return Err(AthanorError::Chat("nothing to regenerate".into()));
    }
    conv.model_sha = Some(model_sha.clone());
    save(app, &conv)?;
    run_generation(app, llm, ops, conv, &model_sha)
}

/// Edit a user turn and resend: replace its content, drop everything after it,
/// and regenerate from there.
pub fn edit_and_resend(
    app: &AppHandle,
    llm: &Llm,
    ops: &Ops,
    workspace_id: &str,
    conversation_id: &str,
    message_index: usize,
    new_content: String,
) -> Result<String> {
    let model_sha = active_model(app, workspace_id)?;
    let mut conv = load(app, workspace_id, conversation_id)?;
    if message_index >= conv.messages.len() || conv.messages[message_index].role != "user" {
        return Err(AthanorError::Chat("can only edit a message you sent".into()));
    }
    if new_content.trim().is_empty() {
        return Err(AthanorError::Chat("the edited message is empty".into()));
    }
    conv.messages.truncate(message_index + 1);
    let m = &mut conv.messages[message_index];
    m.content = new_content;
    m.ts = chrono::Utc::now().to_rfc3339();
    m.stats = None;
    conv.model_sha = Some(model_sha.clone());
    save(app, &conv)?;
    run_generation(app, llm, ops, conv, &model_sha)
}

/// Fork a conversation at a message: a new conversation with the history up to
/// and including that message, so a different direction can be explored.
pub fn fork(
    app: &AppHandle,
    workspace_id: &str,
    conversation_id: &str,
    upto: usize,
) -> Result<String> {
    let src = load(app, workspace_id, conversation_id)?;
    let keep = (upto + 1).min(src.messages.len());
    if keep == 0 {
        return Err(AthanorError::Chat("nothing to branch from".into()));
    }
    let now = chrono::Utc::now().to_rfc3339();
    let title: String = src.title.chars().take(44).collect();
    let forked = Conversation {
        schema: 1,
        id: uuid::Uuid::new_v4().to_string(),
        workspace_id: workspace_id.to_string(),
        title: format!("↳ {title}"),
        model_sha: src.model_sha.clone(),
        created_at: now.clone(),
        updated_at: now,
        messages: src.messages[..keep].to_vec(),
    };
    save(app, &forked)?;
    Ok(forked.id)
}

/// Stream one completion for `messages` through the engine for `model_sha`,
/// calling `on_delta` for each content chunk and returning the measured stats.
/// No conversation, no persistence, capped output.
fn stream_completion(
    app: &AppHandle,
    llm: &Llm,
    model_sha: &str,
    messages: serde_json::Value,
    max_tokens: u32,
    cancel: &AtomicBool,
    mut on_delta: impl FnMut(&str),
) -> Result<GenStats> {
    let port = server::ensure(app, llm, model_sha)?;
    let (api_key, gpu_active) = {
        let guard = llm.lock();
        let active = guard.as_ref().ok_or_else(|| AthanorError::Chat("engine stopped".into()))?;
        (active.api_key.clone(), active.gpu_active.load(Ordering::Relaxed))
    };
    let body = serde_json::json!({
        "messages": messages,
        "stream": true,
        "cache_prompt": false,
        "max_tokens": max_tokens,
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
        .map_err(|e| AthanorError::Chat(format!("request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(AthanorError::Chat(format!("engine returned HTTP {}", resp.status())));
    }
    let mut ttft_ms: Option<u64> = None;
    let mut timings: Option<SseTimings> = None;
    let reader = BufReader::new(resp);
    for line in reader.lines() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let line = line.map_err(|e| AthanorError::Chat(format!("stream read failed: {e}")))?;
        let Some(payload) = line.strip_prefix("data: ") else { continue };
        if payload.trim() == "[DONE]" {
            break;
        }
        let Ok(chunk) = serde_json::from_str::<SseDelta>(payload) else { continue };
        if let Some(t) = chunk.timings {
            timings = Some(t);
        }
        if let Some(d) = chunk.choices.first().and_then(|c| c.delta.content.as_ref()) {
            if !d.is_empty() {
                if ttft_ms.is_none() {
                    ttft_ms = Some(started.elapsed().as_millis() as u64);
                }
                on_delta(d);
            }
        }
    }
    let t = timings.ok_or_else(|| AthanorError::Chat("engine returned no timing data".into()))?;
    Ok(GenStats {
        ttft_ms: ttft_ms.unwrap_or(0),
        prompt_n: t.prompt_n,
        predicted_n: t.predicted_n,
        prompt_per_second: t.prompt_per_second,
        predicted_per_second: t.predicted_per_second,
        context_used: t.cache_n + t.prompt_n + t.predicted_n,
        gpu_active,
        cancelled: cancel.load(Ordering::Relaxed),
    })
}

/// Run a single, unpersisted generation and return only its measured stats —
/// the benchmark's building block.
pub fn measure(
    app: &AppHandle,
    llm: &Llm,
    model_sha: &str,
    prompt: &str,
    max_tokens: u32,
) -> Result<GenStats> {
    let never = AtomicBool::new(false);
    stream_completion(
        app,
        llm,
        model_sha,
        serde_json::json!([{ "role": "user", "content": prompt }]),
        max_tokens,
        &never,
        |_| {},
    )
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

    // Workspace purpose becomes the standing instruction — so replies stay on
    // the workspace's subject.
    let ws_list = workspaces::list(app)?;
    let ws = ws_list.workspaces.iter().find(|w| w.id == conv.workspace_id);
    let purpose = ws.map(|w| w.purpose.clone()).unwrap_or_default();
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

    let client = reqwest::blocking::Client::builder()
        .timeout(None)
        .build()
        .map_err(|e| AthanorError::Chat(e.to_string()))?;
    let body = serde_json::json!({
        "messages": api_messages,
        "stream": true,
        "cache_prompt": true,
    });

    let started = Instant::now();
    let resp = client
        .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .bearer_auth(&api_key)
        .json(&body)
        .send()
        .map_err(|e| AthanorError::Chat(format!("generation request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(AthanorError::Chat(format!("engine returned HTTP {}", resp.status())));
    }

    let mut final_content = String::new();
    let mut ttft_ms: Option<u64> = None;
    let mut timings: Option<SseTimings> = None;
    let reader = BufReader::new(resp);

    for line in reader.lines() {
        if cancel.load(Ordering::Relaxed) {
            break;
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
                final_content.push_str(delta);
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
        content: final_content.clone(),
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
            content: final_content,
            stats: stats.clone(),
            error: None,
        },
    );

    if let Some(s) = &stats {
        metrics::record_generation(app, &model_sha, s, vram_at_load, CTX_SIZE);
    }
    Ok(())
}

pub fn cancel(ops: &Ops, conversation_id: &str) {
    ops.request_cancel(&op_id(conversation_id));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            content: content.into(),
            ts: "2026-07-03T00:00:00Z".into(),
            stats: None,
        }
    }

    fn conv_with(msgs: Vec<ChatMessage>) -> Conversation {
        Conversation {
            schema: 1,
            id: "c1".into(),
            workspace_id: "w1".into(),
            title: "Reactor notes".into(),
            model_sha: None,
            created_at: "2026-07-03T00:00:00Z".into(),
            updated_at: "2026-07-03T00:00:00Z".into(),
            messages: msgs,
        }
    }

    #[test]
    fn snippet_centers_on_the_hit_and_collapses_whitespace() {
        let content = "The quick brown fox\njumps over the lazy calibration constant 8827 kelvin.";
        let s = snippet(content, "8827");
        assert!(s.contains("8827"));
        assert!(!s.contains('\n'), "newlines collapsed");
        // Match is mid-string, so the preview is elided on the left.
        assert!(s.starts_with('…'));
    }

    #[test]
    fn snippet_is_case_insensitive_and_multibyte_safe() {
        // Leading multibyte chars must not cause a panic or bad boundary.
        let content = "café ☕ RÉACTEUR details about the Meridian core";
        let s = snippet(content, "réacteur");
        assert!(s.to_lowercase().contains("réacteur"));
    }

    #[test]
    fn dropping_trailing_assistant_leaves_a_user_turn() {
        // The regenerate transform: pop trailing assistant turns.
        let mut msgs = vec![msg("user", "a"), msg("assistant", "b"), msg("assistant", "c")];
        while msgs.last().map(|m| m.role == "assistant").unwrap_or(false) {
            msgs.pop();
        }
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs.last().unwrap().role, "user");
    }

    #[test]
    fn edit_truncates_after_the_edited_turn() {
        // The edit-and-resend transform: keep [0..=index], drop the rest.
        let mut msgs = vec![
            msg("user", "q1"),
            msg("assistant", "a1"),
            msg("user", "q2"),
            msg("assistant", "a2"),
        ];
        let index = 0usize; // edit the first user turn
        msgs.truncate(index + 1);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "q1");
    }

    #[test]
    fn fork_keeps_history_up_to_and_including_the_point() {
        let msgs = vec![msg("user", "q1"), msg("assistant", "a1"), msg("user", "q2")];
        let upto = 1usize;
        let keep = (upto + 1).min(msgs.len());
        let branched = &msgs[..keep];
        assert_eq!(branched.len(), 2);
        assert_eq!(branched.last().unwrap().content, "a1");
    }

    #[test]
    fn markdown_export_labels_turns_and_notes_stats() {
        let mut m = msg("assistant", "The sum is 99208.");
        m.stats = Some(GenStats {
            ttft_ms: 1200,
            prompt_n: 10,
            predicted_n: 20,
            prompt_per_second: 100.0,
            predicted_per_second: 42.5,
            context_used: 30,
            gpu_active: true,
            cancelled: false,
        });
        let md = to_markdown(&conv_with(vec![msg("user", "add them"), m]));
        assert!(md.contains("# Reactor notes"));
        assert!(md.contains("## You"));
        assert!(md.contains("## Assistant"));
        assert!(md.contains("42.5 tok/s"));
    }
}

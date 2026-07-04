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
use tauri::{AppHandle, Emitter, Manager};

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

/// One autonomous tool invocation the model made during a turn — surfaced so
/// the user sees exactly what was called, with what arguments, and what came
/// back.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStep {
    pub server: String,
    pub tool: String,
    pub arguments: String,
    pub result: String,
    pub ok: bool,
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
    /// Documents/chunks retrieved for this turn — retrieval visibility.
    #[serde(default)]
    pub sources: Vec<crate::rag::Source>,
    /// Tools the model called autonomously during this turn.
    #[serde(default)]
    pub tool_steps: Vec<ToolStep>,
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

/// Emitted the moment retrieval runs, before generation — so the UI can show
/// "consulting the knowledge base" and which documents were pulled.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Retrieval {
    pub workspace_id: String,
    pub conversation_id: String,
    pub sources: Vec<crate::rag::Source>,
}

/// Emitted for each autonomous tool call as it happens — the agentic loop
/// made visible: what the model called, and what came back.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolEvent {
    pub workspace_id: String,
    pub conversation_id: String,
    pub step: ToolStep,
}

pub const EVENT_TOOL: &str = "chat://tool";

/// Max autonomous tool-call rounds before we force a final answer — a
/// runaway-loop backstop.
const MAX_TOOL_ROUNDS: usize = 6;

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
/// quiet blockquote, sources and tool calls noted so nothing is silently lost.
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
        if !m.sources.is_empty() {
            let names: Vec<&str> = m.sources.iter().map(|src| src.doc_name.as_str()).collect();
            let _ = writeln!(s, "> Sources: {}\n", names.join(", "));
        }
        for step in &m.tool_steps {
            let _ = writeln!(s, "> Tool `{}` → {}\n", step.tool, step.result.trim());
        }
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
    #[serde(default)]
    tool_calls: Option<Vec<SseToolCallDelta>>,
}

#[derive(Deserialize)]
struct SseToolCallDelta {
    #[serde(default)]
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<SseFn>,
}

#[derive(Deserialize)]
struct SseFn {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// A tool call the model wants to make, assembled from streamed fragments.
#[derive(Default, Clone)]
struct PendingCall {
    id: String,
    name: String,
    arguments: String,
}

/// The outcome of streaming one model turn.
struct TurnResult {
    content: String,
    tool_calls: Vec<PendingCall>,
    timings: Option<SseTimings>,
    ttft_ms: Option<u64>,
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
        sources: Vec::new(),
        tool_steps: Vec::new(),
    });
    conv.updated_at = now;
    save(app, &conv)?;
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
    let ws = ws_list.workspaces.iter().find(|w| w.id == conv.workspace_id);
    let purpose = ws.map(|w| w.purpose.clone()).unwrap_or_default();
    let system_prompt = ws.and_then(|w| w.system_prompt.clone());
    let mut api_messages = Vec::new();
    // A full system prompt from the prompt library wins; otherwise the short
    // workspace purpose becomes the standing instruction.
    if let Some(sp) = system_prompt.as_ref().filter(|s| !s.trim().is_empty()) {
        api_messages.push(serde_json::json!({ "role": "system", "content": sp }));
    } else if !purpose.trim().is_empty() {
        api_messages.push(serde_json::json!({
            "role": "system",
            "content": format!("You are a focused assistant for this workspace. Its purpose: {purpose}")
        }));
    }

    // Connected MCP tools become OpenAI function definitions the model can call
    // autonomously. A name→server map routes each call to the right server.
    let mcp_tools = crate::mcp::available_tools(&app.state::<crate::mcp::McpManager>(), &conv.workspace_id);
    let tool_server: std::collections::HashMap<String, String> = mcp_tools
        .iter()
        .map(|(sid, t)| (t.name.clone(), sid.clone()))
        .collect();
    // Per-tool input schema, used to heal type-mismatched arguments before the
    // call (models often stringify numeric/boolean args).
    let tool_schema: std::collections::HashMap<String, serde_json::Value> = mcp_tools
        .iter()
        .map(|(_, t)| (t.name.clone(), t.input_schema.clone()))
        .collect();
    let openai_tools: Vec<serde_json::Value> = mcp_tools
        .iter()
        .map(|(_, t)| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description.clone().unwrap_or_default(),
                    "parameters": t.input_schema,
                }
            })
        })
        .collect();

    // Retrieval: embed the latest user turn, pull relevant chunks, inject as
    // context, and surface the sources to the UI immediately (before tokens).
    let query = conv
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_default();
    let sources = {
        let embedder = app.state::<crate::rag::embed::Embedder>();
        match crate::rag::retrieve(app, &embedder, &conv.workspace_id, &query) {
            Ok((block, sources)) => {
                if !block.is_empty() {
                    api_messages.push(serde_json::json!({ "role": "system", "content": block }));
                }
                if !sources.is_empty() {
                    let _ = app.emit(
                        "chat://retrieval",
                        &Retrieval {
                            workspace_id: conv.workspace_id.clone(),
                            conversation_id: conv.id.clone(),
                            sources: sources.clone(),
                        },
                    );
                }
                sources
            }
            Err(e) => {
                log::warn!(target: "rag", "retrieval skipped: {e}");
                Vec::new()
            }
        }
    };

    for m in &conv.messages {
        api_messages.push(serde_json::json!({ "role": m.role, "content": m.content }));
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(None)
        .build()
        .map_err(|e| AthanorError::Chat(e.to_string()))?;

    // ── The agentic loop ──────────────────────────────────────
    // Stream a turn. If the model called tools, execute them, append the
    // results, and loop; otherwise the turn's text is the final answer.
    let mgr = app.state::<crate::mcp::McpManager>();
    let mut final_content = String::new();
    let mut tool_steps: Vec<ToolStep> = Vec::new();
    let mut last_timings: Option<SseTimings> = None;
    let mut first_ttft: Option<u64> = None;

    for round in 0..MAX_TOOL_ROUNDS {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let turn = stream_turn(
            app,
            &client,
            port,
            &api_key,
            &api_messages,
            &openai_tools,
            conv,
            cancel,
        )?;
        if first_ttft.is_none() {
            first_ttft = turn.ttft_ms;
        }
        last_timings = turn.timings.or(last_timings);
        if !turn.content.is_empty() {
            if !final_content.is_empty() {
                final_content.push('\n');
            }
            final_content.push_str(&turn.content);
        }

        if turn.tool_calls.is_empty() || cancel.load(Ordering::Relaxed) {
            break; // final answer produced (or cancelled)
        }
        if round + 1 == MAX_TOOL_ROUNDS {
            final_content.push_str("\n\n(Stopped after the maximum number of tool calls.)");
            break;
        }

        // Echo the assistant's tool_calls back into the transcript…
        api_messages.push(serde_json::json!({
            "role": "assistant",
            "content": turn.content,
            "tool_calls": turn.tool_calls.iter().map(|c| serde_json::json!({
                "id": c.id,
                "type": "function",
                "function": { "name": c.name, "arguments": c.arguments }
            })).collect::<Vec<_>>()
        }));

        // …then run each tool and append its result.
        for call in &turn.tool_calls {
            // `sent_args` is what actually goes to the tool (post-coercion) so
            // the transcript shows the real call, not the model's raw draft.
            let mut sent_args = call.arguments.clone();
            let (result, ok) = match tool_server.get(&call.name) {
                Some(server_id) => {
                    let mut args = parse_tool_args(&call.arguments);
                    if let Some(schema) = tool_schema.get(&call.name) {
                        args = coerce_args(args, schema);
                    }
                    sent_args = args.to_string();
                    match crate::mcp::call_tool(&mgr, server_id, &call.name, args) {
                        Ok(out) => (out, true),
                        Err(e) => (e.to_string(), false),
                    }
                }
                // Unknown tool (often a small model hallucinating a name). Feed
                // back the valid names so the model can self-correct next round
                // instead of looping on the same bad call.
                None => {
                    let mut names: Vec<&str> = tool_server.keys().map(String::as_str).collect();
                    names.sort_unstable();
                    (
                        format!(
                            "Error: no tool named '{}'. Available tools: {}. Call one of these exact names.",
                            call.name,
                            names.join(", ")
                        ),
                        false,
                    )
                }
            };
            let step = ToolStep {
                server: tool_server.get(&call.name).cloned().unwrap_or_default(),
                tool: call.name.clone(),
                arguments: sent_args,
                result: result.clone(),
                ok,
            };
            let _ = app.emit(
                EVENT_TOOL,
                &ToolEvent {
                    workspace_id: conv.workspace_id.clone(),
                    conversation_id: conv.id.clone(),
                    step: step.clone(),
                },
            );
            tool_steps.push(step);
            api_messages.push(serde_json::json!({
                "role": "tool",
                "tool_call_id": call.id,
                "content": result
            }));
        }
    }

    let was_cancelled = cancel.load(Ordering::Relaxed);
    let stats = last_timings.map(|t| GenStats {
        ttft_ms: first_ttft.unwrap_or(0),
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
        sources,
        tool_steps,
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

/// llama.cpp may emit `arguments` as a JSON string (spec) or a bare object
/// (a known regression window) — tolerate both.
fn parse_tool_args(raw: &str) -> serde_json::Value {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        return v;
    }
    serde_json::json!({})
}

/// Does a JSON-Schema `type` node (string or array) admit `want`?
fn schema_admits(type_node: &serde_json::Value, want: &str) -> bool {
    match type_node {
        serde_json::Value::String(s) => s == want,
        serde_json::Value::Array(a) => a.iter().any(|v| v.as_str() == Some(want)),
        _ => false,
    }
}

/// Heal type-mismatched tool arguments against the tool's declared input
/// schema. Small models (and some large ones) routinely emit numeric or
/// boolean arguments as JSON strings — `{"a":"40217"}` where the tool wants a
/// number — which servers reject with a validation error. Where the schema is
/// unambiguous, coerce the string to the declared type so the call succeeds.
/// Only top-level properties are healed, and only when the string parses
/// cleanly; anything ambiguous is left exactly as the model produced it.
fn coerce_args(mut args: serde_json::Value, schema: &serde_json::Value) -> serde_json::Value {
    let (Some(obj), Some(props)) = (
        args.as_object_mut(),
        schema.get("properties").and_then(|p| p.as_object()),
    ) else {
        return args;
    };
    for (key, val) in obj.iter_mut() {
        let Some(ty) = props.get(key).and_then(|p| p.get("type")) else {
            continue;
        };
        let serde_json::Value::String(s) = val else {
            continue;
        };
        let trimmed = s.trim();
        if schema_admits(ty, "integer") {
            if let Ok(n) = trimmed.parse::<i64>() {
                *val = serde_json::json!(n);
                continue;
            }
        }
        if schema_admits(ty, "number") {
            if let Ok(n) = trimmed.parse::<f64>() {
                if let Some(num) = serde_json::Number::from_f64(n) {
                    *val = serde_json::Value::Number(num);
                    continue;
                }
            }
        }
        if schema_admits(ty, "boolean") {
            match trimmed.to_ascii_lowercase().as_str() {
                "true" => *val = serde_json::Value::Bool(true),
                "false" => *val = serde_json::Value::Bool(false),
                _ => {}
            }
        }
    }
    args
}

/// Stream one model turn: forward content deltas to the UI, accumulate any
/// tool-call fragments (keyed by index), and return the assembled result.
#[allow(clippy::too_many_arguments)]
fn stream_turn(
    app: &AppHandle,
    client: &reqwest::blocking::Client,
    port: u16,
    api_key: &str,
    messages: &[serde_json::Value],
    tools: &[serde_json::Value],
    conv: &Conversation,
    cancel: &AtomicBool,
) -> Result<TurnResult> {
    let mut body = serde_json::json!({
        "messages": messages,
        "stream": true,
        "cache_prompt": true,
    });
    if !tools.is_empty() {
        body["tools"] = serde_json::Value::Array(tools.to_vec());
        body["tool_choice"] = serde_json::Value::String("auto".into());
    }

    let started = Instant::now();
    let resp = client
        .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .map_err(|e| AthanorError::Chat(format!("generation request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(AthanorError::Chat(format!("engine returned HTTP {}", resp.status())));
    }

    let mut content = String::new();
    let mut ttft_ms: Option<u64> = None;
    let mut timings: Option<SseTimings> = None;
    // Tool-call fragments keyed by their streamed `index`.
    let mut calls: std::collections::BTreeMap<usize, PendingCall> = std::collections::BTreeMap::new();
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
        let Some(choice) = chunk.choices.first() else { continue };

        if let Some(delta) = choice.delta.content.as_ref() {
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

        if let Some(tcs) = &choice.delta.tool_calls {
            for tc in tcs {
                let entry = calls.entry(tc.index).or_default();
                if let Some(id) = &tc.id {
                    if !id.is_empty() {
                        entry.id = id.clone();
                    }
                }
                if let Some(f) = &tc.function {
                    if let Some(n) = &f.name {
                        if !n.is_empty() {
                            entry.name = n.clone();
                        }
                    }
                    if let Some(a) = &f.arguments {
                        entry.arguments.push_str(a);
                    }
                }
            }
        }
    }

    // llama.cpp may omit tool-call ids for some templates — synthesize one so
    // the follow-up tool result can bind to it.
    let tool_calls: Vec<PendingCall> = calls
        .into_values()
        .filter(|c| !c.name.is_empty())
        .enumerate()
        .map(|(i, mut c)| {
            if c.id.is_empty() {
                c.id = format!("call_{i}");
            }
            c
        })
        .collect();

    Ok(TurnResult {
        content,
        tool_calls,
        timings,
        ttft_ms,
    })
}

pub fn cancel(ops: &Ops, conversation_id: &str) {
    ops.request_cancel(&op_id(conversation_id));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            content: content.into(),
            ts: "2026-07-03T00:00:00Z".into(),
            stats: None,
            sources: Vec::new(),
            tool_steps: Vec::new(),
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
    fn markdown_export_labels_turns_and_notes_tools() {
        let mut m = msg("assistant", "The sum is 99208.");
        m.tool_steps.push(ToolStep {
            server: "everything".into(),
            tool: "get-sum".into(),
            arguments: "{\"a\":1}".into(),
            result: "99208".into(),
            ok: true,
        });
        let md = to_markdown(&conv_with(vec![msg("user", "add them"), m]));
        assert!(md.contains("# Reactor notes"));
        assert!(md.contains("## You"));
        assert!(md.contains("## Assistant"));
        assert!(md.contains("Tool `get-sum`"));
    }

    fn sum_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "a": { "type": "number" },
                "b": { "type": "integer" },
                "flag": { "type": "boolean" },
                "label": { "type": "string" }
            }
        })
    }

    #[test]
    fn coerces_stringified_numbers_and_bools() {
        // The exact failure server-everything rejected: numbers sent as strings.
        let args = json!({ "a": "40217", "b": "58991", "flag": "true", "label": "keep" });
        let out = coerce_args(args, &sum_schema());
        assert_eq!(out["a"], json!(40217.0));
        assert_eq!(out["b"], json!(58991));
        assert_eq!(out["flag"], json!(true));
        // A declared string stays a string — never over-coerce.
        assert_eq!(out["label"], json!("keep"));
    }

    #[test]
    fn integer_stays_integer_not_float() {
        let out = coerce_args(json!({ "b": "42" }), &sum_schema());
        assert!(out["b"].is_i64(), "integer field must stay an integer");
        assert_eq!(out["b"], json!(42));
    }

    #[test]
    fn already_correct_types_are_untouched() {
        let args = json!({ "a": 1.5, "b": 7, "flag": false });
        let out = coerce_args(args.clone(), &sum_schema());
        assert_eq!(out, args);
    }

    #[test]
    fn unparseable_strings_are_left_alone() {
        // "twelve" is not a number — leave it so the server's own validation
        // speaks, rather than silently mangling intent.
        let out = coerce_args(json!({ "a": "twelve" }), &sum_schema());
        assert_eq!(out["a"], json!("twelve"));
    }

    #[test]
    fn nullable_number_via_type_array_is_coerced() {
        let schema = json!({
            "type": "object",
            "properties": { "n": { "type": ["number", "null"] } }
        });
        let out = coerce_args(json!({ "n": "2.5" }), &schema);
        assert_eq!(out["n"], json!(2.5));
    }

    #[test]
    fn missing_schema_properties_pass_through() {
        // No schema info for a key → don't touch it.
        let out = coerce_args(json!({ "x": "5" }), &json!({ "type": "object" }));
        assert_eq!(out["x"], json!("5"));
    }

    #[test]
    fn tolerates_arguments_as_string_or_object() {
        assert_eq!(parse_tool_args(r#"{"a":1}"#), json!({ "a": 1 }));
        // Garbage degrades to an empty object rather than panicking.
        assert_eq!(parse_tool_args("not json"), json!({}));
    }
}

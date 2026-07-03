//! MCP (Model Context Protocol) client — connect workspaces to external
//! tools and data over stdio.
//!
//! Transport is newline-delimited compact UTF-8 JSON-RPC 2.0 on the server's
//! stdin/stdout (NOT LSP Content-Length framing); stderr is logs only.
//! Handshake: `initialize` → `notifications/initialized` → `tools/list`.
//! Protocol version pinned to the current 2025-11-25 (we accept whatever the
//! server echoes back).
//!
//! Every server is a child process under the same guarantees as the engine:
//! job-object bound (dies with the app), registered in the operations
//! registry, duplicate-connection-proof. Configs are per-workspace,
//! schema-versioned JSON.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::error::{AthanorError, Result};
use crate::ops::{OpKind, Ops};
use crate::runtime::guard;
use crate::workspaces::{self, write_atomic};

pub const PROTOCOL_VERSION: &str = "2025-11-25";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerView {
    pub config: McpServerConfig,
    pub connected: bool,
    pub server_name: Option<String>,
    pub tools: Vec<McpTool>,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct McpConfigFile {
    #[serde(default = "workspaces::schema_version_default")]
    schema: u32,
    #[serde(default)]
    servers: Vec<McpServerConfig>,
}

/// A live connection to one MCP server. `stdin` is an Option so a graceful
/// shutdown can close it (signalling the server to exit) before we kill.
struct Connection {
    child: Child,
    stdin: Option<ChildStdin>,
    responses: Receiver<Value>,
    next_id: i64,
    server_name: Option<String>,
    tools: Vec<McpTool>,
    workspace_id: String,
}

#[derive(Default)]
pub struct McpManager {
    conns: Mutex<HashMap<String, Connection>>,
}

impl McpManager {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Connection>> {
        self.conns.lock().unwrap_or_else(|p| p.into_inner())
    }
}

// ── Config storage ────────────────────────────────────────────

fn config_path(app: &AppHandle, workspace_id: &str) -> Result<std::path::PathBuf> {
    let ws = workspaces::data_root(app)?.join("workspaces").join(workspace_id);
    if !ws.exists() {
        return Err(AthanorError::Mcp(format!("workspace {workspace_id} not found")));
    }
    let dir = ws.join("mcp");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("servers.json"))
}

fn read_config(app: &AppHandle, workspace_id: &str) -> McpConfigFile {
    config_path(app, workspace_id)
        .ok()
        .filter(|p| p.exists())
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_config(app: &AppHandle, workspace_id: &str, c: &McpConfigFile) -> Result<()> {
    write_atomic(&config_path(app, workspace_id)?, serde_json::to_string_pretty(c)?.as_bytes())
}

fn view(mgr: &McpManager, cfg: &McpServerConfig) -> McpServerView {
    let conns = mgr.lock();
    match conns.get(&cfg.id) {
        Some(c) => McpServerView {
            config: cfg.clone(),
            connected: true,
            server_name: c.server_name.clone(),
            tools: c.tools.clone(),
            error: None,
        },
        None => McpServerView {
            config: cfg.clone(),
            connected: false,
            server_name: None,
            tools: Vec::new(),
            error: None,
        },
    }
}

pub fn list_servers(app: &AppHandle, workspace_id: &str) -> Result<Vec<McpServerView>> {
    let mgr = app.state::<McpManager>();
    let cfg = read_config(app, workspace_id);
    Ok(cfg.servers.iter().map(|s| view(&mgr, s)).collect())
}

pub fn save_server(
    app: &AppHandle,
    workspace_id: &str,
    mut config: McpServerConfig,
) -> Result<Vec<McpServerView>> {
    if config.id.trim().is_empty() {
        config.id = uuid::Uuid::new_v4().to_string();
    }
    if config.name.trim().is_empty() || config.command.trim().is_empty() {
        return Err(AthanorError::Mcp("server needs a name and a command".into()));
    }
    let mut file = read_config(app, workspace_id);
    if let Some(existing) = file.servers.iter_mut().find(|s| s.id == config.id) {
        *existing = config;
    } else {
        file.servers.push(config);
    }
    write_config(app, workspace_id, &file)?;
    list_servers(app, workspace_id)
}

pub fn remove_server(
    app: &AppHandle,
    mgr: &McpManager,
    workspace_id: &str,
    server_id: &str,
) -> Result<Vec<McpServerView>> {
    disconnect(app, mgr, server_id);
    let mut file = read_config(app, workspace_id);
    file.servers.retain(|s| s.id != server_id);
    write_config(app, workspace_id, &file)?;
    list_servers(app, workspace_id)
}

// ── Connection lifecycle ──────────────────────────────────────

pub fn disconnect(app: &AppHandle, mgr: &McpManager, server_id: &str) {
    if let Some(mut conn) = mgr.lock().remove(server_id) {
        // Graceful stdio shutdown: close stdin (server sees EOF → exits),
        // wait briefly, then kill whatever remains.
        conn.stdin.take();
        std::thread::sleep(Duration::from_millis(120));
        let _ = conn.child.kill();
        let _ = conn.child.wait();
        log::info!(target: "mcp", "disconnected MCP server {server_id}");
    }
    if let Some(ops) = app.try_state::<Ops>() {
        ops.done(app, &op_id(server_id));
    }
}

fn op_id(server_id: &str) -> String {
    format!("mcp:{server_id}")
}

/// Launch + handshake an MCP server, storing the live connection. Blocking.
pub fn connect(
    app: &AppHandle,
    mgr: &McpManager,
    workspace_id: &str,
    server_id: &str,
) -> Result<McpServerView> {
    let file = read_config(app, workspace_id);
    let cfg = file
        .servers
        .iter()
        .find(|s| s.id == server_id)
        .cloned()
        .ok_or_else(|| AthanorError::Mcp("server not configured".into()))?;

    // Duplicate guard: already connected → just return its view.
    if mgr.lock().contains_key(server_id) {
        return Ok(view(mgr, &cfg));
    }

    let ops = app.state::<Ops>();
    let _ = ops.begin(app, &op_id(server_id), OpKind::Mcp, &format!("MCP · {}", cfg.name), true, None);

    let result = do_connect(mgr, workspace_id, &cfg);
    match &result {
        Ok(_) => ops.detail(app, &op_id(server_id), "connected"),
        Err(e) => ops.failed(app, &op_id(server_id), &e.to_string()),
    }
    result
}

fn do_connect(
    mgr: &McpManager,
    workspace_id: &str,
    cfg: &McpServerConfig,
) -> Result<McpServerView> {
    // On Windows, npx/npm must be launched via `cmd /c`.
    #[cfg(windows)]
    let mut command = {
        let mut c = Command::new("cmd");
        c.arg("/c").arg(&cfg.command).args(&cfg.args);
        c
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut c = Command::new(&cfg.command);
        c.args(&cfg.args);
        c
    };
    for (k, v) in &cfg.env {
        command.env(k, v);
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }

    let mut child = command
        .spawn()
        .map_err(|e| AthanorError::Mcp(format!("could not launch server: {e}")))?;
    guard::adopt(&child);

    let stdin = child.stdin.take().ok_or_else(|| AthanorError::Mcp("no stdin".into()))?;
    let stdout = child.stdout.take().ok_or_else(|| AthanorError::Mcp("no stdout".into()))?;

    // stderr → logs only, never parsed as protocol.
    if let Some(stderr) = child.stderr.take() {
        std::thread::Builder::new()
            .name("mcp-stderr".into())
            .spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(|l| l.ok()) {
                    log::info!(target: "mcp", "{line}");
                }
            })
            .ok();
    }

    // Reader thread parses newline-delimited JSON into a channel.
    let (tx, rx) = mpsc::channel::<Value>();
    std::thread::Builder::new()
        .name("mcp-stdout".into())
        .spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<Value>(line) {
                    if tx.send(v).is_err() {
                        break;
                    }
                }
            }
        })
        .ok();

    let mut conn = Connection {
        child,
        stdin: Some(stdin),
        responses: rx,
        next_id: 1,
        server_name: None,
        tools: Vec::new(),
        workspace_id: workspace_id.to_string(),
    };

    // Handshake.
    let init = conn.request(
        "initialize",
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": { "name": "athanor", "version": env!("CARGO_PKG_VERSION") }
        }),
        Duration::from_secs(30),
    )?;
    conn.server_name = init
        .get("serverInfo")
        .and_then(|s| s.get("name"))
        .and_then(|n| n.as_str())
        .map(|s| s.to_string());
    conn.notify("notifications/initialized", json!({}))?;

    // Enumerate tools (paging).
    let mut tools = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let params = match &cursor {
            Some(c) => json!({ "cursor": c }),
            None => json!({}),
        };
        let res = conn.request("tools/list", params, Duration::from_secs(15))?;
        if let Some(arr) = res.get("tools").and_then(|t| t.as_array()) {
            for t in arr {
                tools.push(McpTool {
                    name: t.get("name").and_then(|n| n.as_str()).unwrap_or_default().to_string(),
                    title: t.get("title").and_then(|n| n.as_str()).map(|s| s.to_string()),
                    description: t
                        .get("description")
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string()),
                });
            }
        }
        cursor = res.get("nextCursor").and_then(|c| c.as_str()).map(|s| s.to_string());
        if cursor.is_none() {
            break;
        }
    }
    conn.tools = tools;

    log::info!(
        target: "mcp",
        "connected {} ({}): {} tools",
        cfg.name,
        conn.server_name.as_deref().unwrap_or("?"),
        conn.tools.len()
    );

    mgr.lock().insert(cfg.id.clone(), conn);
    Ok(view(mgr, cfg))
}

/// Call a tool on a connected server (available to chat + UI).
pub fn call_tool(
    mgr: &McpManager,
    server_id: &str,
    tool: &str,
    args: Value,
) -> Result<String> {
    let mut conns = mgr.lock();
    let conn = conns
        .get_mut(server_id)
        .ok_or_else(|| AthanorError::Mcp("server not connected".into()))?;
    let res = conn.request(
        "tools/call",
        json!({ "name": tool, "arguments": args }),
        Duration::from_secs(60),
    )?;
    // content is an array of blocks; join text blocks.
    let text = res
        .get("content")
        .and_then(|c| c.as_array())
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    if res.get("isError").and_then(|e| e.as_bool()).unwrap_or(false) {
        return Err(AthanorError::Mcp(format!("tool error: {text}")));
    }
    Ok(text)
}

impl Connection {
    fn send(&mut self, msg: &Value) -> Result<()> {
        // Compact single-line JSON + newline; never pretty (embedded newlines
        // are forbidden by the transport spec).
        let line = serde_json::to_string(msg)?;
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| AthanorError::Mcp("connection closed".into()))?;
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush())
            .map_err(|e| AthanorError::Mcp(format!("write to server failed: {e}")))
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    fn request(&mut self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;

        let deadline = std::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(AthanorError::Mcp(format!("{method} timed out")));
            }
            match self.responses.recv_timeout(remaining.min(Duration::from_millis(500))) {
                Ok(v) => {
                    // Match our id; ignore notifications and other-id messages.
                    if v.get("id").and_then(|i| i.as_i64()) == Some(id) {
                        if let Some(err) = v.get("error") {
                            let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("error");
                            return Err(AthanorError::Mcp(format!("{method}: {msg}")));
                        }
                        return Ok(v.get("result").cloned().unwrap_or(Value::Null));
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Ok(Some(code)) = self.child.try_wait() {
                        return Err(AthanorError::Mcp(format!(
                            "server exited (code {code:?}) during {method}"
                        )));
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(AthanorError::Mcp("server closed the connection".into()));
                }
            }
        }
    }
}

/// Names of tools available across all connected servers for a workspace —
/// surfaced to chat so the model can be told what it can call.
pub fn available_tools(mgr: &McpManager, workspace_id: &str) -> Vec<(String, McpTool)> {
    mgr.lock()
        .iter()
        .filter(|(_, c)| c.workspace_id == workspace_id)
        .flat_map(|(sid, c)| c.tools.iter().map(move |t| (sid.clone(), t.clone())))
        .collect()
}

/// Kill every live MCP server (app shutdown).
pub fn shutdown_all(mgr: &McpManager) {
    let mut conns = mgr.lock();
    for (id, mut conn) in conns.drain() {
        let _ = conn.child.kill();
        let _ = conn.child.wait();
        log::info!(target: "mcp", "shut down MCP server {id}");
    }
}

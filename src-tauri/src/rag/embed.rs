//! Embedding server — a dedicated `llama-server` running the embedding model
//! in `--embeddings` mode, coexisting with the chat engine on its own port.
//!
//! Reuses the whole runtime substrate: the pinned llama.cpp build, the job
//! object (no orphans), the operations registry (visible + stoppable), and
//! duplicate-safe serialized bring-up. nomic-embed is ~0.3 GB, so it sits
//! happily alongside a chat model.
//!
//! Task prefixes are mandatory for nomic-embed-text and are NOT added by
//! llama.cpp — we prepend them here (`search_document:` for stored chunks,
//! `search_query:` for the query). Omitting them silently wrecks retrieval.

use std::io::BufRead;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::error::{AthanorError, Result};
use crate::ops::{OpKind, Ops};
use crate::runtime::{ensure_runtime, guard, pick_backend};

/// The catalog model used for embeddings. nomic-embed-text-v1.5, 768-dim.
pub const EMBED_ENTRY: &str = "nomic-embed-text-v1.5";
pub const EMBED_QUANT: &str = "F16";
pub const EMBED_DIM: usize = 768;
pub const DOC_PREFIX: &str = "search_document: ";
pub const QUERY_PREFIX: &str = "search_query: ";
const CTX: u32 = 8192;

pub struct EmbedServer {
    child: Child,
    port: u16,
    api_key: String,
    model_sha: String,
}

/// Independent from the chat `Llm` so the two engines coexist. `spawn`
/// serializes bring-up; the embed server is reused across index + query.
#[derive(Default)]
pub struct Embedder {
    active: Mutex<Option<EmbedServer>>,
    spawn: Mutex<()>,
}

impl Embedder {
    fn active(&self) -> std::sync::MutexGuard<'_, Option<EmbedServer>> {
        self.active.lock().unwrap_or_else(|p| p.into_inner())
    }
    fn spawn_guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.spawn.lock().unwrap_or_else(|p| p.into_inner())
    }
}

fn free_port() -> Result<u16> {
    let l = TcpListener::bind("127.0.0.1:0").map_err(|e| AthanorError::Rag(e.to_string()))?;
    Ok(l.local_addr().map_err(|e| AthanorError::Rag(e.to_string()))?.port())
}

pub fn stop(app: &AppHandle, embedder: &Embedder) {
    if let Some(mut s) = embedder.active().take() {
        log::info!(target: "rag", "stopping embedding server on port {}", s.port);
        let _ = s.child.kill();
        let _ = s.child.wait();
    }
    if let Some(ops) = app.try_state::<Ops>() {
        ops.done(app, "embed-engine");
    }
}

/// Ensure the embedding server is running. Reuses a healthy instance; brings
/// one up (downloading the embedding model first if needed) otherwise.
pub fn ensure(app: &AppHandle, embedder: &Embedder) -> Result<u16> {
    let model = crate::downloads::ensure_installed(app, EMBED_ENTRY, EMBED_QUANT)?;

    let live = |e: &Embedder| -> Option<u16> {
        let mut guard = e.active();
        if let Some(s) = guard.as_mut() {
            if s.model_sha == model.sha256 {
                if let Ok(None) = s.child.try_wait() {
                    return Some(s.port);
                }
            }
        }
        None
    };
    if let Some(p) = live(embedder) {
        return Ok(p);
    }
    let _spawning = embedder.spawn_guard();
    if let Some(p) = live(embedder) {
        return Ok(p);
    }
    stop(app, embedder);

    let ops = app.state::<Ops>();
    let _ = ops.begin(app, "embed-engine", OpKind::Engine, "Embedding engine", true, None);
    let result = bring_up(app, embedder, &model);
    if let Err(e) = &result {
        ops.failed(app, "embed-engine", &e.to_string());
    }
    result
}

fn bring_up(app: &AppHandle, embedder: &Embedder, model: &crate::downloads::LibraryModel) -> Result<u16> {
    let ops = app.state::<Ops>();
    ops.detail(app, "embed-engine", "loading the embedding model");
    let backend = pick_backend();
    let exe = ensure_runtime(app, backend)
        .map_err(|e| AthanorError::Rag(format!("engine unavailable: {e}")))?;
    let port = free_port()?;
    let api_key = uuid::Uuid::new_v4().to_string();

    let mut cmd = Command::new(&exe);
    cmd.current_dir(exe.parent().expect("exe parent"))
        .args([
            "-m",
            &model.path,
            "--embeddings",
            "--pooling",
            "mean",
            "-c",
            &CTX.to_string(),
            "-b",
            &CTX.to_string(),
            "--rope-scaling",
            "yarn",
            "--rope-freq-scale",
            "0.75",
            "-ngl",
            "auto",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--api-key",
            &api_key,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| AthanorError::Rag(format!("failed to start embedding server: {e}")))?;
    guard::adopt(&child);

    for stream in [
        child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        std::thread::Builder::new()
            .name("embed-log".into())
            .spawn(move || {
                for line in std::io::BufReader::new(stream).lines().map_while(|l| l.ok()) {
                    log::info!(target: "embed", "{line}");
                }
            })
            .ok();
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| AthanorError::Rag(e.to_string()))?;
    let health = format!("http://127.0.0.1:{port}/health");
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        if let Ok(Some(code)) = child.try_wait() {
            return Err(AthanorError::Rag(format!(
                "embedding server exited during load (code {code:?})"
            )));
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            return Err(AthanorError::Rag("embedding server load timed out".into()));
        }
        if client.get(&health).send().map(|r| r.status() == 200).unwrap_or(false) {
            break;
        }
        std::thread::sleep(Duration::from_millis(300));
    }

    ops.resource(app, "embed-engine", &format!("port {port} · {EMBED_ENTRY}"));
    log::info!(target: "rag", "embedding server ready on port {port}");
    *embedder.active() = Some(EmbedServer {
        child,
        port,
        api_key,
        model_sha: model.sha256.clone(),
    });
    Ok(port)
}

// ── Embedding requests ────────────────────────────────────────

#[derive(Serialize)]
struct EmbedRequest<'a> {
    input: Vec<String>,
    model: &'a str,
}

#[derive(Deserialize)]
struct EmbedResponse {
    data: Vec<EmbedItem>,
}

#[derive(Deserialize)]
struct EmbedItem {
    index: usize,
    embedding: Vec<f32>,
}

fn embed_raw(app: &AppHandle, embedder: &Embedder, inputs: Vec<String>) -> Result<Vec<Vec<f32>>> {
    if inputs.is_empty() {
        return Ok(Vec::new());
    }
    let port = ensure(app, embedder)?;
    let api_key = embedder
        .active()
        .as_ref()
        .map(|s| s.api_key.clone())
        .ok_or_else(|| AthanorError::Rag("embedding server stopped".into()))?;

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| AthanorError::Rag(e.to_string()))?;

    // Batch so no single request is huge; honor data[].index on reassembly.
    let mut out: Vec<Vec<f32>> = vec![Vec::new(); inputs.len()];
    for (base, batch) in inputs.chunks(48).enumerate() {
        let resp = client
            .post(format!("http://127.0.0.1:{port}/v1/embeddings"))
            .bearer_auth(&api_key)
            .json(&EmbedRequest {
                input: batch.to_vec(),
                model: EMBED_ENTRY,
            })
            .send()
            .map_err(|e| AthanorError::Rag(format!("embedding request failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(AthanorError::Rag(format!(
                "embedding server returned HTTP {}",
                resp.status()
            )));
        }
        let parsed: EmbedResponse = resp
            .json()
            .map_err(|e| AthanorError::Rag(format!("bad embedding response: {e}")))?;
        for item in parsed.data {
            let slot = base * 48 + item.index;
            if slot < out.len() {
                out[slot] = item.embedding;
            }
        }
    }
    for (i, v) in out.iter().enumerate() {
        if v.len() != EMBED_DIM {
            return Err(AthanorError::Rag(format!(
                "embedding {i} has dim {}, expected {EMBED_DIM}",
                v.len()
            )));
        }
    }
    Ok(out)
}

/// Embed stored chunks (with the document task prefix).
pub fn embed_documents(app: &AppHandle, embedder: &Embedder, chunks: &[String]) -> Result<Vec<Vec<f32>>> {
    let prefixed: Vec<String> = chunks.iter().map(|c| format!("{DOC_PREFIX}{c}")).collect();
    embed_raw(app, embedder, prefixed)
}

/// Embed a retrieval query (with the query task prefix).
pub fn embed_query(app: &AppHandle, embedder: &Embedder, query: &str) -> Result<Vec<f32>> {
    let v = embed_raw(app, embedder, vec![format!("{QUERY_PREFIX}{query}")])?;
    v.into_iter()
        .next()
        .ok_or_else(|| AthanorError::Rag("query produced no embedding".into()))
}

//! Retrieval-augmented generation: per-workspace knowledge bases.
//!
//! Pipeline: extract text → chunk → embed (dedicated embedding server) →
//! store vectors (LanceDB). At chat time: embed the query, retrieve top-k,
//! inject as context, and report exactly which documents and chunks were
//! used (retrieval visibility is a first-class output, not a side effect).
//!
//! A per-workspace `knowledge.json` manifest is the source of truth for the
//! document list, status, and counts; LanceDB holds the vectors and text.
//! Indexing is a registered, cancellable operation like everything else.

pub mod embed;
pub mod extract;
pub mod chunk;
pub mod store;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::error::{AthanorError, Result};
use crate::ops::{OpKind, Ops};
use crate::workspaces::{self, write_atomic};

use embed::Embedder;

pub const RETRIEVAL_K: usize = 5;
/// Below this cosine similarity a chunk is noise, not context.
pub const MIN_SCORE: f32 = 0.18;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DocStatus {
    Indexing,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub id: String,
    pub name: String,
    /// Original absolute path (for re-index; the text is in the store).
    pub source_path: String,
    pub bytes: u64,
    pub chunk_count: usize,
    pub status: DocStatus,
    #[serde(default)]
    pub error: Option<String>,
    pub added_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    #[serde(default = "workspaces::schema_version_default")]
    schema: u32,
    #[serde(default)]
    documents: Vec<Document>,
    /// Whether retrieval is on for this workspace's chats.
    #[serde(default = "default_true")]
    retrieval_enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeBase {
    pub documents: Vec<Document>,
    pub retrieval_enabled: bool,
    pub chunk_total: usize,
}

/// One retrieved source, surfaced to the UI under the reply.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub doc_id: String,
    pub doc_name: String,
    pub chunk_index: i32,
    pub score: f32,
    pub excerpt: String,
}

// ── Paths + manifest ──────────────────────────────────────────

fn rag_dir(app: &AppHandle, workspace_id: &str) -> Result<PathBuf> {
    // Validate the id through the workspaces layer by requiring the dir exists.
    let ws = workspaces::data_root(app)?.join("workspaces").join(workspace_id);
    if !ws.exists() {
        return Err(AthanorError::Rag(format!("workspace {workspace_id} not found")));
    }
    let dir = ws.join("rag");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn lance_dir(app: &AppHandle, workspace_id: &str) -> Result<PathBuf> {
    let dir = rag_dir(app, workspace_id)?.join("lance");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn manifest_path(app: &AppHandle, workspace_id: &str) -> Result<PathBuf> {
    Ok(rag_dir(app, workspace_id)?.join("knowledge.json"))
}

fn read_manifest(app: &AppHandle, workspace_id: &str) -> Manifest {
    let Ok(path) = manifest_path(app, workspace_id) else {
        return Manifest { schema: 1, documents: vec![], retrieval_enabled: true };
    };
    if !path.exists() {
        return Manifest { schema: 1, documents: vec![], retrieval_enabled: true };
    }
    fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Manifest { schema: 1, documents: vec![], retrieval_enabled: true })
}

fn write_manifest(app: &AppHandle, workspace_id: &str, m: &Manifest) -> Result<()> {
    write_atomic(&manifest_path(app, workspace_id)?, serde_json::to_string_pretty(m)?.as_bytes())
}

pub fn knowledge_base(app: &AppHandle, workspace_id: &str) -> Result<KnowledgeBase> {
    let m = read_manifest(app, workspace_id);
    let chunk_total = m
        .documents
        .iter()
        .filter(|d| d.status == DocStatus::Ready)
        .map(|d| d.chunk_count)
        .sum();
    Ok(KnowledgeBase {
        documents: m.documents,
        retrieval_enabled: m.retrieval_enabled,
        chunk_total,
    })
}

pub fn set_retrieval_enabled(app: &AppHandle, workspace_id: &str, enabled: bool) -> Result<KnowledgeBase> {
    let mut m = read_manifest(app, workspace_id);
    m.retrieval_enabled = enabled;
    write_manifest(app, workspace_id, &m)?;
    knowledge_base(app, workspace_id)
}

fn upsert_doc(app: &AppHandle, workspace_id: &str, doc: Document) -> Result<()> {
    let mut m = read_manifest(app, workspace_id);
    if let Some(existing) = m.documents.iter_mut().find(|d| d.id == doc.id) {
        *existing = doc;
    } else {
        m.documents.insert(0, doc);
    }
    write_manifest(app, workspace_id, &m)
}

// ── Indexing (a registered operation) ─────────────────────────

fn op_id(workspace_id: &str, doc_id: &str) -> String {
    format!("index:{workspace_id}:{doc_id}")
}

/// Add a document to a workspace's knowledge base. Blocking; call via
/// spawn_blocking. Registered in the operations registry with progress and
/// cancel, orphan-safe (the embedding server is job-guarded).
pub fn add_document(
    app: &AppHandle,
    embedder: &Embedder,
    workspace_id: &str,
    source_path: &str,
) -> Result<Document> {
    let path = PathBuf::from(source_path);
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".into());
    let bytes = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    // Deterministic doc id from the path so re-adding replaces cleanly.
    let doc_id = format!("{:x}", md5_like(source_path));

    let ops = app.state::<Ops>();
    let id = op_id(workspace_id, &doc_id);
    let cancel = ops
        .begin(app, &id, OpKind::Index, &format!("Indexing · {name}"), true, None)
        .ok_or_else(|| AthanorError::Rag("this document is already indexing".into()))?;

    let started = Document {
        id: doc_id.clone(),
        name: name.clone(),
        source_path: source_path.to_string(),
        bytes,
        chunk_count: 0,
        status: DocStatus::Indexing,
        error: None,
        added_at: chrono::Utc::now().to_rfc3339(),
    };
    upsert_doc(app, workspace_id, started.clone())?;

    let result = index_inner(app, embedder, workspace_id, &doc_id, &name, source_path, &path, &cancel);

    let ops = app.state::<Ops>();
    match &result {
        Ok(doc) => {
            upsert_doc(app, workspace_id, doc.clone())?;
            ops.done(app, &id);
        }
        Err(e) if e.to_string().contains("cancelled") => {
            // Remove the half-written document entirely.
            let _ = remove_document(app, workspace_id, &doc_id);
            ops.cancelled(app, &id);
        }
        Err(e) => {
            let mut failed = started.clone();
            failed.status = DocStatus::Failed;
            failed.error = Some(e.to_string());
            let _ = upsert_doc(app, workspace_id, failed);
            ops.failed(app, &id, &e.to_string());
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn index_inner(
    app: &AppHandle,
    embedder: &Embedder,
    workspace_id: &str,
    doc_id: &str,
    name: &str,
    source_path: &str,
    path: &Path,
    cancel: &AtomicBool,
) -> Result<Document> {
    let ops = app.state::<Ops>();
    ops.detail(app, &op_id(workspace_id, doc_id), "reading document");
    let text = extract::extract(path)?;
    let chunks = chunk::chunk(&text);
    if chunks.is_empty() {
        return Err(AthanorError::Rag("document produced no chunks".into()));
    }
    let total = chunks.len() as u64;

    // Replace any prior vectors for this doc before inserting fresh ones.
    let dir = lance_dir(app, workspace_id)?;
    store::delete_doc(&dir, doc_id)?;

    // Embed + store in batches so progress moves and cancel is responsive.
    let batch_size = 32;
    let mut done = 0u64;
    for group in chunks.chunks(batch_size) {
        if cancel.load(Ordering::Relaxed) {
            return Err(AthanorError::Rag("cancelled".into()));
        }
        let texts: Vec<String> = group.iter().map(|c| c.text.clone()).collect();
        let vectors = embed::embed_documents(app, embedder, &texts)?;
        let records: Vec<store::Record> = group
            .iter()
            .zip(vectors)
            .map(|(c, v)| store::Record {
                id: format!("{doc_id}-{}", c.index),
                doc_id: doc_id.to_string(),
                doc_name: name.to_string(),
                chunk_index: c.index as i32,
                text: c.text.clone(),
                vector: v,
            })
            .collect();
        store::add(&dir, records)?;
        done += group.len() as u64;
        ops.progress(app, &op_id(workspace_id, doc_id), done, total, "embedding chunks");
    }

    Ok(Document {
        id: doc_id.to_string(),
        name: name.to_string(),
        source_path: source_path.to_string(),
        bytes: fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        chunk_count: chunks.len(),
        status: DocStatus::Ready,
        error: None,
        added_at: chrono::Utc::now().to_rfc3339(),
    })
}

pub fn remove_document(app: &AppHandle, workspace_id: &str, doc_id: &str) -> Result<KnowledgeBase> {
    let dir = lance_dir(app, workspace_id)?;
    store::delete_doc(&dir, doc_id)?;
    let mut m = read_manifest(app, workspace_id);
    m.documents.retain(|d| d.id != doc_id);
    write_manifest(app, workspace_id, &m)?;
    knowledge_base(app, workspace_id)
}

pub fn cancel_indexing(app: &AppHandle, workspace_id: &str, doc_id: &str) {
    if let Some(ops) = app.try_state::<Ops>() {
        ops.request_cancel(&op_id(workspace_id, doc_id));
    }
}

pub fn preview_chunks(app: &AppHandle, workspace_id: &str, doc_id: &str) -> Result<Vec<Source>> {
    let dir = lance_dir(app, workspace_id)?;
    let hits = store::doc_chunks(&dir, doc_id, 200)?;
    Ok(hits
        .into_iter()
        .map(|h| Source {
            doc_id: h.doc_id,
            doc_name: h.doc_name,
            chunk_index: h.chunk_index,
            score: 1.0,
            excerpt: h.text,
        })
        .collect())
}

// ── Retrieval (chat time) ─────────────────────────────────────

/// Retrieve context for a query. Returns (context_block, sources). Empty when
/// retrieval is off or the base is empty — chat proceeds normally either way.
pub fn retrieve(
    app: &AppHandle,
    embedder: &Embedder,
    workspace_id: &str,
    query: &str,
) -> Result<(String, Vec<Source>)> {
    let m = read_manifest(app, workspace_id);
    let ready = m.documents.iter().any(|d| d.status == DocStatus::Ready);
    if !m.retrieval_enabled || !ready {
        return Ok((String::new(), Vec::new()));
    }

    let dir = lance_dir(app, workspace_id)?;
    let qvec = embed::embed_query(app, embedder, query)?;
    let hits: Vec<store::Hit> = store::search(&dir, qvec, RETRIEVAL_K, None)?
        .into_iter()
        .filter(|h| h.score >= MIN_SCORE)
        .collect();
    if hits.is_empty() {
        return Ok((String::new(), Vec::new()));
    }

    // One search feeds both the context block (full chunk text) and the
    // sources the UI shows (truncated excerpt).
    let mut block = String::from(
        "Use the following context from the workspace's documents to answer. \
         If the answer isn't in the context, say so.\n\n",
    );
    let mut sources = Vec::with_capacity(hits.len());
    for (i, h) in hits.into_iter().enumerate() {
        block.push_str(&format!("[{}] from \"{}\":\n{}\n\n", i + 1, h.doc_name, h.text));
        sources.push(Source {
            doc_id: h.doc_id,
            doc_name: h.doc_name,
            chunk_index: h.chunk_index,
            score: h.score,
            excerpt: h.text.chars().take(240).collect(),
        });
    }
    Ok((block, sources))
}

/// A tiny, stable non-cryptographic hash for deterministic doc ids from paths.
fn md5_like(s: &str) -> u128 {
    // FNV-1a 128-bit — deterministic, dependency-free, collision-safe enough
    // for per-workspace document ids.
    let mut hash: u128 = 0x6c62272e07bb014262b821756295c58d;
    let prime: u128 = 0x0000000001000000000000000000013b;
    for b in s.bytes() {
        hash ^= b as u128;
        hash = hash.wrapping_mul(prime);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_id_is_deterministic() {
        assert_eq!(md5_like("C:/a/b.pdf"), md5_like("C:/a/b.pdf"));
        assert_ne!(md5_like("C:/a/b.pdf"), md5_like("C:/a/c.pdf"));
    }
}

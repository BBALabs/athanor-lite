//! Per-workspace vector store, backed by LanceDB.
//!
//! One Lance database per workspace at `workspaces/<id>/rag/lance`, one table
//! `chunks` holding (id, doc_id, doc_name, chunk_index, text, vector[768]).
//! Vectors from llama.cpp are L2-normalized, so LanceDB's default L2 nearest-
//! neighbor ranking is identical to cosine ordering; we report a cosine
//! similarity score for display.
//!
//! LanceDB is async; the rest of the RAG code is blocking (spawn_blocking).
//! A single dedicated multi-thread Tokio runtime bridges the two — created
//! once, shared process-wide.

use std::path::Path;
use std::sync::Arc;
use std::sync::OnceLock;

use arrow_array::{
    types::Float32Type, FixedSizeListArray, Int32Array, RecordBatch, RecordBatchIterator,
    RecordBatchReader, StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::Connection;

use super::embed::EMBED_DIM;
use crate::error::{AthanorError, Result};

const TABLE: &str = "chunks";

fn rt() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime for lancedb")
    })
}

fn schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("doc_id", DataType::Utf8, false),
        Field::new("doc_name", DataType::Utf8, false),
        Field::new("chunk_index", DataType::Int32, false),
        Field::new("text", DataType::Utf8, false),
        Field::new(
            "vector",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                EMBED_DIM as i32,
            ),
            true,
        ),
    ]))
}

/// One retrieved chunk.
#[derive(Debug, Clone)]
pub struct Hit {
    pub doc_id: String,
    pub doc_name: String,
    pub chunk_index: i32,
    pub text: String,
    /// Cosine similarity in [−1, 1]; higher is closer.
    pub score: f32,
}

/// A chunk to store, with its already-computed embedding.
pub struct Record {
    pub id: String,
    pub doc_id: String,
    pub doc_name: String,
    pub chunk_index: i32,
    pub text: String,
    pub vector: Vec<f32>,
}

async fn connect(dir: &Path) -> Result<Connection> {
    let uri = dir.to_string_lossy().to_string();
    lancedb::connect(&uri)
        .execute()
        .await
        .map_err(|e| AthanorError::Rag(format!("open vector store: {e}")))
}

async fn open_or_create(conn: &Connection) -> Result<lancedb::Table> {
    let names = conn
        .table_names()
        .execute()
        .await
        .map_err(|e| AthanorError::Rag(e.to_string()))?;
    if names.iter().any(|n| n == TABLE) {
        conn.open_table(TABLE)
            .execute()
            .await
            .map_err(|e| AthanorError::Rag(format!("open table: {e}")))
    } else {
        conn.create_empty_table(TABLE, schema())
            .execute()
            .await
            .map_err(|e| AthanorError::Rag(format!("create table: {e}")))
    }
}

fn build_batch(records: &[Record]) -> Result<RecordBatch> {
    let ids = StringArray::from_iter_values(records.iter().map(|r| r.id.as_str()));
    let doc_ids = StringArray::from_iter_values(records.iter().map(|r| r.doc_id.as_str()));
    let doc_names = StringArray::from_iter_values(records.iter().map(|r| r.doc_name.as_str()));
    let indices = Int32Array::from_iter_values(records.iter().map(|r| r.chunk_index));
    let texts = StringArray::from_iter_values(records.iter().map(|r| r.text.as_str()));
    let vectors = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
        records
            .iter()
            .map(|r| Some(r.vector.iter().copied().map(Some).collect::<Vec<_>>())),
        EMBED_DIM as i32,
    );
    RecordBatch::try_new(
        schema(),
        vec![
            Arc::new(ids),
            Arc::new(doc_ids),
            Arc::new(doc_names),
            Arc::new(indices),
            Arc::new(texts),
            Arc::new(vectors),
        ],
    )
    .map_err(|e| AthanorError::Rag(format!("build record batch: {e}")))
}

/// Append chunk records to a workspace's store (creating it on first use).
pub fn add(dir: &Path, records: Vec<Record>) -> Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    rt().block_on(async move {
        let conn = connect(dir).await?;
        let table = open_or_create(&conn).await?;
        let batch = build_batch(&records)?;
        let sch = schema();
        let reader: Box<dyn RecordBatchReader + Send> =
            Box::new(RecordBatchIterator::new(vec![Ok(batch)], sch));
        table
            .add(reader)
            .execute()
            .await
            .map_err(|e| AthanorError::Rag(format!("store chunks: {e}")))?;
        Ok(())
    })
}

/// Top-k nearest chunks to a query vector, optionally scoped to one document.
pub fn search(dir: &Path, query: Vec<f32>, k: usize, doc_id: Option<&str>) -> Result<Vec<Hit>> {
    if !dir.join("chunks.lance").exists() && !has_table(dir) {
        return Ok(Vec::new());
    }
    rt().block_on(async move {
        let conn = connect(dir).await?;
        let names = conn.table_names().execute().await.map_err(|e| AthanorError::Rag(e.to_string()))?;
        if !names.iter().any(|n| n == TABLE) {
            return Ok(Vec::new());
        }
        let table = conn
            .open_table(TABLE)
            .execute()
            .await
            .map_err(|e| AthanorError::Rag(e.to_string()))?;
        let mut q = table
            .vector_search(query)
            .map_err(|e| AthanorError::Rag(format!("vector search: {e}")))?
            .limit(k);
        if let Some(id) = doc_id {
            q = q.only_if(format!("doc_id = '{}'", id.replace('\'', "''")));
        }
        let batches: Vec<RecordBatch> = q
            .execute()
            .await
            .map_err(|e| AthanorError::Rag(format!("run query: {e}")))?
            .try_collect()
            .await
            .map_err(|e| AthanorError::Rag(format!("collect results: {e}")))?;
        Ok(decode_hits(&batches))
    })
}

fn has_table(dir: &Path) -> bool {
    rt().block_on(async move {
        match connect(dir).await {
            Ok(conn) => conn
                .table_names()
                .execute()
                .await
                .map(|n| n.iter().any(|t| t == TABLE))
                .unwrap_or(false),
            Err(_) => false,
        }
    })
}

fn col_str<'a>(batch: &'a RecordBatch, name: &str) -> Option<&'a StringArray> {
    batch.column_by_name(name)?.as_any().downcast_ref::<StringArray>()
}

fn decode_hits(batches: &[RecordBatch]) -> Vec<Hit> {
    let mut hits = Vec::new();
    for batch in batches {
        let doc_ids = col_str(batch, "doc_id");
        let doc_names = col_str(batch, "doc_name");
        let texts = col_str(batch, "text");
        let indices = batch
            .column_by_name("chunk_index")
            .and_then(|c| c.as_any().downcast_ref::<Int32Array>());
        // LanceDB appends `_distance` (L2²) for vector search.
        let dists = batch
            .column_by_name("_distance")
            .and_then(|c| c.as_any().downcast_ref::<arrow_array::Float32Array>());
        for row in 0..batch.num_rows() {
            let l2sq = dists.map(|d| d.value(row)).unwrap_or(0.0);
            // Unit vectors: L2² = 2 − 2·cos ⇒ cos = 1 − L2²/2.
            let score = 1.0 - l2sq / 2.0;
            hits.push(Hit {
                doc_id: doc_ids.map(|a| a.value(row).to_string()).unwrap_or_default(),
                doc_name: doc_names.map(|a| a.value(row).to_string()).unwrap_or_default(),
                chunk_index: indices.map(|a| a.value(row)).unwrap_or(0),
                text: texts.map(|a| a.value(row).to_string()).unwrap_or_default(),
                score,
            });
        }
    }
    hits
}

/// Remove every chunk belonging to a document.
pub fn delete_doc(dir: &Path, doc_id: &str) -> Result<()> {
    if !has_table(dir) {
        return Ok(());
    }
    let doc_id = doc_id.to_string();
    rt().block_on(async move {
        let conn = connect(dir).await?;
        let table = conn
            .open_table(TABLE)
            .execute()
            .await
            .map_err(|e| AthanorError::Rag(e.to_string()))?;
        table
            .delete(format!("doc_id = '{}'", doc_id.replace('\'', "''")).as_str())
            .await
            .map_err(|e| AthanorError::Rag(format!("delete document: {e}")))?;
        Ok(())
    })
}

/// Fetch a document's chunks in order — for the chunk-preview panel.
pub fn doc_chunks(dir: &Path, doc_id: &str, limit: usize) -> Result<Vec<Hit>> {
    if !has_table(dir) {
        return Ok(Vec::new());
    }
    let doc_id = doc_id.to_string();
    rt().block_on(async move {
        let conn = connect(dir).await?;
        let table = conn
            .open_table(TABLE)
            .execute()
            .await
            .map_err(|e| AthanorError::Rag(e.to_string()))?;
        let batches: Vec<RecordBatch> = table
            .query()
            .only_if(format!("doc_id = '{}'", doc_id.replace('\'', "''")))
            .limit(limit)
            .execute()
            .await
            .map_err(|e| AthanorError::Rag(e.to_string()))?
            .try_collect()
            .await
            .map_err(|e| AthanorError::Rag(e.to_string()))?;
        let mut hits = decode_hits(&batches);
        hits.sort_by_key(|h| h.chunk_index);
        Ok(hits)
    })
}

//! Curated model catalog. Reviewed by hand, embedded in the binary — Athanor
//! recommends models it can stand behind, not a scrape of the Hub.

pub mod recommend;
pub mod templates;

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::error::{AthanorError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    General,
    Coding,
    Reasoning,
    Embedding,
}

/// One downloadable artifact, verified against Hugging Face LFS metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuantFile {
    pub name: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuantOption {
    pub label: String,
    /// GGUF file size on disk.
    pub file_gb: f64,
    /// Estimated memory floor: weights + KV cache at 8K context + runtime overhead.
    pub min_mem_gb: f64,
    /// Exact files for this quant (split GGUFs have several, in order).
    #[serde(default)]
    pub files: Vec<QuantFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub family: String,
    pub name: String,
    pub params_b: f64,
    pub roles: Vec<Role>,
    /// Hand-tuned capability ordinal (0–100) relative to the rest of the catalog.
    pub quality: u32,
    pub context_length: u32,
    pub license: String,
    pub hf_repo: String,
    pub blurb: String,
    pub quants: Vec<QuantOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub version: String,
    pub entries: Vec<CatalogEntry>,
}

static CATALOG_JSON: &str = include_str!("catalog.json");

pub fn catalog() -> Result<&'static Catalog> {
    static CATALOG: OnceLock<Option<Catalog>> = OnceLock::new();
    CATALOG
        .get_or_init(|| match serde_json::from_str::<Catalog>(CATALOG_JSON) {
            Ok(c) => {
                log::info!(target: "catalog", "loaded {} entries (v{})", c.entries.len(), c.version);
                Some(c)
            }
            Err(e) => {
                log::error!(target: "catalog", "embedded catalog failed to parse: {e}");
                None
            }
        })
        .as_ref()
        .ok_or_else(|| AthanorError::Catalog("embedded catalog failed to parse".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_parses_and_is_sane() {
        let c = catalog().expect("catalog must parse");
        assert!(c.entries.len() >= 15);
        for e in &c.entries {
            assert!(!e.quants.is_empty(), "{} has no quants", e.id);
            for q in &e.quants {
                assert!(
                    q.min_mem_gb > q.file_gb,
                    "{} {}: memory floor must exceed file size",
                    e.id,
                    q.label
                );
            }
        }
    }

    #[test]
    fn every_quant_has_verified_files() {
        let c = catalog().unwrap();
        for e in &c.entries {
            for q in &e.quants {
                assert!(!q.files.is_empty(), "{} {} has no files", e.id, q.label);
                for f in &q.files {
                    assert_eq!(f.sha256.len(), 64, "{} {} bad sha", e.id, q.label);
                    assert!(f.sha256.chars().all(|c| c.is_ascii_hexdigit()));
                    assert!(f.size_bytes > 0);
                    assert!(f.name.ends_with(".gguf"), "{} {}: {}", e.id, q.label, f.name);
                }
                let total: u64 = q.files.iter().map(|f| f.size_bytes).sum();
                let gb = total as f64 / 1e9;
                assert!(
                    (gb - q.file_gb).abs() < 0.06,
                    "{} {}: fileGb {} vs real {:.2}",
                    e.id,
                    q.label,
                    q.file_gb,
                    gb
                );
            }
        }
    }

    #[test]
    fn catalog_ids_are_unique() {
        let c = catalog().unwrap();
        let mut ids: Vec<_> = c.entries.iter().map(|e| &e.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), c.entries.len());
    }
}

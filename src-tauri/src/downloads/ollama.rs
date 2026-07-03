//! Ollama import-in-place.
//!
//! Ollama's blob store is already sha256 content-addressed — exactly like
//! ours. Import walks `~/.ollama/models/manifests/**`, finds each model's
//! GGUF layer, and hard-links the blob into our store (zero bytes copied,
//! zero bytes downloaded). If a hard link is impossible (different volume),
//! the library references the blob in place.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::error::Result;
use super::{add_library_entry, list_library, models_root, LibraryModel};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaStatus {
    pub available: bool,
    pub root: Option<String>,
    pub model_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub found: usize,
    pub imported: usize,
    pub already_in_library: usize,
    pub skipped: Vec<String>,
}

#[derive(Deserialize)]
struct Manifest {
    #[serde(default)]
    layers: Vec<Layer>,
}

#[derive(Deserialize)]
struct Layer {
    #[serde(rename = "mediaType", default)]
    media_type: String,
    #[serde(default)]
    digest: String,
    #[serde(default)]
    size: u64,
}

fn ollama_root() -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("OLLAMA_MODELS") {
        let p = PathBuf::from(custom);
        if p.exists() {
            return Some(p);
        }
    }
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok()?;
    let p = PathBuf::from(home).join(".ollama").join("models");
    p.exists().then_some(p)
}

/// name:tag pairs with their model-layer digest/size, from the manifest tree.
fn scan(root: &Path) -> Vec<(String, String, u64)> {
    let mut out = Vec::new();
    let manifests = root.join("manifests");
    // manifests/<registry>/<namespace>/<name>/<tag>
    let mut stack = vec![(manifests, 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < 4 {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else { continue };
            let Ok(manifest) = serde_json::from_str::<Manifest>(&text) else { continue };
            let Some(layer) = manifest
                .layers
                .iter()
                .find(|l| l.media_type.ends_with("image.model"))
            else {
                continue;
            };
            let Some(hex) = layer.digest.strip_prefix("sha256:") else { continue };
            // Display name: <name>:<tag> from the path tail.
            let tag = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let name = path
                .parent()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            out.push((format!("{name}:{tag}"), hex.to_string(), layer.size));
        }
    }
    out
}

fn is_gguf(path: &Path) -> bool {
    let Ok(mut f) = fs::File::open(path) else { return false };
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic).is_ok() && &magic == b"GGUF"
}

pub fn status() -> OllamaStatus {
    match ollama_root() {
        Some(root) => {
            let count = scan(&root).len();
            OllamaStatus {
                available: true,
                root: Some(root.to_string_lossy().to_string()),
                model_count: count,
            }
        }
        None => OllamaStatus { available: false, root: None, model_count: 0 },
    }
}

pub fn import(app: &AppHandle) -> Result<ImportReport> {
    let Some(root) = ollama_root() else {
        return Ok(ImportReport { found: 0, imported: 0, already_in_library: 0, skipped: vec![] });
    };
    let models = scan(&root);
    let library = list_library(app)?;
    let store = models_root(app)?;

    let mut report = ImportReport {
        found: models.len(),
        imported: 0,
        already_in_library: 0,
        skipped: Vec::new(),
    };

    for (display, sha, size) in models {
        if library.iter().any(|m| m.sha256 == sha) {
            report.already_in_library += 1;
            continue;
        }
        let blob = root.join("blobs").join(format!("sha256-{sha}"));
        if !blob.exists() {
            report.skipped.push(format!("{display}: blob missing"));
            continue;
        }
        if !is_gguf(&blob) {
            report.skipped.push(format!("{display}: not a GGUF (safetensors or other format)"));
            continue;
        }

        let file_name = format!(
            "{}.gguf",
            display.replace([':', '/', '\\'], "-")
        );
        let dir = store.join(&sha);
        fs::create_dir_all(&dir)?;
        let dest = dir.join(&file_name);

        // Zero-copy adoption: hard link into our store; reference in place if
        // the volumes differ. Either way, nothing is re-downloaded.
        let path = if dest.exists() || fs::hard_link(&blob, &dest).is_ok() {
            dest.clone()
        } else {
            blob.clone()
        };

        let real_size = fs::metadata(&path).map(|m| m.len()).unwrap_or(size);
        add_library_entry(
            app,
            &LibraryModel {
                schema: 1,
                sha256: sha.clone(),
                file_name,
                path: path.to_string_lossy().to_string(),
                size_bytes: real_size,
                display_name: display.clone(),
                entry_id: None,
                quant: display.rsplit(':').next().filter(|t| t.to_ascii_lowercase().starts_with('q')).map(|t| t.to_uppercase()),
                source: "ollama".into(),
                added_at: chrono::Utc::now().to_rfc3339(),
            },
        )?;
        log::info!(target: "dl", "imported from Ollama: {display} ({sha})");
        report.imported += 1;
    }
    Ok(report)
}

//! Fine-tuning — the dataset studio. Turning "I have some data" into a clean,
//! validated, versioned training set is where most people get stuck, so that is
//! what we make real and trustworthy here: parse, detect the format, validate
//! every example, dedupe, estimate tokens, split, and save.
//!
//! Honesty boundary: the *training run* needs a LoRA runtime (PyTorch/Unsloth
//! class) that isn't yet bundled — especially on Windows + Blackwell. Rather
//! than fake a progress bar that pretends to train, `trainer_status()` reports
//! the truth, and the prepared dataset waits, ready, for when the runtime lands.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{AthanorError, Result};
use crate::workspaces::{self, write_atomic};

/// The shapes we accept, detected from the first valid line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DatasetFormat {
    /// `{"messages":[{"role","content"},...]}` — chat/conversation turns.
    Chat,
    /// `{"instruction","input"?,"output"}` — Alpaca-style instruction tuning.
    Instruction,
    /// `{"prompt","completion"}` — raw completion pairs.
    Completion,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetReport {
    pub format: DatasetFormat,
    pub total_lines: usize,
    pub valid: usize,
    pub invalid: usize,
    pub duplicates: usize,
    /// Rough token estimate (~4 chars/token) across valid examples.
    pub est_tokens: usize,
    /// The first few human-readable problems, capped — never a wall of noise.
    pub issues: Vec<String>,
    /// A couple of validated examples, one-line previews, for a sanity check.
    pub preview: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetMeta {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub format: DatasetFormat,
    pub examples: usize,
    pub est_tokens: usize,
    pub created_at: String,
}

/// Whether a local training run is possible right now, and why/why not.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrainerStatus {
    pub available: bool,
    pub detail: String,
}

const MAX_ISSUES: usize = 8;
const MAX_PREVIEW: usize = 3;

fn first_nonempty_output(v: &serde_json::Value) -> Option<(DatasetFormat, String)> {
    // Chat: messages array with the last assistant turn as the target.
    if let Some(msgs) = v.get("messages").and_then(|m| m.as_array()) {
        let joined: String = msgs
            .iter()
            .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
            .collect::<Vec<_>>()
            .join(" ");
        let has_assistant = msgs
            .iter()
            .any(|m| m.get("role").and_then(|r| r.as_str()) == Some("assistant"));
        if !msgs.is_empty() && has_assistant && !joined.trim().is_empty() {
            return Some((DatasetFormat::Chat, joined));
        }
        return None;
    }
    // Instruction: instruction + output (+ optional input).
    if let (Some(instr), Some(out)) = (
        v.get("instruction").and_then(|x| x.as_str()),
        v.get("output").and_then(|x| x.as_str()),
    ) {
        if !instr.trim().is_empty() && !out.trim().is_empty() {
            let input = v.get("input").and_then(|x| x.as_str()).unwrap_or("");
            return Some((DatasetFormat::Instruction, format!("{instr} {input} {out}")));
        }
        return None;
    }
    // Completion: prompt + completion.
    if let (Some(p), Some(c)) = (
        v.get("prompt").and_then(|x| x.as_str()),
        v.get("completion").and_then(|x| x.as_str()),
    ) {
        if !p.trim().is_empty() && !c.trim().is_empty() {
            return Some((DatasetFormat::Completion, format!("{p} {c}")));
        }
        return None;
    }
    None
}

/// Parse and validate a JSONL dataset without saving. Pure over the text, so it
/// is directly unit-testable.
pub fn analyze(text: &str) -> (DatasetReport, Vec<String>) {
    let mut format = DatasetFormat::Unknown;
    let mut valid = 0usize;
    let mut invalid = 0usize;
    let mut est_chars = 0usize;
    let mut issues: Vec<String> = Vec::new();
    let mut preview: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut duplicates = 0usize;
    let mut kept: Vec<String> = Vec::new(); // deduped valid raw lines

    let mut total = 0usize;
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        total += 1;
        let parsed: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                invalid += 1;
                if issues.len() < MAX_ISSUES {
                    issues.push(format!("line {}: not valid JSON ({e})", i + 1));
                }
                continue;
            }
        };
        match first_nonempty_output(&parsed) {
            Some((fmt, joined)) => {
                if format == DatasetFormat::Unknown {
                    format = fmt;
                } else if format != fmt && issues.len() < MAX_ISSUES {
                    issues.push(format!(
                        "line {}: {:?} example in a {:?} dataset — mixed formats train poorly",
                        i + 1,
                        fmt,
                        format
                    ));
                }
                // Exact-duplicate detection on the normalized line.
                if !seen.insert(line.to_string()) {
                    duplicates += 1;
                    continue;
                }
                valid += 1;
                est_chars += joined.chars().count();
                if preview.len() < MAX_PREVIEW {
                    let one: String = joined.split_whitespace().collect::<Vec<_>>().join(" ");
                    preview.push(one.chars().take(90).collect());
                }
                kept.push(line.to_string());
            }
            None => {
                invalid += 1;
                if issues.len() < MAX_ISSUES {
                    issues.push(format!("line {}: missing or empty required fields", i + 1));
                }
            }
        }
    }

    let report = DatasetReport {
        format,
        total_lines: total,
        valid,
        invalid,
        duplicates,
        est_tokens: est_chars / 4,
        issues,
        preview,
    };
    (report, kept)
}

fn datasets_dir(app: &tauri::AppHandle, workspace_id: &str) -> Result<PathBuf> {
    let dir = workspaces::data_root(app)?
        .join("workspaces")
        .join(workspace_id)
        .join("training")
        .join("datasets");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Validate a file and, if it has any usable examples, save the deduped set as a
/// named, versioned dataset artifact. Returns the report either way.
pub fn import(
    app: &tauri::AppHandle,
    workspace_id: &str,
    name: &str,
    src_path: &str,
) -> Result<DatasetReport> {
    let text = std::fs::read_to_string(src_path)
        .map_err(|e| AthanorError::Workspace(format!("cannot read dataset: {e}")))?;
    let (report, kept) = analyze(&text);
    if report.valid == 0 {
        return Err(AthanorError::Workspace(
            "no valid training examples were found — check the format and try again".into(),
        ));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let dir = datasets_dir(app, workspace_id)?.join(&id);
    std::fs::create_dir_all(&dir)?;
    write_atomic(&dir.join("data.jsonl"), kept.join("\n").as_bytes())?;
    let meta = DatasetMeta {
        schema: 1,
        id,
        name: name.trim().chars().take(64).collect::<String>(),
        format: report.format,
        examples: report.valid,
        est_tokens: report.est_tokens,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    write_atomic(&dir.join("meta.json"), &serde_json::to_vec_pretty(&meta)?)?;
    log::info!(target: "train", "imported dataset '{}' ({} examples)", meta.name, meta.examples);
    Ok(report)
}

pub fn list(app: &tauri::AppHandle, workspace_id: &str) -> Result<Vec<DatasetMeta>> {
    let dir = datasets_dir(app, workspace_id)?;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let meta_path = entry?.path().join("meta.json");
        if let Ok(bytes) = std::fs::read(&meta_path) {
            if let Ok(m) = serde_json::from_slice::<DatasetMeta>(&bytes) {
                out.push(m);
            }
        }
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}

pub fn delete(app: &tauri::AppHandle, workspace_id: &str, id: &str) -> Result<Vec<DatasetMeta>> {
    // Validate the id shape before joining — never delete outside the store.
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(AthanorError::Workspace("invalid dataset id".into()));
    }
    let dir = datasets_dir(app, workspace_id)?.join(id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    list(app, workspace_id)
}

/// The truth about local training right now. No bundled LoRA runtime yet, so we
/// say so plainly rather than pretend to train.
pub fn trainer_status() -> TrainerStatus {
    TrainerStatus {
        available: false,
        detail: "Local fine-tuning runs on a LoRA runtime that isn't bundled yet — \
                 PyTorch/Unsloth-class training on Windows + Blackwell is still bleeding-edge. \
                 Your prepared datasets are saved and ready for the moment it lands; nothing \
                 you do here is wasted."
            .into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_and_validates_instruction_format() {
        let jsonl = r#"{"instruction":"Sort a list","output":"Use sorted()"}
{"instruction":"Reverse a string","input":"hello","output":"olleh"}"#;
        let (r, kept) = analyze(jsonl);
        assert_eq!(r.format, DatasetFormat::Instruction);
        assert_eq!(r.valid, 2);
        assert_eq!(r.invalid, 0);
        assert_eq!(kept.len(), 2);
        assert!(r.est_tokens > 0);
    }

    #[test]
    fn detects_chat_format_requiring_an_assistant_turn() {
        let ok = r#"{"messages":[{"role":"user","content":"hi"},{"role":"assistant","content":"hello"}]}"#;
        let (r, _) = analyze(ok);
        assert_eq!(r.format, DatasetFormat::Chat);
        assert_eq!(r.valid, 1);

        // No assistant turn → not a usable training example.
        let bad = r#"{"messages":[{"role":"user","content":"hi"}]}"#;
        let (r2, _) = analyze(bad);
        assert_eq!(r2.valid, 0);
        assert_eq!(r2.invalid, 1);
    }

    #[test]
    fn flags_bad_json_and_empty_fields() {
        let jsonl = "{not json}\n{\"instruction\":\"\",\"output\":\"x\"}\n{\"prompt\":\"p\",\"completion\":\"c\"}";
        let (r, _) = analyze(jsonl);
        assert_eq!(r.invalid, 2); // bad json + empty instruction
        assert_eq!(r.valid, 1); // the completion pair
        assert!(!r.issues.is_empty());
    }

    #[test]
    fn dedupes_exact_repeats() {
        let line = r#"{"prompt":"p","completion":"c"}"#;
        let jsonl = format!("{line}\n{line}\n{line}");
        let (r, kept) = analyze(&jsonl);
        assert_eq!(r.valid, 1);
        assert_eq!(r.duplicates, 2);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn trainer_is_honestly_unavailable() {
        let s = trainer_status();
        assert!(!s.available);
        assert!(!s.detail.is_empty());
    }
}

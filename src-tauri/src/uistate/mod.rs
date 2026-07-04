//! Durable UI state that isn't a user *preference* — chiefly which contextual
//! walkthroughs ("coaches") the user has already seen, so a feature teaches
//! itself exactly once and never nags again. Kept separate from preferences so
//! "reset the tutorials" can't disturb real settings.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::workspaces::{self, write_atomic};

fn schema_default() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoachState {
    #[serde(default = "schema_default")]
    pub schema: u32,
    /// Walkthrough ids the user has completed or dismissed. A set, kept as a
    /// sorted Vec so the file reads cleanly and diffs are stable.
    #[serde(default)]
    pub seen: Vec<String>,
}

impl Default for CoachState {
    fn default() -> Self {
        Self { schema: schema_default(), seen: Vec::new() }
    }
}

fn coach_path(app: &tauri::AppHandle) -> Result<PathBuf> {
    Ok(workspaces::data_root(app)?.join("coach.json"))
}

/// Load the seen-set. A missing or unreadable file is not an error — the worst
/// case is a walkthrough shows once more, never a broken boot.
pub fn load(app: &tauri::AppHandle) -> Result<CoachState> {
    let path = coach_path(app)?;
    match std::fs::read(&path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_default()),
        Err(_) => Ok(CoachState::default()),
    }
}

fn save(app: &tauri::AppHandle, state: &CoachState) -> Result<()> {
    let path = coach_path(app)?;
    write_atomic(&path, &serde_json::to_vec_pretty(state)?)?;
    Ok(())
}

/// Record that a walkthrough has been seen (idempotent) and return the new set.
pub fn mark_seen(app: &tauri::AppHandle, id: &str) -> Result<CoachState> {
    let mut state = load(app)?;
    if !state.seen.iter().any(|s| s == id) {
        state.seen.push(id.to_string());
        state.seen.sort();
        save(app, &state)?;
    }
    Ok(state)
}

/// Forget every seen walkthrough — "replay the tutorials" from settings.
pub fn reset(app: &tauri::AppHandle) -> Result<CoachState> {
    let state = CoachState::default();
    save(app, &state)?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seen_set_is_idempotent_and_sorted() {
        let mut s = CoachState::default();
        // Mirror mark_seen's dedup+sort logic without touching disk.
        for id in ["knowledge", "templates", "knowledge", "mcp"] {
            if !s.seen.iter().any(|x| x == id) {
                s.seen.push(id.to_string());
                s.seen.sort();
            }
        }
        assert_eq!(s.seen, vec!["knowledge", "mcp", "templates"]);
    }

    #[test]
    fn missing_file_deserializes_to_empty() {
        // A blank/garbage payload must degrade to the default, never panic.
        let parsed: CoachState = serde_json::from_slice(b"").unwrap_or_default();
        assert!(parsed.seen.is_empty());
        let parsed: CoachState = serde_json::from_slice(b"{ not json").unwrap_or_default();
        assert!(parsed.seen.is_empty());
    }
}

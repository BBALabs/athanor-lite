//! App-wide preferences — the handful of choices that aren't per-workspace and
//! aren't already owned by another module (metrics has its own consent file, the
//! local API its own key file). Today that's the accent family; the file is
//! versioned so more can join without a migration.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::workspaces::{self, write_atomic};

fn schema_default() -> u32 {
    1
}

fn accent_default() -> String {
    "violet".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    #[serde(default = "schema_default")]
    pub schema: u32,
    /// A curated warm-light accent family id ("violet" is the default, and the
    /// only one the design spec ships as canonical). Hue family only — the
    /// Black Glass material never changes.
    #[serde(default = "accent_default")]
    pub accent: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self { schema: schema_default(), accent: accent_default() }
    }
}

/// The accent ids the UI offers. Kept here so a bad value from a hand-edited
/// file (or a downgraded build) falls back to the default instead of theming
/// the app into an unreadable state.
const ACCENTS: [&str; 4] = ["violet", "indigo", "orchid", "rose"];

fn prefs_path(app: &tauri::AppHandle) -> Result<PathBuf> {
    Ok(workspaces::data_root(app)?.join("preferences.json"))
}

pub fn load(app: &tauri::AppHandle) -> Result<Preferences> {
    let mut p: Preferences = match std::fs::read(prefs_path(app)?) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => Preferences::default(),
    };
    if !ACCENTS.contains(&p.accent.as_str()) {
        p.accent = accent_default();
    }
    Ok(p)
}

fn save(app: &tauri::AppHandle, p: &Preferences) -> Result<()> {
    write_atomic(&prefs_path(app)?, &serde_json::to_vec_pretty(p)?)?;
    Ok(())
}

pub fn set_accent(app: &tauri::AppHandle, accent: &str) -> Result<Preferences> {
    let mut p = load(app)?;
    // Ignore anything not on the curated list — the design language is binding.
    if ACCENTS.contains(&accent) {
        p.accent = accent.to_string();
        save(app, &p)?;
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_accent_falls_back_to_violet() {
        let parsed: Preferences =
            serde_json::from_str(r#"{"schema":1,"accent":"chartreuse"}"#).unwrap();
        // load() sanitizes; emulate that here without disk.
        let accent = if ACCENTS.contains(&parsed.accent.as_str()) {
            parsed.accent
        } else {
            accent_default()
        };
        assert_eq!(accent, "violet");
    }

    #[test]
    fn defaults_when_empty() {
        let p: Preferences = serde_json::from_slice(b"").unwrap_or_default();
        assert_eq!(p.accent, "violet");
        assert_eq!(p.schema, 1);
    }
}

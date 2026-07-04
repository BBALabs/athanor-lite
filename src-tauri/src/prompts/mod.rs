//! System prompt library — a curated, categorized set embedded in the binary,
//! plus the user's own saved prompts persisted in the data root. Applying one
//! sets the active workspace's standing system prompt.

use std::path::PathBuf;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::error::{AthanorError, Result};
use crate::workspaces::{self, write_atomic};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub id: String,
    pub title: String,
    pub category: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CuratedSet {
    pub version: String,
    pub prompts: Vec<Prompt>,
}

static CURATED_JSON: &str = include_str!("curated.json");

pub fn curated() -> Result<&'static CuratedSet> {
    static SET: OnceLock<Option<CuratedSet>> = OnceLock::new();
    SET.get_or_init(|| match serde_json::from_str::<CuratedSet>(CURATED_JSON) {
        Ok(s) => {
            log::info!(target: "prompts", "loaded {} curated prompts", s.prompts.len());
            Some(s)
        }
        Err(e) => {
            log::error!(target: "prompts", "curated prompts failed to parse: {e}");
            None
        }
    })
    .as_ref()
    .ok_or_else(|| AthanorError::Catalog("curated prompts failed to parse".into()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPrompt {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub category: String,
    pub body: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserPromptFile {
    #[serde(default)]
    prompts: Vec<UserPrompt>,
}

fn user_path(app: &tauri::AppHandle) -> Result<PathBuf> {
    Ok(workspaces::data_root(app)?.join("prompts.json"))
}

pub fn list_user(app: &tauri::AppHandle) -> Result<Vec<UserPrompt>> {
    let file: UserPromptFile = match std::fs::read(user_path(app)?) {
        Ok(b) => serde_json::from_slice(&b).unwrap_or_default(),
        Err(_) => UserPromptFile::default(),
    };
    Ok(file.prompts)
}

fn write_user(app: &tauri::AppHandle, prompts: &[UserPrompt]) -> Result<()> {
    let file = UserPromptFile { prompts: prompts.to_vec() };
    write_atomic(&user_path(app)?, &serde_json::to_vec_pretty(&file)?)?;
    Ok(())
}

/// Create a new saved prompt, or update the one whose id matches. Returns the
/// full list, newest first.
pub fn save_user(
    app: &tauri::AppHandle,
    id: Option<String>,
    title: &str,
    category: &str,
    body: &str,
) -> Result<Vec<UserPrompt>> {
    let title = title.trim();
    let body = body.trim();
    if title.is_empty() || body.is_empty() {
        return Err(AthanorError::Workspace("a prompt needs a title and a body".into()));
    }
    let mut prompts = list_user(app)?;
    match id {
        Some(id) => {
            if let Some(p) = prompts.iter_mut().find(|p| p.id == id) {
                p.title = title.chars().take(80).collect();
                p.category = category.trim().chars().take(32).collect();
                p.body = body.chars().take(4000).collect();
            }
        }
        None => prompts.insert(
            0,
            UserPrompt {
                id: uuid::Uuid::new_v4().to_string(),
                title: title.chars().take(80).collect(),
                category: category.trim().chars().take(32).collect(),
                body: body.chars().take(4000).collect(),
                created_at: chrono::Utc::now().to_rfc3339(),
            },
        ),
    }
    write_user(app, &prompts)?;
    Ok(prompts)
}

pub fn delete_user(app: &tauri::AppHandle, id: &str) -> Result<Vec<UserPrompt>> {
    let mut prompts = list_user(app)?;
    prompts.retain(|p| p.id != id);
    write_user(app, &prompts)?;
    Ok(prompts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_parses_and_is_categorized() {
        let set = curated().expect("curated prompts must parse");
        assert!(set.prompts.len() >= 10);
        for p in &set.prompts {
            assert!(!p.id.is_empty() && !p.title.is_empty() && !p.body.is_empty());
            assert!(!p.category.is_empty(), "{} has no category", p.id);
        }
    }

    #[test]
    fn curated_ids_are_unique() {
        let set = curated().unwrap();
        let mut ids: Vec<&str> = set.prompts.iter().map(|p| p.id.as_str()).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate curated prompt id");
    }
}

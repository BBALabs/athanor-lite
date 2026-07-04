//! Workspace templates — opinionated starting points so a new workspace is a
//! working stack in one click, not a blank form. Curated and embedded like the
//! model catalog; each names a model *role* (never a specific id) so it can't
//! break when the catalog evolves.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::Role;
use crate::error::{AthanorError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateEntry {
    pub id: String,
    pub name: String,
    /// One line shown in the picker.
    pub description: String,
    /// Monogram letter for the workspace.
    pub glyph: String,
    pub accent_hue: u16,
    /// The standing instruction this workspace's assistant runs under.
    pub purpose: String,
    /// Which kind of model this stack wants — resolved to a concrete model by
    /// the recommender against the user's hardware and library.
    pub model_role: Role,
    /// Whether this stack is built around retrieving from your own documents.
    pub rag_enabled: bool,
    /// Plain-language tool suggestions (never auto-installed — the user decides).
    #[serde(default)]
    pub suggested_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSet {
    pub version: String,
    pub templates: Vec<TemplateEntry>,
}

static TEMPLATES_JSON: &str = include_str!("templates.json");

pub fn templates() -> Result<&'static TemplateSet> {
    static SET: OnceLock<Option<TemplateSet>> = OnceLock::new();
    SET.get_or_init(|| match serde_json::from_str::<TemplateSet>(TEMPLATES_JSON) {
        Ok(t) => {
            log::info!(target: "catalog", "loaded {} workspace templates (v{})", t.templates.len(), t.version);
            Some(t)
        }
        Err(e) => {
            log::error!(target: "catalog", "embedded templates failed to parse: {e}");
            None
        }
    })
    .as_ref()
    .ok_or_else(|| AthanorError::Catalog("embedded templates failed to parse".into()))
}

#[cfg(test)]
mod tests {
    use super::super::catalog;
    use super::*;

    #[test]
    fn templates_parse_and_are_sane() {
        let set = templates().expect("templates must parse");
        assert!(set.templates.len() >= 5, "want at least five starting points");
        for t in &set.templates {
            assert!(!t.id.is_empty());
            assert!(!t.name.is_empty());
            assert!(!t.purpose.is_empty() && t.purpose.len() <= 200, "{}: purpose length", t.id);
            assert_eq!(t.glyph.chars().count(), 1, "{}: glyph is one letter", t.id);
            assert!(t.accent_hue < 360, "{}: hue in range", t.id);
        }
    }

    #[test]
    fn template_ids_are_unique() {
        let set = templates().unwrap();
        let mut ids: Vec<&str> = set.templates.iter().map(|t| t.id.as_str()).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate template id");
    }

    #[test]
    fn every_template_role_is_serviceable_by_the_catalog() {
        // A template promising a "coding" stack is a lie if no catalog model has
        // that role. This binds templates to the catalog without hard-coding ids.
        let cat = catalog().expect("catalog");
        let set = templates().unwrap();
        for t in &set.templates {
            let served = cat.entries.iter().any(|e| e.roles.contains(&t.model_role));
            assert!(served, "{}: no catalog model serves role {:?}", t.id, t.model_role);
        }
    }
}

//! Feature gates — the SINGLE place tier boundaries are defined.
//!
//! Every gated capability calls [`is_available`] / [`require`] / [`limit`].
//! Moving a feature between tiers is a one-line edit to [`Feature::min_tier`] or
//! [`Limit::cap_for`] — never a scattered `if tier == ...` across the codebase.
//!
//! Product stance (Tony's brief): the free tier must feel COMPLETE, not
//! crippled. Limits exist only where "more" is the natural upgrade reason
//! (more workspaces, more docs, more tools); hitting one yields a tasteful,
//! designed [`AthanorError::FeatureLimited`] the UI renders as an upgrade card —
//! never a nag, never a wall mid-task.

use serde::Serialize;

use crate::error::{AthanorError, Result};
use crate::licensing::{current_tier, Tier};

/// A capability that unlocks at a given tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Feature {
    UnlimitedWorkspaces,
    UnlimitedRagDocs,
    UnlimitedMcpServers,
    FineTuning,
    FleetData,
    CuratedTemplates,
    Vision,
    AdvancedRuntimeConfigurator,
    CustomCatalogEntries,
    TemplateMarketplacePriority,
    // Teams — hooks defined now, enforced when the Teams tier ships.
    CentralizedDeployment,
    SharedTemplateLibrary,
    AggregatePerfDashboard,
    ModelLibraryManagement,
    AdminControls,
}

pub const ALL_FEATURES: [Feature; 15] = [
    Feature::UnlimitedWorkspaces,
    Feature::UnlimitedRagDocs,
    Feature::UnlimitedMcpServers,
    Feature::FineTuning,
    Feature::FleetData,
    Feature::CuratedTemplates,
    Feature::Vision,
    Feature::AdvancedRuntimeConfigurator,
    Feature::CustomCatalogEntries,
    Feature::TemplateMarketplacePriority,
    Feature::CentralizedDeployment,
    Feature::SharedTemplateLibrary,
    Feature::AggregatePerfDashboard,
    Feature::ModelLibraryManagement,
    Feature::AdminControls,
];

impl Feature {
    /// The minimum tier that unlocks this feature. THE one place to change tiering.
    pub const fn min_tier(self) -> Tier {
        use Feature::*;
        match self {
            UnlimitedWorkspaces
            | UnlimitedRagDocs
            | UnlimitedMcpServers
            | FineTuning
            | FleetData
            | CuratedTemplates
            | Vision
            | AdvancedRuntimeConfigurator
            | CustomCatalogEntries
            | TemplateMarketplacePriority => Tier::Pro,
            CentralizedDeployment
            | SharedTemplateLibrary
            | AggregatePerfDashboard
            | ModelLibraryManagement
            | AdminControls => Tier::Teams,
        }
    }

    /// Stable identifier used in serialized errors and the frontend gate map.
    pub const fn key(self) -> &'static str {
        use Feature::*;
        match self {
            UnlimitedWorkspaces => "unlimitedWorkspaces",
            UnlimitedRagDocs => "unlimitedRagDocs",
            UnlimitedMcpServers => "unlimitedMcpServers",
            FineTuning => "fineTuning",
            FleetData => "fleetData",
            CuratedTemplates => "curatedTemplates",
            Vision => "vision",
            AdvancedRuntimeConfigurator => "advancedRuntimeConfigurator",
            CustomCatalogEntries => "customCatalogEntries",
            TemplateMarketplacePriority => "templateMarketplacePriority",
            CentralizedDeployment => "centralizedDeployment",
            SharedTemplateLibrary => "sharedTemplateLibrary",
            AggregatePerfDashboard => "aggregatePerfDashboard",
            ModelLibraryManagement => "modelLibraryManagement",
            AdminControls => "adminControls",
        }
    }

    /// Short human title used in upgrade copy.
    pub const fn title(self) -> &'static str {
        use Feature::*;
        match self {
            UnlimitedWorkspaces => "Unlimited workspaces",
            UnlimitedRagDocs => "Unlimited knowledge documents",
            UnlimitedMcpServers => "Unlimited tool connections",
            FineTuning => "Fine-tuning",
            FleetData => "Fleet performance data",
            CuratedTemplates => "The curated template library",
            Vision => "Vision & multimodal models",
            AdvancedRuntimeConfigurator => "The advanced runtime configurator",
            CustomCatalogEntries => "Custom model catalog entries",
            TemplateMarketplacePriority => "Template marketplace priority",
            CentralizedDeployment => "Centralized deployment",
            SharedTemplateLibrary => "Shared team templates",
            AggregatePerfDashboard => "The aggregate performance dashboard",
            ModelLibraryManagement => "Fleet model-library management",
            AdminControls => "Admin controls",
        }
    }

    /// One-line value statement shown on the upgrade card.
    pub const fn upgrade_hint(self) -> &'static str {
        use Feature::*;
        match self {
            UnlimitedWorkspaces => "Pro removes the 3-workspace limit — spin up a purpose-built AI stack for every job.",
            UnlimitedRagDocs => "Pro lifts the per-workspace document cap so you can ground answers in your whole corpus.",
            UnlimitedMcpServers => "Pro removes the 2-tool-server limit per workspace.",
            FineTuning => "Pro turns the dataset studio into real training runs — teach a model your voice, locally.",
            FleetData => "Pro contributes your measurements and returns crowd-measured recommendations for machines like yours.",
            CuratedTemplates => "Pro unlocks the curated, always-current library of purpose-built workspace templates.",
            Vision => "Pro adds vision models — paste or drop an image straight into chat.",
            AdvancedRuntimeConfigurator => "Pro unlocks the offload slider, KV-cache quantization, and MoE expert offload with a live VRAM projection.",
            CustomCatalogEntries => "Pro lets you paste any Hugging Face repo and auto-assess its fit on your hardware.",
            TemplateMarketplacePriority => "Pro gets priority placement when the template marketplace launches.",
            CentralizedDeployment => "Teams pushes workspace configs to every machine from one place.",
            SharedTemplateLibrary => "Teams shares a template library across everyone.",
            AggregatePerfDashboard => "Teams rolls every machine's measurements into one dashboard.",
            ModelLibraryManagement => "Teams manages the model library across the whole fleet.",
            AdminControls => "Teams adds administrative controls for the whole organization.",
        }
    }
}

/// A countable resource that is capped on Free and unlimited on paid tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Limit {
    Workspaces,
    RagDocsPerWorkspace,
    McpServersPerWorkspace,
}

pub const ALL_LIMITS: [Limit; 3] = [
    Limit::Workspaces,
    Limit::RagDocsPerWorkspace,
    Limit::McpServersPerWorkspace,
];

impl Limit {
    /// Free-tier cap; `None` on any paid tier means unlimited.
    pub const fn cap_for(self, tier: Tier) -> Option<u32> {
        match tier {
            Tier::Free => Some(match self {
                Limit::Workspaces => 3,
                Limit::RagDocsPerWorkspace => 25,
                Limit::McpServersPerWorkspace => 2,
            }),
            _ => None,
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Limit::Workspaces => "workspaces",
            Limit::RagDocsPerWorkspace => "ragDocsPerWorkspace",
            Limit::McpServersPerWorkspace => "mcpServersPerWorkspace",
        }
    }

    /// The upgrade feature a user hits when they run into this cap.
    pub const fn gated_feature(self) -> Feature {
        match self {
            Limit::Workspaces => Feature::UnlimitedWorkspaces,
            Limit::RagDocsPerWorkspace => Feature::UnlimitedRagDocs,
            Limit::McpServersPerWorkspace => Feature::UnlimitedMcpServers,
        }
    }
}

// ── Gate helpers ──────────────────────────────────────────────

/// Is `feature` available at the current live tier?
pub fn is_available(feature: Feature) -> bool {
    current_tier() >= feature.min_tier()
}

/// Tier-parameterized variant (pure; used in tests and matrix building).
pub fn is_available_for(feature: Feature, tier: Tier) -> bool {
    tier >= feature.min_tier()
}

/// Current cap for a limit (`None` = unlimited).
pub fn limit(l: Limit) -> Option<u32> {
    l.cap_for(current_tier())
}

/// Would adding one more (`current_count` → `current_count + 1`) stay within the
/// current tier's cap?
pub fn within_limit(l: Limit, current_count: u32) -> bool {
    match limit(l) {
        Some(cap) => current_count < cap,
        None => true,
    }
}

/// Build the designed `FeatureLimited` error for a gated feature.
pub fn limited(feature: Feature) -> AthanorError {
    AthanorError::FeatureLimited {
        feature: feature.key().to_string(),
        tier: feature.min_tier().label().to_string(),
        message: format!("{} is a {} feature.", feature.title(), feature.min_tier().label()),
        upgrade_hint: feature.upgrade_hint().to_string(),
    }
}

/// Gate a call site in one line: `features::require(Feature::Vision)?;`.
pub fn require(feature: Feature) -> Result<()> {
    if is_available(feature) {
        Ok(())
    } else {
        Err(limited(feature))
    }
}

/// Enforce a countable limit given the current count. On overflow, returns the
/// `FeatureLimited` error for the corresponding upgrade feature, with a message
/// naming the specific cap that was reached.
pub fn enforce_limit(l: Limit, current_count: u32) -> Result<()> {
    if within_limit(l, current_count) {
        return Ok(());
    }
    let feature = l.gated_feature();
    let cap = limit(l).unwrap_or(0);
    let noun = match l {
        Limit::Workspaces => "workspace slots",
        Limit::RagDocsPerWorkspace => "documents in this workspace",
        Limit::McpServersPerWorkspace => "tool connections in this workspace",
    };
    Err(AthanorError::FeatureLimited {
        feature: feature.key().to_string(),
        tier: feature.min_tier().label().to_string(),
        message: format!("You've used all {cap} {noun}."),
        upgrade_hint: feature.upgrade_hint().to_string(),
    })
}

// ── Frontend gate map ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureFlag {
    pub key: &'static str,
    pub title: &'static str,
    pub available: bool,
    pub required_tier: Tier,
    pub upgrade_hint: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitInfo {
    pub key: &'static str,
    /// `None` = unlimited at the current tier.
    pub cap: Option<u32>,
}

/// A complete, serializable snapshot of every gate at the current tier — the
/// one source the frontend reads to render Pro badges and upgrade cards.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureMatrix {
    pub tier: Tier,
    pub features: Vec<FeatureFlag>,
    pub limits: Vec<LimitInfo>,
}

pub fn snapshot() -> FeatureMatrix {
    let tier = current_tier();
    FeatureMatrix {
        tier,
        features: ALL_FEATURES
            .iter()
            .map(|&f| FeatureFlag {
                key: f.key(),
                title: f.title(),
                available: is_available_for(f, tier),
                required_tier: f.min_tier(),
                upgrade_hint: f.upgrade_hint(),
            })
            .collect(),
        limits: ALL_LIMITS
            .iter()
            .map(|&l| LimitInfo { key: l.key(), cap: l.cap_for(tier) })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_gates_pro_features() {
        assert!(!is_available_for(Feature::Vision, Tier::Free));
        assert!(is_available_for(Feature::Vision, Tier::Pro));
        assert!(is_available_for(Feature::Vision, Tier::Teams));
    }

    #[test]
    fn teams_features_need_teams() {
        assert!(!is_available_for(Feature::AdminControls, Tier::Pro));
        assert!(is_available_for(Feature::AdminControls, Tier::Teams));
        assert!(is_available_for(Feature::AdminControls, Tier::Enterprise));
    }

    #[test]
    fn free_limits_finite_paid_unlimited() {
        assert_eq!(Limit::Workspaces.cap_for(Tier::Free), Some(3));
        assert_eq!(Limit::RagDocsPerWorkspace.cap_for(Tier::Free), Some(25));
        assert_eq!(Limit::McpServersPerWorkspace.cap_for(Tier::Free), Some(2));
        for l in ALL_LIMITS {
            assert_eq!(l.cap_for(Tier::Pro), None);
            assert_eq!(l.cap_for(Tier::Teams), None);
        }
    }

    #[test]
    fn within_limit_boundary() {
        // Free workspaces cap = 3: at 2 you may add (→3); at 3 you may not.
        let free = Tier::Free;
        let cap = Limit::Workspaces.cap_for(free).unwrap();
        assert!(2 < cap);
        assert!(!(3 < cap));
        assert_eq!(cap, 3);
    }

    #[test]
    fn every_feature_has_metadata() {
        for f in ALL_FEATURES {
            assert!(!f.key().is_empty());
            assert!(!f.title().is_empty());
            assert!(!f.upgrade_hint().is_empty());
        }
    }
}

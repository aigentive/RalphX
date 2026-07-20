use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdeationPlanMode {
    /// Plan must exist before proposals can be created
    Required,
    /// Plan is optional, orchestrator suggests for complex features
    Optional,
    /// Plan and proposals created together, changes suggest sync
    Parallel,
}

impl Default for IdeationPlanMode {
    fn default() -> Self {
        Self::Optional
    }
}

/// Per-origin overrides for gating policy.
///
/// When `SessionOrigin::External` is the session origin, these values override
/// the corresponding base fields in `IdeationSettings`. `None` means inherit
/// from the base field; `Some(v)` overrides with `v`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ExternalIdeationOverrides {
    /// Override for `auto_verify_plans` for external sessions.
    pub auto_verify_plans: Option<bool>,
    /// Override for `require_verification_for_accept` for external sessions.
    pub require_verification_for_accept: Option<bool>,
    /// Override for `require_verification_for_proposals` for external sessions.
    pub require_verification_for_proposals: Option<bool>,
    /// Override for `require_accept_for_finalize` for external sessions.
    pub require_accept_for_finalize: Option<bool>,
}

/// Ideation-specific settings (separate from QA settings)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdeationSettings {
    /// Master switch for the product Tasks/Kanban/Autopilot feature.
    #[serde(default)]
    pub tasks_enabled: bool,
    /// How implementation plans are created in ideation flow
    pub plan_mode: IdeationPlanMode,
    /// In Required mode, whether explicit approval is needed before proposals
    pub require_plan_approval: bool,
    /// Whether to show plan suggestions for complex features (in Optional mode)
    pub suggest_plans_for_complex: bool,
    /// Auto-link proposals to session plan when created
    pub auto_link_proposals: bool,
    /// Queue model-native verification when required acceptance lacks exact proof.
    #[serde(default)]
    pub auto_verify_plans: bool,
    /// If true, the exact current plan must be verified before accepting proposals.
    #[serde(default)]
    pub require_verification_for_accept: bool,
    /// If true, plans must be verified (or skipped) before proposals can be created
    #[serde(default)]
    pub require_verification_for_proposals: bool,
    /// If true, finalize_proposals pauses for human acceptance before applying proposals
    #[serde(default)]
    pub require_accept_for_finalize: bool,
    /// Per-origin gate overrides for external sessions. NULL columns → None → inherits base.
    #[serde(default)]
    pub external_overrides: ExternalIdeationOverrides,
}

impl Default for IdeationSettings {
    fn default() -> Self {
        Self {
            tasks_enabled: false,
            plan_mode: IdeationPlanMode::Optional,
            require_plan_approval: false, // Plan existence is sufficient by default
            suggest_plans_for_complex: true,
            auto_link_proposals: true,
            auto_verify_plans: false,
            require_verification_for_accept: false, // Opt-in feature
            require_verification_for_proposals: false, // Opt-in feature
            require_accept_for_finalize: false,     // Opt-in feature
            external_overrides: ExternalIdeationOverrides::default(),
        }
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

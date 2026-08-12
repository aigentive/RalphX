use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::entities::{CoordinationMode, PersonaId};

use super::{AgentHarnessKind, AgentLane, AtlassianMcpAccess, LogicalEffort};

pub const ROUTING_ROLE_COUNT: usize = 29;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingRoleFamily {
    Workspace,
    Automation,
    FeedbackLoops,
    Ideation,
    Delegation,
    Execution,
    Utility,
}

impl RoutingRoleFamily {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Automation => "automation",
            Self::FeedbackLoops => "feedback_loops",
            Self::Ideation => "ideation",
            Self::Delegation => "delegation",
            Self::Execution => "execution",
            Self::Utility => "utility",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Workspace => "Workspace",
            Self::Automation => "Automation",
            Self::FeedbackLoops => "Feedback Loops",
            Self::Ideation => "Ideation",
            Self::Delegation => "Delegation",
            Self::Execution => "Execution",
            Self::Utility => "Utility",
        }
    }
}

pub const ROUTING_ROLE_FAMILIES: [RoutingRoleFamily; 7] = [
    RoutingRoleFamily::Workspace,
    RoutingRoleFamily::Automation,
    RoutingRoleFamily::FeedbackLoops,
    RoutingRoleFamily::Ideation,
    RoutingRoleFamily::Delegation,
    RoutingRoleFamily::Execution,
    RoutingRoleFamily::Utility,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingRole {
    WorkspaceChat,
    WorkspaceEdit,
    WorkspacePlan,
    WorkspaceIdeation,
    WorkspaceReviewPr,
    WorkspaceAutomation,
    AutomationPlanJudge,
    AutomationResultJudge,
    WorkspaceReviewer,
    WorkspaceRepair,
    WorkspaceMergeRepair,
    WorkspacePrFixer,
    IdeationPrimary,
    IdeationVerifier,
    IdeationSubagent,
    IdeationVerifierSubagent,
    DelegatedSubagent,
    ExecutionWorker,
    ExecutionQaPrep,
    ExecutionQaRefiner,
    ExecutionQaTester,
    ExecutionReviewer,
    ExecutionReexecutor,
    ExecutionMerger,
    UtilityLightweight,
    UtilityPrDescriber,
    UtilityProjectAnalyzer,
    MemoryCapture,
    MemoryMaintainer,
}

pub const ROUTING_ROLES: [RoutingRole; ROUTING_ROLE_COUNT] = [
    RoutingRole::WorkspaceChat,
    RoutingRole::WorkspaceEdit,
    RoutingRole::WorkspacePlan,
    RoutingRole::WorkspaceIdeation,
    RoutingRole::WorkspaceReviewPr,
    RoutingRole::WorkspaceAutomation,
    RoutingRole::AutomationPlanJudge,
    RoutingRole::AutomationResultJudge,
    RoutingRole::WorkspaceReviewer,
    RoutingRole::WorkspaceRepair,
    RoutingRole::WorkspaceMergeRepair,
    RoutingRole::WorkspacePrFixer,
    RoutingRole::IdeationPrimary,
    RoutingRole::IdeationVerifier,
    RoutingRole::IdeationSubagent,
    RoutingRole::IdeationVerifierSubagent,
    RoutingRole::DelegatedSubagent,
    RoutingRole::ExecutionWorker,
    RoutingRole::ExecutionQaPrep,
    RoutingRole::ExecutionQaRefiner,
    RoutingRole::ExecutionQaTester,
    RoutingRole::ExecutionReviewer,
    RoutingRole::ExecutionReexecutor,
    RoutingRole::ExecutionMerger,
    RoutingRole::UtilityLightweight,
    RoutingRole::UtilityPrDescriber,
    RoutingRole::UtilityProjectAnalyzer,
    RoutingRole::MemoryCapture,
    RoutingRole::MemoryMaintainer,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutingRoleMetadata {
    pub key: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub family: RoutingRoleFamily,
    pub requires_tasks: bool,
}

impl RoutingRole {
    pub const fn metadata(self) -> RoutingRoleMetadata {
        use RoutingRoleFamily as Family;

        match self {
            Self::WorkspaceChat => metadata(self, "workspace_chat", "Chat", Family::Workspace),
            Self::WorkspaceEdit => metadata(self, "workspace_edit", "Edit", Family::Workspace),
            Self::WorkspacePlan => metadata(self, "workspace_plan", "Plan", Family::Workspace),
            Self::WorkspaceIdeation => {
                metadata(self, "workspace_ideation", "Ideation", Family::Workspace)
            }
            Self::WorkspaceReviewPr => {
                metadata(self, "workspace_review_pr", "Review PR", Family::Workspace)
            }
            Self::WorkspaceAutomation => metadata(
                self,
                "workspace_automation",
                "Automation",
                Family::Workspace,
            ),
            Self::AutomationPlanJudge => metadata(
                self,
                "automation_plan_judge",
                "Plan Judge",
                Family::Automation,
            ),
            Self::AutomationResultJudge => metadata(
                self,
                "automation_result_judge",
                "Result Judge",
                Family::Automation,
            ),
            Self::WorkspaceReviewer => metadata(
                self,
                "workspace_reviewer",
                "Reviewer",
                Family::FeedbackLoops,
            ),
            Self::WorkspaceRepair => {
                metadata(self, "workspace_repair", "Repair", Family::FeedbackLoops)
            }
            Self::WorkspaceMergeRepair => metadata(
                self,
                "workspace_merge_repair",
                "Merge Repair",
                Family::FeedbackLoops,
            ),
            Self::WorkspacePrFixer => metadata(
                self,
                "workspace_pr_fixer",
                "PR Fixer",
                Family::FeedbackLoops,
            ),
            Self::IdeationPrimary => {
                metadata(self, "ideation_primary", "Primary", Family::Ideation)
            }
            Self::IdeationVerifier => {
                metadata(self, "ideation_verifier", "Verifier", Family::Ideation)
            }
            Self::IdeationSubagent => {
                metadata(self, "ideation_subagent", "Subagent", Family::Ideation)
            }
            Self::IdeationVerifierSubagent => metadata(
                self,
                "ideation_verifier_subagent",
                "Verifier Subagent",
                Family::Ideation,
            ),
            Self::DelegatedSubagent => metadata(
                self,
                "delegated_subagent",
                "Delegated Subagent",
                Family::Delegation,
            ),
            Self::ExecutionWorker => {
                metadata(self, "execution_worker", "Worker", Family::Execution)
            }
            Self::ExecutionQaPrep => {
                metadata(self, "execution_qa_prep", "QA Prep", Family::Execution)
            }
            Self::ExecutionQaRefiner => metadata(
                self,
                "execution_qa_refiner",
                "QA Refiner",
                Family::Execution,
            ),
            Self::ExecutionQaTester => {
                metadata(self, "execution_qa_tester", "QA Tester", Family::Execution)
            }
            Self::ExecutionReviewer => {
                metadata(self, "execution_reviewer", "Reviewer", Family::Execution)
            }
            Self::ExecutionReexecutor => metadata(
                self,
                "execution_reexecutor",
                "Re-executor",
                Family::Execution,
            ),
            Self::ExecutionMerger => {
                metadata(self, "execution_merger", "Merger", Family::Execution)
            }
            Self::UtilityLightweight => {
                metadata(self, "utility_lightweight", "Lightweight", Family::Utility)
            }
            Self::UtilityPrDescriber => metadata(
                self,
                "utility_pr_describer",
                "PR Describer",
                Family::Utility,
            ),
            Self::UtilityProjectAnalyzer => metadata(
                self,
                "utility_project_analyzer",
                "Project Analyzer",
                Family::Utility,
            ),
            Self::MemoryCapture => {
                metadata(self, "memory_capture", "Memory Capture", Family::Utility)
            }
            Self::MemoryMaintainer => metadata(
                self,
                "memory_maintainer",
                "Memory Maintainer",
                Family::Utility,
            ),
        }
    }

    pub const fn from_legacy_lane(lane: AgentLane) -> Self {
        match lane {
            AgentLane::IdeationPrimary => Self::IdeationPrimary,
            AgentLane::IdeationVerifier => Self::IdeationVerifier,
            AgentLane::IdeationSubagent => Self::IdeationSubagent,
            AgentLane::IdeationVerifierSubagent => Self::IdeationVerifierSubagent,
            AgentLane::ExecutionWorker => Self::ExecutionWorker,
            AgentLane::ExecutionReviewer => Self::ExecutionReviewer,
            AgentLane::ExecutionReexecutor => Self::ExecutionReexecutor,
            AgentLane::ExecutionMerger => Self::ExecutionMerger,
            AgentLane::ExecutionBranchUpdater => Self::WorkspaceRepair,
        }
    }

    pub const fn legacy_lane(self) -> Option<AgentLane> {
        match self {
            Self::IdeationPrimary => Some(AgentLane::IdeationPrimary),
            Self::IdeationVerifier => Some(AgentLane::IdeationVerifier),
            Self::IdeationSubagent => Some(AgentLane::IdeationSubagent),
            Self::IdeationVerifierSubagent => Some(AgentLane::IdeationVerifierSubagent),
            Self::ExecutionWorker => Some(AgentLane::ExecutionWorker),
            Self::ExecutionReviewer => Some(AgentLane::ExecutionReviewer),
            Self::ExecutionReexecutor => Some(AgentLane::ExecutionReexecutor),
            Self::ExecutionMerger => Some(AgentLane::ExecutionMerger),
            Self::WorkspaceRepair => Some(AgentLane::ExecutionBranchUpdater),
            _ => None,
        }
    }
}

const fn metadata(
    role: RoutingRole,
    key: &'static str,
    display_name: &'static str,
    family: RoutingRoleFamily,
) -> RoutingRoleMetadata {
    RoutingRoleMetadata {
        key,
        display_name,
        description: super::routing_role_descriptions::routing_role_description(role),
        family,
        requires_tasks: matches!(family, RoutingRoleFamily::Execution),
    }
}

impl fmt::Display for RoutingRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.metadata().key)
    }
}

impl FromStr for RoutingRole {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        ROUTING_ROLES
            .into_iter()
            .find(|role| role.metadata().key == value)
            .ok_or_else(|| format!("Invalid routing role '{value}'"))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManualServiceTier {
    #[default]
    ProviderDefault,
    Standard,
    Fast,
}

impl fmt::Display for ManualServiceTier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ProviderDefault => "provider_default",
            Self::Standard => "standard",
            Self::Fast => "fast",
        })
    }
}

impl FromStr for ManualServiceTier {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "provider_default" => Ok(Self::ProviderDefault),
            "standard" => Ok(Self::Standard),
            "fast" => Ok(Self::Fast),
            other => Err(format!("Invalid manual service tier '{other}'")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualRoleDefault {
    pub harness: AgentHarnessKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<LogicalEffort>,
    #[serde(default)]
    pub service_tier: ManualServiceTier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordination_mode: Option<CoordinationMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona_id: Option<PersonaId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_mode: Option<String>,
    /// Atlassian MCP tool tier override. `None` means "use the built-in
    /// `default_atlassian_access` tier for this role" — resolution is row-wins,
    /// so a matching row that omits this field does not inherit a lower layer's
    /// value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atlassian_access: Option<AtlassianMcpAccess>,
}

/// Complete caller-selectable runtime fields for one backend-derived routing role.
///
/// Approval and sandbox are intentionally absent: launch code always preserves
/// those fields from the effective role default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManualRoleRuntimeOverride {
    pub harness: AgentHarnessKind,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<LogicalEffort>,
    pub service_tier: ManualServiceTier,
    #[serde(default)]
    pub coordination_mode: Option<CoordinationMode>,
    #[serde(default)]
    pub persona_id: Option<PersonaId>,
}

impl ManualRoleDefault {
    pub fn from_legacy(settings: &super::AgentLaneSettings) -> Self {
        Self {
            harness: settings.harness,
            model: settings.model.clone(),
            effort: settings.effort,
            service_tier: ManualServiceTier::ProviderDefault,
            coordination_mode: None,
            persona_id: None,
            approval_policy: settings.approval_policy.clone(),
            sandbox_mode: settings.sandbox_mode.clone(),
            atlassian_access: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredManualRoleDefault {
    pub id: i64,
    pub project_id: Option<String>,
    pub role: RoutingRole,
    pub value: ManualRoleDefault,
    pub updated_at: DateTime<Utc>,
}

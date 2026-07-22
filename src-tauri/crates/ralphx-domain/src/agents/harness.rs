use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::types::ClientType;

/// Provider-neutral harness kind used by RalphX orchestration.
///
/// This is intentionally narrower than `ClientType`: only first-class harnesses
/// that RalphX actively routes user-facing work through should appear here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentHarnessKind {
    Claude,
    Codex,
}

pub const DEFAULT_AGENT_HARNESS: AgentHarnessKind = AgentHarnessKind::Claude;
pub const STANDARD_AGENT_HARNESSES: [AgentHarnessKind; 2] =
    [AgentHarnessKind::Claude, AgentHarnessKind::Codex];
pub const CLAUDE_DEFAULT_PERMISSION_MODE: &str = "bypassPermissions";
pub const CLAUDE_DEFAULT_DANGEROUSLY_SKIP_PERMISSIONS: bool = true;
pub const CLAUDE_DEFAULT_ALLOW_DANGEROUSLY_SKIP_PERMISSIONS: bool = false;
pub const CODEX_DEFAULT_APPROVAL_POLICY: &str = "never";
pub const CODEX_DEFAULT_SANDBOX_MODE: &str = "danger-full-access";

pub fn standard_harness_map<T>(claude: T, codex: T) -> HashMap<AgentHarnessKind, T> {
    HashMap::from([
        (AgentHarnessKind::Claude, claude),
        (AgentHarnessKind::Codex, codex),
    ])
}

pub fn standard_harness_registry<T, F>(mut builder: F) -> HashMap<AgentHarnessKind, T>
where
    F: FnMut(AgentHarnessKind) -> T,
{
    STANDARD_AGENT_HARNESSES
        .into_iter()
        .map(|harness| (harness, builder(harness)))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessModelLabelStrategy {
    ClaudeMapped,
    RawModelId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessEffortStrategy {
    ClaudeEffortFirst,
    LogicalOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessStreamMode {
    ClaudeEvents,
    CodexJsonl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessTeamCapabilities {
    pub rx_native_team: bool,
    pub interactive_delivery: bool,
    pub resume_delivery: bool,
    pub stream_projection: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessBehavior {
    pub team: HarnessTeamCapabilities,
    pub supports_merge_completion_watcher: bool,
    pub model_label_strategy: HarnessModelLabelStrategy,
    pub effort_strategy: HarnessEffortStrategy,
    pub stream_mode: HarnessStreamMode,
}

pub fn standard_harness_behavior(harness: AgentHarnessKind) -> HarnessBehavior {
    match harness {
        AgentHarnessKind::Claude => HarnessBehavior {
            team: HarnessTeamCapabilities {
                rx_native_team: true,
                interactive_delivery: true,
                resume_delivery: true,
                stream_projection: true,
            },
            supports_merge_completion_watcher: true,
            model_label_strategy: HarnessModelLabelStrategy::ClaudeMapped,
            effort_strategy: HarnessEffortStrategy::ClaudeEffortFirst,
            stream_mode: HarnessStreamMode::ClaudeEvents,
        },
        AgentHarnessKind::Codex => HarnessBehavior {
            team: HarnessTeamCapabilities {
                rx_native_team: true,
                interactive_delivery: false,
                resume_delivery: true,
                stream_projection: true,
            },
            supports_merge_completion_watcher: false,
            model_label_strategy: HarnessModelLabelStrategy::RawModelId,
            effort_strategy: HarnessEffortStrategy::LogicalOnly,
            stream_mode: HarnessStreamMode::CodexJsonl,
        },
    }
}

pub fn default_approval_policy_for_harness(harness: AgentHarnessKind) -> Option<&'static str> {
    match harness {
        AgentHarnessKind::Claude => None,
        AgentHarnessKind::Codex => Some(CODEX_DEFAULT_APPROVAL_POLICY),
    }
}

pub fn default_sandbox_mode_for_harness(harness: AgentHarnessKind) -> Option<&'static str> {
    match harness {
        AgentHarnessKind::Claude => None,
        AgentHarnessKind::Codex => Some(CODEX_DEFAULT_SANDBOX_MODE),
    }
}

impl fmt::Display for AgentHarnessKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Claude => write!(f, "claude"),
            Self::Codex => write!(f, "codex"),
        }
    }
}

impl FromStr for AgentHarnessKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            other => Err(format!(
                "Invalid agent harness '{}'. Valid values: claude, codex",
                other
            )),
        }
    }
}

impl From<AgentHarnessKind> for ClientType {
    fn from(value: AgentHarnessKind) -> Self {
        match value {
            AgentHarnessKind::Claude => ClientType::ClaudeCode,
            AgentHarnessKind::Codex => ClientType::Codex,
        }
    }
}

impl TryFrom<ClientType> for AgentHarnessKind {
    type Error = String;

    fn try_from(value: ClientType) -> Result<Self, Self::Error> {
        match value {
            ClientType::ClaudeCode => Ok(Self::Claude),
            ClientType::Codex => Ok(Self::Codex),
            other => Err(format!(
                "Client type '{}' does not map to a first-class agent harness",
                other
            )),
        }
    }
}

/// Provider-neutral reasoning effort surfaced in RalphX lane settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogicalEffort {
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
    Max,
    Ultra,
}

impl fmt::Display for LogicalEffort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
            Self::XHigh => write!(f, "xhigh"),
            Self::Max => write!(f, "max"),
            Self::Ultra => write!(f, "ultra"),
        }
    }
}

impl FromStr for LogicalEffort {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "xhigh" => Ok(Self::XHigh),
            "max" => Ok(Self::Max),
            "ultra" => Ok(Self::Ultra),
            other => Err(format!(
                "Invalid logical effort '{}'. Valid values: low, medium, high, xhigh, max, ultra",
                other
            )),
        }
    }
}

impl LogicalEffort {
    /// Convert to Claude CLI effort labels for legacy Claude-only callsites.
    pub fn to_legacy_claude_effort(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
            Self::Ultra => "ultra",
        }
    }
}

/// Provider-neutral lane key for harness/model/effort routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentLane {
    IdeationPrimary,
    IdeationVerifier,
    IdeationSubagent,
    IdeationVerifierSubagent,
    ExecutionWorker,
    ExecutionReviewer,
    ExecutionReexecutor,
    ExecutionMerger,
    ExecutionBranchUpdater,
}

impl fmt::Display for AgentLane {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdeationPrimary => write!(f, "ideation_primary"),
            Self::IdeationVerifier => write!(f, "ideation_verifier"),
            Self::IdeationSubagent => write!(f, "ideation_subagent"),
            Self::IdeationVerifierSubagent => write!(f, "ideation_verifier_subagent"),
            Self::ExecutionWorker => write!(f, "execution_worker"),
            Self::ExecutionReviewer => write!(f, "execution_reviewer"),
            Self::ExecutionReexecutor => write!(f, "execution_reexecutor"),
            Self::ExecutionMerger => write!(f, "execution_merger"),
            Self::ExecutionBranchUpdater => write!(f, "execution_branch_updater"),
        }
    }
}

impl FromStr for AgentLane {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ideation_primary" => Ok(Self::IdeationPrimary),
            "ideation_verifier" => Ok(Self::IdeationVerifier),
            "ideation_subagent" => Ok(Self::IdeationSubagent),
            "ideation_verifier_subagent" => Ok(Self::IdeationVerifierSubagent),
            "execution_worker" => Ok(Self::ExecutionWorker),
            "execution_reviewer" => Ok(Self::ExecutionReviewer),
            "execution_reexecutor" => Ok(Self::ExecutionReexecutor),
            "execution_merger" => Ok(Self::ExecutionMerger),
            "execution_branch_updater" => Ok(Self::ExecutionBranchUpdater),
            other => Err(format!("Invalid agent lane '{}'", other)),
        }
    }
}

/// Minimal provider-neutral session handle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSessionRef {
    pub harness: AgentHarnessKind,
    pub provider_session_id: String,
}

/// Stored lane settings shape used by the upcoming multi-harness config layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentLaneSettings {
    pub harness: AgentHarnessKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<LogicalEffort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_mode: Option<String>,
}

impl AgentLaneSettings {
    pub fn new(harness: AgentHarnessKind) -> Self {
        Self {
            harness,
            model: None,
            effort: None,
            approval_policy: None,
            sandbox_mode: None,
        }
    }
}

/// Provider-keyed default runtime settings for Workspace Review.
///
/// The Workspace Review provider is inherited from the owning chat/run, so this
/// stores only the configurable defaults applied after provider resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceReviewRuntimeSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<LogicalEffort>,
}

impl WorkspaceReviewRuntimeSettings {
    pub fn new(model: Option<String>, effort: Option<LogicalEffort>) -> Self {
        Self { model, effort }
    }

    pub fn utility_defaults(provider: AgentHarnessKind) -> Self {
        Self {
            model: Some(
                super::model_registry::lightweight_model_for_provider(provider).to_string(),
            ),
            effort: Some(LogicalEffort::Medium),
        }
    }

    pub fn resolve_effective(
        provider: AgentHarnessKind,
        global: Option<&WorkspaceReviewRuntimeSettings>,
        project: Option<&WorkspaceReviewRuntimeSettings>,
    ) -> Self {
        let defaults = Self::utility_defaults(provider);
        Self {
            model: project
                .and_then(|settings| settings.model.clone())
                .or_else(|| global.and_then(|settings| settings.model.clone()))
                .or(defaults.model),
            effort: project
                .and_then(|settings| settings.effort)
                .or_else(|| global.and_then(|settings| settings.effort))
                .or(defaults.effort),
        }
    }
}

pub fn generic_harness_lane_defaults(
    harness: AgentHarnessKind,
    lane: AgentLane,
) -> AgentLaneSettings {
    match harness {
        AgentHarnessKind::Claude => AgentLaneSettings::new(AgentHarnessKind::Claude),
        AgentHarnessKind::Codex => {
            let mut settings = AgentLaneSettings::new(AgentHarnessKind::Codex);

            match lane {
                AgentLane::IdeationPrimary => {
                    settings.model = Some(
                        super::model_registry::default_model_for_provider(harness).to_string(),
                    );
                    settings.effort = Some(LogicalEffort::XHigh);
                    settings.approval_policy = Some(CODEX_DEFAULT_APPROVAL_POLICY.to_string());
                    settings.sandbox_mode = Some(CODEX_DEFAULT_SANDBOX_MODE.to_string());
                }
                AgentLane::IdeationVerifier => {
                    settings.model = Some(
                        super::model_registry::lightweight_model_for_provider(harness).to_string(),
                    );
                    settings.effort = Some(LogicalEffort::Medium);
                    settings.approval_policy = Some(CODEX_DEFAULT_APPROVAL_POLICY.to_string());
                    settings.sandbox_mode = Some(CODEX_DEFAULT_SANDBOX_MODE.to_string());
                }
                AgentLane::IdeationSubagent | AgentLane::IdeationVerifierSubagent => {
                    settings.model = Some(
                        super::model_registry::lightweight_model_for_provider(harness).to_string(),
                    );
                    settings.effort = Some(LogicalEffort::Medium);
                    settings.approval_policy = Some(CODEX_DEFAULT_APPROVAL_POLICY.to_string());
                    settings.sandbox_mode = Some(CODEX_DEFAULT_SANDBOX_MODE.to_string());
                }
                AgentLane::ExecutionWorker
                | AgentLane::ExecutionReviewer
                | AgentLane::ExecutionReexecutor
                | AgentLane::ExecutionMerger
                | AgentLane::ExecutionBranchUpdater => {
                    settings.model = Some(
                        super::model_registry::default_model_for_provider(harness).to_string(),
                    );
                    settings.effort = Some(LogicalEffort::XHigh);
                    settings.approval_policy = Some(CODEX_DEFAULT_APPROVAL_POLICY.to_string());
                    settings.sandbox_mode = Some(CODEX_DEFAULT_SANDBOX_MODE.to_string());
                }
            }

            settings
        }
    }
}

/// Provider-keyed defaults for semantic roles that do not have a legacy lane.
pub fn generic_harness_role_defaults(
    harness: AgentHarnessKind,
    role: super::routing_role::RoutingRole,
) -> AgentLaneSettings {
    if let Some(lane) = role.legacy_lane() {
        return generic_harness_lane_defaults(harness, lane);
    }

    if matches!(
        role,
        super::routing_role::RoutingRole::ExecutionQaPrep
            | super::routing_role::RoutingRole::ExecutionQaRefiner
            | super::routing_role::RoutingRole::ExecutionQaTester
    ) {
        return generic_harness_lane_defaults(harness, AgentLane::ExecutionWorker);
    }

    let mut settings = AgentLaneSettings::new(harness);
    if role.metadata().family == super::routing_role::RoutingRoleFamily::Utility {
        settings.model =
            Some(super::model_registry::lightweight_model_for_provider(harness).to_string());
        settings.effort = Some(LogicalEffort::Medium);
    } else {
        settings.model =
            Some(super::model_registry::default_model_for_provider(harness).to_string());
        settings.effort = Some(super::model_registry::default_effort_for_provider(harness));
    }
    settings.approval_policy = default_approval_policy_for_harness(harness).map(str::to_string);
    settings.sandbox_mode = default_sandbox_mode_for_harness(harness).map(str::to_string);
    settings
}

pub fn standard_agent_lane_defaults() -> HashMap<AgentLane, AgentLaneSettings> {
    HashMap::from([
        (
            AgentLane::IdeationPrimary,
            generic_harness_lane_defaults(AgentHarnessKind::Codex, AgentLane::IdeationPrimary),
        ),
        (
            AgentLane::IdeationVerifier,
            generic_harness_lane_defaults(AgentHarnessKind::Codex, AgentLane::IdeationVerifier),
        ),
        (
            AgentLane::IdeationSubagent,
            generic_harness_lane_defaults(AgentHarnessKind::Codex, AgentLane::IdeationSubagent),
        ),
        (
            AgentLane::IdeationVerifierSubagent,
            generic_harness_lane_defaults(
                AgentHarnessKind::Codex,
                AgentLane::IdeationVerifierSubagent,
            ),
        ),
        (
            AgentLane::ExecutionWorker,
            AgentLaneSettings::new(DEFAULT_AGENT_HARNESS),
        ),
        (
            AgentLane::ExecutionReviewer,
            AgentLaneSettings::new(DEFAULT_AGENT_HARNESS),
        ),
        (
            AgentLane::ExecutionReexecutor,
            AgentLaneSettings::new(DEFAULT_AGENT_HARNESS),
        ),
        (
            AgentLane::ExecutionMerger,
            AgentLaneSettings::new(DEFAULT_AGENT_HARNESS),
        ),
        (
            AgentLane::ExecutionBranchUpdater,
            AgentLaneSettings::new(DEFAULT_AGENT_HARNESS),
        ),
    ])
}

/// Persisted lane settings row scoped either globally or to a specific project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredAgentLaneSettings {
    pub id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub lane: AgentLane,
    pub settings: AgentLaneSettings,
    pub updated_at: DateTime<Utc>,
}

/// Persisted Workspace Review runtime settings row scoped globally or per project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredWorkspaceReviewRuntimeSettings {
    pub id: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub provider: AgentHarnessKind,
    pub settings: WorkspaceReviewRuntimeSettings,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
#[path = "harness_tests.rs"]
mod tests;

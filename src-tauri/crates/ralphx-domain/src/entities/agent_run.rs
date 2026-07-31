use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

use super::usage::{
    processed_tokens, AgentRunUsage, ProviderUsageSnapshot, UsageCapture, UsageProvenance,
};

use crate::agents::{AgentHarnessKind, LogicalEffort};

use super::{ChatConversation, ChatConversationId};

/// Unique identifier for an agent run
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentRunId(Uuid);

impl AgentRunId {
    /// Create a new random agent run ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Get the underlying UUID
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    /// Get as string
    pub fn as_str(&self) -> String {
        self.0.to_string()
    }

    /// Create from string (for database deserialization)
    pub fn from_string(s: impl Into<String>) -> Self {
        let s = s.into();
        Self(Uuid::parse_str(&s).unwrap_or_else(|_| Uuid::nil()))
    }
}

impl Default for AgentRunId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AgentRunId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<Uuid> for AgentRunId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl From<AgentRunId> for String {
    fn from(id: AgentRunId) -> Self {
        id.0.to_string()
    }
}

impl std::str::FromStr for AgentRunId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// Status of an agent run
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentRunStatus {
    /// Agent is currently running
    Running,
    /// Agent completed successfully
    Completed,
    /// Agent failed with an error
    Failed,
    /// Agent was cancelled by user
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunActionKind {
    VerifyPlan,
    WorkspaceReviewFixer,
    PrAutofix,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunAction {
    pub kind: AgentRunActionKind,
    pub context_id: String,
    pub target_id: String,
}

impl AgentRunAction {
    /// Parse only a complete backend-owned action tuple.
    pub fn from_metadata_json(metadata: Option<&str>) -> Option<Self> {
        let value = serde_json::from_str::<serde_json::Value>(metadata?).ok()?;
        let object = value.as_object()?;
        let kind = object
            .get("ralphx_action_kind")?
            .as_str()?
            .parse::<AgentRunActionKind>()
            .ok()?;
        let context_id = object.get("ralphx_action_context_id")?.as_str()?.trim();
        let target_id = object.get("ralphx_action_target_id")?.as_str()?.trim();
        if context_id.is_empty() || target_id.is_empty() {
            return None;
        }
        Some(Self {
            kind,
            context_id: context_id.to_string(),
            target_id: target_id.to_string(),
        })
    }
}

impl fmt::Display for AgentRunActionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VerifyPlan => write!(f, "verify_plan"),
            Self::WorkspaceReviewFixer => write!(f, "workspace_review_fixer"),
            Self::PrAutofix => write!(f, "pr_autofix"),
        }
    }
}

impl std::str::FromStr for AgentRunActionKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "verify_plan" => Ok(Self::VerifyPlan),
            "workspace_review_fixer" => Ok(Self::WorkspaceReviewFixer),
            "pr_autofix" => Ok(Self::PrAutofix),
            _ => Err(format!("Invalid agent run action kind: {value}")),
        }
    }
}

impl fmt::Display for AgentRunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentRunStatus::Running => write!(f, "running"),
            AgentRunStatus::Completed => write!(f, "completed"),
            AgentRunStatus::Failed => write!(f, "failed"),
            AgentRunStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::str::FromStr for AgentRunStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "running" => Ok(AgentRunStatus::Running),
            "completed" => Ok(AgentRunStatus::Completed),
            "failed" => Ok(AgentRunStatus::Failed),
            "cancelled" => Ok(AgentRunStatus::Cancelled),
            _ => Err(format!("Invalid agent run status: {}", s)),
        }
    }
}

impl AgentRunStatus {
    /// Check if the run is in a terminal state (completed, failed, or cancelled)
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            AgentRunStatus::Completed | AgentRunStatus::Failed | AgentRunStatus::Cancelled
        )
    }

    /// Check if the run is still active
    pub fn is_active(&self) -> bool {
        matches!(self, AgentRunStatus::Running)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AgentRunAttribution {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness: Option<AgentHarnessKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_effort: Option<LogicalEffort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
}

/// Body-free persona identity and delivery outcome attributed to one run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonaRunAttribution {
    pub persona_id: String,
    pub persona_slug: String,
    pub persona_version: i64,
    pub persona_content_hash: String,
    pub injected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_reason: Option<String>,
}

/// An agent run tracks the execution of a Claude agent for a conversation
///
/// This enables:
/// - Streaming persistence (messages saved as they arrive)
/// - Leave-and-come-back (user can navigate away and return)
/// - Message queueing (queue messages while agent is running)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRun {
    /// Unique identifier for this run
    pub id: AgentRunId,
    /// The conversation this run belongs to
    pub conversation_id: ChatConversationId,
    /// Current status of the run
    pub status: AgentRunStatus,
    /// When the run started
    pub started_at: DateTime<Utc>,
    /// When the run completed (if finished)
    pub completed_at: Option<DateTime<Utc>>,
    /// Error message (if failed)
    pub error_message: Option<String>,
    /// Harness that executed this run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub harness: Option<AgentHarnessKind>,
    /// Provider session ID associated with this run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    /// Upstream provider backing the selected harness.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_provider: Option<String>,
    /// Optional harness/runtime profile used for the upstream provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_profile: Option<String>,
    /// User-facing configured model for the run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_model: Option<String>,
    /// Resolved provider model ID used at runtime.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_model_id: Option<String>,
    /// Logical reasoning effort used for cross-provider configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_effort: Option<LogicalEffort>,
    /// Resolved provider-specific effort actually used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effective_effort: Option<String>,
    /// Provider service tier used for this run (for example Codex fast mode).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,
    /// Provider input tokens attributed to this run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    /// Provider output tokens attributed to this run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// Provider cache creation tokens attributed to this run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u64>,
    /// Provider cache read/hit tokens attributed to this run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    /// Estimated USD cost attributed to this run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_usd: Option<f64>,
    /// Semantics of the normalized usage tuple. `None` denotes legacy data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_provenance: Option<UsageProvenance>,
    /// Raw cumulative provider snapshot retained only for future delta derivation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_usage_snapshot: Option<ProviderUsageSnapshot>,
    /// Approval policy used for the run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_policy: Option<String>,
    /// Sandbox mode used for the run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_source: Option<String>,
    /// Correlation ID linking all runs in a single message chain
    /// (initial run + all queue continuations via --resume)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_chain_id: Option<String>,
    /// The agent_run ID that triggered this continuation (None for initial runs)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<String>,
    /// Optional generic action discriminator for backend-requested conversation turns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_kind: Option<AgentRunActionKind>,
    /// Backend-owned context id for the action (planning session for `verify_plan`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_context_id: Option<String>,
    /// Immutable target id captured when the action was requested (plan artifact for `verify_plan`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_target_id: Option<String>,
    /// Persona identity resolved for this run, without persona body content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona_id: Option<String>,
    /// Stable human-readable persona slug resolved for this run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona_slug: Option<String>,
    /// Persona version resolved for this run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona_version: Option<i64>,
    /// Content fingerprint of the resolved persona body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona_content_hash: Option<String>,
    /// Whether the resolved persona body reached the launched prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona_injected: Option<bool>,
    /// Body-free reason code when a resolved persona was not injected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona_skipped_reason: Option<String>,
}

impl AgentRun {
    /// Create a new agent run in the running state with a fresh run_chain_id
    pub fn new(conversation_id: ChatConversationId) -> Self {
        let chain_id = Uuid::new_v4().to_string();
        Self {
            id: AgentRunId::new(),
            conversation_id,
            status: AgentRunStatus::Running,
            started_at: Utc::now(),
            completed_at: None,
            error_message: None,
            harness: None,
            provider_session_id: None,
            upstream_provider: None,
            provider_profile: None,
            logical_model: None,
            effective_model_id: None,
            logical_effort: None,
            effective_effort: None,
            service_tier: None,
            input_tokens: None,
            output_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
            estimated_usd: None,
            usage_provenance: None,
            raw_usage_snapshot: None,
            approval_policy: None,
            sandbox_mode: None,
            agent_name: None,
            launch_role: None,
            runtime_source: None,
            run_chain_id: Some(chain_id),
            parent_run_id: None,
            action_kind: None,
            action_context_id: None,
            action_target_id: None,
            persona_id: None,
            persona_slug: None,
            persona_version: None,
            persona_content_hash: None,
            persona_injected: None,
            persona_skipped_reason: None,
        }
    }

    /// Create a continuation run inheriting the chain from a parent run
    pub fn new_continuation(
        conversation_id: ChatConversationId,
        run_chain_id: String,
        parent_run_id: String,
    ) -> Self {
        Self {
            id: AgentRunId::new(),
            conversation_id,
            status: AgentRunStatus::Running,
            started_at: Utc::now(),
            completed_at: None,
            error_message: None,
            harness: None,
            provider_session_id: None,
            upstream_provider: None,
            provider_profile: None,
            logical_model: None,
            effective_model_id: None,
            logical_effort: None,
            effective_effort: None,
            service_tier: None,
            input_tokens: None,
            output_tokens: None,
            cache_creation_tokens: None,
            cache_read_tokens: None,
            estimated_usd: None,
            usage_provenance: None,
            raw_usage_snapshot: None,
            approval_policy: None,
            sandbox_mode: None,
            agent_name: None,
            launch_role: None,
            runtime_source: None,
            run_chain_id: Some(run_chain_id),
            parent_run_id: Some(parent_run_id),
            action_kind: None,
            action_context_id: None,
            action_target_id: None,
            persona_id: None,
            persona_slug: None,
            persona_version: None,
            persona_content_hash: None,
            persona_injected: None,
            persona_skipped_reason: None,
        }
    }

    /// Mark the run as completed
    pub fn complete(&mut self) {
        self.status = AgentRunStatus::Completed;
        self.completed_at = Some(Utc::now());
        self.error_message = None;
    }

    /// Mark the run as failed with an error message
    pub fn fail(&mut self, error: impl Into<String>) {
        self.status = AgentRunStatus::Failed;
        self.completed_at = Some(Utc::now());
        self.error_message = Some(error.into());
    }

    /// Mark the run as cancelled
    pub fn cancel(&mut self) {
        self.status = AgentRunStatus::Cancelled;
        self.completed_at = Some(Utc::now());
        self.error_message = None;
    }

    pub fn apply_usage(&mut self, usage: &AgentRunUsage) {
        if let Some(value) = usage.input_tokens {
            self.input_tokens = Some(value);
        }
        if let Some(value) = usage.output_tokens {
            self.output_tokens = Some(value);
        }
        if let Some(value) = usage.cache_creation_tokens {
            self.cache_creation_tokens = Some(value);
        }
        if let Some(value) = usage.cache_read_tokens {
            self.cache_read_tokens = Some(value);
        }
        if let Some(value) = usage.estimated_usd {
            self.estimated_usd = Some(value);
        }
    }

    pub fn replace_usage_capture(&mut self, capture: &UsageCapture) {
        self.input_tokens = capture.normalized.input_tokens;
        self.output_tokens = capture.normalized.output_tokens;
        self.cache_creation_tokens = capture.normalized.cache_creation_tokens;
        self.cache_read_tokens = capture.normalized.cache_read_tokens;
        self.estimated_usd = capture.normalized.estimated_usd;
        self.usage_provenance = Some(capture.provenance);
        self.raw_usage_snapshot = capture.raw_snapshot.clone();
    }

    pub fn processed_tokens(&self) -> Option<u64> {
        processed_tokens(
            self.harness,
            &AgentRunUsage {
                input_tokens: self.input_tokens,
                output_tokens: self.output_tokens,
                cache_creation_tokens: self.cache_creation_tokens,
                cache_read_tokens: self.cache_read_tokens,
                estimated_usd: self.estimated_usd,
            },
            self.usage_provenance,
        )
    }

    pub fn apply_attribution(&mut self, attribution: &AgentRunAttribution) {
        if let Some(value) = attribution.harness {
            self.harness = Some(value);
        }
        if let Some(value) = attribution.provider_session_id.as_ref() {
            self.provider_session_id = Some(value.clone());
        }
        if let Some(value) = attribution.upstream_provider.as_ref() {
            self.upstream_provider = Some(value.clone());
        }
        if let Some(value) = attribution.provider_profile.as_ref() {
            self.provider_profile = Some(value.clone());
        }
        if let Some(value) = attribution.logical_model.as_ref() {
            self.logical_model = Some(value.clone());
        }
        if let Some(value) = attribution.effective_model_id.as_ref() {
            self.effective_model_id = Some(value.clone());
        }
        if let Some(value) = attribution.logical_effort {
            self.logical_effort = Some(value);
        }
        if let Some(value) = attribution.effective_effort.as_ref() {
            self.effective_effort = Some(value.clone());
        }
        if let Some(value) = attribution.service_tier.as_ref() {
            self.service_tier = Some(value.clone());
        }
    }

    pub fn apply_persona_attribution(&mut self, attribution: PersonaRunAttribution) {
        self.persona_id = Some(attribution.persona_id);
        self.persona_slug = Some(attribution.persona_slug);
        self.persona_version = Some(attribution.persona_version);
        self.persona_content_hash = Some(attribution.persona_content_hash);
        self.persona_injected = Some(attribution.injected);
        self.persona_skipped_reason = attribution.skipped_reason;
    }

    /// Apply backend-owned action metadata only when the complete typed tuple is present.
    pub fn apply_action_metadata_json(&mut self, metadata: Option<&str>) {
        let Some(action) = AgentRunAction::from_metadata_json(metadata) else {
            return;
        };

        self.action_kind = Some(action.kind);
        self.launch_role = match action.kind {
            AgentRunActionKind::WorkspaceReviewFixer => Some("workspace_repair".to_string()),
            AgentRunActionKind::PrAutofix => Some("pr_fixer".to_string()),
            _ => self.launch_role.take(),
        };
        self.action_context_id = Some(action.context_id);
        self.action_target_id = Some(action.target_id);
    }

    /// Get the duration of the run (if completed)
    pub fn duration(&self) -> Option<chrono::Duration> {
        self.completed_at
            .map(|completed| completed.signed_duration_since(self.started_at))
    }

    /// Check if this run is still active
    pub fn is_active(&self) -> bool {
        self.status.is_active()
    }

    /// Check if this run is in a terminal state
    pub fn is_terminal(&self) -> bool {
        self.status.is_terminal()
    }
}

/// A conversation that was interrupted during app shutdown
///
/// Contains the conversation that was interrupted along with the
/// last agent run that was orphaned. Used by ChatResumptionRunner
/// to resume conversations on app startup.
#[derive(Debug, Clone)]
pub struct InterruptedConversation {
    /// The conversation that was interrupted
    pub conversation: ChatConversation,
    /// The last agent run that was orphaned
    pub last_run: AgentRun,
}

#[cfg(test)]
#[path = "agent_run_tests.rs"]
mod tests;

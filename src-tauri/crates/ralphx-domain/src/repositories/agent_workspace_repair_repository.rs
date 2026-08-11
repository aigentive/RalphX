use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::entities::{
    AgentConversationWorkspacePublicationEvent, AgentRunId, AgentWorkspaceRepairAttempt,
    AgentWorkspaceRepairAttemptId, AgentWorkspaceRepairEffect, AgentWorkspaceRepairOutcome,
    AgentWorkspaceRepairPhase, ChatConversationId,
};
use crate::error::AppResult;

/// Legacy workspace fields updated atomically with a repair-attempt transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWorkspaceRepairCompatibilityProjection {
    pub publication_push_status: Option<String>,
    pub pr_supervision_status: Option<String>,
    pub pr_supervision_summary: Option<String>,
    pub pr_supervision_updated_at: Option<DateTime<Utc>>,
    pub pr_auto_merge_current: Option<bool>,
    /// Optional preference changes that must commit in the same transaction as the repair CAS.
    /// `None` preserves the existing workspace preference.
    pub pr_autofix_enabled: Option<bool>,
    pub pr_auto_merge_desired: Option<bool>,
    pub base_commit: Option<String>,
}

/// Insert-or-join input. A new attempt carries generation zero; the repository assigns the next
/// positive generation only when it atomically starts the row.
#[derive(Debug, Clone)]
pub struct StartOrJoinAgentWorkspaceRepairAttempt {
    pub attempt: AgentWorkspaceRepairAttempt,
    pub reason: String,
    /// Only a Git-verified coordinator may advance the stored target commit while joining.
    pub verified_newer_base: bool,
    pub compatibility_projection: Option<AgentWorkspaceRepairCompatibilityProjection>,
    pub events: Vec<AgentConversationWorkspacePublicationEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartOrJoinAgentWorkspaceRepairAttemptOutcome {
    Started(AgentWorkspaceRepairAttempt),
    Joined(AgentWorkspaceRepairAttempt),
    BlockedByCurrent(AgentWorkspaceRepairAttempt),
}

#[derive(Debug, Clone)]
pub struct BindAgentWorkspaceRepairAttemptRun {
    pub attempt_id: AgentWorkspaceRepairAttemptId,
    pub generation: u64,
    pub expected_phase: AgentWorkspaceRepairPhase,
    /// Exact optimistic version observed before reserving the run. A same-phase join or retry
    /// schedule must not let an older dispatcher bind a run onto newer durable authority.
    pub expected_updated_at: DateTime<Utc>,
    pub run_id: AgentRunId,
    /// First reservation records the attempt's dedicated fixer child; redelivery must preserve
    /// the existing value.
    pub runtime_conversation_id: Option<ChatConversationId>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AgentWorkspaceRepairAttemptTransition {
    pub attempt: AgentWorkspaceRepairAttempt,
    pub expected_phase: AgentWorkspaceRepairPhase,
    /// Optimistic version captured before the caller mutates `attempt` for its next phase.
    /// This prevents a same-phase join from being overwritten by a stale transition.
    pub expected_updated_at: DateTime<Utc>,
    pub next_phase: AgentWorkspaceRepairPhase,
    pub compatibility_projection: Option<AgentWorkspaceRepairCompatibilityProjection>,
    pub events: Vec<AgentConversationWorkspacePublicationEvent>,
}

impl AgentWorkspaceRepairAttemptTransition {
    /// Matches only the same current, unsettled attempt in the same conversation. A settled
    /// attempt cannot be revived and a request cannot redirect its durable identity.
    pub fn matches_attempt(&self, current: &AgentWorkspaceRepairAttempt) -> bool {
        current.id == self.attempt.id
            && current.generation == self.attempt.generation
            && current.phase == self.expected_phase
            && current.updated_at == self.expected_updated_at
            && current.settled_at.is_none()
            && current.conversation_id == self.attempt.conversation_id
            && self.attempt.settled_at.is_none()
            && self.attempt.phase == self.next_phase
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentWorkspaceRepairAttemptTransitionOutcome {
    Applied(AgentWorkspaceRepairAttempt),
    Stale(AgentWorkspaceRepairAttempt),
    Missing,
}

/// Closes an attempt after its durable continuation has handed off to an authoritative
/// downstream owner such as PR supervision. Unlike `Ready`, a settled attempt no longer exposes
/// an active maintenance operation or authorizes another automatic continuation.
#[derive(Debug, Clone)]
pub struct SettleAgentWorkspaceRepairAttempt {
    pub attempt_id: AgentWorkspaceRepairAttemptId,
    pub generation: u64,
    pub expected_phase: AgentWorkspaceRepairPhase,
    pub expected_updated_at: DateTime<Utc>,
    pub outcome: AgentWorkspaceRepairOutcome,
    pub settled_at: DateTime<Utc>,
    pub compatibility_projection: Option<AgentWorkspaceRepairCompatibilityProjection>,
    pub events: Vec<AgentConversationWorkspacePublicationEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettleAgentWorkspaceRepairAttemptOutcome {
    Applied(AgentWorkspaceRepairAttempt),
    Stale(AgentWorkspaceRepairAttempt),
    Missing,
}

#[derive(Debug, Clone)]
pub struct SettleAndStartAgentWorkspaceRepairSuccessor {
    pub attempt_id: AgentWorkspaceRepairAttemptId,
    pub generation: u64,
    pub expected_phase: AgentWorkspaceRepairPhase,
    /// Exact optimistic version observed before settling. A same-phase retry must not settle a
    /// newer attempt or duplicate a successor after another writer has already settled it.
    pub expected_updated_at: DateTime<Utc>,
    pub outcome: AgentWorkspaceRepairOutcome,
    pub settled_at: DateTime<Utc>,
    pub successor: StartOrJoinAgentWorkspaceRepairAttempt,
    pub compatibility_projection: Option<AgentWorkspaceRepairCompatibilityProjection>,
    pub events: Vec<AgentConversationWorkspacePublicationEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettleAndStartAgentWorkspaceRepairSuccessorOutcome {
    Started(AgentWorkspaceRepairAttempt),
    Stale(AgentWorkspaceRepairAttempt),
    Missing,
}

#[derive(Debug, Clone)]
pub struct CreateAgentWorkspaceRepairEffect {
    pub attempt_id: AgentWorkspaceRepairAttemptId,
    pub generation: u64,
    pub expected_phase: AgentWorkspaceRepairPhase,
    /// Exact unsettled attempt version that authorizes attaching this effect.
    pub expected_attempt_updated_at: DateTime<Utc>,
    pub effect: AgentWorkspaceRepairEffect,
    pub compatibility_projection: Option<AgentWorkspaceRepairCompatibilityProjection>,
    pub events: Vec<AgentConversationWorkspacePublicationEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateAgentWorkspaceRepairEffectOutcome {
    Created(AgentWorkspaceRepairEffect),
    OpenEffectExists(AgentWorkspaceRepairEffect),
    Stale(Box<AgentWorkspaceRepairAttempt>),
    Missing,
}

#[derive(Debug, Clone)]
pub struct CompleteAgentWorkspaceRepairEffect {
    pub attempt_id: AgentWorkspaceRepairAttemptId,
    pub generation: u64,
    pub expected_phase: AgentWorkspaceRepairPhase,
    /// Exact unsettled attempt version that authorizes recording this receipt.
    pub expected_attempt_updated_at: DateTime<Utc>,
    /// Exact effect version and status observed before this writer changes it.
    pub expected_effect_updated_at: DateTime<Utc>,
    pub expected_effect_status: crate::entities::AgentWorkspaceRepairEffectStatus,
    pub effect: AgentWorkspaceRepairEffect,
    pub compatibility_projection: Option<AgentWorkspaceRepairCompatibilityProjection>,
    pub events: Vec<AgentConversationWorkspacePublicationEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompleteAgentWorkspaceRepairEffectOutcome {
    Applied(Box<AgentWorkspaceRepairEffect>),
    Stale(Box<AgentWorkspaceRepairAttempt>),
    Missing,
}

#[derive(Debug, Clone)]
pub struct ImportLegacyAgentWorkspaceRepairAttempt {
    pub attempt: AgentWorkspaceRepairAttempt,
    pub compatibility_projection: Option<AgentWorkspaceRepairCompatibilityProjection>,
    pub events: Vec<AgentConversationWorkspacePublicationEvent>,
}

/// Legacy projection data may seed the durable workflow only before any durable generation
/// exists for the conversation. Existing durable state wins without replaying projection/events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportLegacyAgentWorkspaceRepairAttemptOutcome {
    Imported(AgentWorkspaceRepairAttempt),
    ExistingDurable(AgentWorkspaceRepairAttempt),
}

/// Persistence seam for the repair aggregate. Implementations keep the repair attempt, legacy
/// workspace projection, and publication events in one transactional boundary.
#[async_trait]
pub trait AgentWorkspaceRepairRepository: Send + Sync {
    async fn get_current_repair_attempt(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>>;

    /// Resolve the attempt whose dedicated runtime conversation is `runtime_conversation_id`.
    ///
    /// Returns `None` when no unsettled attempt owns that conversation.
    ///
    /// # Errors
    /// Returns a repository error when the lookup fails. A failed lookup MUST NOT be reported as
    /// `Ok(None)`; callers treat `Ok(None)` as "not authorized".
    async fn get_unsettled_attempt_by_runtime_conversation(
        &self,
        runtime_conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>>;

    /// Returns the latest durable generation, including settled generations. Legacy import is
    /// allowed only when this returns `None` so stale projections can never revive an old flow.
    async fn get_latest_repair_attempt_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>>;

    async fn get_repair_attempt(
        &self,
        attempt_id: &AgentWorkspaceRepairAttemptId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>>;

    async fn get_repair_attempt_for_run(
        &self,
        conversation_id: &ChatConversationId,
        run_id: &AgentRunId,
    ) -> AppResult<Option<AgentWorkspaceRepairAttempt>>;

    async fn list_recoverable_repair_attempts(&self)
        -> AppResult<Vec<AgentWorkspaceRepairAttempt>>;

    /// Every durable generation recorded for this conversation, settled or not, newest last.
    /// Cost accounting spans generations, so it cannot use the current-attempt view alone.
    async fn list_repair_attempts_for_conversation(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<AgentWorkspaceRepairAttempt>>;

    async fn start_or_join_repair_attempt(
        &self,
        request: StartOrJoinAgentWorkspaceRepairAttempt,
    ) -> AppResult<StartOrJoinAgentWorkspaceRepairAttemptOutcome>;

    async fn bind_repair_attempt_run(
        &self,
        request: BindAgentWorkspaceRepairAttemptRun,
    ) -> AppResult<AgentWorkspaceRepairAttemptTransitionOutcome>;

    async fn transition_repair_attempt(
        &self,
        request: AgentWorkspaceRepairAttemptTransition,
    ) -> AppResult<AgentWorkspaceRepairAttemptTransitionOutcome>;

    async fn settle_repair_attempt(
        &self,
        request: SettleAgentWorkspaceRepairAttempt,
    ) -> AppResult<SettleAgentWorkspaceRepairAttemptOutcome>;

    async fn settle_and_start_repair_successor(
        &self,
        request: SettleAndStartAgentWorkspaceRepairSuccessor,
    ) -> AppResult<SettleAndStartAgentWorkspaceRepairSuccessorOutcome>;

    async fn create_repair_effect(
        &self,
        request: CreateAgentWorkspaceRepairEffect,
    ) -> AppResult<CreateAgentWorkspaceRepairEffectOutcome>;

    async fn get_repair_effect_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> AppResult<Option<AgentWorkspaceRepairEffect>>;

    async fn get_open_repair_effect(
        &self,
        attempt_id: &AgentWorkspaceRepairAttemptId,
    ) -> AppResult<Option<AgentWorkspaceRepairEffect>>;

    async fn complete_repair_effect(
        &self,
        request: CompleteAgentWorkspaceRepairEffect,
    ) -> AppResult<CompleteAgentWorkspaceRepairEffectOutcome>;

    async fn import_legacy_repair_attempt(
        &self,
        request: ImportLegacyAgentWorkspaceRepairAttempt,
    ) -> AppResult<ImportLegacyAgentWorkspaceRepairAttemptOutcome>;
}

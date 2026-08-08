//! Shared managed-Team authority service.
//!
//! One instance is constructed per process and shared across both AppState
//! object graphs (Tauri commands + HTTP/MCP handlers) so ensure/status reads
//! and run-binding writes see one durable authority.

use std::sync::Arc;

use ralphx_events::{emit_serialized, EventSink, NullEventSink};
use serde::Serialize;

use crate::application::managed_team::budgets::ManagedTeamUsage;
use crate::application::managed_team::lifecycle::{new_coordinator_run_binding, new_team_session};
use crate::application::managed_team::recovery::ManagedTeamStartupBarrier;
use crate::domain::entities::{
    AgentRunId, ChatContextType, ChatConversationId, ProjectId, TeamMember, TeamMemberStatus,
    TeamRunBinding, TeamRunBindingStatus, TeamSession, TeamSessionId,
};
use crate::domain::repositories::{
    AgentRunRepository, ChatConversationRepository, QueuedMessageRepository,
    TeamCoordinationTransitionRepository, TeamMessageRepository, TeamRepository,
    TeamRunBindingRepository, TeamWakeBatchRepository, TeamWorkspaceReservationRepository,
    UiFeatureFlagOverridesRepository,
};
use crate::error::{AppError, AppResult};

/// Session plus roster projection returned by status reads.
#[derive(Debug, Clone)]
pub struct ManagedTeamStatus {
    pub session: TeamSession,
    pub members: Vec<TeamMember>,
    pub usage: ManagedTeamUsage,
}

pub struct ManagedTeamService {
    pub(super) team_repo: Arc<dyn TeamRepository>,
    pub(super) coordination_transition_repo: Arc<dyn TeamCoordinationTransitionRepository>,
    pub(super) run_binding_repo: Arc<dyn TeamRunBindingRepository>,
    pub(super) message_repo: Arc<dyn TeamMessageRepository>,
    pub(super) wake_batch_repo: Arc<dyn TeamWakeBatchRepository>,
    pub(super) queued_message_repo: Arc<dyn QueuedMessageRepository>,
    pub(super) chat_conversation_repo: Arc<dyn ChatConversationRepository>,
    pub(super) agent_run_repo: Arc<dyn AgentRunRepository>,
    pub(super) reservation_repo: Arc<dyn TeamWorkspaceReservationRepository>,
    pub(super) feature_overrides_repo: Arc<dyn UiFeatureFlagOverridesRepository>,
    pub(super) event_sink: Arc<dyn EventSink>,
    startup_barrier: Arc<ManagedTeamStartupBarrier>,
}

#[derive(Serialize)]
struct ManagedTeamMemberEventPayload {
    conversation_id: String,
    member: ManagedTeamMemberEventMember,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_run_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManagedTeamMemberEventMember {
    id: String,
    team_id: String,
    name: String,
    normalized_name: String,
    canonical_agent_name: String,
    role_summary: String,
    status: TeamMemberStatus,
    generation: i64,
}

impl ManagedTeamService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        team_repo: Arc<dyn TeamRepository>,
        coordination_transition_repo: Arc<dyn TeamCoordinationTransitionRepository>,
        run_binding_repo: Arc<dyn TeamRunBindingRepository>,
        message_repo: Arc<dyn TeamMessageRepository>,
        wake_batch_repo: Arc<dyn TeamWakeBatchRepository>,
        queued_message_repo: Arc<dyn QueuedMessageRepository>,
        chat_conversation_repo: Arc<dyn ChatConversationRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        reservation_repo: Arc<dyn TeamWorkspaceReservationRepository>,
        feature_overrides_repo: Arc<dyn UiFeatureFlagOverridesRepository>,
    ) -> Self {
        Self::new_with_event_sink(
            team_repo,
            coordination_transition_repo,
            run_binding_repo,
            message_repo,
            wake_batch_repo,
            queued_message_repo,
            chat_conversation_repo,
            agent_run_repo,
            reservation_repo,
            feature_overrides_repo,
            Arc::new(NullEventSink),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_event_sink(
        team_repo: Arc<dyn TeamRepository>,
        coordination_transition_repo: Arc<dyn TeamCoordinationTransitionRepository>,
        run_binding_repo: Arc<dyn TeamRunBindingRepository>,
        message_repo: Arc<dyn TeamMessageRepository>,
        wake_batch_repo: Arc<dyn TeamWakeBatchRepository>,
        queued_message_repo: Arc<dyn QueuedMessageRepository>,
        chat_conversation_repo: Arc<dyn ChatConversationRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        reservation_repo: Arc<dyn TeamWorkspaceReservationRepository>,
        feature_overrides_repo: Arc<dyn UiFeatureFlagOverridesRepository>,
        event_sink: Arc<dyn EventSink>,
    ) -> Self {
        Self {
            team_repo,
            coordination_transition_repo,
            run_binding_repo,
            message_repo,
            wake_batch_repo,
            queued_message_repo,
            chat_conversation_repo,
            agent_run_repo,
            reservation_repo,
            feature_overrides_repo,
            event_sink,
            startup_barrier: Arc::new(ManagedTeamStartupBarrier::new()),
        }
    }

    pub fn startup_barrier(&self) -> Arc<ManagedTeamStartupBarrier> {
        Arc::clone(&self.startup_barrier)
    }

    pub fn team_repo(&self) -> Arc<dyn TeamRepository> {
        Arc::clone(&self.team_repo)
    }

    pub fn run_binding_repo(&self) -> Arc<dyn TeamRunBindingRepository> {
        Arc::clone(&self.run_binding_repo)
    }

    pub fn wake_batch_repo(&self) -> Arc<dyn TeamWakeBatchRepository> {
        Arc::clone(&self.wake_batch_repo)
    }

    pub(super) async fn emit_member_updated(&self, member: &TeamMember) {
        let Ok(Some(session)) = self.team_repo.get_session(&member.team_id).await else {
            return;
        };
        let parent_run_id = self
            .run_binding_repo
            .list_for_team(&member.team_id)
            .await
            .ok()
            .and_then(|bindings| {
                bindings
                    .into_iter()
                    .filter(|binding| {
                        binding.team_member_id.is_none()
                            && matches!(
                                binding.status,
                                TeamRunBindingStatus::Planned
                                    | TeamRunBindingStatus::Launching
                                    | TeamRunBindingStatus::Running
                            )
                    })
                    .max_by_key(|binding| binding.created_at)
                    .map(|binding| binding.agent_run_id.as_str().to_string())
            });
        let payload = ManagedTeamMemberEventPayload {
            conversation_id: session.coordinator_conversation_id.as_str(),
            member: ManagedTeamMemberEventMember {
                id: member.id.as_str().to_string(),
                team_id: member.team_id.as_str().to_string(),
                name: member.name.clone(),
                normalized_name: member.normalized_name.clone(),
                canonical_agent_name: member.canonical_agent_name.clone(),
                role_summary: member.role_summary.clone(),
                status: member.status,
                generation: member.generation,
            },
            parent_run_id,
        };
        if let Err(error) = emit_serialized(&*self.event_sink, "team:member_updated", &payload) {
            tracing::warn!(%error, "Managed Team member event serialization failed");
        }
    }

    /// Whether the Team capability override is enabled. Read errors propagate
    /// as typed errors; callers must not treat them as "disabled".
    pub async fn team_capability_enabled(&self) -> AppResult<bool> {
        let overrides = self.feature_overrides_repo.get().await?;
        Ok(overrides.agent_conversation_team)
    }

    /// Ensures one open Team session exists for the coordinator conversation.
    /// Re-entry returns the existing open session.
    pub async fn ensure_team(
        &self,
        project_id: ProjectId,
        conversation_id: &ChatConversationId,
    ) -> AppResult<TeamSession> {
        let conversation = self
            .chat_conversation_repo
            .get_by_id(conversation_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound("Team coordinator conversation was not found".to_string())
            })?;
        if conversation.context_type != ChatContextType::Project
            || conversation.context_id != project_id.as_str()
        {
            return Err(AppError::Conflict(
                "Team coordinator conversation does not belong to the requested project"
                    .to_string(),
            ));
        }
        let session = new_team_session(project_id, *conversation_id);
        self.coordination_transition_repo
            .enter_team(conversation_id, session)
            .await
    }

    /// Session plus roster for a coordinator conversation, if a Team is open.
    pub async fn team_status(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<ManagedTeamStatus>> {
        let Some(session) = self
            .team_repo
            .get_open_session_for_conversation(conversation_id)
            .await?
        else {
            return Ok(None);
        };
        let members = self.team_repo.list_members(&session.id).await?;
        let usage = self.team_usage(&session.id).await?;
        Ok(Some(ManagedTeamStatus {
            session,
            members,
            usage,
        }))
    }

    pub async fn roster(&self, team_id: &TeamSessionId) -> AppResult<Vec<TeamMember>> {
        self.team_repo.list_members(team_id).await
    }

    /// Records the member-null, coordination-only run binding for a managed
    /// coordinator send before the run launches.
    ///
    /// Idempotent for a wake-driven send that already carries a binding for
    /// this exact run id: a wake batch claim (`claim_next_actionable_wake_batch`)
    /// creates the member-null binding before the coordinator send happens, so
    /// this call must return that binding unchanged instead of creating a
    /// second one for the same `agent_run_id` — a duplicate would make
    /// `run_binding_repo.get_by_agent_run_id` ambiguous for
    /// `validate_message_sender` and `settle_member_assignment`. A binding for
    /// a different Team or conversation is a real conflict, not something to
    /// silently adopt.
    ///
    /// Returns `Ok(None)` when the Team capability override is disabled (the
    /// send proceeds as an ordinary turn). Override read errors and binding
    /// write errors propagate; callers must fail the send instead of launching
    /// an unbound Team run.
    pub async fn preallocate_coordinator_run_binding(
        &self,
        project_id: ProjectId,
        conversation_id: &ChatConversationId,
        agent_run_id: &AgentRunId,
    ) -> AppResult<Option<TeamRunBinding>> {
        if !self.team_capability_enabled().await? {
            return Ok(None);
        }
        let session = self.ensure_team(project_id, conversation_id).await?;
        if let Some(existing) = self
            .run_binding_repo
            .get_by_agent_run_id(agent_run_id)
            .await?
        {
            if existing.team_id == session.id
                && existing.conversation_id == *conversation_id
                && existing.team_member_id.is_none()
            {
                return Ok(Some(existing));
            }
            return Err(AppError::Conflict(
                "agent run already carries a Team run binding for a different Team or conversation"
                    .to_string(),
            ));
        }
        let binding = new_coordinator_run_binding(session.id, *conversation_id, *agent_run_id);
        let binding = self.run_binding_repo.create(binding).await?;
        Ok(Some(binding))
    }
}

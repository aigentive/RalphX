//! Shared managed-Team authority service.
//!
//! One instance is constructed per process and shared across both AppState
//! object graphs (Tauri commands + HTTP/MCP handlers) so ensure/status reads
//! and run-binding writes see one durable authority.

use std::sync::Arc;

use crate::application::managed_team::lifecycle::{new_coordinator_run_binding, new_team_session};
use crate::application::managed_team::recovery::ManagedTeamStartupBarrier;
use crate::domain::entities::{
    AgentRunId, ChatConversationId, ProjectId, TeamMember, TeamRunBinding, TeamSession,
    TeamSessionId,
};
use crate::domain::repositories::{
    TeamCoordinationTransitionRepository, TeamMessageRepository, TeamRepository,
    TeamRunBindingRepository, TeamWakeBatchRepository, TeamWorkspaceReservationRepository,
    UiFeatureFlagOverridesRepository,
};
use crate::error::AppResult;

/// Session plus roster projection returned by status reads.
#[derive(Debug, Clone)]
pub struct ManagedTeamStatus {
    pub session: TeamSession,
    pub members: Vec<TeamMember>,
}

pub struct ManagedTeamService {
    team_repo: Arc<dyn TeamRepository>,
    coordination_transition_repo: Arc<dyn TeamCoordinationTransitionRepository>,
    run_binding_repo: Arc<dyn TeamRunBindingRepository>,
    #[allow(dead_code)] // wired for later slices (durable messaging)
    message_repo: Arc<dyn TeamMessageRepository>,
    #[allow(dead_code)] // wired for later slices (wake batching)
    wake_batch_repo: Arc<dyn TeamWakeBatchRepository>,
    #[allow(dead_code)] // wired for later slices (write reservations)
    reservation_repo: Arc<dyn TeamWorkspaceReservationRepository>,
    feature_overrides_repo: Arc<dyn UiFeatureFlagOverridesRepository>,
    startup_barrier: Arc<ManagedTeamStartupBarrier>,
}

impl ManagedTeamService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        team_repo: Arc<dyn TeamRepository>,
        coordination_transition_repo: Arc<dyn TeamCoordinationTransitionRepository>,
        run_binding_repo: Arc<dyn TeamRunBindingRepository>,
        message_repo: Arc<dyn TeamMessageRepository>,
        wake_batch_repo: Arc<dyn TeamWakeBatchRepository>,
        reservation_repo: Arc<dyn TeamWorkspaceReservationRepository>,
        feature_overrides_repo: Arc<dyn UiFeatureFlagOverridesRepository>,
    ) -> Self {
        Self {
            team_repo,
            coordination_transition_repo,
            run_binding_repo,
            message_repo,
            wake_batch_repo,
            reservation_repo,
            feature_overrides_repo,
            startup_barrier: Arc::new(ManagedTeamStartupBarrier::new()),
        }
    }

    pub fn startup_barrier(&self) -> Arc<ManagedTeamStartupBarrier> {
        Arc::clone(&self.startup_barrier)
    }

    pub fn team_repo(&self) -> Arc<dyn TeamRepository> {
        Arc::clone(&self.team_repo)
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
        Ok(Some(ManagedTeamStatus { session, members }))
    }

    pub async fn roster(&self, team_id: &TeamSessionId) -> AppResult<Vec<TeamMember>> {
        self.team_repo.list_members(team_id).await
    }

    /// Records the member-null, coordination-only run binding for a managed
    /// coordinator send before the run launches.
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
        let binding = new_coordinator_run_binding(session.id, *conversation_id, *agent_run_id);
        let binding = self.run_binding_repo.create(binding).await?;
        Ok(Some(binding))
    }
}

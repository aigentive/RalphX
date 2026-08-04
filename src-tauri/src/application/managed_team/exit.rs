//! Staged, durable Team-to-Solo exit.

use chrono::Utc;

use crate::application::AgentTaskService;
use crate::domain::entities::{
    AgentTaskAssignmentTerminalStatus, CoordinationMode, TeamMemberStatus, TeamRunBindingStatus,
    TeamSession, TeamSessionId, TeamSessionStatus,
};
use crate::domain::repositories::TeamExitMarker;
use crate::error::{AppError, AppResult};

use super::service::ManagedTeamService;

const EXIT_ACTION_MISMATCH: &str = "managed Team already has a different exit action pending";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamExitAction {
    Suspend,
    DrainAndClose,
}

impl TeamExitAction {
    pub fn parse(value: &str) -> AppResult<Self> {
        match value {
            "suspend" => Ok(Self::Suspend),
            "drain_and_close" => Ok(Self::DrainAndClose),
            _ => Err(AppError::Validation(
                "managed Team exit action must be suspend or drain_and_close".to_string(),
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Suspend => "suspend",
            Self::DrainAndClose => "drain_and_close",
        }
    }
}

impl ManagedTeamService {
    /// Reuses any durable exit choice before an external writer persists a
    /// non-Team coordination mode. Callers may continue when no open Team
    /// session exists, but repository, marker, and exit failures are blocking.
    pub async fn exit_team_before_coordination_change(
        &self,
        task_service: &AgentTaskService,
        conversation_id: &crate::domain::entities::ChatConversationId,
    ) -> AppResult<bool> {
        let Some(session) = self
            .team_repo
            .get_open_session_for_conversation(conversation_id)
            .await?
        else {
            return Ok(false);
        };
        let action = match session.pending_exit_action.as_deref() {
            Some(raw) => TeamExitAction::parse(raw)?,
            None => TeamExitAction::DrainAndClose,
        };
        self.exit_team(task_service, &session.id, action).await?;
        Ok(true)
    }

    /// Marks an exit before cleanup. A retry reads the marker and resumes the
    /// same action; it never changes a durable exit choice mid-flight.
    pub async fn exit_team(
        &self,
        task_service: &AgentTaskService,
        team_id: &TeamSessionId,
        action: TeamExitAction,
    ) -> AppResult<TeamSession> {
        let mut session = self
            .team_repo
            .get_session(team_id)
            .await?
            .ok_or_else(|| AppError::NotFound("managed Team was not found".to_string()))?;
        let conversation = self
            .chat_conversation_repo
            .get_by_id(&session.coordinator_conversation_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound("Team coordinator conversation was not found".to_string())
            })?;
        if conversation.coordination_mode == CoordinationMode::Solo {
            return Ok(session);
        }
        match session.pending_exit_action.as_deref() {
            Some(current) if current != action.as_str() => {
                return Err(AppError::Conflict(EXIT_ACTION_MISMATCH.to_string()));
            }
            Some(_) => {}
            None => {
                if !self
                    .coordination_transition_repo
                    .mark_pending_exit(
                        &session.id,
                        session.version,
                        TeamExitMarker {
                            coordination_mode: CoordinationMode::Solo,
                            exit_action: action.as_str().to_string(),
                        },
                    )
                    .await?
                {
                    return Err(AppError::Conflict(
                        "managed Team changed before exit could be marked".to_string(),
                    ));
                }
                session =
                    self.team_repo.get_session(team_id).await?.ok_or_else(|| {
                        AppError::NotFound("managed Team was not found".to_string())
                    })?;
            }
        }

        session = match action {
            TeamExitAction::Suspend => self.suspend_team(session).await?,
            TeamExitAction::DrainAndClose => self.drain_team(task_service, session).await?,
        };
        if !self
            .coordination_transition_repo
            .commit_exit(
                &session.coordinator_conversation_id,
                &session.id,
                session.version,
            )
            .await?
        {
            return Err(AppError::Conflict(
                "managed Team changed before exit could be committed".to_string(),
            ));
        }
        // This is deliberately the last externally visible write. A crash
        // before it retains Team mode plus the durable pending marker.
        self.chat_conversation_repo
            .update_coordination_mode(&session.coordinator_conversation_id, CoordinationMode::Solo)
            .await?;
        self.team_repo
            .get_session(team_id)
            .await?
            .ok_or_else(|| AppError::NotFound("managed Team was not found".to_string()))
    }

    async fn suspend_team(&self, session: TeamSession) -> AppResult<TeamSession> {
        if session.status == TeamSessionStatus::Suspended {
            return Ok(session);
        }
        let session = self
            .transition_session(session, TeamSessionStatus::Suspending)
            .await?;
        for member in self.team_repo.list_members(&session.id).await? {
            if member.status == TeamMemberStatus::Idle {
                let mut suspended = member.clone();
                suspended
                    .transition_to(TeamMemberStatus::Suspended, Utc::now())
                    .map_err(AppError::Validation)?;
                if !self
                    .team_repo
                    .update_member(suspended.clone(), member.generation)
                    .await?
                {
                    return Err(AppError::Conflict(
                        "managed Team member changed while suspending".to_string(),
                    ));
                }
                self.emit_member_updated(&suspended).await;
            }
        }
        self.transition_session(session, TeamSessionStatus::Suspended)
            .await
    }

    async fn drain_team(
        &self,
        task_service: &AgentTaskService,
        session: TeamSession,
    ) -> AppResult<TeamSession> {
        if session.status == TeamSessionStatus::Closed {
            return Ok(session);
        }
        let session = self
            .transition_session(session, TeamSessionStatus::Draining)
            .await?;
        for binding in self.run_binding_repo.list_for_team(&session.id).await? {
            if matches!(
                binding.status,
                TeamRunBindingStatus::Planned
                    | TeamRunBindingStatus::Launching
                    | TeamRunBindingStatus::Running
            ) {
                let mut cancelled = binding.clone();
                cancelled
                    .transition_to(TeamRunBindingStatus::Cancelled, Utc::now())
                    .map_err(AppError::Validation)?;
                cancelled.last_error = Some("team_exit_drain".to_string());
                let expected = cancelled.version;
                cancelled.version += 1;
                if !self
                    .run_binding_repo
                    .transition(&cancelled.id.clone(), expected, cancelled)
                    .await?
                {
                    return Err(AppError::Conflict(
                        "managed Team binding changed while draining".to_string(),
                    ));
                }
            }
            if let Some(assignment_id) = binding.assignment_id.as_ref() {
                for reservation in self
                    .reservation_repo
                    .list_active_for_assignment(assignment_id.as_str())
                    .await?
                {
                    if !self
                        .reservation_repo
                        .release_if_current(
                            &reservation.id,
                            reservation.team_member_generation,
                            reservation.attempt_number,
                        )
                        .await?
                    {
                        return Err(AppError::Conflict(
                            "managed Team reservation changed while draining".to_string(),
                        ));
                    }
                }
            }
            if binding.assignment_id.is_some() {
                let _ = task_service
                    .settle_assignment_for_run(
                        &binding.agent_run_id,
                        AgentTaskAssignmentTerminalStatus::Cancelled,
                        Some("team_exit_drain"),
                    )
                    .await?;
            }
        }
        for member in self.team_repo.list_members(&session.id).await? {
            if matches!(
                member.status,
                TeamMemberStatus::Working
                    | TeamMemberStatus::AwaitingInput
                    | TeamMemberStatus::AwaitingApproval
                    | TeamMemberStatus::Idle
                    | TeamMemberStatus::Suspended
                    | TeamMemberStatus::Failed
            ) {
                let mut stopped = member.clone();
                if stopped.status != TeamMemberStatus::Stopping {
                    stopped
                        .transition_to(TeamMemberStatus::Stopping, Utc::now())
                        .map_err(AppError::Validation)?;
                }
                stopped
                    .transition_to(TeamMemberStatus::Stopped, Utc::now())
                    .map_err(AppError::Validation)?;
                stopped.current_run_id = None;
                stopped.current_assignment_id = None;
                stopped.delegated_session_id = None;
                if !self
                    .team_repo
                    .update_member(stopped.clone(), member.generation)
                    .await?
                {
                    return Err(AppError::Conflict(
                        "managed Team member changed while draining".to_string(),
                    ));
                }
                self.emit_member_updated(&stopped).await;
            }
        }
        self.transition_session(session, TeamSessionStatus::Closed)
            .await
    }

    async fn transition_session(
        &self,
        session: TeamSession,
        next: TeamSessionStatus,
    ) -> AppResult<TeamSession> {
        if session.status == next {
            return Ok(session);
        }
        let mut updated = session.clone();
        updated
            .transition_to(next, Utc::now())
            .map_err(AppError::Validation)?;
        updated.version += 1;
        if !self
            .team_repo
            .update_session(updated.clone(), session.version)
            .await?
        {
            return Err(AppError::Conflict(
                "managed Team session changed during exit".to_string(),
            ));
        }
        Ok(updated)
    }
}

use std::sync::Arc;

use crate::application::managed_team::ManagedTeamService;
use crate::application::AgentTaskService;
use crate::domain::entities::{
    AgentRunId, AgentRunStatus, AgentTaskAssignmentState, AgentTaskAssignmentTerminalStatus,
    ChatContextType, DelegatedSessionId,
};
use crate::domain::repositories::{
    AgentRunRepository, AgentTaskRepository, ChatConversationRepository, DelegatedSessionRepository,
};
use crate::domain::services::running_agent_registry::{RunningAgentKey, RunningAgentRegistry};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgentTaskAssignmentRecoveryReport {
    pub inspected: usize,
    pub settled: usize,
    pub retained_running: usize,
}

pub struct AgentTaskAssignmentRecoveryService {
    task_service: AgentTaskService,
    delegated_session_repo: Arc<dyn DelegatedSessionRepository>,
    conversation_repo: Arc<dyn ChatConversationRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    running_agent_registry: Arc<dyn RunningAgentRegistry>,
    managed_team: Option<Arc<ManagedTeamService>>,
}

impl AgentTaskAssignmentRecoveryService {
    pub fn new(
        agent_task_repo: Arc<dyn AgentTaskRepository>,
        delegated_session_repo: Arc<dyn DelegatedSessionRepository>,
        conversation_repo: Arc<dyn ChatConversationRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        running_agent_registry: Arc<dyn RunningAgentRegistry>,
    ) -> Self {
        Self {
            task_service: AgentTaskService::new(agent_task_repo),
            delegated_session_repo,
            conversation_repo,
            agent_run_repo,
            running_agent_registry,
            managed_team: None,
        }
    }

    /// Production startup supplies the shared Team authority. Keeping this
    /// optional preserves legacy/Solo test construction and behavior.
    pub fn with_managed_team(mut self, managed_team: Arc<ManagedTeamService>) -> Self {
        self.managed_team = Some(managed_team);
        self
    }

    pub async fn recover(&self) -> AppResult<AgentTaskAssignmentRecoveryReport> {
        let assignments = self.task_service.list_unresolved_assignments().await?;
        let mut report = AgentTaskAssignmentRecoveryReport {
            inspected: assignments.len(),
            ..Default::default()
        };
        for assignment in assignments {
            let session_id = &assignment.assignment.delegated_session_id;
            let run_id = match assignment.assignment.state {
                AgentTaskAssignmentState::Reserved => {
                    if assignment.assignment.delegated_agent_run_id.is_some() {
                        return Err(AppError::Conflict(format!(
                            "reserved delegate assignment {} already has a bound run",
                            assignment.assignment.id
                        )));
                    }
                    if let Some(planned_run_id) =
                        assignment.assignment.planned_delegated_agent_run_id
                    {
                        if self
                            .recoverable_planned_run(session_id, &planned_run_id)
                            .await?
                        {
                            let bound = self
                                .task_service
                                .bind_assignment_run(
                                    &assignment.assignment.id,
                                    session_id,
                                    &planned_run_id,
                                )
                                .await?;
                            if bound.is_none() {
                                return Err(AppError::Conflict(format!(
                                    "reserved delegate assignment {} disappeared before recovery binding",
                                    assignment.assignment.id
                                )));
                            }
                            planned_run_id
                        } else {
                            if self
                                .task_service
                                .fail_reserved_assignment(
                                    session_id,
                                    "orphaned_reservation_after_restart",
                                )
                                .await?
                                .is_some()
                            {
                                self.recover_team_projection(
                                    &assignment,
                                    "orphaned_reservation_after_restart",
                                )
                                .await?;
                                report.settled += 1;
                            }
                            continue;
                        }
                    } else {
                        let settled = self
                            .task_service
                            .fail_reserved_assignment(
                                session_id,
                                "uncorrelated_reservation_after_restart",
                            )
                            .await?;
                        if settled.is_some() {
                            self.recover_team_projection(
                                &assignment,
                                "uncorrelated_reservation_after_restart",
                            )
                            .await?;
                            report.settled += 1;
                        }
                        continue;
                    }
                }
                AgentTaskAssignmentState::Active
                | AgentTaskAssignmentState::CompletionRequested
                | AgentTaskAssignmentState::ReleaseRequested => {
                    let Some(run_id) = assignment.assignment.delegated_agent_run_id else {
                        return Err(AppError::Conflict(format!(
                            "unresolved delegate assignment {} has no bound run",
                            assignment.assignment.id
                        )));
                    };
                    run_id
                }
                AgentTaskAssignmentState::Completed
                | AgentTaskAssignmentState::Released
                | AgentTaskAssignmentState::Failed
                | AgentTaskAssignmentState::Cancelled => {
                    return Err(AppError::Conflict(format!(
                        "settled delegate assignment {} was returned as unresolved",
                        assignment.assignment.id
                    )));
                }
            };
            let session = self.delegated_session_repo.get_by_id(session_id).await?;
            let conversation = self
                .conversation_repo
                .get_active_for_context(ChatContextType::Delegation, session_id.as_str())
                .await?;
            let run = self.agent_run_repo.get_by_id(&run_id).await?;
            let terminal = match run.as_ref().map(|run| run.status) {
                Some(AgentRunStatus::Completed) => {
                    Some(AgentTaskAssignmentTerminalStatus::Completed)
                }
                Some(AgentRunStatus::Failed) => Some(AgentTaskAssignmentTerminalStatus::Failed),
                Some(AgentRunStatus::Cancelled) => {
                    Some(AgentTaskAssignmentTerminalStatus::Cancelled)
                }
                Some(AgentRunStatus::Running) => None,
                None => Some(AgentTaskAssignmentTerminalStatus::Failed),
            };
            if let Some(terminal) = terminal {
                if self
                    .task_service
                    .settle_assignment_for_run(
                        &run_id,
                        terminal,
                        (run.is_none()).then_some("bound_run_missing_after_restart"),
                    )
                    .await?
                    .is_some()
                {
                    self.settle_team_projection(
                        &assignment,
                        terminal,
                        (run.is_none()).then_some("bound_run_missing_after_restart"),
                    )
                    .await?;
                    report.settled += 1;
                }
                continue;
            }

            let valid_session = session
                .as_ref()
                .is_some_and(|session| session.status == "running");
            let valid_conversation = conversation.as_ref().is_some_and(|conversation| {
                run.as_ref()
                    .is_some_and(|run| run.conversation_id == conversation.id)
            });
            let key = RunningAgentKey::new("delegation", session_id.as_str());
            let valid_process = self
                .running_agent_registry
                .get(&key)
                .await
                .is_some_and(|info| {
                    info.agent_run_id == run_id.as_str()
                        && conversation.as_ref().is_some_and(|conversation| {
                            info.conversation_id == conversation.id.as_str()
                        })
                });
            if valid_session && valid_conversation && valid_process {
                report.retained_running += 1;
                continue;
            }
            if self
                .task_service
                .settle_assignment_for_run(
                    &run_id,
                    AgentTaskAssignmentTerminalStatus::Failed,
                    Some("orphaned_running_assignment_after_restart"),
                )
                .await?
                .is_some()
            {
                self.recover_team_projection(
                    &assignment,
                    "orphaned_running_assignment_after_restart",
                )
                .await?;
                report.settled += 1;
            }
        }
        Ok(report)
    }

    async fn recoverable_planned_run(
        &self,
        session_id: &DelegatedSessionId,
        planned_run_id: &AgentRunId,
    ) -> AppResult<bool> {
        let Some(session) = self.delegated_session_repo.get_by_id(session_id).await? else {
            return Ok(false);
        };
        if session.status != "running" {
            return Ok(false);
        }
        let Some(conversation) = self
            .conversation_repo
            .get_active_for_context(ChatContextType::Delegation, session_id.as_str())
            .await?
        else {
            return Ok(false);
        };
        let Some(run) = self.agent_run_repo.get_by_id(planned_run_id).await? else {
            return Ok(false);
        };
        if run.status != AgentRunStatus::Running || run.conversation_id != conversation.id {
            return Ok(false);
        }
        let key = RunningAgentKey::new("delegation", session_id.as_str());
        let exact_process = self
            .running_agent_registry
            .get(&key)
            .await
            .is_some_and(|info| {
                info.agent_run_id == planned_run_id.as_str()
                    && info.conversation_id == conversation.id.as_str()
            });
        Ok(exact_process)
    }

    async fn recover_team_projection(
        &self,
        assignment: &crate::domain::entities::AgentTaskAssignmentView,
        reason: &str,
    ) -> AppResult<()> {
        if let Some(managed_team) = &self.managed_team {
            managed_team
                .recover_orphaned_member_assignment(assignment, reason)
                .await?;
        }
        Ok(())
    }

    async fn settle_team_projection(
        &self,
        assignment: &crate::domain::entities::AgentTaskAssignmentView,
        terminal_status: AgentTaskAssignmentTerminalStatus,
        reason: Option<&str>,
    ) -> AppResult<()> {
        if let Some(managed_team) = &self.managed_team {
            managed_team
                .settle_member_assignment(assignment, terminal_status, reason)
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "agent_task_assignment_recovery_tests.rs"]
mod tests;

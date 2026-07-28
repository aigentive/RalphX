//! Durable standing-member operations and pre-launch assignment planning.
//!
//! A member row is deliberately created lazily: this module never starts a
//! provider process. Provider work begins only after the caller has persisted
//! the complete assignment plan and hands it to `NativeDelegationLauncher`.

use chrono::Utc;

use crate::application::managed_team::lifecycle::new_member_assignment_run_binding;
use crate::application::managed_team::ManagedTeamService;
use crate::application::AgentTaskService;
use crate::domain::agents::{AgentHarnessKind, LogicalEffort};
use crate::domain::entities::{
    normalize_team_member_name, AgentRunId, AgentTaskAssignmentTerminalStatus,
    AgentTaskAssignmentView, AgentTaskScope, ChatConversationId, DelegatedSessionId, TeamMember,
    TeamMemberId, TeamMemberStatus, TeamRunBinding, TeamRunBindingStatus, TeamSessionId,
    TeamWorkClassification, TeamWorkspaceReservation, TeamWorkspaceReservationId,
};
use crate::error::{AppError, AppResult};

/// Coordinator supplied durable member metadata. It is intentionally process
/// free; a member becomes live only when an exact assignment is launched.
#[derive(Debug, Clone)]
pub struct ManagedTeamMemberSpec {
    pub name: String,
    pub canonical_agent_name: String,
    pub role_summary: String,
    pub harness: Option<AgentHarnessKind>,
    pub logical_model: Option<String>,
    pub logical_effort: Option<LogicalEffort>,
}

#[derive(Debug, Clone)]
pub struct ManagedTeamWorkspaceRequest {
    pub writable_paths: Vec<String>,
    pub generated_outputs: Vec<String>,
    pub resource_locks: Vec<String>,
}

/// Inputs whose authority has already been resolved by the HTTP transport.
#[derive(Debug, Clone)]
pub struct ManagedTeamAssignmentRequest {
    pub team_id: TeamSessionId,
    pub member_name: String,
    pub expected_member_generation: i64,
    pub caller_scope: AgentTaskScope,
    pub caller_agent_run_id: AgentRunId,
    pub task_ref: String,
    pub delegated_session_id: DelegatedSessionId,
    pub delegated_conversation_id: ChatConversationId,
    pub planned_agent_run_id: AgentRunId,
    pub work_classification: TeamWorkClassification,
    pub workspace: Option<ManagedTeamWorkspaceRequest>,
}

/// Fully persisted stages 1–3 of an exact Team assignment.
#[derive(Debug, Clone)]
pub struct ManagedTeamAssignmentPlan {
    pub member: TeamMember,
    pub assignment: AgentTaskAssignmentView,
    pub binding: TeamRunBinding,
    pub reservation: Option<TeamWorkspaceReservation>,
}

impl ManagedTeamService {
    /// Adds an idle durable member without spawning a harness process.
    pub async fn add_member(
        &self,
        team_id: &TeamSessionId,
        spec: ManagedTeamMemberSpec,
    ) -> AppResult<TeamMember> {
        let normalized_name =
            normalize_team_member_name(&spec.name).map_err(AppError::Validation)?;
        if spec.canonical_agent_name.trim().is_empty() {
            return Err(AppError::Validation(
                "team member canonical agent name is required".to_string(),
            ));
        }
        if spec.role_summary.trim().is_empty() {
            return Err(AppError::Validation(
                "team member role summary is required".to_string(),
            ));
        }
        if self.team_repo.get_session(team_id).await?.is_none() {
            return Err(AppError::NotFound("managed Team was not found".to_string()));
        }
        let now = Utc::now();
        self.team_repo
            .create_member(TeamMember {
                id: TeamMemberId::new(),
                team_id: team_id.clone(),
                normalized_name,
                name: spec.name.trim().to_string(),
                canonical_agent_name: spec.canonical_agent_name,
                role_summary: spec.role_summary,
                harness: spec.harness,
                logical_model: spec.logical_model,
                logical_effort: spec.logical_effort,
                delegated_session_id: None,
                generation: 0,
                current_run_id: None,
                current_assignment_id: None,
                status: TeamMemberStatus::Idle,
                last_activity_at: Some(now),
                last_error: None,
                created_at: now,
                updated_at: now,
                stopped_at: None,
            })
            .await
    }

    pub async fn idle_members(&self, team_id: &TeamSessionId) -> AppResult<Vec<TeamMember>> {
        Ok(self
            .team_repo
            .list_members(team_id)
            .await?
            .into_iter()
            .filter(|member| member.status == TeamMemberStatus::Idle)
            .collect())
    }

    pub async fn member_by_normalized_name(
        &self,
        team_id: &TeamSessionId,
        member_name: &str,
    ) -> AppResult<TeamMember> {
        let normalized_name =
            normalize_team_member_name(member_name).map_err(AppError::Validation)?;
        self.team_repo
            .list_members(team_id)
            .await?
            .into_iter()
            .find(|member| member.normalized_name == normalized_name)
            .ok_or_else(|| AppError::NotFound("managed Team member was not found".to_string()))
    }

    /// Persists reservation, exact task reservation, Team identity, planned
    /// run, and planned binding. Every post-reservation failure compensates
    /// already-persisted state before returning the original error.
    pub async fn plan_member_assignment(
        &self,
        task_service: &AgentTaskService,
        request: ManagedTeamAssignmentRequest,
    ) -> AppResult<ManagedTeamAssignmentPlan> {
        let session = self.admit_dispatch(&request.team_id).await?;
        if self.configured_dispatch_count(&request.team_id).await? >= session.configured_concurrency
        {
            return Err(AppError::Conflict(
                "managed Team configured concurrency is exhausted".to_string(),
            ));
        }
        let member = self
            .member_by_normalized_name(&request.team_id, &request.member_name)
            .await?;
        if !member.is_current_generation(request.expected_member_generation) {
            return Err(AppError::Conflict(
                "managed Team member generation is stale".to_string(),
            ));
        }
        if member.status != TeamMemberStatus::Idle || member.current_assignment_id.is_some() {
            return Err(AppError::Conflict(
                "managed Team member is busy".to_string(),
            ));
        }
        if request.work_classification == TeamWorkClassification::Write
            && request.workspace.is_none()
        {
            return Err(AppError::Validation(
                "write-classified Team assignments require a workspace reservation".to_string(),
            ));
        }

        let reservation = match request.workspace.as_ref() {
            Some(workspace) => Some(
                self.reservation_repo
                    .acquire(TeamWorkspaceReservation {
                        id: TeamWorkspaceReservationId::new(),
                        team_id: request.team_id.clone(),
                        team_member_id: member.id.clone(),
                        assignment_id: None,
                        team_member_generation: member.generation,
                        writable_paths: workspace.writable_paths.clone(),
                        generated_outputs: workspace.generated_outputs.clone(),
                        resource_locks: workspace.resource_locks.clone(),
                        work_classification: request.work_classification,
                        attempt_number: 1,
                        acquired_at: Utc::now(),
                        released_at: None,
                    })
                    .await?,
            ),
            None => None,
        };

        let assignment = match task_service
            .reserve_assignment(
                &request.caller_scope,
                &request.task_ref,
                &request.delegated_session_id,
                &request.caller_agent_run_id,
                &member.canonical_agent_name,
            )
            .await
        {
            Ok(Some(value)) => value.assignment,
            Ok(None) => {
                self.release_reservation(&reservation).await;
                return Err(AppError::NotFound(
                    "managed Team task was not found".to_string(),
                ));
            }
            Err(error) => {
                self.release_reservation(&reservation).await;
                return Err(error);
            }
        };

        let planned = async {
            let identity = task_service
                .set_assignment_team_identity(
                    &assignment.assignment.id,
                    &request.delegated_session_id,
                    &request.team_id,
                    &member.id,
                    member.generation,
                )
                .await?
                .ok_or_else(|| {
                    AppError::Conflict("reserved Team task assignment disappeared".to_string())
                })?;
            if let Some(reservation) = &reservation {
                if !self
                    .reservation_repo
                    .attach_assignment_if_current(
                        &reservation.id,
                        reservation.team_member_generation,
                        reservation.attempt_number,
                        &identity.assignment.id,
                    )
                    .await?
                {
                    return Err(AppError::Conflict(
                        "Team workspace reservation changed before task assignment link"
                            .to_string(),
                    ));
                }
            }
            task_service
                .plan_assignment_run(
                    &identity.assignment.id,
                    &request.delegated_session_id,
                    &request.planned_agent_run_id,
                )
                .await?
                .ok_or_else(|| {
                    AppError::Conflict("reserved Team task assignment disappeared".to_string())
                })?;

            let mut working = member.clone();
            working
                .transition_to(TeamMemberStatus::Working, Utc::now())
                .map_err(AppError::Validation)?;
            working.current_assignment_id = Some(identity.assignment.id.clone());
            working.current_run_id = Some(request.planned_agent_run_id);
            working.delegated_session_id = Some(request.delegated_session_id.clone());
            working.last_activity_at = Some(Utc::now());
            if !self
                .team_repo
                .update_member(working.clone(), member.generation)
                .await?
            {
                return Err(AppError::Conflict(
                    "managed Team member changed before assignment planning".to_string(),
                ));
            }

            let binding = self
                .run_binding_repo
                .create(new_member_assignment_run_binding(
                    &working,
                    request.delegated_session_id.clone(),
                    request.delegated_conversation_id,
                    request.planned_agent_run_id,
                    identity.assignment.id.clone(),
                    request.work_classification,
                ))
                .await?;
            Ok::<_, AppError>((working, identity, binding))
        }
        .await;

        match planned {
            Ok((member, assignment, binding)) => Ok(ManagedTeamAssignmentPlan {
                member,
                assignment,
                binding,
                reservation,
            }),
            Err(error) => {
                self.compensate_member_assignment(
                    task_service,
                    &member,
                    &request.delegated_session_id,
                    None,
                    &reservation,
                    "assignment_planning_failed",
                )
                .await;
                Err(error)
            }
        }
    }

    /// Revalidates the generation, task and planned binding directly before a
    /// provider launch. A stale runtime must never become a new process.
    pub async fn revalidate_member_assignment_launch(
        &self,
        plan: &ManagedTeamAssignmentPlan,
    ) -> AppResult<()> {
        let current = self
            .team_repo
            .get_member(&plan.member.id)
            .await?
            .ok_or_else(|| AppError::NotFound("managed Team member disappeared".to_string()))?;
        if !current.is_current_generation(plan.member.generation)
            || current.status != TeamMemberStatus::Working
            || current.current_assignment_id != Some(plan.assignment.assignment.id.clone())
            || current.current_run_id != Some(plan.binding.agent_run_id)
        {
            return Err(AppError::Conflict(
                "managed Team member changed before launch".to_string(),
            ));
        }
        let binding = self
            .run_binding_repo
            .get_by_id(&plan.binding.id)
            .await?
            .ok_or_else(|| {
                AppError::Conflict("planned Team run binding disappeared".to_string())
            })?;
        if binding.status != TeamRunBindingStatus::Planned
            || !binding.is_current_member_authority(&current.id, current.generation)
            || binding.assignment_id != Some(plan.assignment.assignment.id.clone())
        {
            return Err(AppError::Conflict(
                "planned Team run binding is no longer authoritative".to_string(),
            ));
        }
        Ok(())
    }

    pub async fn mark_member_assignment_launching(
        &self,
        plan: &ManagedTeamAssignmentPlan,
    ) -> AppResult<TeamRunBinding> {
        self.revalidate_member_assignment_launch(plan).await?;
        let session = self.admit_dispatch(&plan.member.team_id).await?;
        if self.configured_dispatch_count(&plan.member.team_id).await?
            >= session.configured_concurrency
        {
            return Err(AppError::Conflict(
                "managed Team configured concurrency is exhausted".to_string(),
            ));
        }
        let mut binding = self
            .run_binding_repo
            .get_by_id(&plan.binding.id)
            .await?
            .ok_or_else(|| {
                AppError::Conflict("planned Team run binding disappeared".to_string())
            })?;
        binding
            .transition_to(TeamRunBindingStatus::Launching, Utc::now())
            .map_err(AppError::Validation)?;
        binding.version += 1;
        if !self
            .run_binding_repo
            .transition(&binding.id, plan.binding.version, binding.clone())
            .await?
        {
            return Err(AppError::Conflict(
                "planned Team run binding changed before launch".to_string(),
            ));
        }
        Ok(binding)
    }

    pub async fn complete_member_assignment_launch(
        &self,
        task_service: &AgentTaskService,
        plan: &ManagedTeamAssignmentPlan,
    ) -> AppResult<AgentTaskAssignmentView> {
        let bound = task_service
            .bind_assignment_run(
                &plan.assignment.assignment.id,
                plan.member.delegated_session_id.as_ref().ok_or_else(|| {
                    AppError::Conflict("planned Team member has no delegated session".to_string())
                })?,
                &plan.binding.agent_run_id,
            )
            .await?
            .ok_or_else(|| {
                AppError::Conflict("planned Team task assignment disappeared".to_string())
            })?;
        let mut binding = self
            .run_binding_repo
            .get_by_id(&plan.binding.id)
            .await?
            .ok_or_else(|| {
                AppError::Conflict("launching Team run binding disappeared".to_string())
            })?;
        binding
            .transition_to(TeamRunBindingStatus::Running, Utc::now())
            .map_err(AppError::Validation)?;
        let expected = binding.version;
        binding.version += 1;
        if !self
            .run_binding_repo
            .transition(&binding.id.clone(), expected, binding)
            .await?
        {
            return Err(AppError::Conflict(
                "launching Team run binding changed before run binding".to_string(),
            ));
        }
        Ok(bound)
    }

    pub async fn fail_member_assignment_launch(
        &self,
        task_service: &AgentTaskService,
        plan: &ManagedTeamAssignmentPlan,
        reason: &str,
    ) {
        self.compensate_member_assignment(
            task_service,
            &plan.member,
            plan.member
                .delegated_session_id
                .as_ref()
                .expect("planned member has session"),
            Some(&plan.binding),
            &plan.reservation,
            reason,
        )
        .await;
    }

    pub async fn stop_member(
        &self,
        team_id: &TeamSessionId,
        member_name: &str,
        expected_generation: i64,
    ) -> AppResult<TeamMember> {
        let mut member = self.member_by_normalized_name(team_id, member_name).await?;
        if !member.is_current_generation(expected_generation) {
            return Err(AppError::Conflict(
                "managed Team member generation is stale".to_string(),
            ));
        }
        let current_generation = member.generation;
        if member.status != TeamMemberStatus::Stopped {
            member
                .transition_to(TeamMemberStatus::Stopping, Utc::now())
                .map_err(AppError::Validation)?;
            member
                .transition_to(TeamMemberStatus::Stopped, Utc::now())
                .map_err(AppError::Validation)?;
            member.current_run_id = None;
            member.current_assignment_id = None;
            if !self
                .team_repo
                .update_member(member.clone(), current_generation)
                .await?
            {
                return Err(AppError::Conflict(
                    "managed Team member changed before stop".to_string(),
                ));
            }
        }
        Ok(member)
    }

    /// Settles the Team-side projection after assignment recovery has reopened
    /// an orphaned caller task. All identity checks are generation scoped so a
    /// stale recovery cannot release a replacement member runtime.
    pub async fn recover_orphaned_member_assignment(
        &self,
        assignment: &AgentTaskAssignmentView,
        reason: &str,
    ) -> AppResult<()> {
        let (Some(team_id), Some(member_id), Some(generation)) = (
            assignment.assignment.team_id.as_ref(),
            assignment.assignment.team_member_id.as_ref(),
            assignment.assignment.team_member_generation,
        ) else {
            return Ok(());
        };
        let member = self.team_repo.get_member(member_id).await?.ok_or_else(|| {
            AppError::Conflict("Team assignment member disappeared during recovery".to_string())
        })?;
        if member.team_id != *team_id {
            return Err(AppError::Conflict(
                "Team assignment member belongs to a different Team".to_string(),
            ));
        }
        for reservation in self
            .reservation_repo
            .list_active_for_assignment(assignment.assignment.id.as_str())
            .await?
        {
            if reservation.team_member_id == *member_id
                && reservation.team_member_generation == generation
            {
                let _ = self
                    .reservation_repo
                    .release_if_current(
                        &reservation.id,
                        reservation.team_member_generation,
                        reservation.attempt_number,
                    )
                    .await?;
            }
        }
        let binding = match assignment.assignment.delegated_agent_run_id {
            Some(run_id) => self.run_binding_repo.get_by_agent_run_id(&run_id).await?,
            None => {
                self.run_binding_repo
                    .get_current_member_binding(member_id, generation)
                    .await?
            }
        };
        if let Some(mut binding) = binding {
            if binding.assignment_id == Some(assignment.assignment.id.clone())
                && matches!(
                    binding.status,
                    TeamRunBindingStatus::Planned
                        | TeamRunBindingStatus::Launching
                        | TeamRunBindingStatus::Running
                )
            {
                binding
                    .transition_to(TeamRunBindingStatus::Failed, Utc::now())
                    .map_err(AppError::Validation)?;
                binding.last_error = Some(reason.to_string());
                let expected = binding.version;
                binding.version += 1;
                if !self
                    .run_binding_repo
                    .transition(&binding.id.clone(), expected, binding)
                    .await?
                {
                    return Err(AppError::Conflict(
                        "Team run binding changed during orphan recovery".to_string(),
                    ));
                }
            }
        }
        if member.is_current_generation(generation)
            && member.current_assignment_id == Some(assignment.assignment.id.clone())
            && member.status == TeamMemberStatus::Working
        {
            let mut idle = member.clone();
            idle.transition_to(TeamMemberStatus::Idle, Utc::now())
                .map_err(AppError::Validation)?;
            idle.current_assignment_id = None;
            idle.current_run_id = None;
            idle.delegated_session_id = None;
            idle.last_error = Some(reason.to_string());
            if !self.team_repo.update_member(idle, generation).await? {
                return Err(AppError::Conflict(
                    "Team member changed during orphan recovery".to_string(),
                ));
            }
        }
        Ok(())
    }

    /// Applies normal terminal settlement to the Team projection after the
    /// task repository has settled the exact assignment. Solo assignments are
    /// ignored; Team rows are fenced by member generation and assignment id.
    pub async fn settle_member_assignment(
        &self,
        assignment: &AgentTaskAssignmentView,
        terminal_status: AgentTaskAssignmentTerminalStatus,
        reason: Option<&str>,
    ) -> AppResult<()> {
        let (Some(team_id), Some(member_id), Some(generation), Some(run_id)) = (
            assignment.assignment.team_id.as_ref(),
            assignment.assignment.team_member_id.as_ref(),
            assignment.assignment.team_member_generation,
            assignment.assignment.delegated_agent_run_id,
        ) else {
            return Ok(());
        };
        let member = self.team_repo.get_member(member_id).await?.ok_or_else(|| {
            AppError::Conflict("Team assignment member disappeared during settlement".to_string())
        })?;
        if member.team_id != *team_id {
            return Err(AppError::Conflict(
                "Team assignment member belongs to a different Team".to_string(),
            ));
        }
        for reservation in self
            .reservation_repo
            .list_active_for_assignment(assignment.assignment.id.as_str())
            .await?
        {
            if reservation.team_member_id == *member_id
                && reservation.team_member_generation == generation
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
                        "Team workspace reservation changed during settlement".to_string(),
                    ));
                }
            }
        }
        let binding = self
            .run_binding_repo
            .get_by_agent_run_id(&run_id)
            .await?
            .ok_or_else(|| {
                AppError::Conflict("Team run binding disappeared during settlement".to_string())
            })?;
        if binding.team_id != *team_id
            || binding.assignment_id != Some(assignment.assignment.id.clone())
            || !binding.is_current_member_authority(member_id, generation)
        {
            return Err(AppError::Conflict(
                "Team run binding is not authoritative for settled assignment".to_string(),
            ));
        }
        let next_binding_status = match terminal_status {
            AgentTaskAssignmentTerminalStatus::Completed => TeamRunBindingStatus::Terminal,
            AgentTaskAssignmentTerminalStatus::Failed => TeamRunBindingStatus::Failed,
            AgentTaskAssignmentTerminalStatus::Cancelled => TeamRunBindingStatus::Cancelled,
        };
        if binding.status == TeamRunBindingStatus::Running {
            let mut terminal = binding.clone();
            terminal
                .transition_to(next_binding_status, Utc::now())
                .map_err(AppError::Validation)?;
            terminal.last_error = reason.map(str::to_string);
            let expected = terminal.version;
            terminal.version += 1;
            if !self
                .run_binding_repo
                .transition(&terminal.id.clone(), expected, terminal)
                .await?
            {
                return Err(AppError::Conflict(
                    "Team run binding changed during settlement".to_string(),
                ));
            }
        } else if !matches!(
            binding.status,
            TeamRunBindingStatus::Terminal
                | TeamRunBindingStatus::Cancelled
                | TeamRunBindingStatus::Failed
        ) {
            return Err(AppError::Conflict(
                "Team run binding is not running or terminal for settled assignment".to_string(),
            ));
        }
        if member.is_current_generation(generation)
            && member.current_assignment_id == Some(assignment.assignment.id.clone())
            && member.status == TeamMemberStatus::Working
        {
            let mut idle = member.clone();
            idle.transition_to(TeamMemberStatus::Idle, Utc::now())
                .map_err(AppError::Validation)?;
            idle.current_assignment_id = None;
            idle.current_run_id = None;
            idle.delegated_session_id = None;
            idle.last_error = reason.map(str::to_string);
            if !self.team_repo.update_member(idle, generation).await? {
                return Err(AppError::Conflict(
                    "Team member changed during settlement".to_string(),
                ));
            }
        }
        Ok(())
    }

    async fn release_reservation(&self, reservation: &Option<TeamWorkspaceReservation>) {
        if let Some(reservation) = reservation {
            let _ = self
                .reservation_repo
                .release_if_current(
                    &reservation.id,
                    reservation.team_member_generation,
                    reservation.attempt_number,
                )
                .await;
        }
    }

    async fn compensate_member_assignment(
        &self,
        task_service: &AgentTaskService,
        original_member: &TeamMember,
        delegated_session_id: &DelegatedSessionId,
        binding: Option<&TeamRunBinding>,
        reservation: &Option<TeamWorkspaceReservation>,
        reason: &str,
    ) {
        if let Some(binding) = binding {
            if let Ok(Some(mut current)) = self.run_binding_repo.get_by_id(&binding.id).await {
                if matches!(
                    current.status,
                    TeamRunBindingStatus::Planned | TeamRunBindingStatus::Launching
                ) && current
                    .transition_to(TeamRunBindingStatus::Failed, Utc::now())
                    .is_ok()
                {
                    current.last_error = Some(reason.to_string());
                    let expected = current.version;
                    current.version += 1;
                    let _ = self
                        .run_binding_repo
                        .transition(&current.id.clone(), expected, current)
                        .await;
                }
            }
        }
        self.release_reservation(reservation).await;
        let settled_active = if let Some(binding) = binding {
            task_service
                .settle_assignment_for_run(
                    &binding.agent_run_id,
                    crate::domain::entities::AgentTaskAssignmentTerminalStatus::Failed,
                    Some(reason),
                )
                .await
                .ok()
                .flatten()
                .is_some()
        } else {
            false
        };
        if !settled_active {
            let _ = task_service
                .fail_reserved_assignment(delegated_session_id, reason)
                .await;
        }
        if let Ok(Some(mut member)) = self.team_repo.get_member(&original_member.id).await {
            if member.is_current_generation(original_member.generation)
                && member.status == TeamMemberStatus::Working
            {
                if member
                    .transition_to(TeamMemberStatus::Idle, Utc::now())
                    .is_ok()
                {
                    member.current_assignment_id = None;
                    member.current_run_id = None;
                    member.delegated_session_id = None;
                    member.last_error = Some(reason.to_string());
                    let _ = self
                        .team_repo
                        .update_member(member, original_member.generation)
                        .await;
                }
            }
        }
    }
}

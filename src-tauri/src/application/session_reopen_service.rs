// Service for reopening accepted/archived ideation sessions
// Delegates task cleanup to TaskCleanupService. Handles session-level ops:
// plan branch cleanup, proposal clearing, session status reset.

use std::path::PathBuf;
use std::sync::Arc;

use crate::application::app_state::AppState;
use crate::application::git_service::GitService;
use crate::application::task_cleanup_service::{StopMode, TaskCleanupService};
use crate::domain::entities::plan_branch::PlanBranchStatus;
use crate::domain::entities::{IdeationSessionId, IdeationSessionStatus, VerificationStatus};
use crate::domain::repositories::{
    ExecutionPlanRepository, IdeationSessionRepository, PlanBranchRepository, ProjectRepository,
    TaskProposalRepository, TaskRepository,
};
use crate::domain::services::{
    clear_verification_snapshot, load_current_verification_snapshot_or_default,
};
use crate::error::AppResult;
use crate::http_server::handlers::ideation::{ChildFilter, stop_and_archive_children};

pub struct SessionReopenService {
    task_repo: Arc<dyn TaskRepository>,
    task_proposal_repo: Arc<dyn TaskProposalRepository>,
    ideation_session_repo: Arc<dyn IdeationSessionRepository>,
    plan_branch_repo: Arc<dyn PlanBranchRepository>,
    project_repo: Arc<dyn ProjectRepository>,
    execution_plan_repo: Arc<dyn ExecutionPlanRepository>,
    task_cleanup_service: TaskCleanupService,
}

impl SessionReopenService {
    pub fn new(
        task_repo: Arc<dyn TaskRepository>,
        task_proposal_repo: Arc<dyn TaskProposalRepository>,
        ideation_session_repo: Arc<dyn IdeationSessionRepository>,
        plan_branch_repo: Arc<dyn PlanBranchRepository>,
        project_repo: Arc<dyn ProjectRepository>,
        execution_plan_repo: Arc<dyn ExecutionPlanRepository>,
        task_cleanup_service: TaskCleanupService,
    ) -> Self {
        Self {
            task_repo,
            task_proposal_repo,
            ideation_session_repo,
            plan_branch_repo,
            project_repo,
            execution_plan_repo,
            task_cleanup_service,
        }
    }

    /// Reopen an accepted/archived session back to Active.
    ///
    /// Cleanup order:
    /// 0. Stop agents + archive ALL active child sessions (before resetting parent)
    /// 1. Validate session is Accepted or Archived
    /// 2. Mark active ExecutionPlan as superseded (preserves history)
    /// 3. Get all tasks for this session
    /// 4. Delegate task cleanup to TaskCleanupService (stop agents, git cleanup, DB delete)
    /// 5. Clean plan branch: delete git branch + DB record (unblocks next accept's INSERT)
    /// 6. Clear created_task_id on all proposals
    /// 7. Reset acceptance-cycle fields (expected_proposal_count, dependencies_acknowledged, etc.)
    /// 8. Set session status to Active
    /// 9. Reset verification state
    pub async fn reopen(&self, session_id: &IdeationSessionId, app_state: &AppState) -> AppResult<()> {
        // 0. Stop and archive ALL active child sessions before resetting parent.
        //    Must happen first to avoid race with a running verification agent modifying the plan.
        //    Best-effort: errors are logged but do not block the reopen.
        stop_and_archive_children(session_id.as_str(), app_state, ChildFilter::AllChildren, true)
            .await
            .ok();

        // 1. Validate session is Accepted or Archived
        let session = self
            .ideation_session_repo
            .get_by_id(session_id)
            .await?
            .ok_or_else(|| {
                crate::error::AppError::NotFound(format!(
                    "Session not found: {}",
                    session_id.as_str()
                ))
            })?;

        match session.status {
            IdeationSessionStatus::Accepted | IdeationSessionStatus::Archived => {}
            _ => {
                return Err(crate::error::AppError::Validation(format!(
                    "Cannot reopen session in '{}' status. Only Accepted or Archived sessions can be reopened.",
                    session.status
                )));
            }
        }

        // 2. Mark the active ExecutionPlan as superseded before cleaning up tasks.
        //    This preserves history — the record stays for data integrity.
        if let Ok(Some(plan)) = self
            .execution_plan_repo
            .get_active_for_session(session_id)
            .await
        {
            let _ = self.execution_plan_repo.mark_superseded(&plan.id).await;
        }

        // 3. Get all tasks for this session
        let tasks = self.task_repo.get_by_ideation_session(session_id).await?;

        // 4. Cleanup tasks: stop agents (DirectStop), delete git resources, delete from DB
        //    No events emitted — the session-level event handles UI updates.
        let _report = self
            .task_cleanup_service
            .cleanup_tasks(&tasks, StopMode::DirectStop, false)
            .await;

        // 5. Clean plan branch: delete the physical git branch so the next accept can create a
        //    fresh one with a unique name. Keep the DB record for historical reference.
        if let Ok(Some(plan_branch)) = self.plan_branch_repo.get_by_session_id(session_id).await {
            if plan_branch.status == PlanBranchStatus::Active {
                if let Ok(Some(project)) = self.project_repo.get_by_id(&session.project_id).await {
                    let repo_path = PathBuf::from(&project.working_directory);
                    let _ = GitService::delete_feature_branch(&repo_path, &plan_branch.branch_name)
                        .await;
                }
                // Delete the DB record so the next accept can INSERT without hitting the UNIQUE INDEX.
                let _ = self.plan_branch_repo.delete(&plan_branch.id).await;
            }
        }

        // 6. Clear created_task_id on all proposals
        self.task_proposal_repo
            .clear_created_task_ids_by_session(session_id)
            .await?;

        // 7. Reset acceptance-cycle fields (expected_proposal_count, dependencies_acknowledged,
        //    auto_accept_status, auto_accept_started_at, cross_project_checked) so the next
        //    accept flow starts from a clean slate.
        self.ideation_session_repo
            .reset_acceptance_cycle_fields(session_id.as_str())
            .await?;

        // 8. Set session status to Active (clears archived_at/converted_at)
        self.ideation_session_repo
            .update_status(session_id, IdeationSessionStatus::Active)
            .await?;

        // 9. Reset the authoritative verification snapshot for the active generation.
        // A reopened session starts a fresh verification cycle, so both the session summary
        // and the current-generation snapshot must be cleared together.
        let mut snapshot = load_current_verification_snapshot_or_default(
            self.ideation_session_repo.as_ref(),
            &session,
            VerificationStatus::Unverified,
            false,
        )
        .await?;
        clear_verification_snapshot(&mut snapshot, VerificationStatus::Unverified, false);
        self.ideation_session_repo
            .save_verification_run_snapshot(session_id, &snapshot)
            .await?;

        // 10. Clear external activity phase so the session exits the archival filter.
        // A reopened session should not be treated as stale by the reconciler.
        self.ideation_session_repo
            .update_external_activity_phase(session_id, None)
            .await?;

        tracing::info!(session_id = session_id.as_str(), "Session reopened; verification state reset");

        Ok(())
    }
}

#[cfg(test)]
#[path = "session_reopen_service_tests.rs"]
mod tests;

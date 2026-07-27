// PlanBranch repository trait - domain layer abstraction
//
// Defines the contract for plan branch persistence.
// Implementations can use SQLite, in-memory, etc.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus};
use crate::domain::entities::{
    ArtifactId, ExecutionPlanId, IdeationSessionId, PlanBranch, PlanBranchId, PlanBranchStatus,
    ProjectId, TaskId,
};
use crate::error::AppResult;

/// Repository trait for PlanBranch persistence.
#[async_trait]
pub trait PlanBranchRepository: Send + Sync {
    /// Create a new plan branch record
    async fn create(&self, branch: PlanBranch) -> AppResult<PlanBranch>;

    /// Insert or update a plan branch record (upsert by session_id).
    /// If a row with the same session_id already exists, all mutable fields are updated.
    async fn create_or_update(&self, branch: PlanBranch) -> AppResult<PlanBranch>;

    /// Get a plan branch by its ID
    async fn get_by_id(&self, id: &PlanBranchId) -> AppResult<Option<PlanBranch>>;

    /// Get plan branches by plan artifact ID (multiple sessions can share the same artifact)
    async fn get_by_plan_artifact_id(&self, id: &ArtifactId) -> AppResult<Vec<PlanBranch>>;

    /// Get plan branch by execution plan ID (unique constraint)
    async fn get_by_execution_plan_id(&self, id: &ExecutionPlanId)
        -> AppResult<Option<PlanBranch>>;

    /// Get plan branch by session ID (unique constraint, primary lookup)
    async fn get_by_session_id(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Option<PlanBranch>>;

    /// Get plan branch by its merge task ID
    async fn get_by_merge_task_id(&self, task_id: &TaskId) -> AppResult<Option<PlanBranch>>;

    /// Get all plan branches for a project
    async fn get_by_project_id(&self, project_id: &ProjectId) -> AppResult<Vec<PlanBranch>>;

    /// Get active merge-task branches that may need startup PR creation or persisted PR metadata recovery.
    ///
    /// Candidates are PR-eligible or already have a persisted PR number.
    async fn get_startup_pr_recovery_candidates_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<PlanBranch>> {
        let branches = self.get_by_project_id(project_id).await?;
        Ok(branches
            .into_iter()
            .filter(|branch| {
                branch.status == PlanBranchStatus::Active
                    && branch.merge_task_id.is_some()
                    && (branch.pr_eligible || branch.pr_number.is_some())
            })
            .collect())
    }

    /// Get terminal plan branches that still need startup local-artifact cleanup.
    async fn get_terminal_local_cleanup_candidates_by_project_id(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<PlanBranch>> {
        let branches = self.get_by_project_id(project_id).await?;
        Ok(branches
            .into_iter()
            .filter(|branch| {
                branch.status == PlanBranchStatus::Merged
                    || matches!(branch.pr_status, Some(PrStatus::Merged))
            })
            .collect())
    }

    /// Mark startup local-artifact cleanup as no longer needing repeated launch checks.
    async fn mark_local_cleanup_status(
        &self,
        _id: &PlanBranchId,
        _status: &str,
        _checked_at: DateTime<Utc>,
    ) -> AppResult<()> {
        Ok(())
    }

    async fn get_local_cleanup_status(&self, _id: &PlanBranchId) -> AppResult<Option<String>> {
        Ok(None)
    }

    async fn clear_local_cleanup_status(&self, _id: &PlanBranchId) -> AppResult<()> {
        Ok(())
    }

    /// Update plan branch status
    async fn update_status(&self, id: &PlanBranchId, status: PlanBranchStatus) -> AppResult<()>;

    /// Update whether the plan branch is eligible for PR mode.
    async fn update_pr_eligible(&self, id: &PlanBranchId, enabled: bool) -> AppResult<()>;

    /// Set the merge task ID for a plan branch
    async fn set_merge_task_id(&self, id: &PlanBranchId, task_id: &TaskId) -> AppResult<()>;

    /// Clear the merge task ID for a plan branch (set to NULL)
    async fn clear_merge_task_id(&self, id: &PlanBranchId) -> AppResult<()>;

    /// Mark a plan branch as merged (sets status to Merged and merged_at timestamp)
    async fn set_merged(&self, id: &PlanBranchId) -> AppResult<()>;

    /// Abandon all active plan branches for a given plan artifact ID.
    /// Used during re-accept to mark old branches as abandoned before creating new ones.
    /// Returns the number of branches abandoned.
    async fn abandon_active_for_artifact(&self, artifact_id: &ArtifactId) -> AppResult<u32>;

    /// Delete a plan branch record
    async fn delete(&self, id: &PlanBranchId) -> AppResult<()>;

    /// Update PR info after PR creation
    async fn update_pr_info(
        &self,
        id: &PlanBranchId,
        pr_number: i64,
        pr_url: String,
        pr_status: PrStatus,
        pr_draft: bool,
    ) -> AppResult<()>;

    /// Clear PR info (reset to pre-PR state)
    async fn clear_pr_info(&self, id: &PlanBranchId) -> AppResult<()>;

    /// Update PR status only
    async fn update_pr_status(&self, id: &PlanBranchId, status: PrStatus) -> AppResult<()>;

    /// Set merge commit SHA after merge
    async fn set_merge_commit_sha(&self, id: &PlanBranchId, sha: String) -> AppResult<()>;

    /// Update last_polled_at timestamp
    async fn update_last_polled_at(
        &self,
        id: &PlanBranchId,
        polled_at: DateTime<Utc>,
    ) -> AppResult<()>;

    /// Clear pr_polling_active for all branches belonging to a task
    async fn clear_polling_active_by_task(&self, task_id: &TaskId) -> AppResult<()>;

    /// Find task IDs where pr_polling_active = true
    async fn find_pr_polling_task_ids(&self) -> AppResult<Vec<TaskId>>;

    /// Update pr_push_status only
    async fn update_pr_push_status(&self, id: &PlanBranchId, status: PrPushStatus)
        -> AppResult<()>;
}

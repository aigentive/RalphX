// SQL query constants for task repository operations

/// Standard task SELECT columns
pub(super) const TASK_COLUMNS: &str =
    "id, project_id, category, title, description, priority, internal_status, needs_review_point, source_proposal_id, plan_artifact_id, plan_blueprint_artifact_id, ideation_session_id, execution_plan_id, created_at, updated_at, started_at, completed_at, archived_at, blocked_reason, task_branch, task_branch_base_ref, task_branch_base_sha, worktree_path, merge_commit_sha, metadata, merge_pipeline_active";

/// Get task by ID
pub(super) const GET_BY_ID: &str =
    "SELECT id, project_id, category, title, description, priority, internal_status, needs_review_point, source_proposal_id, plan_artifact_id, plan_blueprint_artifact_id, ideation_session_id, execution_plan_id, created_at, updated_at, started_at, completed_at, archived_at, blocked_reason, task_branch, task_branch_base_ref, task_branch_base_sha, worktree_path, merge_commit_sha, metadata, merge_pipeline_active
     FROM tasks WHERE id = ?1";

/// Get tasks by project (sorted by priority and creation date)
pub(super) const GET_BY_PROJECT: &str =
    "SELECT id, project_id, category, title, description, priority, internal_status, needs_review_point, source_proposal_id, plan_artifact_id, plan_blueprint_artifact_id, ideation_session_id, execution_plan_id, created_at, updated_at, started_at, completed_at, archived_at, blocked_reason, task_branch, task_branch_base_ref, task_branch_base_sha, worktree_path, merge_commit_sha, metadata, merge_pipeline_active
     FROM tasks WHERE project_id = ?1
     ORDER BY priority DESC, created_at ASC";

/// Get tasks by ideation session ID (excludes archived tasks)
pub(super) const GET_BY_IDEATION_SESSION: &str =
    "SELECT id, project_id, category, title, description, priority, internal_status, needs_review_point, source_proposal_id, plan_artifact_id, plan_blueprint_artifact_id, ideation_session_id, execution_plan_id, created_at, updated_at, started_at, completed_at, archived_at, blocked_reason, task_branch, task_branch_base_ref, task_branch_base_sha, worktree_path, merge_commit_sha, metadata, merge_pipeline_active
     FROM tasks WHERE ideation_session_id = ?1 AND archived_at IS NULL
     ORDER BY created_at ASC";

/// Delete task query
pub(super) const DELETE_TASK: &str = "DELETE FROM tasks WHERE id = ?1";

/// Get the oldest Ready task across all projects (Phase 26 - Auto-Scheduler)
/// Returns the task with earliest created_at that is in Ready status and not archived
pub(super) const GET_OLDEST_READY_TASK: &str =
    "SELECT id, project_id, category, title, description, priority, internal_status, needs_review_point, source_proposal_id, plan_artifact_id, plan_blueprint_artifact_id, ideation_session_id, execution_plan_id, created_at, updated_at, started_at, completed_at, archived_at, blocked_reason, task_branch, task_branch_base_ref, task_branch_base_sha, worktree_path, merge_commit_sha, metadata, merge_pipeline_active
     FROM tasks
     WHERE internal_status = 'ready'
       AND archived_at IS NULL
     ORDER BY created_at ASC
     LIMIT 1";

/// Get Ready tasks across all projects (Phase 66 - Local Mode Enforcement)
/// Returns tasks ordered by created_at ASC with a limit
pub(super) const GET_OLDEST_READY_TASKS: &str =
    "SELECT id, project_id, category, title, description, priority, internal_status, needs_review_point, source_proposal_id, plan_artifact_id, plan_blueprint_artifact_id, ideation_session_id, execution_plan_id, created_at, updated_at, started_at, completed_at, archived_at, blocked_reason, task_branch, task_branch_base_ref, task_branch_base_sha, worktree_path, merge_commit_sha, metadata, merge_pipeline_active
     FROM tasks
     WHERE internal_status = 'ready'
       AND archived_at IS NULL
     ORDER BY created_at ASC
     LIMIT ?1";

/// Get Ready tasks older than a threshold (watchdog query)
/// Returns tasks where updated_at is before (now - threshold_secs) seconds ago,
/// ordered by updated_at ASC (oldest first). Used by the Ready-task watchdog.
/// Parameter ?1 is an ISO-8601 datetime cutoff (tasks with updated_at < ?1 are returned).
pub(super) const GET_STALE_READY_TASKS: &str =
    "SELECT id, project_id, category, title, description, priority, internal_status, needs_review_point, source_proposal_id, plan_artifact_id, plan_blueprint_artifact_id, ideation_session_id, execution_plan_id, created_at, updated_at, started_at, completed_at, archived_at, blocked_reason, task_branch, task_branch_base_ref, task_branch_base_sha, worktree_path, merge_commit_sha, metadata, merge_pipeline_active
     FROM tasks
     WHERE internal_status = 'ready'
       AND archived_at IS NULL
       AND updated_at < ?1
     ORDER BY updated_at ASC";

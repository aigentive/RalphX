// Plan branch commands for feature branch management
// Thin layer bridging frontend to plan branch repository and git operations

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

use crate::application::git_service::GitService;
use crate::application::AppState;
use crate::commands::branch_helpers::ensure_base_branch_exists;
use crate::domain::entities::{
    ArtifactId, IdeationSessionId, InternalStatus, PlanBranch, PlanBranchStatus, ProjectId, Task,
    TaskCategory, TaskId,
};

/// Response for plan branch queries
#[derive(Debug, Serialize)]
pub struct PlanBranchResponse {
    pub id: String,
    pub plan_artifact_id: String,
    pub session_id: String,
    pub project_id: String,
    pub branch_name: String,
    pub source_branch: String,
    pub status: String,
    pub merge_task_id: Option<String>,
    pub created_at: String,
    pub merged_at: Option<String>,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub pr_draft: Option<bool>,
    pub pr_push_status: Option<String>,
    pub pr_status: Option<String>,
    pub pr_polling_active: bool,
    pub pr_eligible: bool,
    pub merge_commit_sha: Option<String>,
    pub base_branch_override: Option<String>,
}

impl From<PlanBranch> for PlanBranchResponse {
    fn from(pb: PlanBranch) -> Self {
        Self {
            id: pb.id.as_str().to_string(),
            plan_artifact_id: pb.plan_artifact_id.as_str().to_string(),
            session_id: pb.session_id.as_str().to_string(),
            project_id: pb.project_id.as_str().to_string(),
            branch_name: pb.branch_name,
            source_branch: pb.source_branch,
            status: pb.status.to_db_string().to_string(),
            merge_task_id: pb.merge_task_id.map(|id| id.as_str().to_string()),
            created_at: pb.created_at.to_rfc3339(),
            merged_at: pb.merged_at.map(|dt| dt.to_rfc3339()),
            pr_number: pb.pr_number,
            pr_url: pb.pr_url,
            pr_draft: pb.pr_draft,
            pr_push_status: Some(pb.pr_push_status.to_db_string().to_string()),
            pr_status: pb.pr_status.as_ref().map(|s| s.to_db_string().to_string()),
            pr_polling_active: pb.pr_polling_active,
            pr_eligible: pb.pr_eligible,
            merge_commit_sha: pb.merge_commit_sha,
            base_branch_override: pb.base_branch_override,
        }
    }
}

/// Input for enable_feature_branch command
#[derive(Debug, Deserialize)]
pub struct EnableFeatureBranchInput {
    pub plan_artifact_id: String,
    pub session_id: String,
    pub project_id: String,
    /// Per-plan override for base branch (None = use project default)
    #[serde(default)]
    pub base_branch_override: Option<String>,
}

// ============================================================================
// Query Commands
// ============================================================================

/// Get plan branch by plan artifact ID or session ID
///
/// The frontend may pass either a real plan_artifact_id or a session_id
/// (graph uses session_id as fallback). Try session-first, then artifact.
#[tauri::command]
pub async fn get_plan_branch(
    plan_artifact_id: String,
    state: State<'_, AppState>,
) -> Result<Option<PlanBranchResponse>, String> {
    // Try as session_id first (common case: graph sends session_id as fallback)
    let session_id = IdeationSessionId::from_string(plan_artifact_id.clone());
    let by_session = state
        .plan_branch_repo
        .get_by_session_id(&session_id)
        .await
        .map_err(|e| e.to_string())?;
    if by_session.is_some() {
        return Ok(by_session.map(PlanBranchResponse::from));
    }

    // Fallback: try as plan_artifact_id (backward compat)
    // Returns Vec since multiple sessions can share the same artifact — pick active only
    let artifact_id = ArtifactId::from_string(plan_artifact_id);
    let branches = state
        .plan_branch_repo
        .get_by_plan_artifact_id(&artifact_id)
        .await
        .map_err(|e| e.to_string())?;
    // Filter to active branches only (defense-in-depth: merged/abandoned should not be returned)
    let active_branches: Vec<PlanBranch> = branches
        .into_iter()
        .filter(|b| b.status == PlanBranchStatus::Active)
        .collect();
    if active_branches.len() > 1 {
        tracing::warn!(
            "Multiple active plan branches found for artifact_id={}, returning first",
            artifact_id.as_str()
        );
    }
    Ok(active_branches
        .into_iter()
        .next()
        .map(PlanBranchResponse::from))
}

/// Get all plan branches for a project
///
/// Returns all feature branches associated with a project.
#[tauri::command]
pub async fn get_project_plan_branches(
    project_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<PlanBranchResponse>, String> {
    let project_id = ProjectId::from_string(project_id);

    state
        .plan_branch_repo
        .get_by_project_id(&project_id)
        .await
        .map(|branches| branches.into_iter().map(PlanBranchResponse::from).collect())
        .map_err(|e| e.to_string())
}

/// Get plan branch by merge task ID
///
/// Used by detail views to fetch PR status for the current merge task.
#[tauri::command]
pub async fn get_plan_branch_by_task_id(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<Option<PlanBranchResponse>, String> {
    let task_id = TaskId::from_string(task_id);
    state
        .plan_branch_repo
        .get_by_merge_task_id(&task_id)
        .await
        .map(|opt| opt.map(PlanBranchResponse::from))
        .map_err(|e| e.to_string())
}

// ============================================================================
// Mutation Commands
// ============================================================================

/// Enable feature branch for a plan (mid-plan conversion)
///
/// Creates a git feature branch, DB record, and merge task with dependencies
/// on all unmerged plan tasks. Used when enabling feature branches after
/// some tasks may have already been created/merged.
#[tauri::command]
pub async fn enable_feature_branch(
    input: EnableFeatureBranchInput,
    state: State<'_, AppState>,
) -> Result<PlanBranchResponse, String> {
    let plan_artifact_id = ArtifactId::from_string(input.plan_artifact_id);
    let session_id = IdeationSessionId::from_string(input.session_id);
    let project_id = ProjectId::from_string(input.project_id);

    // Check if a feature branch already exists for this plan (session-first lookup)
    let existing = state
        .plan_branch_repo
        .get_by_session_id(&session_id)
        .await
        .map_err(|e| e.to_string())?;
    if existing.is_some() {
        return Err("Feature branch already exists for this plan".to_string());
    }

    // Get the project for base branch and working directory
    let project = state
        .project_repo
        .get_by_id(&project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project not found: {}", project_id.as_str()))?;

    let base_branch = input.base_branch_override.clone().unwrap_or_else(|| {
        project.base_branch.as_deref().unwrap_or("main").to_string()
    });
    let repo_path = PathBuf::from(&project.working_directory);

    // Ensure base branch exists, auto-creating from project default if needed
    let was_created =
        ensure_base_branch_exists(&repo_path, &base_branch, project.base_branch.as_deref())
            .await?;
    if was_created {
        tracing::info!(
            "Auto-created base branch '{}' from project default for enable_feature_branch",
            base_branch
        );
    }

    // Generate branch name: ralphx/{project-slug}/plan-{short-artifact-id}
    let project_slug = slug_from_name(&project.name);
    let short_id = &plan_artifact_id.as_str()[..8.min(plan_artifact_id.as_str().len())];
    let branch_name = format!("ralphx/{}/plan-{}", project_slug, short_id);

    // Create git feature branch from base branch
    GitService::create_feature_branch(&repo_path, &branch_name, &base_branch)
        .await
        .map_err(|e| format!("Failed to create feature branch: {}", e))?;

    // Insert plan_branches DB record
    let mut plan_branch = PlanBranch::new(
        plan_artifact_id.clone(),
        session_id.clone(),
        project_id.clone(),
        branch_name,
        base_branch.clone(),
    );
    plan_branch.base_branch_override = input.base_branch_override.clone();
    let created_branch = state
        .plan_branch_repo
        .create_or_update(plan_branch)
        .await
        .map_err(|e| format!("Failed to create plan branch record: {}", e))?;

    // Find all unmerged plan tasks for this plan artifact.
    // Also find tasks linked via session proposals whose plan_artifact_id is NULL
    // (created before the session_id fallback fix), and backfill their plan_artifact_id.
    let all_tasks = state
        .task_repo
        .get_by_project(&project_id)
        .await
        .map_err(|e| e.to_string())?;

    // Collect task IDs from session proposals (for tasks with NULL plan_artifact_id)
    let proposals = state
        .task_proposal_repo
        .get_by_session(&session_id)
        .await
        .map_err(|e| e.to_string())?;
    let session_task_ids: std::collections::HashSet<TaskId> = proposals
        .iter()
        .filter_map(|p| p.created_task_id.clone())
        .collect();

    // Backfill plan_artifact_id and ideation_session_id on tasks created from session proposals
    for task in &all_tasks {
        if session_task_ids.contains(&task.id)
            && (task.plan_artifact_id.is_none() || task.ideation_session_id.is_none())
        {
            let mut task_to_update = task.clone();
            if task_to_update.plan_artifact_id.is_none() {
                task_to_update.plan_artifact_id = Some(plan_artifact_id.clone());
            }
            if task_to_update.ideation_session_id.is_none() {
                task_to_update.ideation_session_id = Some(session_id.clone());
            }
            state
                .task_repo
                .update(&task_to_update)
                .await
                .map_err(|e| format!("Failed to backfill task fields: {}", e))?;
        }
    }

    let unmerged_plan_tasks: Vec<&Task> = all_tasks
        .iter()
        .filter(|t| {
            let matches_plan = t.plan_artifact_id.as_ref() == Some(&plan_artifact_id)
                || (t.plan_artifact_id.is_none() && session_task_ids.contains(&t.id));
            matches_plan
                && t.internal_status != InternalStatus::Merged
                && t.internal_status != InternalStatus::Approved
        })
        .collect();

    // Create merge task
    let plan_title = match state.ideation_session_repo.get_by_id(&session_id).await {
        Ok(Some(session)) if session.title.as_ref().is_some_and(|t| !t.trim().is_empty()) => {
            session.title.unwrap()
        }
        _ => format!("Merge plan into {}", base_branch),
    };
    let mut merge_task =
        Task::new_with_category(project_id.clone(), plan_title, TaskCategory::PlanMerge);
    merge_task.description = Some(format!(
        "Auto-created merge task: merges feature branch into {}",
        base_branch
    ));
    merge_task.plan_artifact_id = Some(plan_artifact_id);
    merge_task.ideation_session_id = Some(session_id.clone());
    merge_task.internal_status = InternalStatus::Blocked;
    merge_task.blocked_reason = Some("Waiting for all plan tasks to complete".to_string());

    let created_merge_task = state
        .task_repo
        .create(merge_task)
        .await
        .map_err(|e| format!("Failed to create merge task: {}", e))?;

    // Add blockedBy dependencies: merge task blocked by all unmerged plan tasks
    for plan_task in &unmerged_plan_tasks {
        state
            .task_dependency_repo
            .add_dependency(&created_merge_task.id, &plan_task.id)
            .await
            .map_err(|e| format!("Failed to add dependency: {}", e))?;
    }

    // Set merge_task_id on the plan branch record
    state
        .plan_branch_repo
        .set_merge_task_id(&created_branch.id, &created_merge_task.id)
        .await
        .map_err(|e| format!("Failed to set merge task ID: {}", e))?;

    // Re-fetch the plan branch to get updated data (session-first lookup)
    let updated_branch = state
        .plan_branch_repo
        .get_by_session_id(&session_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Plan branch not found after creation".to_string())?;

    Ok(PlanBranchResponse::from(updated_branch))
}

// ============================================================================
// Helpers
// ============================================================================

#[cfg(test)]
#[path = "plan_branch_commands_tests.rs"]
mod tests;

/// Generate a URL-safe slug from a project name
pub fn slug_from_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

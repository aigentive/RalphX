use std::path::{Path, PathBuf};

use crate::application::agent_conversation_workspace::{
    agent_conversation_branch_name, resolve_agent_conversation_workspace_path,
};
use crate::application::git_service::GitService;
use crate::domain::entities::{
    plan_branch::PrStatus, AgentConversationWorkspace, PlanBranch, PlanBranchStatus, Project,
};
use crate::domain::state_machine::transition_handler::resolve_plan_branch_pr_base;
use crate::error::{AppError, AppResult};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct LocalGitArtifactCleanupReport {
    pub worktree_removed: bool,
    pub branch_deleted: bool,
    pub skipped_reason: Option<String>,
}

pub(crate) async fn cleanup_merged_plan_branch_local_artifacts(
    project: &Project,
    plan_branch: &PlanBranch,
) -> AppResult<LocalGitArtifactCleanupReport> {
    if plan_branch.status != PlanBranchStatus::Merged
        && !matches!(plan_branch.pr_status, Some(PrStatus::Merged))
    {
        return Ok(LocalGitArtifactCleanupReport {
            skipped_reason: Some("plan_branch_not_merged".to_string()),
            ..LocalGitArtifactCleanupReport::default()
        });
    }

    let repo_path = Path::new(&project.working_directory);
    if !is_ralphx_plan_branch_name(&plan_branch.branch_name) {
        return Ok(LocalGitArtifactCleanupReport {
            skipped_reason: Some("branch_not_ralphx_owned".to_string()),
            ..LocalGitArtifactCleanupReport::default()
        });
    }

    let target_ref = resolve_plan_branch_pr_base(project, plan_branch);
    delete_branch_when_merged_or_equivalent(repo_path, &plan_branch.branch_name, &target_ref).await
}

pub(crate) async fn cleanup_terminal_agent_workspace_local_artifacts(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    delete_branch_if_merged: bool,
) -> AppResult<LocalGitArtifactCleanupReport> {
    let repo_path = Path::new(&project.working_directory);
    let expected_path =
        resolve_agent_conversation_workspace_path(project, &workspace.conversation_id)?;
    let stored_path = PathBuf::from(&workspace.worktree_path);
    if stored_path != expected_path {
        return Ok(LocalGitArtifactCleanupReport {
            skipped_reason: Some("workspace_path_mismatch".to_string()),
            ..LocalGitArtifactCleanupReport::default()
        });
    }

    let project_root = PathBuf::from(&project.working_directory);
    if expected_path == project_root {
        return Ok(LocalGitArtifactCleanupReport {
            skipped_reason: Some("workspace_points_to_project_root".to_string()),
            ..LocalGitArtifactCleanupReport::default()
        });
    }

    let mut report = LocalGitArtifactCleanupReport::default();
    remove_clean_agent_worktree(repo_path, &expected_path, &mut report).await?;

    if delete_branch_if_merged && report.skipped_reason.is_none() {
        if !is_expected_agent_workspace_branch(project, workspace) {
            report.skipped_reason = Some("branch_not_ralphx_owned".to_string());
            return Ok(report);
        }

        let branch_report = delete_branch_when_merged_or_equivalent(
            repo_path,
            &workspace.branch_name,
            &workspace.base_ref,
        )
        .await?;
        report.branch_deleted = branch_report.branch_deleted;
        if report.skipped_reason.is_none() {
            report.skipped_reason = branch_report.skipped_reason;
        }
    }

    Ok(report)
}

async fn remove_clean_agent_worktree(
    repo_path: &Path,
    expected_path: &Path,
    report: &mut LocalGitArtifactCleanupReport,
) -> AppResult<()> {
    let safe_path = crate::utils::path_safety::validate_absolute_non_root_path(
        expected_path,
        "agent workspace cleanup",
    )?;
    if !safe_path.exists() {
        GitService::delete_worktree(repo_path, &safe_path).await?;
        return Ok(());
    }

    if !safe_path.is_dir() {
        report.skipped_reason = Some("workspace_path_not_directory".to_string());
        return Ok(());
    }

    if GitService::has_uncommitted_changes(&safe_path).await? {
        report.skipped_reason = Some("workspace_has_uncommitted_changes".to_string());
        return Ok(());
    }

    GitService::delete_worktree(repo_path, &safe_path).await?;
    report.worktree_removed = true;
    Ok(())
}

async fn delete_branch_when_merged_or_equivalent(
    repo_path: &Path,
    branch: &str,
    target_ref: &str,
) -> AppResult<LocalGitArtifactCleanupReport> {
    let mut report = LocalGitArtifactCleanupReport::default();

    if !GitService::branch_exists(repo_path, branch).await? {
        report.skipped_reason = Some("branch_missing".to_string());
        return Ok(report);
    }

    let Some(existing_target_ref) =
        resolve_existing_cleanup_target_ref(repo_path, target_ref).await?
    else {
        report.skipped_reason = Some(format!("target_ref_missing:{target_ref}"));
        return Ok(report);
    };

    let (safe_to_delete, reason) =
        GitService::is_branch_merged_or_content_equivalent(repo_path, branch, &existing_target_ref)
            .await;
    if !safe_to_delete {
        report.skipped_reason = Some(format!("branch_not_merged:{reason}"));
        return Ok(report);
    }

    GitService::delete_branch(repo_path, branch, true)
        .await
        .map_err(|error| {
            AppError::GitOperation(format!(
                "Failed to delete local branch '{branch}' after terminal PR cleanup: {error}"
            ))
        })?;
    report.branch_deleted = true;
    Ok(report)
}

async fn resolve_existing_cleanup_target_ref(
    repo_path: &Path,
    target_ref: &str,
) -> AppResult<Option<String>> {
    if !target_ref.starts_with("origin/") {
        let remote_ref = format!("origin/{target_ref}");
        if GitService::ref_exists(repo_path, &remote_ref).await? {
            return Ok(Some(remote_ref));
        }
    }

    if GitService::ref_exists(repo_path, target_ref).await? {
        return Ok(Some(target_ref.to_string()));
    }

    Ok(None)
}

fn is_ralphx_plan_branch_name(branch: &str) -> bool {
    let mut parts = branch.split('/');
    let Some(namespace) = parts.next() else {
        return false;
    };
    let Some(project_slug) = parts.next() else {
        return false;
    };
    let Some(branch_leaf) = parts.next() else {
        return false;
    };

    namespace == "ralphx"
        && !project_slug.is_empty()
        && branch_leaf
            .strip_prefix("plan-")
            .is_some_and(|suffix| !suffix.is_empty())
        && parts.next().is_none()
}

fn is_expected_agent_workspace_branch(
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> bool {
    let canonical_branch = agent_conversation_branch_name(project, &workspace.conversation_id);
    if workspace.branch_name == canonical_branch {
        return true;
    }

    let continuation_prefix = format!("{canonical_branch}-");
    workspace
        .branch_name
        .strip_prefix(&continuation_prefix)
        .is_some_and(|suffix| !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit()))
}

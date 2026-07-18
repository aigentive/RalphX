use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::application::agent_conversation_workspace::{
    expand_worktree_parent_public, is_expected_agent_conversation_branch_name,
    resolve_agent_conversation_project_workspace_dir, resolve_agent_conversation_workspace_path,
    resolve_linked_plan_branch_agent_worktree_path, validate_workspace_linked_plan_branch,
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

pub(crate) const LOCAL_CLEANUP_STATUS_CLEANED: &str = "cleaned";
pub(crate) const LOCAL_CLEANUP_STATUS_BRANCH_MISSING: &str = "branch_missing";
pub(crate) const LOCAL_CLEANUP_STATUS_BRANCH_PRESERVED_NON_OWNED: &str =
    "branch_preserved_non_owned";
pub(crate) const LOCAL_CLEANUP_STATUS_WORKSPACE_DIRTY: &str = "workspace_dirty";
pub(crate) const LOCAL_CLEANUP_STATUS_UNSAFE: &str = "unsafe";
pub(crate) const LOCAL_CLEANUP_STATUS_TARGET_REF_MISSING: &str = "target_ref_missing";
pub(crate) const LOCAL_CLEANUP_STATUS_FAILED_UNSAFE: &str = "failed_unsafe";
pub(crate) const LOCAL_CLEANUP_STATUS_FAILED_OPERATIONAL: &str = "failed_operational";

pub(crate) fn terminal_plan_branch_cleanup_marker_for_report(
    report: &LocalGitArtifactCleanupReport,
) -> Option<&'static str> {
    terminal_cleanup_marker_for_report(report, false)
}

fn terminal_cleanup_marker_for_report(
    report: &LocalGitArtifactCleanupReport,
    branch_cleanup_not_required: bool,
) -> Option<&'static str> {
    match report.skipped_reason.as_deref() {
        Some("branch_missing") => Some(LOCAL_CLEANUP_STATUS_BRANCH_MISSING),
        Some("branch_not_ralphx_owned") => Some(LOCAL_CLEANUP_STATUS_BRANCH_PRESERVED_NON_OWNED),
        Some("workspace_has_uncommitted_changes") => Some(LOCAL_CLEANUP_STATUS_WORKSPACE_DIRTY),
        Some(reason) if reason.starts_with("branch_not_merged:") => {
            Some(LOCAL_CLEANUP_STATUS_UNSAFE)
        }
        Some(reason) if reason.starts_with("target_ref_missing:") => {
            Some(LOCAL_CLEANUP_STATUS_TARGET_REF_MISSING)
        }
        Some("plan_branch_not_merged")
        | Some("workspace_path_mismatch")
        | Some("workspace_path_not_directory")
        | Some("workspace_points_to_project_root") => Some(LOCAL_CLEANUP_STATUS_UNSAFE),
        Some(_) => None,
        None if report.branch_deleted || report.worktree_removed || branch_cleanup_not_required => {
            Some(LOCAL_CLEANUP_STATUS_CLEANED)
        }
        None => None,
    }
}

pub(crate) async fn cleanup_merged_plan_branch_local_artifacts(
    project: &Project,
    plan_branch: &PlanBranch,
) -> AppResult<LocalGitArtifactCleanupReport> {
    cleanup_merged_plan_branch_local_artifacts_with_known_local_branches(project, plan_branch, None)
        .await
}

pub(crate) async fn cleanup_merged_plan_branch_local_artifacts_with_known_local_branches(
    project: &Project,
    plan_branch: &PlanBranch,
    known_local_branches: Option<&HashSet<String>>,
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
    delete_branch_when_merged_or_equivalent(
        repo_path,
        &plan_branch.branch_name,
        &target_ref,
        known_local_branches,
    )
    .await
}

pub(crate) async fn cleanup_terminal_agent_workspace_local_artifacts(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    delete_branch_if_merged: bool,
) -> AppResult<LocalGitArtifactCleanupReport> {
    cleanup_terminal_agent_workspace_local_artifacts_with_known_local_branches(
        project,
        workspace,
        delete_branch_if_merged,
        None,
    )
    .await
}

pub(crate) async fn cleanup_terminal_agent_workspace_local_artifacts_with_known_local_branches(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    _delete_branch_if_merged: bool,
    known_local_branches: Option<&HashSet<String>>,
) -> AppResult<LocalGitArtifactCleanupReport> {
    let repo_path = crate::utils::path_safety::validate_absolute_non_root_path(
        Path::new(&project.working_directory),
        "project checkout",
    )?;
    let expected_path =
        resolve_agent_conversation_workspace_path(project, &workspace.conversation_id)?;
    let stored_path = PathBuf::from(&workspace.worktree_path);
    if stored_path != expected_path {
        return Ok(LocalGitArtifactCleanupReport {
            skipped_reason: Some("workspace_path_mismatch".to_string()),
            ..LocalGitArtifactCleanupReport::default()
        });
    }

    if expected_path == repo_path {
        return Ok(LocalGitArtifactCleanupReport {
            skipped_reason: Some("workspace_points_to_project_root".to_string()),
            ..LocalGitArtifactCleanupReport::default()
        });
    }

    if !is_expected_agent_workspace_branch(project, workspace) {
        return Ok(LocalGitArtifactCleanupReport {
            skipped_reason: Some("branch_not_ralphx_owned".to_string()),
            ..LocalGitArtifactCleanupReport::default()
        });
    }

    cleanup_force_owned_terminal_artifacts(
        project,
        &repo_path,
        &expected_path,
        &workspace.branch_name,
        known_local_branches,
    )
    .await
}

pub(crate) async fn cleanup_terminal_linked_plan_branch_local_artifacts(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    plan_branch: &PlanBranch,
) -> AppResult<LocalGitArtifactCleanupReport> {
    if let Err(error) = validate_workspace_linked_plan_branch(project, workspace, plan_branch) {
        return Ok(LocalGitArtifactCleanupReport {
            skipped_reason: Some(format!("linked_plan_identity_mismatch:{error}")),
            ..LocalGitArtifactCleanupReport::default()
        });
    }
    if !is_ralphx_plan_branch_name(&plan_branch.branch_name) {
        return Ok(LocalGitArtifactCleanupReport {
            skipped_reason: Some("branch_not_ralphx_owned".to_string()),
            ..LocalGitArtifactCleanupReport::default()
        });
    }

    let repo_path = crate::utils::path_safety::validate_absolute_non_root_path(
        Path::new(&project.working_directory),
        "project checkout",
    )?;
    let expected_path = resolve_linked_plan_branch_agent_worktree_path(project, plan_branch)?;
    cleanup_force_owned_terminal_artifacts(
        project,
        &repo_path,
        &expected_path,
        &plan_branch.branch_name,
        None,
    )
    .await
}

async fn cleanup_force_owned_terminal_artifacts(
    project: &Project,
    repo_path: &Path,
    expected_path: &Path,
    branch_name: &str,
    known_local_branches: Option<&HashSet<String>>,
) -> AppResult<LocalGitArtifactCleanupReport> {
    let project_workspace_dir = resolve_agent_conversation_project_workspace_dir(project)?;
    let worktree_parent = expand_worktree_parent_public(project.worktree_parent_or_default())?;
    if expected_path == project_workspace_dir
        || expected_path == worktree_parent
        || !expected_path.starts_with(&project_workspace_dir)
    {
        return Ok(LocalGitArtifactCleanupReport {
            skipped_reason: Some("workspace_path_outside_ralphx_project".to_string()),
            ..LocalGitArtifactCleanupReport::default()
        });
    }
    if !GitService::check_ref_format(repo_path, branch_name).await? {
        return Ok(LocalGitArtifactCleanupReport {
            skipped_reason: Some("branch_ref_invalid".to_string()),
            ..LocalGitArtifactCleanupReport::default()
        });
    }

    let mut report = LocalGitArtifactCleanupReport::default();
    let safe_path = crate::utils::path_safety::validate_absolute_non_root_path(
        expected_path,
        "agent workspace cleanup",
    )?;
    let metadata = match std::fs::symlink_metadata(&safe_path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(AppError::Infrastructure(format!(
                "Failed to inspect workspace cleanup target {}: {error}",
                safe_path.display()
            )));
        }
    };
    if let Some(metadata) = metadata {
        if metadata.file_type().is_symlink() {
            report.skipped_reason = Some("workspace_path_symlink".to_string());
            return Ok(report);
        }
        let canonical_project_workspace_dir =
            project_workspace_dir.canonicalize().map_err(|error| {
                AppError::Infrastructure(format!(
                    "Failed to canonicalize RalphX project workspace directory {}: {error}",
                    project_workspace_dir.display()
                ))
            })?;
        let canonical_safe_path = safe_path.canonicalize().map_err(|error| {
            AppError::Infrastructure(format!(
                "Failed to canonicalize workspace cleanup target {}: {error}",
                safe_path.display()
            ))
        })?;
        if canonical_safe_path == canonical_project_workspace_dir
            || !canonical_safe_path.starts_with(&canonical_project_workspace_dir)
        {
            report.skipped_reason = Some("workspace_path_canonical_escape".to_string());
            return Ok(report);
        }
        if !safe_path.is_dir() {
            report.skipped_reason = Some("workspace_path_not_directory".to_string());
            return Ok(report);
        }

        let safe_path_for_compare = safe_path
            .canonicalize()
            .unwrap_or_else(|_| safe_path.clone());
        let is_registered_worktree =
            GitService::list_worktrees(repo_path)
                .await?
                .iter()
                .any(|worktree| {
                    let worktree_path = PathBuf::from(&worktree.path);
                    worktree_path.canonicalize().unwrap_or(worktree_path) == safe_path_for_compare
                });

        if is_registered_worktree {
            GitService::delete_worktree(repo_path, &safe_path).await?;
        } else {
            crate::utils::path_safety::checked_remove_dir_all(&safe_path, "agent workspace")
                .await?;
        }
        report.worktree_removed = true;
    }

    let branch_exists = match known_local_branches {
        Some(local_branches) => local_branches.contains(branch_name),
        None => GitService::branch_exists(repo_path, branch_name).await?,
    };
    if branch_exists {
        GitService::delete_branch(repo_path, branch_name, true)
            .await
            .map_err(|error| {
                AppError::GitOperation(format!(
                    "Failed to force-delete verified terminal local branch '{branch_name}': {error}"
                ))
            })?;
        report.branch_deleted = true;
    }

    Ok(report)
}

async fn delete_branch_when_merged_or_equivalent(
    repo_path: &Path,
    branch: &str,
    target_ref: &str,
    known_local_branches: Option<&HashSet<String>>,
) -> AppResult<LocalGitArtifactCleanupReport> {
    let mut report = LocalGitArtifactCleanupReport::default();

    let branch_exists = match known_local_branches {
        Some(local_branches) => local_branches.contains(branch),
        None => GitService::branch_exists(repo_path, branch).await?,
    };

    if !branch_exists {
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
    is_expected_agent_conversation_branch_name(
        project,
        &workspace.conversation_id,
        &workspace.branch_name,
    )
}

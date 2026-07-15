use std::fmt;
use std::path::{Path, PathBuf};

use crate::application::agent_conversation_workspace::{
    ensure_linked_plan_branch_agent_worktree,
    resolve_agent_conversation_workspace_path_from_record_identity,
    resolve_linked_plan_branch_agent_worktree_path, validate_workspace_linked_plan_branch,
};
use crate::application::git_artifact_cleanup::LOCAL_CLEANUP_STATUS_CLEANED;
use crate::application::GitService;
use crate::domain::entities::plan_branch::PrStatus;
use crate::domain::entities::{AgentConversationWorkspace, PlanBranch, PlanBranchStatus, Project};
use crate::error::{AppError, AppResult};
use crate::utils::path_safety::validate_absolute_non_root_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestartWorkspaceCleanupProof {
    None,
    OwnedMergedCleanup,
}

pub(crate) fn resolve_restart_workspace_cleanup_proof(
    workspace: &AgentConversationWorkspace,
    workspace_cleanup_status: Option<&str>,
    plan_branch: &PlanBranch,
    plan_branch_cleanup_status: Option<&str>,
) -> RestartWorkspaceCleanupProof {
    let workspace_cleanup_owned = workspace.publication_pr_status.as_deref() == Some("merged")
        && workspace_cleanup_status == Some(LOCAL_CLEANUP_STATUS_CLEANED);
    let plan_cleanup_owned = (plan_branch.status == PlanBranchStatus::Merged
        || matches!(plan_branch.pr_status, Some(PrStatus::Merged)))
        && plan_branch_cleanup_status == Some(LOCAL_CLEANUP_STATUS_CLEANED);
    if workspace_cleanup_owned || plan_cleanup_owned {
        RestartWorkspaceCleanupProof::OwnedMergedCleanup
    } else {
        RestartWorkspaceCleanupProof::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestartWorkspacePreparationSource {
    ExistingLinked,
    RelocatedConversation,
    ReattachedBranch,
    RecreatedFromCleanup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RestartWorkspacePreparation {
    pub path: PathBuf,
    pub source: RestartWorkspacePreparationSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RestartWorkspacePreparationError {
    UnsafeOwnership { detail: String },
    MissingBranchWithoutCleanupProof,
    Operation { detail: String },
}

impl RestartWorkspacePreparationError {
    fn unsafe_ownership(error: AppError) -> Self {
        Self::UnsafeOwnership {
            detail: error.to_string(),
        }
    }

    fn operation(error: AppError) -> Self {
        Self::Operation {
            detail: error.to_string(),
        }
    }

    pub(crate) fn into_app_error(self) -> AppError {
        tracing::warn!(detail = %self, "Restart workspace preparation failed");
        let message = match self {
            Self::UnsafeOwnership { .. } => {
                "RalphX could not safely restore this implementation workspace because the linked branch is checked out elsewhere or no longer matches the plan"
            }
            Self::MissingBranchWithoutCleanupProof => {
                "RalphX could not safely restore this implementation workspace because ownership of the missing branch could not be verified"
            }
            Self::Operation { .. } => {
                "RalphX could not restore this implementation workspace. Check Git access and try again"
            }
        };
        AppError::Validation(message.to_string())
    }
}

impl fmt::Display for RestartWorkspacePreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsafeOwnership { detail } => write!(formatter, "unsafe ownership: {detail}"),
            Self::MissingBranchWithoutCleanupProof => {
                write!(formatter, "missing branch has no owned cleanup proof")
            }
            Self::Operation { detail } => write!(formatter, "operation failed: {detail}"),
        }
    }
}

/// Prepare the linked implementation worktree without touching unknown owners.
pub(crate) async fn prepare_linked_plan_branch_agent_worktree_for_restart(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    plan_branch: &PlanBranch,
    origin_base_ref: &str,
    cleanup_proof: RestartWorkspaceCleanupProof,
) -> Result<RestartWorkspacePreparation, RestartWorkspacePreparationError> {
    validate_workspace_linked_plan_branch(project, workspace, plan_branch)
        .map_err(RestartWorkspacePreparationError::unsafe_ownership)?;
    let linked_worktree_path = resolve_linked_plan_branch_agent_worktree_path(project, plan_branch)
        .map_err(RestartWorkspacePreparationError::unsafe_ownership)?;
    if linked_worktree_path.exists() {
        let path = ensure_linked_plan_branch_agent_worktree(project, plan_branch)
            .await
            .map_err(RestartWorkspacePreparationError::unsafe_ownership)?;
        return Ok(RestartWorkspacePreparation {
            path,
            source: RestartWorkspacePreparationSource::ExistingLinked,
        });
    }

    let conversation_worktree_path =
        resolve_agent_conversation_workspace_path_from_record_identity(project, workspace)
            .map_err(RestartWorkspacePreparationError::unsafe_ownership)?;
    if conversation_worktree_path.exists() {
        let checked_out = GitService::get_current_branch(&conversation_worktree_path)
            .await
            .map_err(RestartWorkspacePreparationError::unsafe_ownership)?;
        if checked_out != plan_branch.branch_name {
            return Err(RestartWorkspacePreparationError::UnsafeOwnership {
                detail: format!(
                    "owned conversation worktree branch '{}' does not match '{}'",
                    checked_out, plan_branch.branch_name
                ),
            });
        }

        let project_root = validated_project_root(project)
            .map_err(RestartWorkspacePreparationError::unsafe_ownership)?;
        GitService::move_worktree(
            &project_root,
            &conversation_worktree_path,
            &linked_worktree_path,
        )
        .await
        .map_err(RestartWorkspacePreparationError::operation)?;
        return Ok(RestartWorkspacePreparation {
            path: linked_worktree_path,
            source: RestartWorkspacePreparationSource::RelocatedConversation,
        });
    }

    let project_root = validated_project_root(project)
        .map_err(RestartWorkspacePreparationError::unsafe_ownership)?;
    if GitService::branch_exists_strict(&project_root, &plan_branch.branch_name)
        .await
        .map_err(RestartWorkspacePreparationError::operation)?
    {
        GitService::checkout_existing_branch_worktree_strict(
            &project_root,
            &linked_worktree_path,
            &plan_branch.branch_name,
        )
        .await
        .map_err(RestartWorkspacePreparationError::unsafe_ownership)?;
        return Ok(RestartWorkspacePreparation {
            path: linked_worktree_path,
            source: RestartWorkspacePreparationSource::ReattachedBranch,
        });
    }

    if cleanup_proof != RestartWorkspaceCleanupProof::OwnedMergedCleanup {
        return Err(RestartWorkspacePreparationError::MissingBranchWithoutCleanupProof);
    }

    GitService::create_worktree_strict(
        &project_root,
        &linked_worktree_path,
        &plan_branch.branch_name,
        origin_base_ref,
    )
    .await
    .map_err(RestartWorkspacePreparationError::operation)?;
    Ok(RestartWorkspacePreparation {
        path: linked_worktree_path,
        source: RestartWorkspacePreparationSource::RecreatedFromCleanup,
    })
}

fn validated_project_root(project: &Project) -> AppResult<PathBuf> {
    validate_absolute_non_root_path(Path::new(&project.working_directory), "project checkout")
}

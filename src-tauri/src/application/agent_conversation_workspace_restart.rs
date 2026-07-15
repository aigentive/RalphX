use std::fmt;
use std::path::{Path, PathBuf};

use crate::application::agent_conversation_workspace::{
    ensure_linked_plan_branch_agent_worktree, expand_worktree_parent_public,
    resolve_agent_conversation_workspace_path_from_record_identity,
    resolve_linked_plan_branch_agent_worktree_path, validate_workspace_linked_plan_branch,
};
use crate::application::git_artifact_cleanup::LOCAL_CLEANUP_STATUS_CLEANED;
use crate::application::GitService;
use crate::domain::entities::plan_branch::PrStatus;
use crate::domain::entities::{
    AgentConversationWorkspace, ExecutionPlanId, IdeationSessionId, PlanBranch, PlanBranchStatus,
    Project, Task, TaskId,
};
use crate::domain::state_machine::transition_handler::compute_merge_worktree_path;
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
pub(crate) enum RestartWorkspaceOwner {
    ExistingLinked,
    OwnedConversation,
    UnownedPreservedBranch,
    CurrentAttemptMerge { task_id: TaskId, path: PathBuf },
    RecreatableFromCleanup,
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

/// Classify the preserved plan branch owner without mutating worktrees or refs.
pub(crate) async fn inspect_linked_plan_branch_owner_for_restart(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    plan_branch: &PlanBranch,
    session_id: &IdeationSessionId,
    execution_plan_id: &ExecutionPlanId,
    current_tasks: &[Task],
    cleanup_proof: RestartWorkspaceCleanupProof,
) -> Result<RestartWorkspaceOwner, RestartWorkspacePreparationError> {
    validate_workspace_linked_plan_branch(project, workspace, plan_branch)
        .map_err(RestartWorkspacePreparationError::unsafe_ownership)?;
    if workspace.linked_plan_branch_id.as_ref() != Some(&plan_branch.id)
        || workspace.linked_ideation_session_id.as_ref() != Some(session_id)
        || &plan_branch.session_id != session_id
        || plan_branch.execution_plan_id.as_ref() != Some(execution_plan_id)
    {
        return Err(RestartWorkspacePreparationError::UnsafeOwnership {
            detail: "linked workspace, session, branch, and execution attempt do not match"
                .to_string(),
        });
    }

    let project_root = validated_project_root(project)
        .map_err(RestartWorkspacePreparationError::unsafe_ownership)?;
    let linked_path = resolve_linked_plan_branch_agent_worktree_path(project, plan_branch)
        .map_err(RestartWorkspacePreparationError::unsafe_ownership)?;
    let conversation_path =
        resolve_agent_conversation_workspace_path_from_record_identity(project, workspace)
            .map_err(RestartWorkspacePreparationError::unsafe_ownership)?;
    let worktree_root = expand_worktree_parent_public(project.worktree_parent_or_default())
        .and_then(|path| validate_absolute_non_root_path(&path, "configured worktree root"))
        .map_err(RestartWorkspacePreparationError::unsafe_ownership)?;

    let owners: Vec<_> = GitService::list_worktrees(&project_root)
        .await
        .map_err(RestartWorkspacePreparationError::operation)?
        .into_iter()
        .filter(|worktree| worktree.branch.as_deref() == Some(&plan_branch.branch_name))
        .collect();
    if owners.len() > 1 {
        return Err(RestartWorkspacePreparationError::UnsafeOwnership {
            detail: format!(
                "preserved branch '{}' has multiple registered owners",
                plan_branch.branch_name
            ),
        });
    }

    if let Some(owner) = owners.first() {
        let owner_path = validate_absolute_non_root_path(
            Path::new(&owner.path),
            "registered plan branch worktree",
        )
        .map_err(RestartWorkspacePreparationError::unsafe_ownership)?;
        if paths_match(&owner_path, &project_root) {
            return Err(RestartWorkspacePreparationError::UnsafeOwnership {
                detail: "preserved branch is checked out in the project root".to_string(),
            });
        }
        if paths_match(&owner_path, &linked_path) {
            return Ok(RestartWorkspaceOwner::ExistingLinked);
        }
        if paths_match(&owner_path, &conversation_path) {
            return Ok(RestartWorkspaceOwner::OwnedConversation);
        }

        for task in current_tasks {
            if task.project_id != project.id
                || task.execution_plan_id.as_ref() != Some(execution_plan_id)
            {
                continue;
            }
            let derived_path = validate_absolute_non_root_path(
                Path::new(&compute_merge_worktree_path(project, task.id.as_str())),
                "derived current-attempt merge worktree",
            )
            .map_err(RestartWorkspacePreparationError::unsafe_ownership)?;
            let stored_path = task.worktree_path.as_deref().map(PathBuf::from);
            if stored_path.as_ref() != Some(&derived_path)
                || !paths_match(&owner_path, &derived_path)
            {
                continue;
            }
            if !path_is_contained(&owner_path, &worktree_root) {
                return Err(RestartWorkspacePreparationError::UnsafeOwnership {
                    detail: "current-attempt merge worktree escapes the configured root"
                        .to_string(),
                });
            }
            return Ok(RestartWorkspaceOwner::CurrentAttemptMerge {
                task_id: task.id.clone(),
                path: derived_path,
            });
        }

        return Err(RestartWorkspacePreparationError::UnsafeOwnership {
            detail: format!(
                "preserved branch '{}' is checked out by an unknown or stale worktree",
                plan_branch.branch_name
            ),
        });
    }

    if linked_path.exists() || conversation_path.exists() {
        return Err(RestartWorkspacePreparationError::UnsafeOwnership {
            detail:
                "a derived implementation workspace path exists without Git worktree registration"
                    .to_string(),
        });
    }

    if GitService::branch_exists_strict(&project_root, &plan_branch.branch_name)
        .await
        .map_err(RestartWorkspacePreparationError::operation)?
    {
        return Ok(RestartWorkspaceOwner::UnownedPreservedBranch);
    }
    if cleanup_proof == RestartWorkspaceCleanupProof::OwnedMergedCleanup {
        return Ok(RestartWorkspaceOwner::RecreatableFromCleanup);
    }
    Err(RestartWorkspacePreparationError::MissingBranchWithoutCleanupProof)
}

fn paths_match(left: &Path, right: &Path) -> bool {
    left == right
        || left
            .canonicalize()
            .ok()
            .zip(right.canonicalize().ok())
            .is_some_and(|(left, right)| left == right)
}

fn path_is_contained(path: &Path, root: &Path) -> bool {
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    canonical_path.starts_with(&canonical_root) && canonical_path != canonical_root
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

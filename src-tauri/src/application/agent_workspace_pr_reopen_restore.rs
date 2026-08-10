//! Local checkout restore for reopened agent workspace pull requests.
//!
//! Terminal PR cleanup force-deletes both the workspace worktree and its local
//! branch (`git_artifact_cleanup::cleanup_force_owned_terminal_artifacts`). The
//! only surviving copy of the work is `origin/<branch>`, which GitHub preserves
//! for a closed — as opposed to merged-and-deleted — pull request. Reopening the
//! PR therefore has to rebuild both artifacts from the remote before the
//! workspace is usable again.
//!
//! Restore never rewrites a local branch that survived cleanup: an existing
//! local branch is reused as-is, so uncommitted-but-committed local work is
//! never clobbered by the remote tip.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::application::agent_conversation_workspace::{
    ensure_linked_agent_conversation_branch_worktree, ensure_linked_plan_branch_agent_worktree,
    resolve_agent_conversation_workspace_path_from_record_identity,
    resolve_linked_plan_branch_agent_worktree_path,
    run_or_defer_agent_conversation_workspace_setup, AgentConversationWorkspaceSetupMode,
};
use crate::application::git_service::GitService;
use crate::domain::entities::{AgentConversationWorkspace, PlanBranch, Project};

/// Outcome of rebuilding a reopened workspace's local branch and worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReopenLocalWorkspaceState {
    /// The worktree survived cleanup and is checked out on the expected branch.
    AlreadyPresent,
    /// The local branch and/or worktree were rebuilt from `origin/<branch>`.
    Restored,
    /// The local checkout could not be rebuilt; the PR is still reopened.
    RestoreFailed,
}

#[derive(Debug, Clone)]
pub(crate) struct WorkspaceLocalRestore {
    pub state: ReopenLocalWorkspaceState,
    /// Present whenever a usable worktree exists after the attempt.
    pub worktree_path: Option<PathBuf>,
    pub failure_reason: Option<String>,
}

impl WorkspaceLocalRestore {
    fn failed(reason: String) -> Self {
        Self {
            state: ReopenLocalWorkspaceState::RestoreFailed,
            worktree_path: None,
            failure_reason: Some(reason),
        }
    }

    pub fn is_restore_failure(&self) -> bool {
        self.state == ReopenLocalWorkspaceState::RestoreFailed
    }
}

/// Rebuild the local branch and worktree for a reopened workspace.
///
/// Idempotent: an intact worktree on the expected branch is left untouched.
/// Never returns `Err` — the PR reopen already succeeded remotely, so a local
/// restore failure is reported as state rather than failing the whole action.
pub(crate) async fn restore_agent_workspace_local_artifacts(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    linked_plan_branch: Option<&PlanBranch>,
) -> WorkspaceLocalRestore {
    let (worktree_path, branch_name) =
        match resolve_restore_target(project, workspace, linked_plan_branch) {
            Ok(target) => target,
            Err(error) => return WorkspaceLocalRestore::failed(error),
        };

    match inspect_existing_worktree(&worktree_path, &branch_name).await {
        Ok(Some(restore)) => return restore,
        Ok(None) => {}
        Err(error) => return WorkspaceLocalRestore::failed(error),
    }

    let repo_path = Path::new(&project.working_directory);
    if let Err(error) = ensure_local_branch_from_remote(repo_path, &branch_name).await {
        return WorkspaceLocalRestore::failed(error);
    }

    let created = match linked_plan_branch {
        Some(plan_branch) => ensure_linked_plan_branch_agent_worktree(project, plan_branch)
            .await
            .map(|_| ()),
        None => {
            ensure_linked_agent_conversation_branch_worktree(
                repo_path,
                &worktree_path,
                &branch_name,
            )
            .await
        }
    };
    if let Err(error) = created {
        return WorkspaceLocalRestore::failed(format!(
            "Could not recreate the workspace checkout at {}: {error}",
            worktree_path.display()
        ));
    }

    // Project setup commands ran against the deleted checkout, so a rebuilt
    // worktree needs them again. Deferred keeps the reopen action responsive.
    run_or_defer_agent_conversation_workspace_setup(
        project,
        &workspace.conversation_id,
        &worktree_path,
        &branch_name,
        AgentConversationWorkspaceSetupMode::Deferred,
    )
    .await;

    tracing::info!(
        conversation_id = %workspace.conversation_id,
        branch_name = %branch_name,
        worktree_path = %worktree_path.display(),
        "Restored agent workspace local checkout after PR reopen"
    );
    WorkspaceLocalRestore {
        state: ReopenLocalWorkspaceState::Restored,
        worktree_path: Some(worktree_path),
        failure_reason: None,
    }
}

fn resolve_restore_target(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    linked_plan_branch: Option<&PlanBranch>,
) -> Result<(PathBuf, String), String> {
    if let Some(plan_branch) = linked_plan_branch {
        let path = resolve_linked_plan_branch_agent_worktree_path(project, plan_branch)
            .map_err(|error| error.to_string())?;
        return Ok((path, plan_branch.branch_name.clone()));
    }

    let path = resolve_agent_conversation_workspace_path_from_record_identity(project, workspace)
        .map_err(|error| error.to_string())?;
    Ok((path, workspace.branch_name.clone()))
}

/// `Ok(Some(_))` when the existing checkout settles the outcome, `Ok(None)`
/// when the worktree is absent and must be rebuilt.
async fn inspect_existing_worktree(
    worktree_path: &Path,
    branch_name: &str,
) -> Result<Option<WorkspaceLocalRestore>, String> {
    if !worktree_path.exists() {
        return Ok(None);
    }
    if !worktree_path.is_dir() {
        return Err(format!(
            "Workspace path {} exists but is not a directory",
            worktree_path.display()
        ));
    }

    let checked_out = GitService::get_current_branch(worktree_path)
        .await
        .map_err(|error| {
            format!(
                "Could not read the branch checked out at {}: {error}",
                worktree_path.display()
            )
        })?;
    if checked_out != branch_name {
        return Err(format!(
            "Workspace {} is checked out at '{checked_out}' instead of '{branch_name}'",
            worktree_path.display()
        ));
    }

    Ok(Some(WorkspaceLocalRestore {
        state: ReopenLocalWorkspaceState::AlreadyPresent,
        worktree_path: Some(worktree_path.to_path_buf()),
        failure_reason: None,
    }))
}

async fn ensure_local_branch_from_remote(
    repo_path: &Path,
    branch_name: &str,
) -> Result<(), String> {
    // Best-effort refresh so a surviving-but-stale `origin/<branch>` does not
    // silently restore an older head. A busy or offline fetch falls through to
    // the escalation inside `ensure_local_branch_from_origin_if_missing`.
    match GitService::try_fetch_origin_ref_for_maintenance(repo_path, branch_name).await {
        Ok(outcome) => tracing::debug!(
            branch_name,
            fetch_outcome = ?outcome,
            "Refreshed origin ref before agent workspace restore"
        ),
        Err(error) => tracing::warn!(
            branch_name,
            error = %error,
            "Could not refresh origin ref before agent workspace restore; using local remote-tracking state"
        ),
    }

    GitService::ensure_local_branch_from_origin_if_missing(repo_path, branch_name)
        .await
        .map(|_| ())
        .map_err(|error| {
            format!(
                "Could not restore branch '{branch_name}' from origin: {error}. The remote branch may have been deleted."
            )
        })
}

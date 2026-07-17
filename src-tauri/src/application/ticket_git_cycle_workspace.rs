use std::path::Path;

use crate::application::git_service::GitService;
use crate::domain::entities::{
    AgentConversationWorkspace, TicketCanonicalBranch, TicketCanonicalBranchCycleState,
};
use crate::error::{AppError, AppResult};

pub(crate) async fn active_cycle_is_partial_rollover(
    repo_path: &Path,
    workspace: &AgentConversationWorkspace,
    binding: &TicketCanonicalBranch,
) -> AppResult<bool> {
    if binding.cycle.state != TicketCanonicalBranchCycleState::Active {
        return Ok(false);
    }
    let Some(cycle_base) = binding.cycle.base_commit.as_deref() else {
        return Ok(false);
    };
    if workspace.base_commit.as_deref() == Some(cycle_base)
        || !GitService::branch_exists_strict(repo_path, &binding.branch_name).await?
    {
        return Ok(false);
    }
    Ok(GitService::get_branch_sha(repo_path, &binding.branch_name).await? == cycle_base)
}

pub(super) fn validate_binding_workspace_identity(
    binding: &TicketCanonicalBranch,
    workspace: &AgentConversationWorkspace,
) -> AppResult<()> {
    binding.validate_policy().map_err(AppError::Validation)?;
    if binding.project_id != workspace.project_id || binding.branch_name != workspace.branch_name {
        return Err(AppError::Validation(
            "Strict ticket binding does not match the workspace project and branch".to_string(),
        ));
    }
    Ok(())
}

pub(super) async fn validate_clean_workspace(path: &Path) -> AppResult<()> {
    if !path.exists() {
        return Ok(());
    }
    if !path.is_dir() {
        return Err(AppError::Validation(format!(
            "Strict ticket workspace path is not a directory: {}",
            path.display()
        )));
    }
    if GitService::has_uncommitted_changes(path).await? {
        return Err(AppError::Validation(
            "Strict ticket workspace has uncommitted changes; branch reuse is blocked".to_string(),
        ));
    }
    if GitService::is_merge_in_progress(path) || GitService::is_rebase_in_progress(path) {
        return Err(AppError::Validation(
            "Strict ticket workspace has an unfinished Git operation; branch reuse is blocked"
                .to_string(),
        ));
    }
    Ok(())
}

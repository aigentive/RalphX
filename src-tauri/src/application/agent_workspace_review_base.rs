use crate::application::GitService;
use crate::domain::entities::{AgentConversationWorkspace, AgentConversationWorkspaceBranchMode};
use crate::error::{AppError, AppResult};
use std::path::Path;

/// Resolve the baseline for user-visible workspace review surfaces.
///
/// Isolated workspaces are created from their captured base commit, so that
/// snapshot remains authoritative. Linked workspaces can start from an older
/// branch while the selected project base has advanced; their review baseline
/// is the branch's merge base, which excludes unrelated base-branch progress.
pub async fn resolve_agent_workspace_review_base(
    repo_path: &Path,
    workspace: &AgentConversationWorkspace,
    head_ref: &str,
    captured_base: &str,
) -> AppResult<String> {
    let captured_base = captured_base.trim();
    if captured_base.is_empty() {
        return Err(AppError::Validation(
            "Workspace review requires a captured base commit".to_string(),
        ));
    }

    if workspace.branch_mode != AgentConversationWorkspaceBranchMode::Linked {
        return Ok(captured_base.to_string());
    }

    let base_ref = workspace.base_ref.trim();
    if base_ref.is_empty() {
        return Err(AppError::Validation(
            "Linked workspace review requires a base ref".to_string(),
        ));
    }
    let head_ref = head_ref.trim();
    if head_ref.is_empty() {
        return Err(AppError::Validation(
            "Linked workspace review requires a head ref".to_string(),
        ));
    }

    GitService::get_merge_base(repo_path, base_ref, head_ref).await
}

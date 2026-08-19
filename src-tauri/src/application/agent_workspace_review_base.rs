use crate::application::GitService;
use crate::domain::entities::{AgentConversationWorkspace, AgentConversationWorkspaceBranchMode};
use crate::error::{AppError, AppResult};
use std::path::Path;

/// Resolve the baseline for user-visible workspace review surfaces.
///
/// Isolated workspaces are created from their captured base commit, but that
/// snapshot is authoritative only while it remains an ancestor of the head being
/// reviewed. A snapshot that was retargeted ahead of the branch would render
/// base-branch progress as inverted workspace changes, so the resolver degrades
/// to the merge base instead. Linked workspaces can start from an older branch
/// while the selected project base has advanced; their review baseline is the
/// branch's merge base, which excludes unrelated base-branch progress.
///
/// # Errors
///
/// Returns [`AppError::Validation`] when the captured base, the base ref (Linked
/// only), or the head ref is empty, and [`AppError::GitOperation`] when git
/// cannot resolve a merge base — an unresolvable captured base surfaces as a
/// visible diff error rather than a silently wrong baseline.
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
        let head_ref = head_ref.trim();
        if head_ref.is_empty() {
            return Err(AppError::Validation(
                "Workspace review requires a head ref".to_string(),
            ));
        }
        // `is_ancestor` collapses git failures to `false` by design, so an unknown or malformed
        // captured base falls through to `get_merge_base`, which surfaces git's own error. A
        // visible diff error is the correct outcome here; a silently inverted diff is not.
        if GitService::is_ancestor(repo_path, captured_base, head_ref).await? {
            return Ok(captured_base.to_string());
        }
        return GitService::get_merge_base(repo_path, captured_base, head_ref).await;
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

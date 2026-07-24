use crate::application::agent_conversation_workspace::resolve_valid_agent_conversation_workspace_path;
use crate::application::agent_workspace_review::{
    load_agent_workspace_review_context, lock_workspace_review_lifecycle,
    review_gate_publish_blocker,
};
use crate::application::{AppState, GitService};
use crate::domain::entities::{
    is_publication_push_active, AgentConversationWorkspace, AgentConversationWorkspaceMode,
    ChatContextType, ChatConversationId,
};
use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct AgentWorkspaceLocalCommitRequest {
    pub expected_head_sha: String,
    pub review_artifact_id: Option<String>,
    pub review_artifact_version: Option<u32>,
    pub reviewed_head_sha: Option<String>,
    pub reviewed_diff_fingerprint: Option<String>,
    pub attempt_token: String,
    #[cfg(test)]
    pub(crate) before_staging: Option<fn(&std::path::Path)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentWorkspaceLocalCommitOutcome {
    CommittedLocal,
    AlreadyCommitted,
    NoChanges,
}

impl AgentWorkspaceLocalCommitOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CommittedLocal => "committed_local",
            Self::AlreadyCommitted => "already_committed",
            Self::NoChanges => "no_changes",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentWorkspaceLocalCommitResult {
    pub workspace: AgentConversationWorkspace,
    pub outcome: AgentWorkspaceLocalCommitOutcome,
    pub branch_name: String,
    pub previous_head_sha: String,
    pub commit_sha: String,
    pub had_changes: bool,
    pub attempt_token: String,
}

/// Explicitly commit the isolated Agent workspace branch without entering the
/// publication lifecycle. The review lifecycle lock serializes this mutation
/// with review receipts and publication; callers must not take it themselves.
pub async fn commit_agent_workspace_locally(
    state: &AppState,
    conversation_id: ChatConversationId,
    request: AgentWorkspaceLocalCommitRequest,
) -> Result<AgentWorkspaceLocalCommitResult, String> {
    let _lifecycle_guard = lock_workspace_review_lifecycle(&conversation_id).await;
    commit_agent_workspace_locally_unlocked(state, conversation_id, request).await
}

async fn commit_agent_workspace_locally_unlocked(
    state: &AppState,
    conversation_id: ChatConversationId,
    request: AgentWorkspaceLocalCommitRequest,
) -> Result<AgentWorkspaceLocalCommitResult, String> {
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            format!("Agent conversation workspace not found for conversation {conversation_id}")
        })?;
    if workspace.mode != AgentConversationWorkspaceMode::Edit || workspace.is_execution_owned() {
        return Err(
            "Only standalone Edit-mode agent workspaces can be committed locally".to_string(),
        );
    }
    if is_publication_push_active(workspace.publication_push_status.as_deref()) {
        return Err(
            "Commit & Publish is running; wait for it to finish before committing locally."
                .to_string(),
        );
    }
    validate_review_gate_and_receipt(state, &workspace, &request).await?;
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Conversation not found: {conversation_id}"))?;
    if conversation.context_type != ChatContextType::Project
        || conversation.context_id != workspace.project_id.as_str()
    {
        return Err(format!(
            "Conversation {} does not match agent workspace project {}",
            conversation.id, workspace.project_id
        ));
    }
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Project not found: {}", workspace.project_id))?;
    let worktree_path = resolve_valid_agent_conversation_workspace_path(&project, &workspace)
        .await
        .map_err(|error| error.to_string())?;
    let previous_head_sha = GitService::get_head_sha(&worktree_path)
        .await
        .map_err(|error| error.to_string())?;
    let had_changes = GitService::has_uncommitted_changes(&worktree_path)
        .await
        .map_err(|error| error.to_string())?;

    if request.expected_head_sha != previous_head_sha {
        if had_changes {
            return Err("Workspace branch changed since this commit attempt started; refresh before committing.".to_string());
        }
        return Ok(AgentWorkspaceLocalCommitResult {
            workspace: workspace.clone(),
            outcome: AgentWorkspaceLocalCommitOutcome::AlreadyCommitted,
            branch_name: workspace.branch_name.clone(),
            previous_head_sha: previous_head_sha.clone(),
            commit_sha: previous_head_sha,
            had_changes,
            attempt_token: request.attempt_token,
        });
    }

    if !had_changes {
        return Ok(AgentWorkspaceLocalCommitResult {
            workspace: workspace.clone(),
            outcome: AgentWorkspaceLocalCommitOutcome::NoChanges,
            branch_name: workspace.branch_name.clone(),
            previous_head_sha: previous_head_sha.clone(),
            commit_sha: previous_head_sha,
            had_changes,
            attempt_token: request.attempt_token,
        });
    }

    GitService::ensure_commit_identity(&worktree_path)
        .await
        .map_err(|error| error.to_string())?;
    let message = build_commit_message(conversation.title.as_deref());
    let index_snapshot =
        GitService::stage_all_including_deletions_with_index_snapshot(&worktree_path)
            .await
            .map_err(|error| error.to_string())?;
    #[cfg(test)]
    if let Some(before_staging) = request.before_staging {
        before_staging(&worktree_path);
    }
    if let Err(error) = validate_review_gate_and_receipt(state, &workspace, &request).await {
        return match GitService::restore_index_snapshot(&worktree_path, &index_snapshot).await {
            Ok(()) => Err(error),
            Err(restore_error) => Err(format!(
                "{error} Additionally, RalphX could not restore the pre-commit Git index: {restore_error}"
            )),
        };
    }
    let commit_head_sha = GitService::get_head_sha(&worktree_path)
        .await
        .map_err(|error| error.to_string())?;
    if commit_head_sha != previous_head_sha {
        return match GitService::restore_index_snapshot(&worktree_path, &index_snapshot).await {
            Ok(()) => Err(
                "Workspace branch changed since this commit attempt started; refresh before committing."
                    .to_string(),
            ),
            Err(restore_error) => Err(format!(
                "Workspace branch changed since this commit attempt started; refresh before committing. Additionally, RalphX could not restore the pre-commit Git index: {restore_error}"
            )),
        };
    }
    let commit_sha = GitService::commit_staged_changes(&worktree_path, &message)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Git reported local changes but did not create a commit".to_string())?;
    Ok(AgentWorkspaceLocalCommitResult {
        workspace: workspace.clone(),
        outcome: AgentWorkspaceLocalCommitOutcome::CommittedLocal,
        branch_name: workspace.branch_name.clone(),
        previous_head_sha,
        commit_sha,
        had_changes,
        attempt_token: request.attempt_token,
    })
}

async fn validate_review_gate_and_receipt(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    request: &AgentWorkspaceLocalCommitRequest,
) -> Result<(), String> {
    let review_settings = state
        .review_settings_repo
        .get_settings()
        .await
        .map_err(|error| error.to_string())?;
    if !review_settings.require_workspace_review {
        // Optional reviews are informative only. The monitor may still carry a
        // current receipt, so clients can safely send it without turning an
        // optional review into a commit-time gate.
        return Ok(());
    }
    let context = load_agent_workspace_review_context(state, workspace)
        .await
        .map_err(|error| error.to_string())?;
    if let Some(blocker) = review_gate_publish_blocker(&context) {
        return Err(blocker);
    }
    let monitor = context.monitor;
    let receipt_matches = request.review_artifact_id.as_deref()
        == monitor.review_artifact_id.as_ref().map(|id| id.as_str())
        && request.review_artifact_version == monitor.review_artifact_version
        && request.reviewed_head_sha.as_deref() == monitor.reviewed_head_sha.as_deref()
        && request.reviewed_diff_fingerprint.as_deref()
            == monitor.reviewed_diff_fingerprint.as_deref();
    if receipt_matches {
        Ok(())
    } else {
        Err(AppError::Conflict(
            "Workspace Review receipt changed; refresh before committing.".to_string(),
        )
        .to_string())
    }
}

fn build_commit_message(title: Option<&str>) -> String {
    let title = title
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "Untitled agent")
        .unwrap_or("agent conversation work");
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("feat: {title}")
}

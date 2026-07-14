//! Durable GitHub auto-merge coordination for workspace Reviews.

use std::sync::Arc;

use crate::application::agent_workspace_review::{
    load_or_create_monitor, resolve_review_target, start_agent_workspace_review,
    AgentWorkspaceReviewStart, AgentWorkspaceReviewTarget,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentWorkspaceReviewAutoMergeGuard,
    AgentWorkspaceReviewAutoMergeGuardStatus, AgentWorkspaceReviewMonitor,
    AgentWorkspaceReviewOutcome, AgentWorkspaceReviewTargetScope,
};
use crate::error::{AppError, AppResult};

const REVIEW_AUTO_MERGE_PAUSED_SUMMARY: &str =
    "GitHub auto-merge is paused while the workspace Review is authoritative.";
const REVIEW_AUTO_MERGE_RESTORED_SUMMARY: &str =
    "GitHub auto-merge was restored after the workspace Review passed.";
const REVIEW_AUTO_MERGE_RESTORE_FAILED_SUMMARY: &str =
    "Workspace Review passed, but GitHub auto-merge could not be restored yet.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceReviewStartOrigin {
    Manual,
    Automated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceReviewAutoMergePreview {
    pub target: AgentWorkspaceReviewTarget,
    pub pr_number: i64,
    pub merge_method: String,
    pub restore_after_publish: bool,
}

/// Re-reads GitHub state for a review target. A preview is absent when there is no open PR,
/// GitHub integration is unavailable, or auto-merge is already disabled.
pub async fn preview_workspace_review_auto_merge_guard(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<Option<WorkspaceReviewAutoMergePreview>> {
    let Some(github) = state.github_service.as_ref() else {
        return Ok(None);
    };
    let Some(target) = resolve_current_target(state, workspace).await? else {
        return Ok(None);
    };
    let Some(pr_number) = review_target_pr_number(workspace, &target) else {
        return Ok(None);
    };
    let health = github
        .fetch_pr_health(&target.working_directory, pr_number)
        .await?;
    let Some(request) = health.auto_merge_request else {
        return Ok(None);
    };
    let merge_method = request
        .merge_method
        .filter(|method| !method.trim().is_empty())
        .unwrap_or_else(|| workspace.pr_auto_merge_method.clone());
    Ok(Some(WorkspaceReviewAutoMergePreview {
        restore_after_publish: target.scope == AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        target,
        pr_number,
        merge_method,
    }))
}

/// Starts the existing reviewer only after a durable guard owns a currently-enabled GitHub
/// auto-merge request and GitHub confirms it was disabled. Automated callers intentionally skip
/// UI confirmation, but retain the same backend ordering.
pub async fn start_guarded_agent_workspace_review(
    state: Arc<AppState>,
    workspace: &AgentConversationWorkspace,
    force: bool,
    _origin: WorkspaceReviewStartOrigin,
) -> AppResult<AgentWorkspaceReviewStart> {
    let Some(preview) =
        preview_workspace_review_auto_merge_guard(state.as_ref(), workspace).await?
    else {
        return start_agent_workspace_review(state, workspace, force).await;
    };

    let monitor = load_or_create_monitor(state.as_ref(), workspace).await?;
    if let Some(existing_guard) = monitor.auto_merge_guard.as_ref() {
        if guard_matches_target(existing_guard, &preview.target, preview.pr_number) {
            return start_agent_workspace_review(state, workspace, force).await;
        }
        return Err(AppError::Conflict(
            "another workspace Review auto-merge guard is still authoritative".to_string(),
        ));
    }

    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor.clone())
        .await?;
    let pausing = guard_for_preview(&preview, AgentWorkspaceReviewAutoMergeGuardStatus::Pausing);
    let claimed = state
        .agent_conversation_workspace_repo
        .compare_and_set_workspace_review_auto_merge_guard(
            &workspace.conversation_id,
            None,
            Some(pausing.clone()),
        )
        .await?;
    if !claimed {
        return Err(AppError::Conflict(
            "workspace Review auto-merge state changed; refresh and retry".to_string(),
        ));
    }

    let github = state.github_service.as_ref().ok_or_else(|| {
        AppError::Infrastructure(
            "GitHub integration became unavailable before review start".to_string(),
        )
    })?;
    if let Err(error) = github
        .disable_pr_auto_merge(&preview.target.working_directory, preview.pr_number)
        .await
    {
        let failed = guard_with_error(pausing.clone(), error.to_string());
        let _ = state
            .agent_conversation_workspace_repo
            .compare_and_set_workspace_review_auto_merge_guard(
                &workspace.conversation_id,
                Some(pausing),
                None,
            )
            .await;
        return Err(AppError::Infrastructure(format!(
            "could not disable GitHub auto-merge before workspace Review: {error}"
        )));
    }

    state
        .agent_conversation_workspace_repo
        .update_pr_auto_merge_state(
            &workspace.conversation_id,
            Some(false),
            Some("review_paused"),
            Some(REVIEW_AUTO_MERGE_PAUSED_SUMMARY),
        )
        .await?;
    let paused = guard_for_preview(
        &preview,
        AgentWorkspaceReviewAutoMergeGuardStatus::PausedForReview,
    );
    let paused_persisted = state
        .agent_conversation_workspace_repo
        .compare_and_set_workspace_review_auto_merge_guard(
            &workspace.conversation_id,
            Some(pausing),
            Some(paused.clone()),
        )
        .await?;
    if !paused_persisted {
        return Err(AppError::Conflict(
            "workspace Review auto-merge guard changed after GitHub was paused".to_string(),
        ));
    }

    let started = start_agent_workspace_review(Arc::clone(&state), workspace, force).await;
    match started {
        Ok(start) if start.started => Ok(start),
        Ok(start) => {
            restore_guarded_auto_merge(state.as_ref(), workspace, &paused).await?;
            Ok(start)
        }
        Err(error) => {
            let restore_result =
                restore_guarded_auto_merge(state.as_ref(), workspace, &paused).await;
            if let Err(restore_error) = restore_result {
                return Err(restore_error);
            }
            Err(error)
        }
    }
}

/// Advances only a current passing Review. Local workspace deltas remain paused until a successful
/// publish calls [`restore_guarded_auto_merge_after_publish`].
pub async fn handle_passing_workspace_review_auto_merge_guard(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    monitor: &AgentWorkspaceReviewMonitor,
) -> AppResult<AgentWorkspaceReviewMonitor> {
    if monitor.review_outcome != AgentWorkspaceReviewOutcome::Passed {
        return Ok(monitor.clone());
    }
    let Some(guard) = monitor.auto_merge_guard.as_ref() else {
        return Ok(monitor.clone());
    };
    let Some(target) = resolve_current_target(state, workspace).await? else {
        return Ok(monitor.clone());
    };
    if !guard_matches_target(guard, &target, guard.pr_number) {
        return Ok(monitor.clone());
    }
    match target.scope {
        AgentWorkspaceReviewTargetScope::SelectedSource => {
            restore_guarded_auto_merge(state, workspace, guard).await?;
        }
        AgentWorkspaceReviewTargetScope::WorkspaceDelta => {
            let awaiting_publish = AgentWorkspaceReviewAutoMergeGuard {
                status: AgentWorkspaceReviewAutoMergeGuardStatus::AwaitingPublish,
                ..guard.clone()
            };
            state
                .agent_conversation_workspace_repo
                .compare_and_set_workspace_review_auto_merge_guard(
                    &workspace.conversation_id,
                    Some(guard.clone()),
                    Some(awaiting_publish),
                )
                .await?;
        }
    }
    load_or_create_monitor(state, workspace).await
}

/// Called after a successful workspace publication proves that the guarded workspace delta reached
/// the same PR.
pub async fn restore_guarded_auto_merge_after_publish(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<()> {
    let monitor = load_or_create_monitor(state, workspace).await?;
    let Some(guard) = monitor.auto_merge_guard.as_ref() else {
        return Ok(());
    };
    if guard.status != AgentWorkspaceReviewAutoMergeGuardStatus::AwaitingPublish {
        return Ok(());
    }
    restore_guarded_auto_merge(state, workspace, guard).await
}

pub fn auto_merge_guard_blocks_enable(monitor: Option<&AgentWorkspaceReviewMonitor>) -> bool {
    monitor
        .and_then(|monitor| monitor.auto_merge_guard.as_ref())
        .is_some()
}

pub async fn cancel_workspace_review_auto_merge_guard(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<bool> {
    let monitor = load_or_create_monitor(state, workspace).await?;
    let Some(guard) = monitor.auto_merge_guard else {
        return Ok(false);
    };
    state
        .agent_conversation_workspace_repo
        .compare_and_set_workspace_review_auto_merge_guard(
            &workspace.conversation_id,
            Some(guard),
            None,
        )
        .await
}

async fn resolve_current_target(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<Option<AgentWorkspaceReviewTarget>> {
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Project not found".to_string()))?;
    resolve_review_target(workspace, &project).await
}

fn review_target_pr_number(
    workspace: &AgentConversationWorkspace,
    target: &AgentWorkspaceReviewTarget,
) -> Option<i64> {
    target
        .source_pull_request_number
        .or(workspace.publication_pr_number)
        .filter(|_| !workspace.has_terminal_publication_pr_status())
}

fn guard_for_preview(
    preview: &WorkspaceReviewAutoMergePreview,
    status: AgentWorkspaceReviewAutoMergeGuardStatus,
) -> AgentWorkspaceReviewAutoMergeGuard {
    AgentWorkspaceReviewAutoMergeGuard {
        status,
        pr_number: preview.pr_number,
        merge_method: preview.merge_method.clone(),
        target_scope: preview.target.scope,
        diff_fingerprint: preview.target.diff_fingerprint.clone(),
        head_sha: preview.target.head_sha.clone(),
        last_error: None,
    }
}

fn guard_matches_target(
    guard: &AgentWorkspaceReviewAutoMergeGuard,
    target: &AgentWorkspaceReviewTarget,
    pr_number: i64,
) -> bool {
    guard.pr_number == pr_number
        && guard.target_scope == target.scope
        && guard.diff_fingerprint == target.diff_fingerprint
        && (target.scope == AgentWorkspaceReviewTargetScope::WorkspaceDelta
            || guard.head_sha == target.head_sha)
}

async fn restore_guarded_auto_merge(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    guard: &AgentWorkspaceReviewAutoMergeGuard,
) -> AppResult<()> {
    let restoring = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::Restoring,
        last_error: None,
        ..guard.clone()
    };
    if !state
        .agent_conversation_workspace_repo
        .compare_and_set_workspace_review_auto_merge_guard(
            &workspace.conversation_id,
            Some(guard.clone()),
            Some(restoring.clone()),
        )
        .await?
    {
        return Ok(());
    }
    let target = resolve_current_target(state, workspace).await?;
    let Some(target) = target else {
        return mark_restore_failed(
            state,
            workspace,
            restoring,
            "workspace Review target no longer exists".to_string(),
        )
        .await;
    };
    if !guard_matches_target(&restoring, &target, restoring.pr_number) {
        return mark_restore_failed(
            state,
            workspace,
            restoring,
            "workspace Review target changed before auto-merge restoration".to_string(),
        )
        .await;
    }
    let github = state.github_service.as_ref().ok_or_else(|| {
        AppError::Infrastructure(
            "GitHub integration became unavailable before auto-merge restoration".to_string(),
        )
    })?;
    if let Err(error) = github
        .enable_pr_auto_merge(
            &target.working_directory,
            restoring.pr_number,
            &restoring.merge_method,
        )
        .await
    {
        return mark_restore_failed(state, workspace, restoring, error.to_string()).await;
    }
    state
        .agent_conversation_workspace_repo
        .update_pr_auto_merge_state(
            &workspace.conversation_id,
            Some(true),
            Some("monitoring"),
            Some(REVIEW_AUTO_MERGE_RESTORED_SUMMARY),
        )
        .await?;
    state
        .agent_conversation_workspace_repo
        .compare_and_set_workspace_review_auto_merge_guard(
            &workspace.conversation_id,
            Some(restoring),
            None,
        )
        .await?;
    Ok(())
}

async fn mark_restore_failed(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    restoring: AgentWorkspaceReviewAutoMergeGuard,
    error: String,
) -> AppResult<()> {
    let failed = AgentWorkspaceReviewAutoMergeGuard {
        status: AgentWorkspaceReviewAutoMergeGuardStatus::RestoreFailed,
        last_error: Some(error),
        ..restoring.clone()
    };
    state
        .agent_conversation_workspace_repo
        .compare_and_set_workspace_review_auto_merge_guard(
            &workspace.conversation_id,
            Some(restoring),
            Some(failed),
        )
        .await?;
    state
        .agent_conversation_workspace_repo
        .update_pr_auto_merge_state(
            &workspace.conversation_id,
            Some(false),
            Some("review_restore_failed"),
            Some(REVIEW_AUTO_MERGE_RESTORE_FAILED_SUMMARY),
        )
        .await?;
    Ok(())
}

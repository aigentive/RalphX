use chrono::Utc;

use crate::domain::entities::{
    is_publication_push_active, workspace_review_fixer_status_is_active,
    AgentConversationWorkspace, AgentWorkspaceReviewApprovalSnapshot,
    AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus,
    AgentWorkspaceReviewOutcome,
};
use crate::error::{AppError, AppResult};

use super::agent_workspace_review::{
    load_current_workspace_review_eligible, lock_workspace_review_lifecycle, resolve_review_target,
};
use super::AppState;

pub async fn approve_agent_workspace_review_anyway(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    snapshot: &AgentWorkspaceReviewApprovalSnapshot,
) -> AppResult<AgentWorkspaceReviewMonitor> {
    let _lifecycle_guard = lock_workspace_review_lifecycle(&workspace.conversation_id).await;
    let workspace = load_current_workspace_review_eligible(state, workspace).await?;
    let workspace = &workspace;

    if is_publication_push_active(workspace.publication_push_status.as_deref()) {
        return Err(AppError::Conflict(
            "Workspace Review cannot be approved while Commit & Publish is running".to_string(),
        ));
    }

    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Project not found".to_string()))?;
    let target = resolve_review_target(workspace, &project).await?;
    let target = target.as_ref().ok_or_else(|| {
        AppError::Conflict(
            "Workspace Review approval no longer matches the current workspace changes".to_string(),
        )
    })?;
    let monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::Conflict(
                "Workspace Review changed before it could be approved; refresh and review the current blockers"
                    .to_string(),
            )
        })?;
    let fixer_active =
        workspace_review_fixer_status_is_active(monitor.review_fixer_status.as_deref());
    let artifact_current = monitor.is_current_for_target(
        target.scope,
        target.head_sha.as_deref(),
        &target.diff_fingerprint,
    ) && monitor.has_review_artifact_pair();
    let snapshot_matches = artifact_current
        && monitor.status == AgentWorkspaceReviewMonitorStatus::Ready
        && monitor.review_outcome == AgentWorkspaceReviewOutcome::Blocking
        && monitor.review_gate_status == AgentWorkspaceReviewGateStatus::Blocking
        && target.scope == snapshot.target_scope
        && target.diff_fingerprint == snapshot.diff_fingerprint
        && monitor.current_target_scope == Some(snapshot.target_scope)
        && monitor.current_diff_fingerprint.as_deref() == Some(snapshot.diff_fingerprint.as_str())
        && monitor.review_artifact_id.as_ref() == Some(&snapshot.artifact_id)
        && monitor.review_artifact_version == Some(snapshot.artifact_version)
        && !fixer_active;
    if !snapshot_matches {
        return Err(AppError::Conflict(
            "Workspace Review changed before it could be approved; refresh and review the current blockers"
                .to_string(),
        ));
    }

    state
        .agent_conversation_workspace_repo
        .approve_workspace_review_anyway(&workspace.conversation_id, snapshot, Utc::now())
        .await?
        .ok_or_else(|| {
            AppError::Conflict(
                "Workspace Review changed before it could be approved; refresh and try again"
                    .to_string(),
            )
        })
}

use chrono::Utc;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentWorkspaceReviewApprovalSnapshot,
    AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus,
    AgentWorkspaceReviewOutcome,
};
use crate::error::{AppError, AppResult};

use super::agent_workspace_review::load_agent_workspace_review_context;
use super::AppState;

pub async fn approve_agent_workspace_review_anyway(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    snapshot: &AgentWorkspaceReviewApprovalSnapshot,
) -> AppResult<AgentWorkspaceReviewMonitor> {
    if workspace_publish_is_active(workspace.publication_push_status.as_deref()) {
        return Err(AppError::Conflict(
            "Workspace Review cannot be approved while Commit & Publish is running".to_string(),
        ));
    }

    let context = load_agent_workspace_review_context(state, workspace).await?;
    let target = context.target.as_ref().ok_or_else(|| {
        AppError::Conflict(
            "Workspace Review approval no longer matches the current workspace changes".to_string(),
        )
    })?;
    let fixer_active = matches!(
        context.monitor.review_fixer_status.as_deref(),
        Some("routing" | "queued" | "running")
    );
    let snapshot_matches = context.is_current
        && !context.is_outdated
        && context.monitor.status == AgentWorkspaceReviewMonitorStatus::Ready
        && context.monitor.review_outcome == AgentWorkspaceReviewOutcome::Blocking
        && context.monitor.review_gate_status == AgentWorkspaceReviewGateStatus::Blocking
        && target.scope == snapshot.target_scope
        && target.diff_fingerprint == snapshot.diff_fingerprint
        && context.monitor.review_artifact_id.as_ref() == Some(&snapshot.artifact_id)
        && context.monitor.review_artifact_version == Some(snapshot.artifact_version)
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

fn workspace_publish_is_active(status: Option<&str>) -> bool {
    matches!(
        status,
        Some("checking" | "committing" | "refreshing" | "describing" | "pushing")
    )
}

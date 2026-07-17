use std::sync::Arc;

use crate::application::agent_workspace_review::{
    load_agent_workspace_review_context, start_agent_workspace_review,
    start_agent_workspace_review_blocking_fixer,
};
use crate::application::agent_workspace_review_approval::approve_agent_workspace_review_anyway;
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentWorkspaceReviewApprovalSnapshot, AgentWorkspaceReviewTargetScope, ArtifactId,
    ChatConversationId, IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::error::AppError;

fn review_pr_workspace() -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        ChatConversationId::from_string("review-pr-mode-guard".to_string()),
        ProjectId::from_string("project-review-pr-mode-guard".to_string()),
        AgentConversationWorkspaceMode::ReviewPr,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/review-pr-mode-guard".to_string(),
        "/tmp/ralphx-review-pr-mode-guard".to_string(),
    )
}

fn assert_review_pr_mode_error(error: AppError) {
    assert!(matches!(
        error,
        AppError::Validation(message) if message.contains("unavailable in Review PR mode")
    ));
}

#[tokio::test]
async fn load_workspace_review_context_rejects_review_pr_mode_before_project_lookup() {
    let state = AppState::new_test();
    let workspace = review_pr_workspace();

    let error = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect_err("Review PR workspaces must not expose workspace Review context");

    assert_review_pr_mode_error(error);
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .is_none());
}

#[tokio::test]
async fn start_workspace_review_rejects_review_pr_mode_before_monitor_write() {
    let state = Arc::new(AppState::new_test());
    let workspace = review_pr_workspace();

    let error = start_agent_workspace_review(Arc::clone(&state), &workspace, true)
        .await
        .expect_err("Review PR workspaces must not start workspace Review");

    assert_review_pr_mode_error(error);
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .is_none());
}

#[tokio::test]
async fn start_workspace_review_fixer_rejects_review_pr_mode_before_monitor_write() {
    let state = AppState::new_test();
    let workspace = review_pr_workspace();

    let error = start_agent_workspace_review_blocking_fixer(&state, &workspace)
        .await
        .expect_err("Review PR workspaces must not start workspace Review fixer");

    assert_review_pr_mode_error(error);
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .is_none());
}

#[tokio::test]
async fn approve_workspace_review_anyway_rejects_review_pr_mode_before_monitor_write() {
    let state = AppState::new_test();
    let workspace = review_pr_workspace();
    let snapshot = AgentWorkspaceReviewApprovalSnapshot {
        target_scope: AgentWorkspaceReviewTargetScope::WorkspaceDelta,
        diff_fingerprint: "review-pr-must-not-bypass".to_string(),
        artifact_id: ArtifactId::from_string("review-pr-artifact".to_string()),
        artifact_version: 1,
    };

    let error = approve_agent_workspace_review_anyway(&state, &workspace, &snapshot)
        .await
        .expect_err("Review PR workspaces must not allow a workspace Review bypass");

    assert_review_pr_mode_error(error);
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .is_none());
}

use std::sync::Arc;

use crate::application::agent_workspace_review::{
    load_agent_workspace_review_context, start_agent_workspace_review,
    start_agent_workspace_review_blocking_fixer, workspace_review_mode_is_eligible,
};
use crate::application::agent_workspace_review_approval::approve_agent_workspace_review_anyway;
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentWorkspaceReviewApprovalSnapshot, AgentWorkspaceReviewTargetScope, ArtifactId,
    ChatConversationId, IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::error::AppError;

fn workspace_with_mode(mode: AgentConversationWorkspaceMode) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        ChatConversationId::from_string("review-pr-mode-guard".to_string()),
        ProjectId::from_string("project-review-pr-mode-guard".to_string()),
        mode,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/review-pr-mode-guard".to_string(),
        "/tmp/ralphx-review-pr-mode-guard".to_string(),
    )
}

fn review_pr_workspace() -> AgentConversationWorkspace {
    workspace_with_mode(AgentConversationWorkspaceMode::ReviewPr)
}

fn assert_ineligible_mode_error(error: AppError) {
    assert!(matches!(
        error,
        AppError::Validation(message) if message.contains("unavailable in")
    ));
}

#[test]
fn workspace_review_mode_eligibility_is_edit_and_ideation_only() {
    assert!(workspace_review_mode_is_eligible(
        AgentConversationWorkspaceMode::Edit
    ));
    assert!(workspace_review_mode_is_eligible(
        AgentConversationWorkspaceMode::Ideation
    ));
    assert!(!workspace_review_mode_is_eligible(
        AgentConversationWorkspaceMode::Plan
    ));
    assert!(!workspace_review_mode_is_eligible(
        AgentConversationWorkspaceMode::ReviewPr
    ));
    assert!(!workspace_review_mode_is_eligible(
        AgentConversationWorkspaceMode::Chat
    ));
}

#[tokio::test]
async fn load_workspace_review_context_rejects_review_pr_mode_before_project_lookup() {
    let state = AppState::new_test();
    let workspace = review_pr_workspace();

    let error = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect_err("Review PR workspaces must not expose workspace Review context");

    assert_ineligible_mode_error(error);
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

    assert_ineligible_mode_error(error);
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

    assert_ineligible_mode_error(error);
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

    assert_ineligible_mode_error(error);
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .is_none());
}

#[tokio::test]
async fn stale_edit_copy_cannot_bypass_current_persisted_plan_mode() {
    let state = AppState::new_test();
    let stale_edit_workspace = workspace_with_mode(AgentConversationWorkspaceMode::Edit);
    let mut persisted_plan_workspace = stale_edit_workspace.clone();
    persisted_plan_workspace.mode = AgentConversationWorkspaceMode::Plan;
    state
        .agent_conversation_workspace_repo
        .create_or_update(persisted_plan_workspace)
        .await
        .expect("PLAN workspace should persist");

    let error = load_agent_workspace_review_context(&state, &stale_edit_workspace)
        .await
        .expect_err("persisted PLAN mode must override a stale Edit copy");

    assert_ineligible_mode_error(error);
    assert!(state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&stale_edit_workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .is_none());
}

use crate::application::agent_workspace_review_context::{
    load_agent_workspace_review_presentation_context, AgentWorkspaceReviewContextReadMode,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspaceReviewGateStatus,
    AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus,
    AgentWorkspaceReviewTargetScope, ChatConversationId, IdeationAnalysisBaseRefKind, ProjectId,
};
use crate::AppError;

fn workspace(
    conversation_id: ChatConversationId,
    project_id: ProjectId,
) -> AgentConversationWorkspace {
    AgentConversationWorkspace::new(
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/test/context-snapshot".to_string(),
        "/path/that/must/not/be-read-for-status".to_string(),
    )
}

#[tokio::test]
async fn complete_reviewing_monitor_uses_status_snapshot_without_project_or_git_reads() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::new();
    let workspace = workspace(conversation_id.clone(), project_id.clone());
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project_id);
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    monitor.current_target_scope = Some(AgentWorkspaceReviewTargetScope::WorkspaceDelta);
    monitor.current_diff_fingerprint = Some("current-fingerprint".to_string());
    monitor.workspace_base_ref = Some("main".to_string());
    monitor.workspace_base_sha = Some("base-sha".to_string());
    monitor.workspace_head_ref = Some("HEAD".to_string());
    monitor.workspace_head_sha = Some("head-sha".to_string());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("seed monitor");

    let context = load_agent_workspace_review_presentation_context(
        &state,
        &workspace,
        AgentWorkspaceReviewContextReadMode::StatusSnapshot,
    )
    .await
    .expect("load status snapshot");

    assert_eq!(
        context.monitor.status,
        AgentWorkspaceReviewMonitorStatus::Reviewing
    );
    assert_eq!(
        context
            .target
            .as_ref()
            .map(|target| target.diff_fingerprint.as_str()),
        Some("current-fingerprint")
    );
    assert!(context
        .target
        .as_ref()
        .expect("snapshot target")
        .review_packet
        .changed_files
        .is_empty());
}

#[tokio::test]
async fn incomplete_reviewing_monitor_fails_closed_instead_of_becoming_idle() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::new();
    let workspace = workspace(conversation_id.clone(), project_id.clone());
    let mut monitor = AgentWorkspaceReviewMonitor::new(conversation_id, project_id);
    monitor.status = AgentWorkspaceReviewMonitorStatus::Reviewing;
    monitor.review_gate_status = AgentWorkspaceReviewGateStatus::Reviewing;
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await
        .expect("seed monitor");

    let error = load_agent_workspace_review_presentation_context(
        &state,
        &workspace,
        AgentWorkspaceReviewContextReadMode::StatusSnapshot,
    )
    .await
    .expect_err("incomplete reviewing state must fail closed");

    assert!(matches!(error, AppError::Conflict(_)));
}

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;

use super::*;
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspaceSourcePullRequest,
    ChatConversationId, IdeationAnalysisBaseRefKind, Project,
};
use crate::domain::services::github_service::{
    GithubServiceTrait, PrHealth, PrStatus, PrSyncState,
};
use crate::tests::mock_github_service::MockGithubService;

#[tokio::test]
async fn reconciles_live_terminal_state_before_returning_actions() {
    let project_root = tempfile::TempDir::new().expect("project tempdir");
    let mut app_state = AppState::new_test();
    let github = Arc::new(MockGithubService::new());
    github.state().fetch_pr_health_result = Some(Ok(PrHealth {
        sync_state: PrSyncState {
            status: PrStatus::Merged {
                merge_commit_sha: Some("a".repeat(40)),
                merged_at: None,
            },
            merge_state_status: None,
            mergeable: None,
            is_draft: false,
            head_ref_name: "feature/review-workflow".to_string(),
            base_ref_name: "main".to_string(),
            head_ref_oid: Some("head-sha".to_string()),
            base_ref_oid: None,
        },
        review_decision: None,
        checks: Vec::new(),
        issue_comments: Vec::new(),
        auto_merge_request: None,
    }));
    app_state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
    let app_state = Arc::new(app_state);

    let project = Project::new(
        "Review PR context".to_string(),
        project_root.path().to_string_lossy().to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let conversation_id = ChatConversationId::new();
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id.clone(),
        project.id.clone(),
        AgentConversationWorkspaceMode::ReviewPr,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("0".repeat(40)),
        "feature/review-workflow".to_string(),
        project_root
            .path()
            .join("missing-review-worktree")
            .to_string_lossy()
            .to_string(),
    );
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: 411,
        url: Some("https://github.com/mock/project/pull/411".to_string()),
        title: Some("Fix review workflow".to_string()),
        head_ref_name: "feature/review-workflow".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("head-sha".to_string()),
    });
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();
    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id.clone(),
        project.id,
        411,
        Some("head-sha".to_string()),
    );
    monitor.status = AgentWorkspacePrReviewMonitorStatus::AwaitingUser;
    app_state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .unwrap();
    app_state
        .agent_conversation_workspace_repo
        .create_or_update_pr_review_action(AgentWorkspacePrReviewAction::new(
            conversation_id.clone(),
            411,
            "head-sha".to_string(),
            AgentWorkspacePrReviewActionKind::Approve,
            "Ready".to_string(),
            "Approve this PR".to_string(),
            None,
            Some("run-1".to_string()),
        ))
        .await
        .unwrap();

    let Json(response) = get_agent_workspace_pr_review_context(
        State(HttpServerState::new_test(Arc::clone(&app_state))),
        Path(conversation_id.to_string()),
    )
    .await
    .expect("terminal Review PR context should converge");

    assert_eq!(
        response.workspace.publication_pr_status.as_deref(),
        Some("merged")
    );
    assert_eq!(response.monitor.unwrap().status, "terminal");
    assert!(response.pending_action.is_none());
    assert!(response
        .recent_actions
        .iter()
        .all(|action| action.status == "superseded"));
    assert_eq!(
        response
            .events
            .iter()
            .filter(|event| event.step == "pr_merged")
            .count(),
        1
    );
}

use std::sync::Arc;

use axum::{extract::Path, extract::State, http::StatusCode, Json};
use ralphx_lib::application::{AppState, TeamService, TeamStateTracker};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspacePrReviewMonitor,
    AgentWorkspacePrReviewMonitorStatus, AgentWorkspaceSourcePullRequest, ArtifactId,
    ChatConversationId, IdeationAnalysisBaseRefKind, NotificationCategory, NotificationTargetKind,
    ProjectId,
};
use ralphx_lib::domain::services::github_service::GithubServiceTrait;
use ralphx_lib::http_server::handlers::agent_workspaces::{
    propose_agent_workspace_pr_review_action, submit_agent_workspace_pr_review_action,
    ProposeAgentWorkspacePrReviewActionRequest, SubmitAgentWorkspacePrReviewActionRequest,
};
use ralphx_lib::http_server::types::HttpServerState;

use crate::common::MockGithubService;

fn test_http_state(app_state: Arc<AppState>) -> HttpServerState {
    let tracker = TeamStateTracker::new();
    let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
    HttpServerState {
        app_state,
        execution_state: Arc::new(ExecutionState::new()),
        team_tracker: tracker,
        team_service,
        delegation_service: Default::default(),
    }
}

async fn setup_review_workspace(
    with_github: bool,
) -> (Arc<AppState>, HttpServerState, AgentConversationWorkspace) {
    let mut app_state = AppState::new_test();
    if with_github {
        app_state.github_service =
            Some(Arc::new(MockGithubService::new()) as Arc<dyn GithubServiceTrait>);
    }
    let app_state = Arc::new(app_state);
    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::from_string("project-pr-review-notifications".to_string());
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project_id,
        AgentConversationWorkspaceMode::ReviewPr,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-sha".to_string()),
        "ralphx/test/pr-review-notifications".to_string(),
        "/tmp/ralphx-pr-review-notifications".to_string(),
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
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id,
        workspace.project_id.clone(),
        411,
        Some("head-sha".to_string()),
    );
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
    monitor.review_artifact_id = Some(ArtifactId::from_string("review-artifact-1".to_string()));
    monitor.review_artifact_head_sha = Some("head-sha".to_string());
    monitor.review_artifact_version = Some(1);
    app_state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .expect("review monitor should persist");

    let state = test_http_state(Arc::clone(&app_state));
    (app_state, state, workspace)
}

fn proposal_request() -> ProposeAgentWorkspacePrReviewActionRequest {
    ProposeAgentWorkspacePrReviewActionRequest {
        head_sha: "head-sha".to_string(),
        proposed_action: "request_changes".to_string(),
        summary: "Found a blocking regression".to_string(),
        review_body: "Please fix the regression before merge.".to_string(),
        findings_json: None,
        created_by_run_id: Some("run-1".to_string()),
    }
}

#[tokio::test]
async fn propose_pr_review_action_records_one_durable_action_notification_with_conversation_target()
{
    let (app_state, state, workspace) = setup_review_workspace(false).await;

    let Json(response) = propose_agent_workspace_pr_review_action(
        State(state),
        Path(workspace.conversation_id.to_string()),
        Json(proposal_request()),
    )
    .await
    .expect("proposal should enter AwaitingUser");

    let notifications = app_state
        .notification_repo
        .list(None, None, 50)
        .await
        .expect("notifications should list")
        .notifications;
    assert_eq!(notifications.len(), 1);
    let notification = &notifications[0];
    assert_eq!(notification.category, NotificationCategory::PrReviewAction);
    assert_eq!(
        notification.target.kind,
        NotificationTargetKind::AgentConversation
    );
    let project_id = workspace.project_id.to_string();
    let conversation_id = workspace.conversation_id.to_string();
    assert_eq!(
        notification.target.project_id.as_deref(),
        Some(project_id.as_str())
    );
    assert_eq!(
        notification.target.conversation_id.as_deref(),
        Some(conversation_id.as_str())
    );
    assert_eq!(
        notification.dedupe_key.as_deref(),
        Some(
            format!(
                "pr-review:{}:awaiting_user:{}",
                workspace.conversation_id, response.action.id
            )
            .as_str()
        )
    );
}

#[tokio::test]
async fn pr_review_submit_failure_does_not_duplicate_existing_awaiting_user_notification() {
    let (app_state, state, workspace) = setup_review_workspace(true).await;

    let Json(response) = propose_agent_workspace_pr_review_action(
        State(state.clone()),
        Path(workspace.conversation_id.to_string()),
        Json(proposal_request()),
    )
    .await
    .expect("proposal should enter AwaitingUser");
    let initial_notification = app_state
        .notification_repo
        .list(None, None, 50)
        .await
        .expect("notifications should list")
        .notifications
        .into_iter()
        .find(|notification| notification.category == NotificationCategory::PrReviewAction)
        .expect("AwaitingUser notification should exist");

    let (status, _) = submit_agent_workspace_pr_review_action(
        State(state),
        Path((workspace.conversation_id.to_string(), response.action.id)),
        Json(SubmitAgentWorkspacePrReviewActionRequest { action_kind: None }),
    )
    .await
    .expect_err("failed submit should return Bad Gateway and re-enter AwaitingUser");
    assert_eq!(status, StatusCode::BAD_GATEWAY);

    let notifications = app_state
        .notification_repo
        .list(None, None, 50)
        .await
        .expect("notifications should list")
        .notifications
        .into_iter()
        .filter(|notification| notification.category == NotificationCategory::PrReviewAction)
        .collect::<Vec<_>>();
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].dedupe_key, initial_notification.dedupe_key);
}

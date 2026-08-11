use std::sync::Arc;

use axum::extract::{Path, State};
use ralphx_lib::application::interactive_notification_producer::pr_review_notification_key;
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspacePrReviewAction,
    AgentWorkspacePrReviewActionKind, AgentWorkspacePrReviewActionStatus,
    AgentWorkspacePrReviewMonitor, AgentWorkspacePrReviewMonitorStatus,
    AgentWorkspaceSourcePullRequest, ArtifactId, ChatConversationId, IdeationAnalysisBaseRefKind,
    NewNotification, NotificationCategory, NotificationSeverity, NotificationTarget, ProjectId,
};
use ralphx_lib::domain::services::github_service::{
    GithubServiceTrait, PrMergeStateStatus, PrMergeableState, PrStatus, PrSyncState,
};
use ralphx_lib::http_server::handlers::agent_workspaces::{
    get_agent_workspace_pr_review_context, AgentWorkspacePrReviewActionHeadStatus,
};
use ralphx_lib::http_server::types::HttpServerState;
use ralphx_lib::infrastructure::sqlite::SqliteAgentConversationWorkspaceRepository;

use crate::support::mock_github_service::MockGithubService;

const PR_NUMBER: i64 = 411;

fn http_state(app_state: Arc<AppState>) -> HttpServerState {
    HttpServerState {
        app_state,
        execution_state: Arc::new(ExecutionState::new()),
        delegation_service: Default::default(),
    }
}

fn remote_head(head_sha: Option<&str>) -> PrSyncState {
    PrSyncState {
        status: PrStatus::Open,
        merge_state_status: Some(PrMergeStateStatus::Clean),
        mergeable: Some(PrMergeableState::Mergeable),
        is_draft: false,
        head_ref_name: "feature/review-reload".to_string(),
        base_ref_name: "main".to_string(),
        head_ref_oid: head_sha.map(str::to_string),
        base_ref_oid: Some("base-head".to_string()),
    }
}

async fn setup(
    verified_remote_head: Result<Option<&str>, &str>,
) -> (HttpServerState, ChatConversationId, Arc<MockGithubService>) {
    let github = Arc::new(MockGithubService::new());
    match verified_remote_head {
        Ok(head) => github.will_return_sync_state(remote_head(head)),
        Err(message) => github.will_fail_sync_state(message),
    }

    let mut app_state = AppState::new_sqlite_test();
    app_state.agent_conversation_workspace_repo = Arc::new(
        SqliteAgentConversationWorkspaceRepository::from_shared(Arc::clone(app_state.db.inner())),
    );
    app_state.github_service = Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>);
    let conversation_id = ChatConversationId::new();
    let project_id = ProjectId::from_string("project-pr-review-reload".to_string());
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project_id.clone(),
        AgentConversationWorkspaceMode::ReviewPr,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        Some("base-head".to_string()),
        "ralphx/test/review-reload".to_string(),
        "/tmp/ralphx-review-reload".to_string(),
    );
    workspace.source_pull_request = Some(AgentWorkspaceSourcePullRequest {
        number: PR_NUMBER,
        url: Some("https://github.com/example/repo/pull/411".to_string()),
        title: Some("Restore proposed action".to_string()),
        head_ref_name: "feature/review-reload".to_string(),
        base_ref_name: Some("main".to_string()),
        head_ref_oid: Some("source-snapshot-head".to_string()),
    });
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("seed Review PR workspace");

    let mut monitor = AgentWorkspacePrReviewMonitor::new(
        conversation_id,
        project_id,
        PR_NUMBER,
        Some("source-snapshot-head".to_string()),
    );
    monitor.status = AgentWorkspacePrReviewMonitorStatus::AwaitingUser;
    monitor.monitor_enabled = true;
    monitor.review_artifact_id = Some(ArtifactId::from_string("review-artifact"));
    monitor.review_artifact_head_sha = Some("action-head-a".to_string());
    monitor.review_artifact_version = Some(1);
    app_state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .expect("seed Review PR monitor");

    (http_state(Arc::new(app_state)), conversation_id, github)
}

async fn create_action(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    head_sha: &str,
) -> AgentWorkspacePrReviewAction {
    state
        .app_state
        .agent_conversation_workspace_repo
        .create_or_update_pr_review_action(AgentWorkspacePrReviewAction::new(
            *conversation_id,
            PR_NUMBER,
            head_sha.to_string(),
            AgentWorkspacePrReviewActionKind::RequestChanges,
            format!("Review proposal for {head_sha}"),
            format!("Please address findings on {head_sha}."),
            None,
            Some(format!("run-{head_sha}")),
        ))
        .await
        .expect("seed pending Review PR action")
}

async fn load_context(
    state: HttpServerState,
    conversation_id: &ChatConversationId,
) -> ralphx_lib::http_server::handlers::agent_workspaces::AgentWorkspacePrReviewContextResponse {
    get_agent_workspace_pr_review_context(State(state), Path(conversation_id.as_str()))
        .await
        .expect("load Review PR context")
        .0
}

async fn seed_action_notification(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    action_id: &str,
) -> String {
    let dedupe_key = pr_review_notification_key(conversation_id.as_str(), action_id);
    state
        .app_state
        .notification_service()
        .record(NewNotification {
            project_id: Some("project-pr-review-reload".to_string()),
            category: NotificationCategory::PrReviewAction,
            severity: NotificationSeverity::ActionRequired,
            title: "PR review needs a decision".to_string(),
            body: None,
            target: NotificationTarget::none(),
            dedupe_key: Some(dedupe_key.clone()),
        })
        .await;
    dedupe_key
}

async fn notification_is_unread(state: &HttpServerState, dedupe_key: &str) -> bool {
    state
        .app_state
        .notification_repo
        .list(None, None, 50)
        .await
        .expect("list notifications")
        .notifications
        .into_iter()
        .find(|notification| notification.dedupe_key.as_deref() == Some(dedupe_key))
        .expect("seeded notification remains")
        .read_at
        .is_none()
}

#[tokio::test]
async fn matching_remote_head_restores_pending_action_as_current_after_reload() {
    let (state, conversation_id, _github) = setup(Ok(Some("action-head-a"))).await;
    let action = create_action(&state, &conversation_id, "action-head-a").await;

    let context = load_context(state, &conversation_id).await;

    assert_eq!(
        context.pending_action.expect("pending action").id,
        action.id
    );
    assert_eq!(
        context.pending_action_head_status,
        Some(AgentWorkspacePrReviewActionHeadStatus::Current)
    );
}

#[tokio::test]
async fn current_remote_head_wins_over_a_newer_stale_pending_action() {
    let (state, conversation_id, _github) = setup(Ok(Some("action-head-a"))).await;
    let current = create_action(&state, &conversation_id, "action-head-a").await;
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    create_action(&state, &conversation_id, "newer-stale-head").await;

    let context = load_context(state, &conversation_id).await;

    assert_eq!(
        context.pending_action.expect("current action").id,
        current.id
    );
    assert_eq!(
        context.pending_action_head_status,
        Some(AgentWorkspacePrReviewActionHeadStatus::Current)
    );
}

#[tokio::test]
async fn changed_remote_head_keeps_stale_action_visible_without_mutating_state() {
    let (state, conversation_id, _github) = setup(Ok(Some("remote-head-b"))).await;
    let action = create_action(&state, &conversation_id, "action-head-a").await;
    let notification_key = seed_action_notification(&state, &conversation_id, &action.id).await;

    let context = load_context(state.clone(), &conversation_id).await;

    assert_eq!(context.pending_action.expect("stale action").id, action.id);
    assert_eq!(
        context.pending_action_head_status,
        Some(AgentWorkspacePrReviewActionHeadStatus::Stale)
    );
    assert_eq!(
        state
            .app_state
            .agent_conversation_workspace_repo
            .get_pr_review_action(&action.id)
            .await
            .expect("load action")
            .expect("action remains")
            .status,
        AgentWorkspacePrReviewActionStatus::Pending
    );
    assert_eq!(
        state
            .app_state
            .agent_conversation_workspace_repo
            .get_pr_review_monitor(&conversation_id)
            .await
            .expect("load monitor")
            .expect("monitor remains")
            .status,
        AgentWorkspacePrReviewMonitorStatus::AwaitingUser
    );
    assert!(notification_is_unread(&state, &notification_key).await);
}

#[tokio::test]
async fn remote_head_failure_keeps_pending_action_visible_as_unverified() {
    let (state, conversation_id, _github) = setup(Err("remote unavailable")).await;
    let action = create_action(&state, &conversation_id, "action-head-a").await;

    let context = load_context(state, &conversation_id).await;

    assert_eq!(
        context.pending_action.expect("unverified action").id,
        action.id
    );
    assert_eq!(
        context.pending_action_head_status,
        Some(AgentWorkspacePrReviewActionHeadStatus::Unverified)
    );
    assert_eq!(
        context.current_head_sha.as_deref(),
        Some("source-snapshot-head")
    );
}

#[tokio::test]
async fn awaiting_user_without_pending_action_is_reported_without_repair_mutation() {
    let (state, conversation_id, _github) = setup(Ok(Some("action-head-a"))).await;

    let context = load_context(state.clone(), &conversation_id).await;

    assert!(context.pending_action.is_none());
    assert!(context.pending_action_head_status.is_none());
    assert_eq!(
        state
            .app_state
            .agent_conversation_workspace_repo
            .get_pr_review_monitor(&conversation_id)
            .await
            .expect("load monitor")
            .expect("monitor remains")
            .status,
        AgentWorkspacePrReviewMonitorStatus::AwaitingUser
    );
}

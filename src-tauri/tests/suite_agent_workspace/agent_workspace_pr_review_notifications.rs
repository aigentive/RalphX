use std::sync::Arc;

use axum::{extract::Path, extract::State, http::StatusCode, Json};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspacePrReviewActionKind,
    AgentWorkspacePrReviewActionStatus, AgentWorkspacePrReviewMonitor,
    AgentWorkspacePrReviewMonitorStatus, AgentWorkspaceSourcePullRequest, ArtifactId,
    ChatConversationId, IdeationAnalysisBaseRefKind, NotificationCategory, NotificationTargetKind,
    ProjectId,
};
use ralphx_lib::domain::services::github_service::{GithubServiceTrait, PrStatus, PrSyncState};
use ralphx_lib::http_server::handlers::agent_workspaces::{
    propose_agent_workspace_pr_review_action, skip_agent_workspace_pr_review_action,
    submit_agent_workspace_pr_review_action, update_agent_workspace_pr_review_settings,
    ProposeAgentWorkspacePrReviewActionRequest, SkipAgentWorkspacePrReviewActionRequest,
    SubmitAgentWorkspacePrReviewActionRequest, UpdateAgentWorkspacePrReviewSettingsRequest,
};
use ralphx_lib::http_server::types::HttpServerState;

use crate::common::MockGithubService;

fn test_http_state(app_state: Arc<AppState>) -> HttpServerState {
    HttpServerState {
        app_state,
        execution_state: Arc::new(ExecutionState::new()),
        delegation_service: Default::default(),
    }
}

async fn setup_review_workspace(
    with_github: bool,
) -> (
    Arc<AppState>,
    HttpServerState,
    AgentConversationWorkspace,
    Option<Arc<MockGithubService>>,
) {
    setup_review_workspace_with_monitor(with_github, true).await
}

async fn setup_review_workspace_with_monitor(
    with_github: bool,
    seed_monitor: bool,
) -> (
    Arc<AppState>,
    HttpServerState,
    AgentConversationWorkspace,
    Option<Arc<MockGithubService>>,
) {
    let mut app_state = AppState::new_test();
    let github = with_github.then(|| Arc::new(MockGithubService::new()));
    if let Some(github) = github.as_ref() {
        app_state.github_service = Some(Arc::clone(github) as Arc<dyn GithubServiceTrait>);
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

    if seed_monitor {
        let mut monitor = AgentWorkspacePrReviewMonitor::new(
            conversation_id,
            workspace.project_id.clone(),
            411,
            Some("head-sha".to_string()),
        );
        monitor.status = AgentWorkspacePrReviewMonitorStatus::Watching;
        monitor.monitor_enabled = true;
        monitor.review_artifact_id = Some(ArtifactId::from_string("review-artifact-1".to_string()));
        monitor.review_artifact_head_sha = Some("head-sha".to_string());
        monitor.review_artifact_version = Some(1);
        app_state
            .agent_conversation_workspace_repo
            .upsert_pr_review_monitor(monitor)
            .await
            .expect("review monitor should persist");
    }

    let state = test_http_state(Arc::clone(&app_state));
    (app_state, state, workspace, github)
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

fn approve_request() -> ProposeAgentWorkspacePrReviewActionRequest {
    ProposeAgentWorkspacePrReviewActionRequest {
        proposed_action: "approve".to_string(),
        summary: "No blocking findings".to_string(),
        review_body: "Approved after reviewing the current PR head.".to_string(),
        ..proposal_request()
    }
}

fn current_head_sync_state() -> PrSyncState {
    PrSyncState {
        status: PrStatus::Open,
        merge_state_status: None,
        mergeable: None,
        is_draft: false,
        head_ref_name: "feature/review-workflow".to_string(),
        base_ref_name: "main".to_string(),
        head_ref_oid: Some("head-sha".to_string()),
        base_ref_oid: Some("base-sha".to_string()),
    }
}

async fn enable_subsequent_auto_approval(
    app_state: &Arc<AppState>,
    workspace: &AgentConversationWorkspace,
) {
    let mut monitor = app_state
        .agent_conversation_workspace_repo
        .get_pr_review_monitor(&workspace.conversation_id)
        .await
        .unwrap()
        .expect("review monitor should exist");
    monitor.monitor_enabled = true;
    monitor.last_review_run_id = Some("run-1".to_string());
    app_state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .unwrap();
    app_state
        .agent_conversation_workspace_repo
        .mark_pr_review_first_action_resolved(&workspace.conversation_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn propose_pr_review_action_records_one_durable_action_notification_with_conversation_target()
{
    let (app_state, state, workspace, github) = setup_review_workspace(true).await;
    github
        .expect("GitHub mock should exist")
        .will_return_sync_state(current_head_sync_state());

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
    let (app_state, state, workspace, github) = setup_review_workspace(true).await;
    let github = github.expect("GitHub mock should exist");
    github.will_return_sync_state(current_head_sync_state());
    github.will_return_sync_state(current_head_sync_state());

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

#[tokio::test]
async fn first_passing_pr_review_stays_manual_even_when_auto_approve_defaults_on() {
    let (app_state, state, workspace, github) = setup_review_workspace(true).await;
    let github = github.expect("GitHub mock should exist");
    github.will_return_sync_state(current_head_sync_state());
    github.will_submit_pr_review("review-1", None);

    let Json(response) = propose_agent_workspace_pr_review_action(
        State(state),
        Path(workspace.conversation_id.to_string()),
        Json(approve_request()),
    )
    .await
    .expect("first proposal should await manual review");

    assert_eq!(
        response.action.status,
        AgentWorkspacePrReviewActionStatus::Pending.to_string()
    );
    assert_eq!(response.monitor.status, "awaiting_user");
    assert!(response.monitor.auto_approve_enabled);
    assert!(!response.monitor.first_action_resolved);
    assert_eq!(github.submit_review_calls(), 0);
    assert_eq!(
        app_state
            .notification_repo
            .list(None, None, 50)
            .await
            .unwrap()
            .notifications
            .len(),
        1
    );
}

#[tokio::test]
async fn review_pr_auto_approve_setting_persists_per_conversation() {
    let (_app_state, state, workspace, _) = setup_review_workspace(false).await;

    let Json(disabled) = update_agent_workspace_pr_review_settings(
        State(state.clone()),
        Path(workspace.conversation_id.to_string()),
        Json(UpdateAgentWorkspacePrReviewSettingsRequest {
            auto_approve_enabled: Some(false),
            monitor_enabled: None,
            active_review_policy: None,
        }),
    )
    .await
    .expect("setting should save");
    assert!(!disabled.monitor.auto_approve_enabled);
    assert!(!disabled.monitor.first_action_resolved);

    let Json(enabled) = update_agent_workspace_pr_review_settings(
        State(state),
        Path(workspace.conversation_id.to_string()),
        Json(UpdateAgentWorkspacePrReviewSettingsRequest {
            auto_approve_enabled: Some(true),
            monitor_enabled: None,
            active_review_policy: None,
        }),
    )
    .await
    .expect("setting should update");
    assert!(enabled.monitor.auto_approve_enabled);
}

#[tokio::test]
async fn review_pr_auto_approve_setting_without_monitor_does_not_enable_monitoring() {
    let (app_state, state, workspace, _) = setup_review_workspace_with_monitor(false, false).await;

    assert!(app_state
        .agent_conversation_workspace_repo
        .get_pr_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .is_none());

    let Json(response) = update_agent_workspace_pr_review_settings(
        State(state),
        Path(workspace.conversation_id.to_string()),
        Json(UpdateAgentWorkspacePrReviewSettingsRequest {
            auto_approve_enabled: Some(false),
            monitor_enabled: None,
            active_review_policy: None,
        }),
    )
    .await
    .expect("auto approve setting should save without starting monitoring");

    assert!(!response.monitor.auto_approve_enabled);
    assert!(!response.monitor.monitor_enabled);
    assert_eq!(response.monitor.status, "idle");
    let persisted = app_state
        .agent_conversation_workspace_repo
        .get_pr_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("settings row should persist");
    assert!(!persisted.auto_approve_enabled);
    assert!(!persisted.monitor_enabled);
    assert_eq!(persisted.status, AgentWorkspacePrReviewMonitorStatus::Idle);
}

#[tokio::test]
async fn review_pr_monitoring_pause_and_restart_persist_independently_of_auto_approve() {
    let (_app_state, state, workspace, _) = setup_review_workspace(false).await;

    let Json(paused) = update_agent_workspace_pr_review_settings(
        State(state.clone()),
        Path(workspace.conversation_id.to_string()),
        Json(UpdateAgentWorkspacePrReviewSettingsRequest {
            auto_approve_enabled: None,
            monitor_enabled: Some(false),
            active_review_policy: None,
        }),
    )
    .await
    .expect("monitor should pause");
    assert!(!paused.monitor.monitor_enabled);
    assert_eq!(paused.monitor.status, "paused");
    assert!(paused.monitor.auto_approve_enabled);

    let Json(restarted) = update_agent_workspace_pr_review_settings(
        State(state),
        Path(workspace.conversation_id.to_string()),
        Json(UpdateAgentWorkspacePrReviewSettingsRequest {
            auto_approve_enabled: None,
            monitor_enabled: Some(true),
            active_review_policy: None,
        }),
    )
    .await
    .expect("monitor should restart");
    assert!(restarted.monitor.monitor_enabled);
    assert_eq!(restarted.monitor.status, "watching");
    assert!(restarted.monitor.auto_approve_enabled);
}

#[tokio::test]
async fn stopping_an_active_pr_review_requires_an_explicit_policy() {
    let (app_state, state, workspace, _) = setup_review_workspace(false).await;
    let mut monitor = app_state
        .agent_conversation_workspace_repo
        .get_pr_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("monitor should exist");
    monitor.status = AgentWorkspacePrReviewMonitorStatus::Reviewing;
    app_state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(monitor)
        .await
        .expect("active monitor should persist");

    let (status, Json(body)) = update_agent_workspace_pr_review_settings(
        State(state.clone()),
        Path(workspace.conversation_id.to_string()),
        Json(UpdateAgentWorkspacePrReviewSettingsRequest {
            auto_approve_enabled: None,
            monitor_enabled: Some(false),
            active_review_policy: None,
        }),
    )
    .await
    .expect_err("active Review PR monitor must require a stop policy");
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "active_review_choice_required");

    let Json(paused) = update_agent_workspace_pr_review_settings(
        State(state),
        Path(workspace.conversation_id.to_string()),
        Json(UpdateAgentWorkspacePrReviewSettingsRequest {
            auto_approve_enabled: None,
            monitor_enabled: Some(false),
            active_review_policy: Some("finish_current".to_string()),
        }),
    )
    .await
    .expect("finish-current stop policy should pause monitoring");
    assert!(!paused.monitor.monitor_enabled);
    assert_eq!(paused.monitor.status, "paused");
}

#[tokio::test]
async fn review_pr_auto_approve_setting_rejects_ineligible_workspaces() {
    let (app_state, state, mut workspace, _) = setup_review_workspace(false).await;
    workspace.mode = AgentConversationWorkspaceMode::Edit;
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();

    let (status, _) = update_agent_workspace_pr_review_settings(
        State(state.clone()),
        Path(workspace.conversation_id.to_string()),
        Json(UpdateAgentWorkspacePrReviewSettingsRequest {
            auto_approve_enabled: Some(false),
            monitor_enabled: None,
            active_review_policy: None,
        }),
    )
    .await
    .expect_err("non-Review PR workspaces cannot enable Auto Approve");
    assert_eq!(status, StatusCode::BAD_REQUEST);

    workspace.mode = AgentConversationWorkspaceMode::ReviewPr;
    workspace.source_pull_request = None;
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();
    let (status, _) = update_agent_workspace_pr_review_settings(
        State(state),
        Path(workspace.conversation_id.to_string()),
        Json(UpdateAgentWorkspacePrReviewSettingsRequest {
            auto_approve_enabled: Some(false),
            monitor_enabled: None,
            active_review_policy: None,
        }),
    )
    .await
    .expect_err("Review PR workspaces require a linked pull request");
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn manual_pr_review_submission_resolves_first_action_and_keeps_monitoring() {
    let (_app_state, state, workspace, github) = setup_review_workspace(true).await;
    let github = github.expect("GitHub mock should exist");
    github.will_return_sync_state(current_head_sync_state());
    github.will_return_sync_state(current_head_sync_state());
    github.will_submit_pr_review("review-manual", None);

    let Json(proposal) = propose_agent_workspace_pr_review_action(
        State(state.clone()),
        Path(workspace.conversation_id.to_string()),
        Json(approve_request()),
    )
    .await
    .expect("first review should await a manual action");

    let Json(submitted) = submit_agent_workspace_pr_review_action(
        State(state),
        Path((workspace.conversation_id.to_string(), proposal.action.id)),
        Json(SubmitAgentWorkspacePrReviewActionRequest { action_kind: None }),
    )
    .await
    .expect("manual approval should submit");

    assert_eq!(
        submitted.action.status,
        AgentWorkspacePrReviewActionStatus::Submitted.to_string()
    );
    assert_eq!(submitted.monitor.status, "watching");
    assert!(submitted.monitor.first_action_resolved);
    assert_eq!(github.submit_review_calls(), 1);
}

#[tokio::test]
async fn skipped_pr_review_action_resolves_first_action_without_enabling_monitoring() {
    let (_app_state, state, workspace, github) = setup_review_workspace(true).await;
    github
        .expect("GitHub mock should exist")
        .will_return_sync_state(current_head_sync_state());
    let Json(proposal) = propose_agent_workspace_pr_review_action(
        State(state.clone()),
        Path(workspace.conversation_id.to_string()),
        Json(proposal_request()),
    )
    .await
    .expect("review action should await a manual decision");

    let Json(skipped) = skip_agent_workspace_pr_review_action(
        State(state),
        Path((workspace.conversation_id.to_string(), proposal.action.id)),
        Json(SkipAgentWorkspacePrReviewActionRequest {
            reason: Some("The author will follow up manually".to_string()),
        }),
    )
    .await
    .expect("skipping should resolve the first action");

    assert_eq!(
        skipped.action.status,
        AgentWorkspacePrReviewActionStatus::Skipped.to_string()
    );
    assert_eq!(skipped.monitor.status, "watching");
    assert!(skipped.monitor.first_action_resolved);
}

#[tokio::test]
async fn skip_rejects_pending_action_for_a_previous_workspace_pr_without_mutation() {
    let (app_state, state, mut workspace, github) = setup_review_workspace(true).await;
    github
        .expect("GitHub mock should exist")
        .will_return_sync_state(current_head_sync_state());
    let Json(proposal) = propose_agent_workspace_pr_review_action(
        State(state.clone()),
        Path(workspace.conversation_id.to_string()),
        Json(proposal_request()),
    )
    .await
    .expect("review action should await a manual decision");

    let source_pr = workspace
        .source_pull_request
        .as_mut()
        .expect("Review PR workspace should have a source PR");
    source_pr.number = 412;
    source_pr.url = Some("https://github.com/mock/project/pull/412".to_string());
    app_state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("retargeted workspace should persist");

    let (status, _) = skip_agent_workspace_pr_review_action(
        State(state),
        Path((
            workspace.conversation_id.to_string(),
            proposal.action.id.clone(),
        )),
        Json(SkipAgentWorkspacePrReviewActionRequest {
            reason: Some("Stale UI action".to_string()),
        }),
    )
    .await
    .expect_err("an action for the previous PR must not be skipped");

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        app_state
            .agent_conversation_workspace_repo
            .get_pr_review_action(&proposal.action.id)
            .await
            .unwrap()
            .expect("action should remain available")
            .status,
        AgentWorkspacePrReviewActionStatus::Pending
    );
    let monitor = app_state
        .agent_conversation_workspace_repo
        .get_pr_review_monitor(&workspace.conversation_id)
        .await
        .unwrap()
        .expect("monitor should remain available");
    assert_eq!(
        monitor.status,
        AgentWorkspacePrReviewMonitorStatus::AwaitingUser
    );
    assert!(!monitor.first_action_resolved);
    let notifications = app_state
        .notification_repo
        .list(None, None, 50)
        .await
        .unwrap()
        .notifications;
    assert_eq!(notifications.len(), 1);
    assert!(notifications[0].read_at.is_none());
}

#[tokio::test]
async fn pr_review_submission_fails_closed_when_current_head_cannot_be_verified() {
    let (app_state, state, workspace, github) = setup_review_workspace(true).await;
    let github = github.expect("GitHub mock should exist");
    github.will_return_sync_state(current_head_sync_state());
    let Json(proposal) = propose_agent_workspace_pr_review_action(
        State(state.clone()),
        Path(workspace.conversation_id.to_string()),
        Json(approve_request()),
    )
    .await
    .expect("review action should await a manual decision");
    github.will_fail_sync_state("GitHub is temporarily unavailable");

    let (status, _) = submit_agent_workspace_pr_review_action(
        State(state),
        Path((
            workspace.conversation_id.to_string(),
            proposal.action.id.clone(),
        )),
        Json(SubmitAgentWorkspacePrReviewActionRequest { action_kind: None }),
    )
    .await
    .expect_err("submission must not trust a stale local PR head");
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(github.submit_review_calls(), 0);
    assert_eq!(
        app_state
            .agent_conversation_workspace_repo
            .get_pr_review_action(&proposal.action.id)
            .await
            .unwrap()
            .expect("action should remain available")
            .status,
        AgentWorkspacePrReviewActionStatus::Pending
    );
}

#[tokio::test]
async fn pr_review_submission_rejects_a_changed_remote_head_without_claiming_action() {
    let (app_state, state, workspace, github) = setup_review_workspace(true).await;
    let github = github.expect("GitHub mock should exist");
    github.will_return_sync_state(current_head_sync_state());
    let Json(proposal) = propose_agent_workspace_pr_review_action(
        State(state.clone()),
        Path(workspace.conversation_id.to_string()),
        Json(proposal_request()),
    )
    .await
    .expect("review action should await a manual decision");
    let mut changed_head = current_head_sync_state();
    changed_head.head_ref_oid = Some("new-head-sha".to_string());
    github.will_return_sync_state(changed_head);

    let (status, _) = submit_agent_workspace_pr_review_action(
        State(state),
        Path((
            workspace.conversation_id.to_string(),
            proposal.action.id.clone(),
        )),
        Json(SubmitAgentWorkspacePrReviewActionRequest { action_kind: None }),
    )
    .await
    .expect_err("submission must reject an action for an older remote head");

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(github.submit_review_calls(), 0);
    assert_eq!(
        app_state
            .agent_conversation_workspace_repo
            .get_pr_review_action(&proposal.action.id)
            .await
            .unwrap()
            .expect("action should remain available")
            .status,
        AgentWorkspacePrReviewActionStatus::Pending
    );
    let monitor = app_state
        .agent_conversation_workspace_repo
        .get_pr_review_monitor(&workspace.conversation_id)
        .await
        .unwrap()
        .expect("monitor should remain available");
    assert_eq!(
        monitor.status,
        AgentWorkspacePrReviewMonitorStatus::AwaitingUser
    );
    assert!(!monitor.first_action_resolved);
}

#[tokio::test]
async fn passing_subsequent_pr_review_auto_submits_once_without_action_notification() {
    let (app_state, state, workspace, github) = setup_review_workspace(true).await;
    let github = github.expect("GitHub mock should exist");
    enable_subsequent_auto_approval(&app_state, &workspace).await;
    github.will_return_sync_state(current_head_sync_state());
    github.will_return_sync_state(current_head_sync_state());
    github.will_submit_pr_review(
        "review-2",
        Some("https://github.com/mock/review/2".to_string()),
    );

    let Json(response) = propose_agent_workspace_pr_review_action(
        State(state),
        Path(workspace.conversation_id.to_string()),
        Json(approve_request()),
    )
    .await
    .expect("passing subsequent proposal should auto-submit");

    assert_eq!(
        response.action.proposed_action,
        AgentWorkspacePrReviewActionKind::Approve.to_string()
    );
    assert_eq!(
        response.action.status,
        AgentWorkspacePrReviewActionStatus::Submitted.to_string()
    );
    assert_eq!(response.monitor.status, "watching");
    assert!(response.monitor.first_action_resolved);
    assert_eq!(github.submit_review_calls(), 1);
    assert!(app_state
        .notification_repo
        .list(None, None, 50)
        .await
        .unwrap()
        .notifications
        .is_empty());
}

#[tokio::test]
async fn failed_auto_approval_restores_manual_pending_action_and_one_notification() {
    let (app_state, state, workspace, github) = setup_review_workspace(true).await;
    let github = github.expect("GitHub mock should exist");
    enable_subsequent_auto_approval(&app_state, &workspace).await;
    github.will_return_sync_state(current_head_sync_state());
    github.will_return_sync_state(current_head_sync_state());
    github.will_fail_submit_pr_review("temporary GitHub outage");

    let Json(response) = propose_agent_workspace_pr_review_action(
        State(state),
        Path(workspace.conversation_id.to_string()),
        Json(approve_request()),
    )
    .await
    .expect("failed automatic submission should preserve a manual fallback");

    assert_eq!(
        response.action.status,
        AgentWorkspacePrReviewActionStatus::Pending.to_string()
    );
    assert_eq!(response.monitor.status, "awaiting_user");
    assert!(response
        .monitor
        .last_error
        .as_deref()
        .is_some_and(|error| error.contains("temporary GitHub outage")));
    assert_eq!(github.submit_review_calls(), 1);
    assert_eq!(
        app_state
            .notification_repo
            .list(None, None, 50)
            .await
            .unwrap()
            .notifications
            .len(),
        1
    );
}

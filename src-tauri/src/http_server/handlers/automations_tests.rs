use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    Json,
};
use chrono::Utc;

use super::*;
use crate::application::{AppState, TeamService, TeamStateTracker};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    Automation, AutomationId, AutomationStatus, ChatConversation, ChatConversationId, ProjectId,
};

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

fn caller_headers(conversation_id: &ChatConversationId) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        CALLER_SESSION_ID_HEADER,
        HeaderValue::from_str(&conversation_id.as_str()).unwrap(),
    );
    headers
}

fn automation(id: &AutomationId, project_id: ProjectId, status: AutomationStatus) -> Automation {
    let now = Utc::now();
    Automation {
        id: id.clone(),
        project_id,
        name: "Automation 1".to_string(),
        status,
        paused_reason_code: None,
        paused_reason_detail: None,
        goal_prompt: "Implement the plan".to_string(),
        setup_conversation_id: None,
        provider_harness: "claude".to_string(),
        model_id: "sonnet".to_string(),
        logical_effort: None,
        run_mode: "edit".to_string(),
        base_ref_kind: "project_default".to_string(),
        base_ref: String::new(),
        base_display_name: None,
        base_source_pull_request_json: None,
        goal_items_json: None,
        chain_mode: "merged_base".to_string(),
        completion_signal: "pr_merged".to_string(),
        max_runs: 25,
        max_consecutive_failures: 3,
        first_run_prompt: Some("Run 1".to_string()),
        setup_analysis_summary: None,
        created_at: now,
        updated_at: now,
    }
}

async fn seed_bound_conversation(
    app_state: &AppState,
) -> (ProjectId, AutomationId, ChatConversation) {
    let project_id = ProjectId::new();
    let automation_id = AutomationId::from_string("automation-1");
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.automation_id = Some(automation_id.clone());
    app_state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    (project_id, automation_id, conversation)
}

#[tokio::test]
async fn get_and_update_automation_use_server_bound_conversation() {
    let app_state = Arc::new(AppState::new_test());
    let (project_id, automation_id, conversation) = seed_bound_conversation(&app_state).await;
    let mut automation = automation(&automation_id, project_id, AutomationStatus::Draft);
    automation.setup_conversation_id = Some(conversation.id);
    app_state
        .automation_repo
        .create(automation.clone())
        .await
        .unwrap();
    let state = test_http_state(app_state.clone());

    let Json(detail) = get_automation(State(state.clone()), caller_headers(&conversation.id))
        .await
        .unwrap();
    assert_eq!(detail.automation.id, automation_id.as_str());
    assert!(detail.runs.is_empty());

    let Json(updated) = update_automation(
        State(state),
        caller_headers(&conversation.id),
        Json(UpdateAutomationRequest {
            name: Some("Renamed automation".to_string()),
            max_runs: Some(9),
            max_consecutive_failures: None,
        }),
    )
    .await
    .unwrap();

    assert_eq!(updated.name, "Renamed automation");
    assert_eq!(updated.max_runs, 9);
    assert_eq!(
        app_state
            .automation_repo
            .get_by_id(&automation_id)
            .await
            .unwrap()
            .unwrap()
            .name,
        "Renamed automation"
    );
}

#[tokio::test]
async fn finalize_rejects_mismatched_conversation_binding() {
    let app_state = Arc::new(AppState::new_test());
    let (project_id, automation_id, conversation) = seed_bound_conversation(&app_state).await;
    let mut automation = automation(&automation_id, project_id, AutomationStatus::Draft);
    automation.setup_conversation_id = Some(ChatConversationId::new());
    app_state.automation_repo.create(automation).await.unwrap();
    let state = test_http_state(app_state);

    let error = finalize_automation(State(state), caller_headers(&conversation.id))
        .await
        .unwrap_err();

    assert_eq!(error.status, StatusCode::FORBIDDEN);
    assert!(error
        .message
        .as_deref()
        .unwrap_or("")
        .contains("automation_conversation_mismatch"));
}

#[tokio::test]
async fn finalize_activates_complete_bound_draft() {
    let app_state = Arc::new(AppState::new_test());
    let (project_id, automation_id, conversation) = seed_bound_conversation(&app_state).await;
    let mut automation = automation(&automation_id, project_id, AutomationStatus::Draft);
    automation.setup_conversation_id = Some(conversation.id);
    app_state.automation_repo.create(automation).await.unwrap();
    let state = test_http_state(app_state.clone());

    let Json(finalized) = finalize_automation(State(state), caller_headers(&conversation.id))
        .await
        .unwrap();

    assert_eq!(finalized.status, "active");
    assert_eq!(
        app_state
            .automation_repo
            .get_by_id(&automation_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        AutomationStatus::Active
    );
}

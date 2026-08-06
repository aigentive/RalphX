//! Trusted-caller-run authority tests for the managed-Team gate.
//!
//! Proves `resolve_team_authority` decides on liveness (`resolve_live_caller_run`),
//! never on `get_active_for_conversation` recency: a newer `running` ghost row for
//! the same conversation must never outrank a live, correctly bound caller run.

use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue, StatusCode};

use super::authority::resolve_team_authority;
use crate::application::managed_team::new_coordinator_run_binding;
use crate::application::AppState;
use crate::domain::entities::{
    AgentRun, AgentRunId, AgentRunStatus, ChatConversation, ChatConversationId, ProjectId,
};
use crate::http_server::types::HttpServerState;

async fn team_enabled_state() -> (Arc<AppState>, HttpServerState) {
    let app_state = Arc::new(AppState::new_test());
    app_state
        .ui_feature_flag_overrides_repo
        .update_agent_capabilities(Some(true), None, None)
        .await
        .expect("enable Team capability override");
    let state = HttpServerState::new_test(Arc::clone(&app_state));
    (app_state, state)
}

fn trusted_headers(conversation_id: &str, run_id: &AgentRunId) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-conversation-id",
        HeaderValue::from_str(conversation_id).expect("valid header value"),
    );
    headers.insert(
        "x-ralphx-agent-run-id",
        HeaderValue::from_str(&run_id.as_str()).expect("valid header value"),
    );
    headers
}

#[tokio::test]
async fn rejects_a_terminal_caller_run() {
    let (app_state, state) = team_enabled_state().await;
    let conversation_id = ChatConversationId::new();
    let caller = app_state
        .agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed caller run");
    app_state
        .agent_run_repo
        .update_status(&caller.id, AgentRunStatus::Completed)
        .await
        .expect("mark caller run terminal");

    let headers = trusted_headers(&conversation_id.as_str(), &caller.id);
    // `TeamAuthorityResolution` is not `Debug`, so unwrap the rejection by hand rather than
    // widening a production type just to satisfy `expect_err`.
    let (status, body) = match resolve_team_authority(&state, &headers).await {
        Ok(_) => panic!("a finished turn has no Team authority"),
        Err(rejection) => rejection,
    };

    assert_eq!(status, StatusCode::CONFLICT);
    let error = body.0["error"].as_str().expect("error field").to_string();
    assert!(
        error.contains("Trusted Team run has already finished"),
        "unexpected error message: {error}"
    );
}

#[tokio::test]
async fn accepts_a_live_caller_run_outranked_by_a_newer_running_ghost() {
    let (app_state, state) = team_enabled_state().await;
    let project_id = ProjectId::from_string("authority-test-project".to_string());
    let coordinator = ChatConversation::new_project(project_id.clone());
    let conversation_id = coordinator.id;
    app_state
        .chat_conversation_repo
        .create(coordinator)
        .await
        .expect("seed coordinator conversation");
    let team = app_state
        .managed_team
        .ensure_team(project_id, &conversation_id)
        .await
        .expect("ensure Team session");

    let caller = app_state
        .agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed caller run");
    app_state
        .managed_team
        .run_binding_repo()
        .create(new_coordinator_run_binding(
            team.id.clone(),
            conversation_id,
            caller.id,
        ))
        .await
        .expect("bind caller run to Team");

    // The ghost row is a newer `running` sibling for the same conversation with no
    // Team binding at all; the old recency-based gate would have preferred it.
    let ghost = app_state
        .agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed ghost run");
    assert!(
        ghost.started_at >= caller.started_at,
        "the ghost must be the newer running row for this regression to be meaningful"
    );

    let headers = trusted_headers(&conversation_id.as_str(), &caller.id);
    let resolution = resolve_team_authority(&state, &headers)
        .await
        .expect("liveness, not recency, decides Team caller authority");

    assert_eq!(resolution.run.id, caller.id);
    assert_ne!(resolution.run.id, ghost.id);
}

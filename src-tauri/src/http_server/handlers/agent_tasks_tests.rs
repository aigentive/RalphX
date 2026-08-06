use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue};

use super::agent_tasks::{resolve_scope, trusted_delegate_identity};
use crate::application::AppState;
use crate::domain::agents::AgentHarnessKind;
use crate::domain::entities::{
    AgentRun, AgentRunId, AgentRunStatus, ChatConversation, DelegatedSession, ProjectId,
};
use crate::http_server::types::HttpServerState;
use crate::http_server::AgentTaskContextFields;

async fn delegated_state() -> (Arc<AppState>, HttpServerState) {
    let app_state = Arc::new(AppState::new_test());
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
async fn trusted_delegate_identity_rejects_a_terminal_caller_run() {
    let (app_state, state) = delegated_state().await;
    let session = app_state
        .delegated_session_repo
        .create(DelegatedSession::new(
            ProjectId::new(),
            "project",
            "project-1",
            "ralphx-general-worker",
            AgentHarnessKind::Codex,
        ))
        .await
        .expect("seed delegated session");
    let conversation = ChatConversation::new_delegation(session.id);
    let conversation_id = conversation.id;
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed delegated conversation");
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
    let error = trusted_delegate_identity(&state, &headers, "test_operation")
        .await
        .expect_err("a finished turn has no delegate authority");

    assert_eq!(
        error, "test_operation trusted run has already finished",
        "unexpected error message: {error}"
    );
}

#[tokio::test]
async fn trusted_delegate_identity_accepts_a_live_caller_run_outranked_by_a_newer_running_ghost() {
    let (app_state, state) = delegated_state().await;
    let session = app_state
        .delegated_session_repo
        .create(DelegatedSession::new(
            ProjectId::new(),
            "project",
            "project-1",
            "ralphx-general-worker",
            AgentHarnessKind::Codex,
        ))
        .await
        .expect("seed delegated session");
    let conversation = ChatConversation::new_delegation(session.id.clone());
    let conversation_id = conversation.id;
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("seed delegated conversation");
    let caller = app_state
        .agent_run_repo
        .create(AgentRun::new(conversation_id))
        .await
        .expect("seed caller run");

    // The ghost row is a newer `running` sibling for the same delegated
    // conversation; the old recency-based gate would have preferred it.
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
    let (delegated_session_id, run_id, resolved_conversation_id) =
        trusted_delegate_identity(&state, &headers, "test_operation")
            .await
            .expect("liveness, not recency, decides delegate caller authority");

    assert_eq!(delegated_session_id, session.id);
    assert_eq!(run_id, caller.id);
    assert_ne!(run_id, ghost.id);
    assert_eq!(resolved_conversation_id, conversation_id);
}

#[test]
fn resolve_scope_rejects_bare_project_fallback_without_explicit_scope() {
    let error = resolve_scope(&AgentTaskContextFields {
        context_type: None,
        context_id: None,
        project_id: Some("project-id".to_string()),
        actor_agent: Some("ralphx-general-worker".to_string()),
    })
    .expect_err("conversation runtimes without identity must not share a project ledger");

    assert!(error.contains("context_type and context_id are required"));
}

#[test]
fn resolve_scope_preserves_explicit_project_scope() {
    let scope = resolve_scope(&AgentTaskContextFields {
        context_type: Some("project".to_string()),
        context_id: Some("project-id".to_string()),
        project_id: Some("project-id".to_string()),
        actor_agent: Some("ralphx-external-mcp".to_string()),
    })
    .expect("explicit project-scoped external callers remain supported");

    assert_eq!(scope.scope_type, "project");
    assert_eq!(scope.scope_id, "project-id");
    assert_eq!(scope.project_id.expect("project id").as_str(), "project-id");
}

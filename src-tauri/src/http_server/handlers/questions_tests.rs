use std::sync::Arc;

use axum::extract::State;
use axum::Json;

use super::{request_question, resolve_question};
use crate::application::app_state::AppState;
use crate::domain::entities::{
    ChatConversation, NotificationCategory, NotificationTargetKind, Project,
};
use crate::http_server::types::{HttpServerState, QuestionRequestInput, ResolveQuestionInput};

fn make_test_state() -> HttpServerState {
    HttpServerState::new_test(Arc::new(AppState::new_test()))
}

#[tokio::test]
async fn request_question_records_plan_mode_question_once_without_event_listener() {
    let state = make_test_state();
    let project = Project::new("acme-app".into(), "/tmp/acme-app".into());
    state
        .app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();
    let conversation = ChatConversation::new_project(project.id.clone());
    state
        .app_state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    let request = QuestionRequestInput {
        request_id: Some("question-request-1".into()),
        session_id: conversation.id.to_string(),
        question: "Should the proposal include a migration?".into(),
        header: Some("Plan proposal".into()),
        options: vec![],
        multi_select: false,
        allow_skip: true,
        batch_index: None,
        batch_total: None,
        metadata: Some(serde_json::json!({"kind": "plan_mode_proposal"})),
    };

    let first = request_question(State(state.clone()), Json(request))
        .await
        .0;
    let second = request_question(
        State(state.clone()),
        Json(QuestionRequestInput {
            request_id: Some(first.request_id.clone()),
            session_id: conversation.id.to_string(),
            question: "Should the proposal include a migration?".into(),
            header: Some("Plan proposal".into()),
            options: vec![],
            multi_select: false,
            allow_skip: true,
            batch_index: None,
            batch_total: None,
            metadata: Some(serde_json::json!({"kind": "plan_mode_proposal"})),
        }),
    )
    .await
    .0;

    assert_eq!(second.request_id, first.request_id);
    let rows = state
        .app_state
        .notification_repo
        .list(None, None, 50)
        .await
        .unwrap()
        .notifications;
    assert_eq!(rows.len(), 1, "server-side record must not need a listener");
    let row = &rows[0];
    assert_eq!(row.category, NotificationCategory::AgentQuestion);
    assert_eq!(row.title, "Agent has a question");
    assert_eq!(
        row.dedupe_key.as_deref(),
        Some("question:question-request-1")
    );
    assert_eq!(row.target.kind, NotificationTargetKind::AgentConversation);
    let conversation_id = conversation.id.as_str();
    assert_eq!(
        row.target.conversation_id.as_deref(),
        Some(conversation_id.as_str())
    );
    assert_eq!(row.project_id.as_deref(), Some(project.id.as_str()));
    assert_eq!(
        row.body.as_deref(),
        Some("project on acme-app: “Should the proposal include a migration?”")
    );

    assert_eq!(
        resolve_question(
            State(state.clone()),
            Json(ResolveQuestionInput {
                request_id: first.request_id,
                selected_options: Vec::new(),
                text: Some("Yes".into()),
                skipped: false,
            }),
        )
        .await,
        axum::http::StatusCode::OK
    );
    let settled = state
        .app_state
        .notification_repo
        .list(None, None, 50)
        .await
        .unwrap()
        .notifications;
    assert!(settled[0].read_at.is_some());
}

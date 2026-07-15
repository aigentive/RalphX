use std::sync::Arc;

use super::record_team_plan_requested_notification;
use crate::application::app_state::AppState;
use crate::domain::entities::{
    ChatConversation, NotificationCategory, NotificationTargetKind, ProjectId,
};
use crate::http_server::types::{HttpServerState, RequestTeamPlanRequest};

fn make_test_state() -> HttpServerState {
    HttpServerState::new_test(Arc::new(AppState::new_test()))
}

#[tokio::test]
async fn manual_team_plan_notification_records_one_row() {
    let state = make_test_state();
    let project_id = ProjectId::from_string("project-team".into());
    let conversation = ChatConversation::new_project(project_id.clone());
    state
        .app_state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();
    let request = RequestTeamPlanRequest {
        context_type: "project".into(),
        context_id: project_id.to_string(),
        process: "ideation".into(),
        teammates: vec![],
        team_name: "team-plan-test".into(),
        lead_session_id: None,
    };

    record_team_plan_requested_notification(&state, "team-plan-1", &request).await;
    record_team_plan_requested_notification(&state, "team-plan-1", &request).await;

    let rows = state
        .app_state
        .notification_repo
        .list(None, None, 50)
        .await
        .unwrap()
        .notifications;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.category, NotificationCategory::TeamPlanApproval);
    assert_eq!(row.dedupe_key.as_deref(), Some("team-plan:team-plan-1"));
    assert_eq!(row.target.kind, NotificationTargetKind::AgentConversation);
    let conversation_id = conversation.id.as_str();
    assert_eq!(
        row.target.conversation_id.as_deref(),
        Some(conversation_id.as_str())
    );
    assert_eq!(row.project_id.as_deref(), Some(project_id.as_str()));
}

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;

use super::{await_team_plan, record_team_plan_requested_notification};
use crate::application::app_state::AppState;
use crate::application::team_state_tracker::{PendingTeamPlan, PlanDecision};
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

    record_team_plan_requested_notification(&state, "team-plan-1", &request, None).await;
    record_team_plan_requested_notification(&state, "team-plan-1", &request, None).await;

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

#[tokio::test(start_paused = true)]
async fn timeout_waits_for_an_approval_that_already_claimed_the_plan() {
    let state = make_test_state();
    let plan_id = "team-plan-claimed";
    state
        .team_tracker
        .store_pending_plan(PendingTeamPlan {
            plan_id: plan_id.into(),
            context_type: "project".into(),
            context_id: "project-team".into(),
            process: "implement".into(),
            teammates: Vec::new(),
            created_at: chrono::Utc::now(),
            team_name: "team-plan-test".into(),
            lead_session_id: None,
        })
        .await;
    state.team_tracker.register_plan_channel(plan_id).await;
    let waiting_state = state.clone();
    let waiting_plan_id = plan_id.to_string();
    let waiter =
        tokio::spawn(
            async move { await_team_plan(State(waiting_state), Path(waiting_plan_id)).await },
        );
    tokio::task::yield_now().await;

    state
        .team_tracker
        .take_pending_plan(plan_id)
        .await
        .expect("approval should claim the plan");
    tokio::time::advance(tokio::time::Duration::from_secs(840)).await;
    tokio::task::yield_now().await;

    assert!(!waiter.is_finished());
    assert!(
        state
            .team_tracker
            .resolve_plan(
                plan_id,
                PlanDecision {
                    approved: true,
                    team_name: Some("team-plan-test".into()),
                    teammates_spawned: Vec::new(),
                    message: "approved".into(),
                },
            )
            .await
    );
    let Json(response) = waiter.await.unwrap().unwrap();
    assert!(response.success);
    assert_eq!(response.message, "approved");
}

use std::sync::Arc;

use axum::extract::{Json, State};
use ralphx_lib::application::{
    AppState, TeamService, TeamStateTracker, TicketAttachmentListRequest, TicketingTicketIdentity,
};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::http_server::handlers::list_ticket_attachments;
use ralphx_lib::http_server::types::HttpServerState;

#[tokio::test]
async fn list_ticket_attachments_http_returns_unsupported_provider_reason() {
    let response = list_ticket_attachments(
        State(http_state()),
        Json(TicketAttachmentListRequest {
            ticket: TicketingTicketIdentity {
                provider: "github".to_string(),
                id: "issue-1".to_string(),
                key: None,
                local_project_id: Some("project-1".to_string()),
            },
        }),
    )
    .await
    .expect("unsupported provider should return a bounded response");

    assert!(response.0.attachments.is_empty());
    assert_eq!(
        response.0.unsupported_reason.as_deref(),
        Some("Unsupported ticket attachment provider: github")
    );
}

fn http_state() -> HttpServerState {
    let execution_state = Arc::new(ExecutionState::new());
    let tracker = TeamStateTracker::new();
    let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));

    HttpServerState {
        app_state: Arc::new(AppState::new_test()),
        execution_state,
        team_tracker: tracker,
        team_service,
        delegation_service: Default::default(),
    }
}

use std::sync::Arc;

use axum::body::to_bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use super::super::types::HttpServerState;
use super::ticket_attachments::{list_ticket_attachments_http, TicketAttachmentListRequest};
use crate::application::ticket_attachment::TicketAttachmentProvider;
use crate::application::AppState;

#[tokio::test]
async fn list_ticket_attachments_fails_closed_without_leaking_provider_details() {
    let state = HttpServerState::new_test(Arc::new(AppState::new_test()));
    let request = TicketAttachmentListRequest {
        provider: TicketAttachmentProvider::Jira,
        ticket_id: "JIRA-123".to_string(),
    };

    let error = list_ticket_attachments_http(State(state), axum::Json(request))
        .await
        .expect_err("disabled integration should fail closed");
    let response = error.into_response();

    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("error response body should be readable");
    let body = std::str::from_utf8(&body).expect("error response should be utf8");
    assert!(body.contains("ticket_attachment_provider_failed"));
    assert!(!body.contains("http://"));
    assert!(!body.contains("https://"));
    assert!(!body.to_ascii_lowercase().contains("token"));
}

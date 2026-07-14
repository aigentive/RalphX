use std::sync::Arc;

use axum::body::to_bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use super::super::types::HttpServerState;
use super::ticket_attachments::{
    fetch_response, fetch_ticket_attachment_http, list_ticket_attachments_http, provider_item,
    safe_attachment_id, safe_file_name, TicketAttachmentFetchRequest, TicketAttachmentListRequest,
};
use crate::application::ticket_attachment::{
    TicketAttachmentDescriptor, TicketAttachmentFetchResult, TicketAttachmentProvider,
};
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

#[tokio::test]
async fn fetch_ticket_attachment_rejects_direct_download_pointer_before_provider_lookup() {
    let state = HttpServerState::new_test(Arc::new(AppState::new_test()));
    let request = TicketAttachmentFetchRequest {
        provider: TicketAttachmentProvider::Jira,
        ticket_id: "JIRA-123".to_string(),
        content_pointer: "https://example.test/download?token=secret".to_string(),
    };

    let error = fetch_ticket_attachment_http(State(state), axum::Json(request))
        .await
        .expect_err("direct download URL should not be accepted as a pointer");
    let response = error.into_response();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("error response body should be readable");
    let body = std::str::from_utf8(&body).expect("error response should be utf8");
    assert!(body.contains("invalid_ticket_attachment_request"));
    assert!(!body.contains("https://example.test"));
    assert!(!body.contains("token=secret"));
}

#[test]
fn provider_item_normalizes_all_providers_to_safe_metadata_only() {
    let unsafe_attachment_id = "https://example.test/attachment?token=secret";
    let unsafe_file_name = "Bearer secret";

    for provider in [
        TicketAttachmentProvider::Jira,
        TicketAttachmentProvider::Linear,
        TicketAttachmentProvider::ClickUp,
    ] {
        let item = provider_item(
            provider,
            "TICKET-1",
            safe_attachment_id(Some(unsafe_attachment_id), 7),
            safe_file_name(unsafe_file_name, 7),
            Some("text/plain"),
            Some(512),
            Some("2026-07-14T00:00:00Z".to_string()),
        )
        .expect("unsafe provider metadata should fall back to safe descriptor values");

        assert_eq!(item.descriptor.provider, provider);
        assert_eq!(item.descriptor.file_name, "attachment-7");
        assert!(item.descriptor.content_pointer.id().starts_with("ta_"));
        assert!(!item.descriptor.id.contains("https://"));
        assert!(!item.descriptor.id.contains("token=secret"));
        assert!(!item.descriptor.file_name.contains("Bearer"));
        assert!(!item.content_fetch_supported);
    }
}

#[test]
fn fetch_response_returns_only_safe_untrusted_content_reference() {
    let descriptor = TicketAttachmentDescriptor::new(
        TicketAttachmentProvider::Linear,
        "LIN-1",
        "att-1",
        "evidence.txt",
        Some("text/plain"),
        Some(128),
        None,
    )
    .expect("descriptor");
    let response = fetch_response(TicketAttachmentFetchResult {
        descriptor,
        location: None,
    });
    let serialized = serde_json::to_string(&response).expect("fetch response serializes");

    assert_eq!(response.content.kind, "ticket_attachment_content");
    assert_eq!(response.content.trust, "untrusted_external_content");
    assert!(response.content.available);
    assert!(response.content.id.starts_with("ta_"));
    assert!(!serialized.contains("location"));
    assert!(!serialized.contains("path"));
    assert!(!serialized.contains("source"));
    assert!(!serialized.contains("http://"));
    assert!(!serialized.contains("https://"));
    assert!(!serialized.to_ascii_lowercase().contains("token"));
}

use std::sync::Arc;

use axum::{extract::State, Json};

use crate::application::{
    TicketAttachmentFetchRequest, TicketAttachmentFetchResponse, TicketAttachmentListRequest,
    TicketAttachmentListResponse, TicketAttachmentService,
};
use crate::http_server::{HttpError, HttpServerState};

pub async fn list_ticket_attachments(
    State(state): State<HttpServerState>,
    Json(request): Json<TicketAttachmentListRequest>,
) -> Result<Json<TicketAttachmentListResponse>, HttpError> {
    let service = ticket_attachment_service(&state);
    service
        .list_attachments(request)
        .await
        .map(Json)
        .map_err(internal_error)
}

pub async fn fetch_ticket_attachment(
    State(state): State<HttpServerState>,
    Json(request): Json<TicketAttachmentFetchRequest>,
) -> Result<Json<TicketAttachmentFetchResponse>, HttpError> {
    let service = ticket_attachment_service(&state);
    service
        .fetch_attachment(request)
        .await
        .map(Json)
        .map_err(internal_error)
}

fn internal_error(error: String) -> HttpError {
    HttpError {
        status: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        message: Some(error),
    }
}

fn ticket_attachment_service(state: &HttpServerState) -> TicketAttachmentService {
    TicketAttachmentService::new(
        Arc::clone(&state.app_state.atlassian_integration_service),
        Arc::clone(&state.app_state.linear_integration_service),
        Arc::clone(&state.app_state.clickup_integration_service),
        &state.app_state.attachment_storage_path,
    )
}

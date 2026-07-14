use std::sync::Arc;

use async_trait::async_trait;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use super::*;
use crate::application::ticket_attachment::{
    fetch_ticket_attachment_content, BoundedTicketAttachmentBytes, TicketAttachmentContentPointer,
    TicketAttachmentDescriptor, TicketAttachmentError, TicketAttachmentFetchResult,
    TicketAttachmentListResult, TicketAttachmentProvider, TicketAttachmentProviderItem,
    TicketAttachmentProviderReader, TicketAttachmentSourceHandle,
};
use crate::application::ticket_attachment_runtime_store::TicketAttachmentRuntimeStore;
use crate::domain::services::ComposerIntegrationReference;

#[derive(Debug, Deserialize)]
pub struct TicketAttachmentListRequest {
    pub provider: TicketAttachmentProvider,
    pub ticket_id: String,
}

#[derive(Debug, Deserialize)]
pub struct TicketAttachmentFetchRequest {
    pub provider: TicketAttachmentProvider,
    pub ticket_id: String,
    pub content_pointer: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentListResponse {
    pub attachments: Vec<TicketAttachmentDescriptor>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentFetchResponse {
    pub attachment: TicketAttachmentDescriptor,
    pub content: TicketAttachmentContentReference,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentContentReference {
    pub kind: &'static str,
    pub id: String,
    pub trust: &'static str,
    pub available: bool,
}

#[derive(Debug, Serialize)]
struct TicketAttachmentErrorResponse {
    error: &'static str,
    details: &'static str,
}

pub async fn list_ticket_attachments_http(
    State(state): State<HttpServerState>,
    Json(req): Json<TicketAttachmentListRequest>,
) -> Result<Json<TicketAttachmentListResponse>, TicketAttachmentHttpError> {
    let reader = IntegrationTicketAttachmentReader::new(Arc::clone(&state.app_state));
    let result = reader
        .list_attachments(req.provider, &req.ticket_id)
        .await?;
    Ok(Json(TicketAttachmentListResponse {
        attachments: result.attachments,
    }))
}

pub async fn fetch_ticket_attachment_http(
    State(state): State<HttpServerState>,
    Json(req): Json<TicketAttachmentFetchRequest>,
) -> Result<Json<TicketAttachmentFetchResponse>, TicketAttachmentHttpError> {
    let pointer = TicketAttachmentContentPointer::from_id(&req.content_pointer)?;
    let reader = IntegrationTicketAttachmentReader::new(Arc::clone(&state.app_state));
    let store = TicketAttachmentRuntimeStore::new(state.app_state.attachment_storage_path.clone());
    let result =
        fetch_ticket_attachment_content(&reader, &store, req.provider, &req.ticket_id, &pointer)
            .await?;

    Ok(Json(fetch_response(result)))
}

pub(super) fn fetch_response(result: TicketAttachmentFetchResult) -> TicketAttachmentFetchResponse {
    let pointer = result.descriptor.content_pointer.id().to_string();
    TicketAttachmentFetchResponse {
        attachment: result.descriptor,
        content: TicketAttachmentContentReference {
            kind: "ticket_attachment_content",
            id: pointer,
            trust: "untrusted_external_content",
            available: true,
        },
    }
}

struct IntegrationTicketAttachmentReader {
    app_state: Arc<crate::application::AppState>,
}

impl IntegrationTicketAttachmentReader {
    fn new(app_state: Arc<crate::application::AppState>) -> Self {
        Self { app_state }
    }
}

#[async_trait]
impl TicketAttachmentProviderReader for IntegrationTicketAttachmentReader {
    async fn list_attachments(
        &self,
        provider: TicketAttachmentProvider,
        ticket_id: &str,
    ) -> Result<TicketAttachmentListResult, TicketAttachmentError> {
        match provider {
            TicketAttachmentProvider::Jira => self.list_jira(ticket_id).await,
            TicketAttachmentProvider::Linear => self.list_linear(ticket_id).await,
            TicketAttachmentProvider::ClickUp => self.list_clickup(ticket_id).await,
        }
    }

    async fn fetch_attachment(
        &self,
        _source: &TicketAttachmentSourceHandle,
        _max_bytes: usize,
    ) -> Result<BoundedTicketAttachmentBytes, TicketAttachmentError> {
        Err(TicketAttachmentError::UnsupportedContentFetch)
    }
}

impl IntegrationTicketAttachmentReader {
    async fn list_jira(
        &self,
        ticket_id: &str,
    ) -> Result<TicketAttachmentListResult, TicketAttachmentError> {
        let reference = ticket_reference("atlassian", "jira", ticket_id);
        let content = self
            .app_state
            .atlassian_integration_service
            .fetch_resource_content(&reference)
            .await
            .map_err(|_| TicketAttachmentError::ProviderRequestFailed)?;

        let items = content
            .attachments
            .into_iter()
            .enumerate()
            .filter_map(|(index, attachment)| {
                let attachment_id = safe_attachment_id(attachment.id.as_deref(), index);
                provider_item(
                    TicketAttachmentProvider::Jira,
                    ticket_id,
                    attachment_id,
                    safe_file_name(&attachment.filename, index),
                    attachment.mime_type.as_deref(),
                    non_negative_size(attachment.size),
                    attachment.created_at,
                )
            })
            .collect();

        TicketAttachmentListResult::from_items(items)
    }

    async fn list_linear(
        &self,
        ticket_id: &str,
    ) -> Result<TicketAttachmentListResult, TicketAttachmentError> {
        let reference = ticket_reference("linear", "issue", ticket_id);
        let content = self
            .app_state
            .linear_integration_service
            .fetch_issue_content(&reference)
            .await
            .map_err(|_| TicketAttachmentError::ProviderRequestFailed)?;

        let items = content
            .attachments
            .into_iter()
            .enumerate()
            .filter_map(|(index, attachment)| {
                provider_item(
                    TicketAttachmentProvider::Linear,
                    ticket_id,
                    safe_attachment_id(Some(&attachment.id), index),
                    safe_file_name(&attachment.title, index),
                    None,
                    None,
                    None,
                )
            })
            .collect();

        TicketAttachmentListResult::from_items(items)
    }

    async fn list_clickup(
        &self,
        ticket_id: &str,
    ) -> Result<TicketAttachmentListResult, TicketAttachmentError> {
        let content = self
            .app_state
            .clickup_integration_service
            .fetch_task(ticket_id)
            .await
            .map_err(|_| TicketAttachmentError::ProviderRequestFailed)?;

        let items = content
            .attachments
            .into_iter()
            .enumerate()
            .filter_map(|(index, attachment)| {
                provider_item(
                    TicketAttachmentProvider::ClickUp,
                    ticket_id,
                    safe_attachment_id(attachment.id.as_deref(), index),
                    safe_file_name(&attachment.filename, index),
                    attachment.mime_type.as_deref(),
                    non_negative_size(attachment.size),
                    None,
                )
            })
            .collect();

        TicketAttachmentListResult::from_items(items)
    }
}

fn ticket_reference(provider: &str, kind: &str, ticket_id: &str) -> ComposerIntegrationReference {
    ComposerIntegrationReference {
        provider: provider.to_string(),
        kind: kind.to_string(),
        id: ticket_id.to_string(),
        key: Some(ticket_id.to_string()),
        title: None,
        url: None,
        summary_excerpt: None,
        include_transcript: None,
    }
}

pub(super) fn provider_item(
    provider: TicketAttachmentProvider,
    ticket_id: &str,
    attachment_id: String,
    file_name: String,
    media_type: Option<&str>,
    declared_size_bytes: Option<u64>,
    created_at: Option<String>,
) -> Option<TicketAttachmentProviderItem> {
    let descriptor = TicketAttachmentDescriptor::new(
        provider,
        ticket_id,
        &attachment_id,
        &file_name,
        media_type,
        declared_size_bytes,
        created_at,
    )
    .ok()?;
    let source = TicketAttachmentSourceHandle::new(provider, ticket_id, &attachment_id).ok()?;
    Some(TicketAttachmentProviderItem::new(descriptor, source, false))
}

pub(super) fn safe_attachment_id(id: Option<&str>, index: usize) -> String {
    match id {
        Some(value) if is_safe_ticket_attachment_text(value) => value.to_string(),
        _ => format!("attachment-{index}"),
    }
}

pub(super) fn safe_file_name(file_name: &str, index: usize) -> String {
    if is_safe_ticket_attachment_text(file_name)
        && !file_name.contains('/')
        && !file_name.contains('\\')
    {
        file_name.to_string()
    } else {
        format!("attachment-{index}")
    }
}

fn is_safe_ticket_attachment_text(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    !value.is_empty()
        && !value.contains("://")
        && !lower.starts_with("bearer ")
        && !lower.contains("token=")
        && !lower.contains("access_token=")
}

fn non_negative_size(size: Option<i64>) -> Option<u64> {
    size.and_then(|value| u64::try_from(value).ok())
}

#[derive(Debug)]
pub struct TicketAttachmentHttpError(TicketAttachmentError);

impl From<TicketAttachmentError> for TicketAttachmentHttpError {
    fn from(error: TicketAttachmentError) -> Self {
        Self(error)
    }
}

impl IntoResponse for TicketAttachmentHttpError {
    fn into_response(self) -> axum::response::Response {
        let (status, error, details) = match self.0 {
            TicketAttachmentError::EmptyField { .. }
            | TicketAttachmentError::FieldTooLarge { .. }
            | TicketAttachmentError::UnsafeField { .. }
            | TicketAttachmentError::TooManyAttachments { .. } => (
                StatusCode::BAD_REQUEST,
                "invalid_ticket_attachment_request",
                "Ticket attachment request or provider metadata is invalid.",
            ),
            TicketAttachmentError::ContentTooLarge { .. } => (
                StatusCode::PAYLOAD_TOO_LARGE,
                "ticket_attachment_content_too_large",
                "Ticket attachment content exceeds the configured limit.",
            ),
            TicketAttachmentError::UnsupportedProvider => (
                StatusCode::BAD_REQUEST,
                "ticket_attachment_provider_unsupported",
                "Ticket attachment provider is unsupported.",
            ),
            TicketAttachmentError::UnsupportedContentFetch => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "ticket_attachment_content_unsupported",
                "Ticket attachment content fetch is unsupported for this attachment.",
            ),
            TicketAttachmentError::PointerNotFound => (
                StatusCode::NOT_FOUND,
                "ticket_attachment_pointer_not_found",
                "Ticket attachment pointer was not found for the current ticket attachments.",
            ),
            TicketAttachmentError::ProviderRequestFailed => (
                StatusCode::BAD_GATEWAY,
                "ticket_attachment_provider_failed",
                "Ticket attachment provider request failed.",
            ),
            TicketAttachmentError::PathEscapedRoot
            | TicketAttachmentError::StorageRootUnavailable
            | TicketAttachmentError::StorageWriteFailed => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "ticket_attachment_storage_failed",
                "Ticket attachment storage failed.",
            ),
        };

        (
            status,
            Json(TicketAttachmentErrorResponse { error, details }),
        )
            .into_response()
    }
}

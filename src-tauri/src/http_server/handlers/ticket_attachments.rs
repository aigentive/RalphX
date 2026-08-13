use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::{extract::State, http::HeaderMap, http::StatusCode, response::IntoResponse, Json};
use base64::Engine;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request, Uri};
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde::{Deserialize, Serialize};
use tokio_util::bytes::Bytes;

use super::*;
use super::atlassian_mcp::{authorize, AtlassianMcpHttpError};
use super::trusted_run_authority::resolve_live_caller_run;
use crate::application::atlassian_integration_service::{
    AtlassianAuthContext, AtlassianCredential,
};
use crate::application::ticket_attachment::{
    fetch_ticket_attachment_content, BoundedTicketAttachmentBytes, TicketAttachmentContentPointer,
    TicketAttachmentDescriptor, TicketAttachmentError, TicketAttachmentFetchResult,
    TicketAttachmentListResult, TicketAttachmentProvider, TicketAttachmentProviderItem,
    TicketAttachmentProviderReader, TicketAttachmentSourceHandle,
};
use crate::application::ticket_attachment_runtime_store::TicketAttachmentRuntimeStore;
use crate::domain::agents::AtlassianMcpAccess;
use crate::domain::entities::{AgentRunId, ChatConversationId};
use crate::domain::services::ComposerIntegrationReference;

const ATTACHMENT_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(20);

/// Inline `contentText` cap: small enough to stay a preview, not a second
/// download path. Binary/image attachments and text above this size are
/// exposed through `contentPath` only.
const TICKET_ATTACHMENT_INLINE_TEXT_MAX_BYTES: u64 = 64 * 1024;

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
    /// Materialized, containment-checked path under RalphX-managed attachment
    /// storage. Readable by Claude-harness runs; other sandboxed harnesses may
    /// not have filesystem access to this path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_path: Option<String>,
    /// Inline preview for small `text/*` attachments only. See
    /// [`TICKET_ATTACHMENT_INLINE_TEXT_MAX_BYTES`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_text: Option<String>,
}

#[derive(Debug, Serialize)]
struct TicketAttachmentErrorResponse {
    error: &'static str,
    details: &'static str,
}

pub async fn list_ticket_attachments_http(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(req): Json<TicketAttachmentListRequest>,
) -> Result<Json<TicketAttachmentListResponse>, TicketAttachmentHttpError> {
    authorize_ticket_attachment_access(&state, &headers, req.provider).await?;
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
    headers: HeaderMap,
    Json(req): Json<TicketAttachmentFetchRequest>,
) -> Result<Json<TicketAttachmentFetchResponse>, TicketAttachmentHttpError> {
    authorize_ticket_attachment_access(&state, &headers, req.provider).await?;
    let pointer = TicketAttachmentContentPointer::from_id(&req.content_pointer)?;
    let reader = IntegrationTicketAttachmentReader::new(Arc::clone(&state.app_state));
    let store = TicketAttachmentRuntimeStore::new(state.app_state.attachment_storage_path.clone());
    let result =
        fetch_ticket_attachment_content(&reader, &store, req.provider, &req.ticket_id, &pointer)
            .await?;

    Ok(Json(fetch_response_with_inline_text(result).await))
}

/// Authorize a ticket attachment request.
///
/// Jira attachments share the Atlassian role-tiered gate (`authorize()`); the
/// tier is re-derived per request from the run's persisted `routing_role`, so
/// this stays independent of MCP spawn-time tool filtering. Linear and
/// ClickUp attachments are not part of the Atlassian tier system today, so
/// they only require the same trusted, live caller-run identity — never a
/// fall-through with no check at all.
async fn authorize_ticket_attachment_access(
    state: &HttpServerState,
    headers: &HeaderMap,
    provider: TicketAttachmentProvider,
) -> Result<(), TicketAttachmentHttpError> {
    match provider {
        TicketAttachmentProvider::Jira => {
            authorize(state, headers, AtlassianMcpAccess::Read).await?;
            Ok(())
        }
        TicketAttachmentProvider::Linear | TicketAttachmentProvider::ClickUp => {
            verify_trusted_ticket_attachment_caller(state, headers).await
        }
    }
}

async fn verify_trusted_ticket_attachment_caller(
    state: &HttpServerState,
    headers: &HeaderMap,
) -> Result<(), TicketAttachmentHttpError> {
    let conversation_id =
        trusted_ticket_attachment_header(headers, "x-ralphx-conversation-id")?
            .parse::<ChatConversationId>()
            .map_err(|_| {
                TicketAttachmentHttpError::CallerUnauthorized(
                    TicketAttachmentCallerRejection::MissingIdentity,
                )
            })?;
    let run_id = AgentRunId::from_string(trusted_ticket_attachment_header(
        headers,
        "x-ralphx-agent-run-id",
    )?);

    resolve_live_caller_run(&state.app_state.agent_run_repo, &conversation_id, &run_id)
        .await
        .map_err(|_| {
            TicketAttachmentHttpError::CallerUnauthorized(
                TicketAttachmentCallerRejection::Untrusted,
            )
        })?;
    Ok(())
}

fn trusted_ticket_attachment_header(
    headers: &HeaderMap,
    name: &str,
) -> Result<String, TicketAttachmentHttpError> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or(TicketAttachmentHttpError::CallerUnauthorized(
            TicketAttachmentCallerRejection::MissingIdentity,
        ))
}

pub(super) fn fetch_response(result: TicketAttachmentFetchResult) -> TicketAttachmentFetchResponse {
    let pointer = result.descriptor.content_pointer.id().to_string();
    let content_path = result
        .location
        .as_ref()
        .map(|location| location.path().display().to_string());
    TicketAttachmentFetchResponse {
        attachment: result.descriptor,
        content: TicketAttachmentContentReference {
            kind: "ticket_attachment_content",
            id: pointer,
            trust: "untrusted_external_content",
            available: true,
            content_path,
            content_text: None,
        },
    }
}

pub(super) async fn fetch_response_with_inline_text(
    result: TicketAttachmentFetchResult,
) -> TicketAttachmentFetchResponse {
    let inline_text = match (
        &result.location,
        is_inline_text_eligible(result.descriptor.media_type.as_deref()),
    ) {
        (Some(location), true) => read_inline_ticket_attachment_text(location.path()).await,
        _ => None,
    };
    let mut response = fetch_response(result);
    response.content.content_text = inline_text;
    response
}

fn is_inline_text_eligible(media_type: Option<&str>) -> bool {
    media_type.is_some_and(|value| value.to_ascii_lowercase().starts_with("text/"))
}

/// Best-effort inline preview read of an already-persisted, already
/// containment-checked attachment location. Never a new path sink: `path`
/// always comes from a [`crate::application::ticket_attachment::TicketAttachmentContentLocation`]
/// produced by the existing runtime store, never from request input.
async fn read_inline_ticket_attachment_text(path: &Path) -> Option<String> {
    let metadata = tokio::fs::metadata(path).await.ok()?;
    if metadata.len() > TICKET_ATTACHMENT_INLINE_TEXT_MAX_BYTES {
        return None;
    }
    let bytes = tokio::fs::read(path).await.ok()?;
    String::from_utf8(bytes).ok()
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
        source: &TicketAttachmentSourceHandle,
        max_bytes: usize,
    ) -> Result<BoundedTicketAttachmentBytes, TicketAttachmentError> {
        let target = self.fetch_target(source).await?;
        download_ticket_attachment_bytes(target, max_bytes).await
    }
}

impl IntegrationTicketAttachmentReader {
    async fn fetch_target(
        &self,
        source: &TicketAttachmentSourceHandle,
    ) -> Result<AttachmentFetchTarget, TicketAttachmentError> {
        let url = source
            .fetch_url()
            .ok_or(TicketAttachmentError::UnsupportedContentFetch)?;
        let mut target = AttachmentFetchTarget::new(source.provider(), url)?;
        match source.provider() {
            TicketAttachmentProvider::Jira => {
                let auth = self
                    .app_state
                    .atlassian_integration_service
                    .enabled_auth_context()
                    .await
                    .map_err(|_| TicketAttachmentError::ProviderRequestFailed)?;
                ensure_jira_attachment_host(&target, &auth)?;
                target.authorization = Some(atlassian_authorization_header(&auth));
            }
            TicketAttachmentProvider::Linear => {}
            TicketAttachmentProvider::ClickUp => {
                if target.host() == "api.clickup.com" {
                    let auth = self
                        .app_state
                        .clickup_integration_service
                        .enabled_auth_context()
                        .await
                        .map_err(|_| TicketAttachmentError::ProviderRequestFailed)?;
                    target.authorization = Some(auth.api_token);
                }
            }
        }
        Ok(target)
    }

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
                    attachment.content_url,
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
                    Some(attachment.url),
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
                    attachment.url,
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
    fetch_url: Option<String>,
) -> Option<TicketAttachmentProviderItem> {
    let descriptor = TicketAttachmentDescriptor::new(
        provider,
        ticket_id,
        &attachment_id,
        &file_name,
        safe_optional_attachment_text(media_type),
        declared_size_bytes,
        created_at.and_then(|value| {
            if is_safe_ticket_attachment_text(&value) {
                Some(value)
            } else {
                None
            }
        }),
    )
    .ok()?;
    let mut source = TicketAttachmentSourceHandle::new(provider, ticket_id, &attachment_id).ok()?;
    let fetch_url = fetch_url.and_then(|url| safe_attachment_fetch_url(provider, &url));
    let content_fetch_supported = fetch_url.is_some();
    if let Some(fetch_url) = fetch_url {
        source = source.with_fetch_url(fetch_url);
    }
    Some(TicketAttachmentProviderItem::new(
        descriptor,
        source,
        content_fetch_supported,
    ))
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

fn safe_optional_attachment_text(value: Option<&str>) -> Option<&str> {
    value.filter(|candidate| is_safe_ticket_attachment_text(candidate))
}

fn is_safe_ticket_attachment_text(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    !value.is_empty()
        && !value.contains("://")
        && !lower.contains("bearer ")
        && !lower.contains("token=")
        && !lower.contains("access_token=")
}

fn non_negative_size(size: Option<i64>) -> Option<u64> {
    size.and_then(|value| u64::try_from(value).ok())
}

fn safe_attachment_fetch_url(provider: TicketAttachmentProvider, url: &str) -> Option<String> {
    let trimmed = url.trim();
    AttachmentFetchTarget::new(provider, trimmed)
        .ok()
        .map(|_| trimmed.to_string())
}

#[derive(Debug, Clone)]
pub(super) struct AttachmentFetchTarget {
    uri: Uri,
    host: String,
    authorization: Option<String>,
}

impl AttachmentFetchTarget {
    pub(super) fn new(
        provider: TicketAttachmentProvider,
        url: &str,
    ) -> Result<Self, TicketAttachmentError> {
        let uri = url
            .parse::<Uri>()
            .map_err(|_| TicketAttachmentError::UnsafeField {
                field: "attachment_url",
            })?;
        let Some(host) = uri.host().map(str::to_ascii_lowercase) else {
            return Err(TicketAttachmentError::UnsafeField {
                field: "attachment_url",
            });
        };
        if uri.scheme_str() != Some("https")
            || uri
                .authority()
                .is_some_and(|authority| authority.as_str().contains('@'))
            || is_blocked_attachment_host(&host)
            || !is_allowed_attachment_host(provider, &host)
        {
            return Err(TicketAttachmentError::UnsafeField {
                field: "attachment_url",
            });
        }
        Ok(Self {
            uri,
            host,
            authorization: None,
        })
    }

    pub(super) fn host(&self) -> &str {
        &self.host
    }
}

fn is_blocked_attachment_host(host: &str) -> bool {
    host == "localhost" || host.ends_with(".localhost") || host.parse::<std::net::IpAddr>().is_ok()
}

fn is_allowed_attachment_host(provider: TicketAttachmentProvider, host: &str) -> bool {
    match provider {
        TicketAttachmentProvider::Jira => {
            host == "api.atlassian.com" || host.ends_with(".atlassian.net")
        }
        TicketAttachmentProvider::Linear => host == "linear.app" || host.ends_with(".linear.app"),
        TicketAttachmentProvider::ClickUp => {
            host == "clickup.com" || host.ends_with(".clickup.com")
        }
    }
}

fn ensure_jira_attachment_host(
    target: &AttachmentFetchTarget,
    auth: &AtlassianAuthContext,
) -> Result<(), TicketAttachmentError> {
    let site_host = auth
        .site_url
        .parse::<Uri>()
        .ok()
        .and_then(|uri| uri.host().map(str::to_ascii_lowercase));
    let allowed = match &auth.credential {
        AtlassianCredential::ApiToken { .. } => site_host
            .as_deref()
            .is_some_and(|host| target.host() == host),
        AtlassianCredential::OAuth { .. } => target.host() == "api.atlassian.com",
    };
    if allowed {
        Ok(())
    } else {
        Err(TicketAttachmentError::UnsafeField {
            field: "attachment_url",
        })
    }
}

fn atlassian_authorization_header(auth: &AtlassianAuthContext) -> String {
    match &auth.credential {
        AtlassianCredential::ApiToken { email, token } => {
            let encoded =
                base64::engine::general_purpose::STANDARD.encode(format!("{email}:{token}"));
            format!("Basic {encoded}")
        }
        AtlassianCredential::OAuth { access_token, .. } => {
            format!("Bearer {access_token}")
        }
    }
}

async fn download_ticket_attachment_bytes(
    target: AttachmentFetchTarget,
    max_bytes: usize,
) -> Result<BoundedTicketAttachmentBytes, TicketAttachmentError> {
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .map_err(|_| TicketAttachmentError::ProviderRequestFailed)?
        .https_only()
        .enable_http1()
        .build();
    let client: Client<HttpsConnector<HttpConnector>, Full<Bytes>> =
        Client::builder(TokioExecutor::new()).build(https);
    let mut builder = Request::builder()
        .method(Method::GET)
        .uri(target.uri)
        .header("Accept", "application/octet-stream");
    if let Some(authorization) = target.authorization {
        builder = builder.header("Authorization", authorization);
    }
    let request = builder
        .body(Full::new(Bytes::new()))
        .map_err(|_| TicketAttachmentError::ProviderRequestFailed)?;
    let response = tokio::time::timeout(ATTACHMENT_DOWNLOAD_TIMEOUT, client.request(request))
        .await
        .map_err(|_| TicketAttachmentError::ProviderRequestFailed)?
        .map_err(|_| TicketAttachmentError::ProviderRequestFailed)?;
    if !response.status().is_success() {
        return Err(TicketAttachmentError::ProviderRequestFailed);
    }
    if response
        .headers()
        .get(hyper::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(TicketAttachmentError::ContentTooLarge { max: max_bytes });
    }
    let mut body = response.into_body();
    let mut bytes = Vec::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|_| TicketAttachmentError::ProviderRequestFailed)?;
        if let Some(data) = frame.data_ref() {
            if bytes.len().saturating_add(data.len()) > max_bytes {
                return Err(TicketAttachmentError::ContentTooLarge { max: max_bytes });
            }
            bytes.extend_from_slice(data);
        }
    }
    BoundedTicketAttachmentBytes::new(bytes)
}

/// Why a ticket attachment request was refused or failed.
#[derive(Debug)]
pub enum TicketAttachmentHttpError {
    /// The provider/storage layer rejected the request or content.
    Attachment(TicketAttachmentError),
    /// The Jira/Atlassian role-tiered gate refused the caller.
    Atlassian(AtlassianMcpHttpError),
    /// The Linear/ClickUp trusted-caller identity check refused the caller.
    CallerUnauthorized(TicketAttachmentCallerRejection),
}

/// Why the lightweight (non-Atlassian) trusted-caller check refused a
/// Linear/ClickUp ticket attachment request.
#[derive(Debug)]
pub enum TicketAttachmentCallerRejection {
    /// Transport identity headers were missing or unusable.
    MissingIdentity,
    /// The transport-supplied run is not a live, correctly bound caller.
    Untrusted,
}

impl From<TicketAttachmentError> for TicketAttachmentHttpError {
    fn from(error: TicketAttachmentError) -> Self {
        Self::Attachment(error)
    }
}

impl From<AtlassianMcpHttpError> for TicketAttachmentHttpError {
    fn from(error: AtlassianMcpHttpError) -> Self {
        Self::Atlassian(error)
    }
}

impl IntoResponse for TicketAttachmentHttpError {
    fn into_response(self) -> axum::response::Response {
        let error = match self {
            Self::Atlassian(error) => return error.into_response(),
            Self::CallerUnauthorized(rejection) => {
                let (status, error, details) = match rejection {
                    TicketAttachmentCallerRejection::MissingIdentity => (
                        StatusCode::UNAUTHORIZED,
                        "ticket_attachment_missing_identity",
                        "Ticket attachment tools require trusted runtime identity.",
                    ),
                    TicketAttachmentCallerRejection::Untrusted => (
                        StatusCode::UNAUTHORIZED,
                        "ticket_attachment_untrusted_caller",
                        "Ticket attachment tools require a live caller run bound to this conversation.",
                    ),
                };
                return (
                    status,
                    Json(TicketAttachmentErrorResponse { error, details }),
                )
                    .into_response();
            }
            Self::Attachment(error) => error,
        };
        let (status, error, details) = match error {
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

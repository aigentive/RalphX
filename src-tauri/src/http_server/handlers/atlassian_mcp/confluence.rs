//! Confluence endpoints for the Atlassian MCP tool surface.

use axum::{extract::State, http::HeaderMap, Json};
use serde::{Deserialize, Serialize};

use crate::application::{
    AtlassianResourceKind, ConfluencePageContent, ConfluencePageCreateRequest,
    ConfluencePageUpdateRequest, SearchMode,
};
use crate::domain::agents::AtlassianMcpAccess;
use crate::http_server::HttpServerState;

use super::{authorize, required_field, AtlassianMcpHttpError};

/// Upper bound on Confluence search results, mirroring the MCP tool schema.
const MAX_SEARCH_RESULTS: usize = 50;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfluenceSearchRequest {
    pub query: String,
    #[serde(default)]
    pub max_results: Option<usize>,
    /// Opt in to raw CQL pass-through: `query` is submitted to Confluence
    /// verbatim instead of being rewritten into a title/text search.
    #[serde(default)]
    pub cql: bool,
}

#[derive(Debug, Serialize)]
pub struct ConfluenceSearchResponse {
    pub pages: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfluencePageIdRequest {
    pub page_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfluenceListSpacesRequest {
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct ConfluenceListSpacesResponse {
    pub spaces: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct ConfluencePageResponse {
    pub page: ConfluencePageContent,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfluenceCreatePageRequest {
    pub space_id: String,
    pub title: String,
    #[serde(default)]
    pub body_storage: Option<String>,
    /// Markdown body; converted to Confluence storage format. Exactly one of
    /// `body_storage`/`body_markdown` is required.
    #[serde(default)]
    pub body_markdown: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfluenceUpdatePageRequest {
    pub page_id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body_storage: Option<String>,
    /// Markdown body; converted to Confluence storage format. At most one of
    /// `body_storage`/`body_markdown` may be supplied; both omitted leaves the
    /// body unchanged.
    #[serde(default)]
    pub body_markdown: Option<String>,
}

/// Reject a Confluence page body that supplies both `bodyStorage` and
/// `bodyMarkdown`, or (when `require_body` is set) neither.
///
/// `require_body` is true for page creation, where a page must always have
/// body content, and false for updates, where omitting both means "leave the
/// current body untouched" (the existing empty-patch guard already rejects a
/// call that changes nothing at all).
///
/// # Errors
///
/// Returns [`AtlassianMcpHttpError::InvalidRequest`] when the combination is
/// rejected.
fn validate_confluence_body_fields(
    body_storage: Option<&str>,
    body_markdown: Option<&str>,
    require_body: bool,
) -> Result<(), AtlassianMcpHttpError> {
    match (body_storage, body_markdown) {
        (Some(_), Some(_)) => Err(AtlassianMcpHttpError::InvalidRequest(
            "Confluence page body accepts exactly one of bodyStorage or bodyMarkdown, not both"
                .to_string(),
        )),
        (None, None) if require_body => Err(AtlassianMcpHttpError::InvalidRequest(
            "Confluence page body requires exactly one of bodyStorage or bodyMarkdown".to_string(),
        )),
        _ => Ok(()),
    }
}

pub async fn confluence_search_pages(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<ConfluenceSearchRequest>,
) -> Result<Json<ConfluenceSearchResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::Read).await?;
    let query = required_field(&request.query, "Confluence search query is required")?;
    let limit = request
        .max_results
        .unwrap_or(25)
        .clamp(1, MAX_SEARCH_RESULTS);
    let mode = if request.cql {
        SearchMode::RawQuery
    } else {
        SearchMode::Smart
    };

    let results = state
        .app_state
        .atlassian_integration_service
        .search_resources_with_mode(AtlassianResourceKind::Confluence, &query, limit, mode)
        .await
        .map_err(|error| match mode {
            // Raw CQL is caller-authored: surface the real Atlassian status
            // instead of collapsing every failure into a flat 400. No silent
            // raw→smart fallback.
            SearchMode::RawQuery => AtlassianMcpHttpError::Api(error),
            SearchMode::Smart => AtlassianMcpHttpError::InvalidRequest(error.message),
        })?;

    Ok(Json(ConfluenceSearchResponse {
        pages: results
            .into_iter()
            .map(|summary| serde_json::to_value(summary).unwrap_or(serde_json::Value::Null))
            .collect(),
    }))
}

pub async fn confluence_get_page(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<ConfluencePageIdRequest>,
) -> Result<Json<ConfluencePageResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::Read).await?;
    let page_id = required_field(&request.page_id, "Confluence page id is required")?;

    let page = state
        .app_state
        .atlassian_integration_service
        .confluence_get_page(&page_id)
        .await?;

    Ok(Json(ConfluencePageResponse { page }))
}

pub async fn confluence_list_spaces(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<ConfluenceListSpacesRequest>,
) -> Result<Json<ConfluenceListSpacesResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::Read).await?;
    let limit = request.limit.unwrap_or(50).clamp(1, 250);

    let spaces = state
        .app_state
        .atlassian_integration_service
        .list_confluence_spaces(limit)
        .await
        .map_err(AtlassianMcpHttpError::InvalidRequest)?;

    Ok(Json(ConfluenceListSpacesResponse {
        spaces: spaces
            .into_iter()
            .map(|space| serde_json::to_value(space).unwrap_or(serde_json::Value::Null))
            .collect(),
    }))
}

pub async fn confluence_create_page(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<ConfluenceCreatePageRequest>,
) -> Result<Json<ConfluencePageResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::ReadWrite).await?;
    validate_confluence_body_fields(
        request.body_storage.as_deref(),
        request.body_markdown.as_deref(),
        true,
    )?;

    let page = state
        .app_state
        .atlassian_integration_service
        .confluence_create_page(&ConfluencePageCreateRequest {
            space_id: required_field(&request.space_id, "Confluence space id is required")?,
            title: required_field(&request.title, "Confluence page title is required")?,
            body_storage: request.body_storage,
            body_markdown: request.body_markdown,
            parent_id: request.parent_id,
        })
        .await?;

    Ok(Json(ConfluencePageResponse { page }))
}

pub async fn confluence_update_page(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<ConfluenceUpdatePageRequest>,
) -> Result<Json<ConfluencePageResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::ReadWrite).await?;
    let page_id = required_field(&request.page_id, "Confluence page id is required")?;
    validate_confluence_body_fields(
        request.body_storage.as_deref(),
        request.body_markdown.as_deref(),
        false,
    )?;

    let page = state
        .app_state
        .atlassian_integration_service
        .confluence_update_page(
            &page_id,
            &ConfluencePageUpdateRequest {
                title: request.title,
                body_storage: request.body_storage,
                body_markdown: request.body_markdown,
            },
        )
        .await?;

    Ok(Json(ConfluencePageResponse { page }))
}

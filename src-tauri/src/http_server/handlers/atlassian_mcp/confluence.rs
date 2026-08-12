//! Confluence endpoints for the Atlassian MCP tool surface.

use axum::{extract::State, http::HeaderMap, Json};
use serde::{Deserialize, Serialize};

use crate::application::{
    AtlassianResourceKind, ConfluencePageContent, ConfluencePageCreateRequest,
    ConfluencePageUpdateRequest,
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

#[derive(Debug, Serialize)]
pub struct ConfluencePageResponse {
    pub page: ConfluencePageContent,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfluenceCreatePageRequest {
    pub space_id: String,
    pub title: String,
    pub body_storage: String,
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

    let results = state
        .app_state
        .atlassian_integration_service
        .search_resources(AtlassianResourceKind::Confluence, &query, limit)
        .await
        .map_err(AtlassianMcpHttpError::InvalidRequest)?;

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

pub async fn confluence_create_page(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<ConfluenceCreatePageRequest>,
) -> Result<Json<ConfluencePageResponse>, AtlassianMcpHttpError> {
    authorize(&state, &headers, AtlassianMcpAccess::ReadWrite).await?;

    let page = state
        .app_state
        .atlassian_integration_service
        .confluence_create_page(&ConfluencePageCreateRequest {
            space_id: required_field(&request.space_id, "Confluence space id is required")?,
            title: required_field(&request.title, "Confluence page title is required")?,
            body_storage: request.body_storage,
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

    let page = state
        .app_state
        .atlassian_integration_service
        .confluence_update_page(
            &page_id,
            &ConfluencePageUpdateRequest {
                title: request.title,
                body_storage: request.body_storage,
            },
        )
        .await?;

    Ok(Json(ConfluencePageResponse { page }))
}

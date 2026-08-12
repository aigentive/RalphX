//! Bounded generic Atlassian API proxy — the escape hatch for the API long
//! tail that the curated tools do not cover.
//!
//! The HTTP method determines the required tier: read-only methods need the
//! read tier, everything else needs write. Path containment is validated here
//! and again at the request sink inside the client.

use std::str::FromStr;

use axum::{extract::State, http::HeaderMap, Json};
use serde::{Deserialize, Serialize};

use crate::application::{validate_atlassian_raw_path, AtlassianRawMethod, AtlassianResourceKind};
use crate::domain::agents::AtlassianMcpAccess;
use crate::http_server::HttpServerState;

use super::{authorize, AtlassianMcpHttpError};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtlassianRawRequest {
    pub method: String,
    /// `jira` or `confluence`; selects the product base URL.
    pub product: String,
    /// Relative API path, for example `/rest/agile/1.0/board/5/sprint`.
    pub path: String,
    #[serde(default)]
    pub body: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct AtlassianRawApiResponse {
    pub body: serde_json::Value,
}

pub async fn atlassian_api_request(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<AtlassianRawRequest>,
) -> Result<Json<AtlassianRawApiResponse>, AtlassianMcpHttpError> {
    let method = AtlassianRawMethod::from_str(&request.method)
        .map_err(AtlassianMcpHttpError::InvalidRequest)?;
    let product = AtlassianResourceKind::from_str(&request.product).map_err(|_| {
        AtlassianMcpHttpError::InvalidRequest(
            "Atlassian product must be 'jira' or 'confluence'".to_string(),
        )
    })?;

    // Containment is proven here so a rejected path returns 400 rather than
    // being masked by a later credential or transport failure. The client
    // re-validates at the request sink.
    validate_atlassian_raw_path(&request.path).map_err(AtlassianMcpHttpError::InvalidRequest)?;

    // Mutating methods require the write tier; the tool name alone never
    // decides this.
    let required = if method.is_read_only() {
        AtlassianMcpAccess::Read
    } else {
        AtlassianMcpAccess::ReadWrite
    };
    authorize(&state, &headers, required).await?;

    let body = state
        .app_state
        .atlassian_integration_service
        .atlassian_raw_api_request(method, product, &request.path, request.body)
        .await?;

    Ok(Json(AtlassianRawApiResponse { body }))
}

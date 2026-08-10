//! Spawn-free MCP catalog snapshot reads for the remote facade.
//!
//! `get_mcp_catalog` and `refresh_mcp_catalog` cannot cross `:3849`: their shared
//! `build_catalog` closure reaches both authority carriers this twin explicitly drops:
//! `resolve_provider_management_eligibility` refreshes provider CLI readiness, while
//! `discover_provider_catalog` can resolve and launch the Codex app-server probe.
//!
//! [`get_remote_mcp_catalog`] reads only the durable response captured after a successful
//! host-local catalog build. It does not rebuild policy from current `mcp_policy_overrides`:
//! the stored response already contains the override-derived and provider-native halves from
//! one coherent build, and half-refreshing it would make those halves disagree. The twin marks
//! the served response stale while preserving `probed_at`; `captured_at` separately reports
//! when that catalog build was stored.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::AppState;
use crate::commands::mcp_policy_commands::McpCatalogResponse;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteMcpCatalogInput {
    pub project_id: Option<String>,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RemoteMcpCatalogResponse {
    pub snapshot: Option<McpCatalogResponse>,
    pub captured_at: Option<String>,
}

#[tauri::command]
pub async fn get_remote_mcp_catalog(
    input: RemoteMcpCatalogInput,
    state: State<'_, AppState>,
) -> Result<RemoteMcpCatalogResponse, String> {
    get_remote_mcp_catalog_for_app_state(state.inner(), input).await
}

#[doc(hidden)]
pub async fn get_remote_mcp_catalog_for_app_state(
    state: &AppState,
    input: RemoteMcpCatalogInput,
) -> Result<RemoteMcpCatalogResponse, String> {
    if input.provider.trim().is_empty() {
        return Err("invalid_provider: provider cannot be empty".to_string());
    }
    if input
        .project_id
        .as_deref()
        .is_some_and(|project_id| project_id.trim().is_empty())
    {
        return Err("invalid_scope: projectId cannot be empty".to_string());
    }
    let Some(stored) = state
        .mcp_catalog_snapshot_repo
        .get(input.project_id.as_deref(), &input.provider)
        .await
        .map_err(|error| format!("catalog_snapshot_read_failed: {error}"))?
    else {
        return Ok(RemoteMcpCatalogResponse {
            snapshot: None,
            captured_at: None,
        });
    };
    let mut snapshot = serde_json::from_str::<McpCatalogResponse>(&stored.response_json)
        .map_err(|error| format!("catalog_snapshot_decode_failed: {error}"))?;
    snapshot.probe_stale = true;
    Ok(RemoteMcpCatalogResponse {
        snapshot: Some(snapshot),
        captured_at: Some(stored.captured_at),
    })
}

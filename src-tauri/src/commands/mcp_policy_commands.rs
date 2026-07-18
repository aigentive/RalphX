use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::application::provider_management_eligibility::{
    resolve_provider_management_eligibility, ProviderManagementEligibility,
};
use crate::application::AppState;
use crate::domain::agents::{
    AgentHarnessKind, EffectiveMcpServerPolicy, McpOverrideState, McpPolicyOverride,
    McpPolicySource, McpServerKey, NativeMcpServerSnapshot, NativeMcpState, RALPHX_MCP_SERVER_IDS,
};
use crate::domain::entities::ProjectId;

#[derive(Debug, Clone, Serialize)]
pub struct McpCatalogResponse {
    pub eligible_providers: Vec<String>,
    pub eligible_default_provider: Option<String>,
    pub probed_at: String,
    pub probe_stale: bool,
    pub provider_diagnostics: BTreeMap<String, String>,
    pub servers: Vec<McpServerResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpServerResponse {
    pub provider: String,
    pub server_id: String,
    pub native_scope: Option<String>,
    pub native_state: NativeMcpState,
    pub effective_enabled: bool,
    pub configured_state: McpOverrideState,
    pub effective_state: McpOverrideState,
    pub effective_source: McpPolicySource,
    pub known_tools: Vec<McpToolResponse>,
    pub disabled_tools: Vec<String>,
    pub locked: bool,
    pub locked_reason: Option<String>,
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpToolResponse {
    pub tool_name: String,
    pub configured_state: McpOverrideState,
    pub effective_state: McpOverrideState,
    pub effective_source: McpPolicySource,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpCatalogInput {
    pub project_id: Option<String>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshMcpCatalogInput {
    pub project_id: Option<String>,
    pub provider: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerOverrideInput {
    pub project_id: Option<String>,
    pub provider: String,
    pub server_id: String,
    pub state: McpOverrideState,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolOverrideInput {
    pub project_id: Option<String>,
    pub provider: String,
    pub server_id: String,
    pub tool_name: String,
    pub state: McpOverrideState,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearMcpServerOverrideInput {
    pub project_id: Option<String>,
    pub provider: String,
    pub server_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearMcpToolOverrideInput {
    pub project_id: Option<String>,
    pub provider: String,
    pub server_id: String,
    pub tool_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpMutationResponse {
    pub changed: bool,
}

fn parse_provider(provider: &str) -> Result<AgentHarnessKind, String> {
    provider
        .parse::<AgentHarnessKind>()
        .map_err(|error| format!("invalid_provider: {error}"))
}

fn scope_project_id(project_id: Option<&str>) -> Result<Option<&str>, String> {
    match project_id {
        Some(value) if value.trim().is_empty() => {
            Err("invalid_scope: projectId cannot be empty".to_string())
        }
        value => Ok(value),
    }
}

fn mutable_key(provider: AgentHarnessKind, server_id: String) -> Result<McpServerKey, String> {
    let key = McpServerKey::new(provider, server_id)
        .map_err(|error| format!("invalid_identifier: {error}"))?;
    if key.is_ralphx_owned() {
        return Err(format!(
            "locked_internal_server: '{}' is required by RalphX",
            key.server_id
        ));
    }
    Ok(key)
}

async fn project_root(
    state: &AppState,
    project_id: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    let Some(project_id) = project_id else {
        return Ok(None);
    };
    let project = state
        .project_repo
        .get_by_id(&ProjectId(project_id.to_string()))
        .await
        .map_err(|error| format!("project_lookup_failed: {error}"))?
        .ok_or_else(|| format!("project_not_found: {project_id}"))?;
    Ok(Some(PathBuf::from(project.working_directory)))
}

fn policy_server_ids(
    provider: AgentHarnessKind,
    global: &[McpPolicyOverride],
    project: &[McpPolicyOverride],
) -> BTreeSet<String> {
    RALPHX_MCP_SERVER_IDS
        .into_iter()
        .map(str::to_string)
        .chain(
            global
                .iter()
                .chain(project.iter())
                .filter(|row| row.key.provider == provider)
                .map(|row| row.key.server_id.clone()),
        )
        .collect()
}

fn known_policy_tools(
    provider: AgentHarnessKind,
    server_id: &str,
    global: &[McpPolicyOverride],
    project: &[McpPolicyOverride],
) -> Vec<String> {
    global
        .iter()
        .chain(project.iter())
        .filter(|row| row.key.provider == provider && row.key.server_id == server_id)
        .flat_map(|row| row.tool_states.keys().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
pub(crate) fn to_server_response(effective: EffectiveMcpServerPolicy) -> McpServerResponse {
    to_server_response_with_scope(effective, None)
}

fn to_server_response_with_scope(
    effective: EffectiveMcpServerPolicy,
    scoped: Option<&McpPolicyOverride>,
) -> McpServerResponse {
    let known_tools = effective
        .tool_states
        .iter()
        .map(|(tool_name, state)| McpToolResponse {
            tool_name: tool_name.clone(),
            configured_state: scoped
                .and_then(|policy| policy.tool_states.get(tool_name))
                .copied()
                .unwrap_or(McpOverrideState::Follow),
            effective_state: *state,
            effective_source: effective
                .tool_sources
                .get(tool_name)
                .copied()
                .unwrap_or(McpPolicySource::ProviderNative),
        })
        .collect();
    McpServerResponse {
        provider: effective.native.key.provider.to_string(),
        server_id: effective.native.key.server_id.clone(),
        native_scope: effective.native.native_scope,
        native_state: effective.native.native_state,
        effective_enabled: effective.enabled,
        configured_state: scoped
            .map(|policy| policy.server_state)
            .unwrap_or(McpOverrideState::Follow),
        effective_state: effective.server_state,
        effective_source: effective.server_source,
        known_tools,
        disabled_tools: effective.disabled_tools,
        locked: effective.locked,
        locked_reason: effective.locked_reason,
        diagnostic: effective.native.diagnostic,
    }
}

async fn build_catalog(
    state: &AppState,
    project_id: Option<&str>,
    eligibility: ProviderManagementEligibility,
    provider_filter: Option<AgentHarnessKind>,
) -> Result<McpCatalogResponse, String> {
    if let Some(provider) = provider_filter {
        eligibility
            .ensure_ready(provider)
            .map_err(|error| error.to_string())?;
    }
    let project_root = project_root(state, project_id).await?;
    let global = state
        .mcp_policy_repo
        .list_global()
        .await
        .map_err(|error| format!("policy_read_failed: {error}"))?;
    let project = match project_id {
        Some(project_id) => state
            .mcp_policy_repo
            .list_for_project(project_id)
            .await
            .map_err(|error| format!("policy_read_failed: {error}"))?,
        None => Vec::new(),
    };
    let providers = eligibility
        .providers
        .iter()
        .copied()
        .filter(|provider| provider_filter.is_none_or(|filter| filter == *provider))
        .collect::<Vec<_>>();
    let service = state.mcp_policy_service();
    let mut servers = Vec::new();
    let mut provider_diagnostics = BTreeMap::new();
    for provider in providers {
        let discovered = discover_provider_catalog(provider, project_root.as_deref());
        let mut native_by_id = match discovered {
            Ok(rows) => rows
                .into_iter()
                .map(|row| (row.key.server_id.clone(), row))
                .collect::<BTreeMap<_, _>>(),
            Err(error) => {
                provider_diagnostics.insert(provider.to_string(), error);
                BTreeMap::new()
            }
        };
        let server_ids = policy_server_ids(provider, &global, &project)
            .into_iter()
            .chain(native_by_id.keys().cloned())
            .collect::<BTreeSet<_>>();
        for server_id in server_ids {
            let known_policy_tools = known_policy_tools(provider, &server_id, &global, &project);
            let snapshot = match native_by_id.remove(&server_id) {
                Some(mut native) => {
                    native.known_tools.extend(known_policy_tools);
                    native.known_tools.sort();
                    native.known_tools.dedup();
                    native
                }
                None => {
                    let key = McpServerKey::new(provider, server_id.clone())?;
                    let locked = key.is_ralphx_owned();
                    NativeMcpServerSnapshot {
                        key,
                        native_scope: locked.then_some("ralphx".to_string()),
                        native_state: if locked {
                            NativeMcpState::Enabled
                        } else {
                            NativeMcpState::Unknown
                        },
                        known_tools: known_policy_tools,
                        diagnostic: (!locked).then_some(
                            "This exact-name policy is retained even though the provider catalog does not currently expose the server. RalphX leaves native definitions, auth, approvals, and trust unchanged."
                                .to_string(),
                        ),
                    }
                }
            };
            let effective = service
                .resolve(snapshot, project_id, project_root.as_deref())
                .await
                .map_err(|error| error.to_string())?;
            let scoped = if project_id.is_some() {
                project.iter().find(|row| row.key == effective.native.key)
            } else {
                global.iter().find(|row| row.key == effective.native.key)
            };
            servers.push(to_server_response_with_scope(effective, scoped));
        }
    }
    Ok(McpCatalogResponse {
        eligible_providers: eligibility
            .providers
            .iter()
            .map(ToString::to_string)
            .collect(),
        eligible_default_provider: eligibility.default_provider.map(|value| value.to_string()),
        probed_at: eligibility.probed_at.to_rfc3339(),
        probe_stale: false,
        provider_diagnostics,
        servers,
    })
}

fn discover_provider_catalog(
    provider: AgentHarnessKind,
    project_root: Option<&std::path::Path>,
) -> Result<Vec<NativeMcpServerSnapshot>, String> {
    let home_dir =
        dirs::home_dir().ok_or_else(|| "Provider home directory is unavailable".to_string())?;
    match provider {
        AgentHarnessKind::Claude => {
            crate::infrastructure::agents::claude::mcp_catalog::discover_native_mcp_servers(
                &home_dir,
                project_root,
            )
        }
        AgentHarnessKind::Codex => {
            crate::infrastructure::agents::codex::mcp_catalog::discover_native_mcp_servers(
                &home_dir,
                project_root,
            )
        }
    }
}

#[tauri::command]
pub async fn get_mcp_catalog(
    input: McpCatalogInput,
    state: State<'_, AppState>,
) -> Result<McpCatalogResponse, String> {
    let project_id = scope_project_id(input.project_id.as_deref())?;
    let provider = input.provider.as_deref().map(parse_provider).transpose()?;
    let eligibility = resolve_provider_management_eligibility(&state.agent_provider_settings_repo)
        .await
        .map_err(|error| error.to_string())?;
    build_catalog(&state, project_id, eligibility, provider).await
}

#[tauri::command]
pub async fn refresh_mcp_catalog(
    input: RefreshMcpCatalogInput,
    state: State<'_, AppState>,
) -> Result<McpCatalogResponse, String> {
    let project_id = scope_project_id(input.project_id.as_deref())?;
    let provider = parse_provider(&input.provider)?;
    let eligibility = resolve_provider_management_eligibility(&state.agent_provider_settings_repo)
        .await
        .map_err(|error| error.to_string())?;
    build_catalog(&state, project_id, eligibility, Some(provider)).await
}

async fn ensure_mutation_ready(state: &AppState, provider: AgentHarnessKind) -> Result<(), String> {
    resolve_provider_management_eligibility(&state.agent_provider_settings_repo)
        .await
        .map_err(|error| error.to_string())?
        .ensure_ready(provider)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn update_mcp_server_override(
    input: McpServerOverrideInput,
    state: State<'_, AppState>,
) -> Result<McpMutationResponse, String> {
    let project_id = scope_project_id(input.project_id.as_deref())?;
    let provider = parse_provider(&input.provider)?;
    ensure_mutation_ready(&state, provider).await?;
    let key = mutable_key(provider, input.server_id)?;
    state
        .mcp_policy_service()
        .set_server_state(project_id, &key, input.state)
        .await
        .map_err(|error| error.to_string())?;
    Ok(McpMutationResponse { changed: true })
}

#[tauri::command]
pub async fn clear_mcp_server_override(
    input: ClearMcpServerOverrideInput,
    state: State<'_, AppState>,
) -> Result<McpMutationResponse, String> {
    let project_id = scope_project_id(input.project_id.as_deref())?;
    let provider = parse_provider(&input.provider)?;
    ensure_mutation_ready(&state, provider).await?;
    let key = mutable_key(provider, input.server_id)?;
    let changed = state
        .mcp_policy_service()
        .clear_server(project_id, &key)
        .await
        .map_err(|error| error.to_string())?;
    Ok(McpMutationResponse { changed })
}

#[tauri::command]
pub async fn update_mcp_tool_override(
    input: McpToolOverrideInput,
    state: State<'_, AppState>,
) -> Result<McpMutationResponse, String> {
    let project_id = scope_project_id(input.project_id.as_deref())?;
    let provider = parse_provider(&input.provider)?;
    ensure_mutation_ready(&state, provider).await?;
    let key = mutable_key(provider, input.server_id)?;
    state
        .mcp_policy_service()
        .set_tool_state(project_id, &key, &input.tool_name, input.state)
        .await
        .map_err(|error| error.to_string())?;
    Ok(McpMutationResponse { changed: true })
}

#[tauri::command]
pub async fn clear_mcp_tool_override(
    input: ClearMcpToolOverrideInput,
    state: State<'_, AppState>,
) -> Result<McpMutationResponse, String> {
    let project_id = scope_project_id(input.project_id.as_deref())?;
    let provider = parse_provider(&input.provider)?;
    ensure_mutation_ready(&state, provider).await?;
    let key = mutable_key(provider, input.server_id)?;
    let changed = state
        .mcp_policy_service()
        .clear_tool(project_id, &key, &input.tool_name)
        .await
        .map_err(|error| error.to_string())?;
    Ok(McpMutationResponse { changed })
}

#[cfg(test)]
pub(crate) fn response_contains_sensitive_definition_fields(value: &serde_json::Value) -> bool {
    let sensitive = ["command", "args", "env", "headers", "token", "url"];
    match value {
        serde_json::Value::Object(map) => map.iter().any(|(key, value)| {
            sensitive.contains(&key.as_str())
                || response_contains_sensitive_definition_fields(value)
        }),
        serde_json::Value::Array(values) => values
            .iter()
            .any(response_contains_sensitive_definition_fields),
        _ => false,
    }
}

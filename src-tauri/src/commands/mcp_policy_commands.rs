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
    McpPolicySource, McpRepairStatus, McpServerKey, McpSetupConflictKind, NativeMcpServerSnapshot,
    NativeMcpState, RALPHX_MCP_SERVER_IDS,
};
use crate::domain::entities::ProjectId;
use crate::domain::repositories::McpCatalogSnapshot;

pub(crate) struct ProviderCatalogDiscovery {
    pub(crate) servers: Vec<NativeMcpServerSnapshot>,
    pub(crate) diagnostic: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct McpCatalogResponse {
    pub eligible_providers: Vec<String>,
    pub eligible_default_provider: Option<String>,
    pub probed_at: String,
    pub probe_stale: bool,
    pub provider_diagnostics: BTreeMap<String, String>,
    pub policy_diagnostics: Vec<String>,
    pub servers: Vec<McpServerResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    pub conflict_kind: Option<McpSetupConflictKind>,
    pub repair_status: Option<McpRepairStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryLegacyMcpRepairInput {
    pub provider: String,
    pub server_id: String,
    pub scope: String,
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

pub(crate) fn mutable_key(
    provider: AgentHarnessKind,
    server_id: String,
) -> Result<McpServerKey, String> {
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

pub(crate) fn policy_server_ids<'a>(
    provider: AgentHarnessKind,
    policies: impl IntoIterator<Item = &'a McpPolicyOverride>,
) -> BTreeSet<String> {
    RALPHX_MCP_SERVER_IDS
        .into_iter()
        .map(str::to_string)
        .chain(
            policies
                .into_iter()
                .filter(|row| row.key.provider == provider)
                .map(|row| row.key.server_id.clone()),
        )
        .collect()
}

pub(crate) fn known_policy_tools<'a>(
    provider: AgentHarnessKind,
    server_id: &str,
    policies: impl IntoIterator<Item = &'a McpPolicyOverride>,
) -> Vec<String> {
    policies
        .into_iter()
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

#[cfg(test)]
pub(crate) fn to_server_response_with_scope_for_test(
    effective: EffectiveMcpServerPolicy,
    scoped: Option<&McpPolicyOverride>,
) -> McpServerResponse {
    to_server_response_with_scope(effective, scoped)
}

#[cfg(test)]
fn to_server_response_with_scope(
    effective: EffectiveMcpServerPolicy,
    scoped: Option<&McpPolicyOverride>,
) -> McpServerResponse {
    to_server_response_with_repair(effective, scoped, None)
}

fn to_server_response_with_repair(
    effective: EffectiveMcpServerPolicy,
    scoped: Option<&McpPolicyOverride>,
    reserved_registration: Option<
        crate::infrastructure::agents::claude::mcp_catalog::ReservedClaudeUserRegistration,
    >,
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
    let is_reserved_collision = effective.native.key.is_ralphx_owned()
        && effective.native.native_scope.as_deref() != Some("ralphx");
    let (conflict_kind, repair_status) = if is_reserved_collision {
        if reserved_registration
            == Some(crate::infrastructure::agents::claude::mcp_catalog::ReservedClaudeUserRegistration::ReservedUserEntry)
            && effective.native.key.provider == AgentHarnessKind::Claude
            && effective.native.key.server_id == "ralphx"
            && effective.native.native_scope.as_deref() == Some("user")
        {
            (
                Some(McpSetupConflictKind::LegacyRegistration),
                Some(McpRepairStatus::Repairable),
            )
        } else {
            (
                Some(McpSetupConflictKind::AmbiguousReservedId),
                Some(McpRepairStatus::ManualOnly),
            )
        }
    } else {
        (None, None)
    };
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
        conflict_kind,
        repair_status,
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
    let (global_yaml, project_yaml) = service
        .load_policy_configs(project_id, project_root.as_deref())
        .map_err(|error| error.to_string())?;
    let policy_diagnostics = global_yaml
        .diagnostics
        .iter()
        .map(|diagnostic| format!("Global MCP policy: {diagnostic}"))
        .chain(
            project_yaml
                .diagnostics
                .iter()
                .map(|diagnostic| format!("Project MCP policy: {diagnostic}")),
        )
        .collect::<Vec<_>>();
    let mut servers = Vec::new();
    let mut provider_diagnostics = BTreeMap::new();
    for provider in providers {
        let reserved_registration = if provider == AgentHarnessKind::Claude {
            let provider_root = service
                .provider_native_config_root(provider)
                .await
                .map_err(|error| error.to_string())?;
            Some(
                crate::infrastructure::agents::claude::mcp_catalog::classify_reserved_user_registration(
                    &provider_root,
                )?,
            )
        } else {
            None
        };
        let discovered =
            discover_provider_catalog(state, &service, provider, project_root.as_deref()).await;
        let mut native_by_id = match discovered {
            Ok(discovery) => {
                if let Some(diagnostic) = discovery.diagnostic {
                    provider_diagnostics.insert(provider.to_string(), diagnostic);
                }
                discovery
                    .servers
                    .into_iter()
                    .map(|row| (row.key.server_id.clone(), row))
                    .collect::<BTreeMap<_, _>>()
            }
            Err(error) => {
                provider_diagnostics.insert(provider.to_string(), error);
                BTreeMap::new()
            }
        };
        let server_ids = policy_server_ids(
            provider,
            global
                .iter()
                .chain(project.iter())
                .chain(global_yaml.policies.values())
                .chain(project_yaml.policies.values()),
        )
        .into_iter()
        .chain(native_by_id.keys().cloned())
        .collect::<BTreeSet<_>>();
        for server_id in server_ids {
            let known_policy_tools = known_policy_tools(
                provider,
                &server_id,
                global
                    .iter()
                    .chain(project.iter())
                    .chain(global_yaml.policies.values())
                    .chain(project_yaml.policies.values()),
            );
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
            servers.push(to_server_response_with_repair(
                effective,
                scoped,
                reserved_registration,
            ));
        }
    }
    let response = McpCatalogResponse {
        eligible_providers: eligibility
            .providers
            .iter()
            .map(ToString::to_string)
            .collect(),
        eligible_default_provider: eligibility.default_provider.map(|value| value.to_string()),
        probed_at: eligibility.probed_at.to_rfc3339(),
        probe_stale: false,
        provider_diagnostics,
        policy_diagnostics,
        servers,
    };
    persist_catalog_snapshot(state, project_id, provider_filter, response).await
}

#[doc(hidden)]
pub(crate) async fn persist_catalog_snapshot(
    state: &AppState,
    project_id: Option<&str>,
    provider_filter: Option<AgentHarnessKind>,
    response: McpCatalogResponse,
) -> Result<McpCatalogResponse, String> {
    let response_json = serde_json::to_string(&response)
        .map_err(|error| format!("catalog_snapshot_serialize_failed: {error}"))?;
    let captured_at = chrono::Utc::now().to_rfc3339();
    let providers = match provider_filter {
        Some(provider) => BTreeSet::from([provider.to_string()]),
        None => response.eligible_providers.iter().cloned().collect(),
    };
    for provider in providers {
        state
            .mcp_catalog_snapshot_repo
            .upsert(McpCatalogSnapshot {
                scope_project_id: project_id.map(str::to_string),
                provider,
                response_json: response_json.clone(),
                captured_at: captured_at.clone(),
            })
            .await
            .map_err(|error| format!("catalog_snapshot_write_failed: {error}"))?;
    }
    Ok(response)
}

async fn discover_provider_catalog(
    state: &AppState,
    service: &crate::application::mcp_policy_service::McpPolicyService,
    provider: AgentHarnessKind,
    project_root: Option<&std::path::Path>,
) -> Result<ProviderCatalogDiscovery, String> {
    let (provider_config_root, provider_env) = service
        .provider_native_context(provider)
        .await
        .map_err(|error| error.to_string())?;
    match provider {
        AgentHarnessKind::Claude => Ok(ProviderCatalogDiscovery {
            servers:
                crate::infrastructure::agents::claude::mcp_catalog::discover_native_mcp_servers(
                    &provider_config_root,
                    project_root,
                )?,
            diagnostic: None,
        }),
        AgentHarnessKind::Codex => {
            let fallback =
                crate::infrastructure::agents::codex::mcp_catalog::discover_native_mcp_servers(
                    &provider_config_root,
                    project_root,
                )?;
            let structured = match resolve_codex_catalog_cli_path(state).await {
                Ok(cli_path) => crate::infrastructure::agents::codex::app_server_mcp_catalog::discover_native_mcp_servers_via_app_server(
                    &cli_path,
                    &provider_config_root,
                    project_root,
                    &provider_env,
                )
                .await,
                Err(error) => Err(error),
            };
            Ok(select_codex_catalog(fallback, structured))
        }
    }
}

pub(crate) fn select_codex_catalog(
    fallback: Vec<NativeMcpServerSnapshot>,
    structured: Result<Vec<NativeMcpServerSnapshot>, String>,
) -> ProviderCatalogDiscovery {
    match structured {
        Ok(structured) => ProviderCatalogDiscovery {
            servers: merge_codex_catalogs(fallback, structured),
            diagnostic: None,
        },
        Err(_) => ProviderCatalogDiscovery {
            servers: fallback,
            diagnostic: Some(
                "Structured Codex MCP status is unavailable for this installed version; RalphX is showing limited redacted metadata from fixed native config paths."
                    .to_string(),
            ),
        },
    }
}

async fn resolve_codex_catalog_cli_path(state: &AppState) -> Result<PathBuf, String> {
    let settings = state
        .agent_provider_settings_repo
        .get(AgentHarnessKind::Codex)
        .await
        .map_err(|error| format!("provider_settings_failed: {error}"))?
        .ok_or_else(|| "provider_not_ready: Codex settings are unavailable".to_string())?;
    if let Some(path) = crate::application::managed_provider_cli::checked_provider_cli_launch_path(
        &settings,
        "Codex MCP catalog",
    ) {
        return path;
    }
    tokio::task::spawn_blocking(|| crate::infrastructure::agents::codex::resolve_codex_cli())
        .await
        .map_err(|_| "Codex MCP catalog CLI resolution failed".to_string())?
        .map(|resolved| resolved.path)
}

fn merge_codex_catalogs(
    fallback: Vec<NativeMcpServerSnapshot>,
    structured: Vec<NativeMcpServerSnapshot>,
) -> Vec<NativeMcpServerSnapshot> {
    let mut servers = structured
        .into_iter()
        .map(|row| (row.key.server_id.clone(), row))
        .collect::<BTreeMap<_, _>>();
    for fallback_row in fallback {
        match servers.get_mut(&fallback_row.key.server_id) {
            Some(structured_row) => {
                structured_row
                    .known_tools
                    .extend(fallback_row.known_tools.iter().cloned());
                structured_row.known_tools.sort();
                structured_row.known_tools.dedup();
                if structured_row.native_scope.is_none() {
                    structured_row.native_scope = fallback_row.native_scope.clone();
                }
                if fallback_row.native_state == NativeMcpState::Untrusted {
                    structured_row.native_state = NativeMcpState::Untrusted;
                    structured_row.diagnostic = fallback_row.diagnostic.clone();
                    structured_row.native_scope = fallback_row.native_scope.clone();
                }
            }
            None => {
                servers.insert(fallback_row.key.server_id.clone(), fallback_row);
            }
        }
    }
    servers.into_values().collect()
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

#[tauri::command]
pub async fn retry_legacy_mcp_registration_repair(
    input: RetryLegacyMcpRepairInput,
    state: State<'_, AppState>,
) -> Result<McpMutationResponse, String> {
    let provider = parse_provider(&input.provider)?;
    validate_legacy_repair_request(provider, &input.server_id, &input.scope)?;
    ensure_mutation_ready(&state, provider).await?;
    let changed = state
        .mcp_policy_service()
        .retry_reserved_claude_registration_repair()
        .await
        .map_err(|error| error.to_string())?;
    Ok(McpMutationResponse { changed })
}

pub(crate) fn validate_legacy_repair_request(
    provider: AgentHarnessKind,
    server_id: &str,
    scope: &str,
) -> Result<(), String> {
    if provider == AgentHarnessKind::Claude && server_id == "ralphx" && scope == "user" {
        return Ok(());
    }
    Err("legacy_repair_not_allowed: only Claude user-scoped ralphx is eligible".to_string())
}

async fn ensure_mutation_ready(state: &AppState, provider: AgentHarnessKind) -> Result<(), String> {
    resolve_provider_management_eligibility(&state.agent_provider_settings_repo)
        .await
        .map_err(|error| error.to_string())?
        .ensure_ready(provider)
        .map_err(|error| error.to_string())
}

pub(crate) async fn ensure_project_scope_exists(
    state: &AppState,
    project_id: Option<&str>,
) -> Result<(), String> {
    project_root(state, project_id).await.map(|_| ())
}

#[tauri::command]
pub async fn update_mcp_server_override(
    input: McpServerOverrideInput,
    state: State<'_, AppState>,
) -> Result<McpMutationResponse, String> {
    let project_id = scope_project_id(input.project_id.as_deref())?;
    let provider = parse_provider(&input.provider)?;
    ensure_project_scope_exists(&state, project_id).await?;
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
    ensure_project_scope_exists(&state, project_id).await?;
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
    ensure_project_scope_exists(&state, project_id).await?;
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
    ensure_project_scope_exists(&state, project_id).await?;
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

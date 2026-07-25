use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{Runtime, State};

use crate::application::agent_lane_resolution::routing_role_for_chat_launch;
use crate::application::chat_service::ChatService;
use crate::application::manual_role_default_service::{
    validate_role_value, ResolvedManualRoleDefault,
};
use crate::application::personas::PersonaService;
use crate::application::AppState;
use crate::domain::agents::{
    AgentHarnessKind, LogicalEffort, ManualRoleDefault, ManualServiceTier, RoutingRole,
    RoutingRoleFamily, StoredManualRoleDefault, ROUTING_ROLES,
};
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatContextType, ChatConversationId, CoordinationMode,
    PersonaId, ProjectId,
};
use crate::domain::services::RunningAgentKey;
use crate::infrastructure::agents::agent_personas_enabled;
use crate::infrastructure::agents::claude::ui_feature_flags_config;

use super::unified_chat_commands::create_chat_service;
use super::ExecutionState;

#[derive(Debug, Clone, Serialize)]
pub struct ManualRoleDefaultResponse {
    pub provider: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub service_tier: String,
    pub coordination_mode: Option<String>,
    pub persona_id: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ControlOptionResponse {
    pub value: String,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PersonaControlResponse {
    pub enabled: bool,
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RoleControlOptionsResponse {
    pub capabilities: Vec<ControlOptionResponse>,
    pub speeds: Vec<ControlOptionResponse>,
    pub persona: PersonaControlResponse,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManualRoleCatalogEntryResponse {
    pub role: String,
    pub display_name: String,
    pub description: String,
    pub family: String,
    pub family_display_name: String,
    pub requires_tasks: bool,
    pub configured: Option<ManualRoleDefaultResponse>,
    pub effective: Option<ManualRoleDefaultResponse>,
    pub source: Option<String>,
    pub diagnostics: Vec<String>,
    pub controls: RoleControlOptionsResponse,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManualRoleCatalogResponse {
    pub project_id: Option<String>,
    pub roles: Vec<ManualRoleCatalogEntryResponse>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualRoleDefaultInput {
    pub provider: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub service_tier: String,
    pub coordination_mode: Option<String>,
    pub persona_id: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateManualRoleDefaultInput {
    pub project_id: Option<String>,
    pub role: String,
    pub value: ManualRoleDefaultInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearManualRoleDefaultInput {
    pub project_id: Option<String>,
    pub role: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveManualRoleDefaultInput {
    pub project_id: Option<String>,
    pub role: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartComposerRoleDefaultInput {
    pub project_id: String,
    pub mode: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConversationRoleDefaultInput {
    pub conversation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetAgentConversationRoleDefaultInput {
    pub conversation_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComposerRoleDefaultResponse {
    pub role: String,
    pub source: String,
    pub value: ManualRoleDefaultResponse,
}

#[tauri::command]
pub async fn get_manual_role_defaults(
    project_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<ManualRoleCatalogResponse, String> {
    let started_at = std::time::Instant::now();
    let phase_started_at = std::time::Instant::now();
    let project_root = project_root(state.inner(), project_id.as_deref()).await?;
    tracing::info!(
        operation = "manual_role_defaults_phase",
        phase = "load_project_root",
        project_id = %project_id.as_deref().unwrap_or("global"),
        elapsed_ms = phase_started_at.elapsed().as_millis() as u64,
        total_elapsed_ms = started_at.elapsed().as_millis() as u64,
        "Manual role defaults phase completed"
    );
    let phase_started_at = std::time::Instant::now();
    let configured = configured_rows(state.inner(), project_id.as_deref()).await?;
    tracing::info!(
        operation = "manual_role_defaults_phase",
        phase = "load_configured_defaults",
        project_id = %project_id.as_deref().unwrap_or("global"),
        configured_roles = configured.len(),
        elapsed_ms = phase_started_at.elapsed().as_millis() as u64,
        total_elapsed_ms = started_at.elapsed().as_millis() as u64,
        "Manual role defaults phase completed"
    );
    let mut roles = Vec::with_capacity(ROUTING_ROLES.len());

    for role in ROUTING_ROLES {
        let phase_started_at = std::time::Instant::now();
        let configured_value = configured.get(&role).map(|row| response(&row.value));
        let resolution = state
            .resolve_effective_manual_role_default(
                project_id.as_deref(),
                project_root.as_deref(),
                role,
            )
            .await;
        tracing::info!(
            operation = "manual_role_defaults_phase",
            phase = "resolve_role",
            project_id = %project_id.as_deref().unwrap_or("global"),
            role = %role,
            elapsed_ms = phase_started_at.elapsed().as_millis() as u64,
            total_elapsed_ms = started_at.elapsed().as_millis() as u64,
            "Manual role defaults phase completed"
        );
        roles.push(catalog_entry(
            role,
            configured_value,
            resolution,
            agent_personas_enabled(),
            &state.agent_capability_gate,
        ));
    }

    tracing::info!(
        operation = "manual_role_defaults_phase",
        phase = "total",
        project_id = %project_id.as_deref().unwrap_or("global"),
        role_count = roles.len(),
        elapsed_ms = started_at.elapsed().as_millis() as u64,
        total_elapsed_ms = started_at.elapsed().as_millis() as u64,
        "Manual role defaults phase completed"
    );
    Ok(ManualRoleCatalogResponse { project_id, roles })
}

#[tauri::command]
pub async fn get_effective_manual_role_default(
    input: EffectiveManualRoleDefaultInput,
    state: State<'_, AppState>,
) -> Result<ManualRoleCatalogEntryResponse, String> {
    let role = parse_role(&input.role)?;
    let project_root = project_root(state.inner(), input.project_id.as_deref()).await?;
    let configured = configured_rows(state.inner(), input.project_id.as_deref()).await?;
    let resolution = state
        .resolve_effective_manual_role_default(
            input.project_id.as_deref(),
            project_root.as_deref(),
            role,
        )
        .await;
    Ok(catalog_entry(
        role,
        configured.get(&role).map(|row| response(&row.value)),
        resolution,
        agent_personas_enabled(),
        &state.agent_capability_gate,
    ))
}

/// Resolve the backend-owned role default for a new Workspace conversation.
#[tauri::command]
pub async fn get_start_composer_role_default(
    input: StartComposerRoleDefaultInput,
    state: State<'_, AppState>,
) -> Result<ComposerRoleDefaultResponse, String> {
    let mode = AgentConversationWorkspaceMode::from_str(input.mode.trim())?;
    let role = routing_role_for_chat_launch(
        "ralphx-general-worker",
        ChatContextType::Project,
        None,
        Some(mode),
        false,
    );
    resolve_composer_role_default(state.inner(), &input.project_id, role).await
}

/// Resolve the backend-owned role default for an existing Workspace conversation.
#[tauri::command]
pub async fn get_agent_conversation_role_default(
    input: AgentConversationRoleDefaultInput,
    state: State<'_, AppState>,
) -> Result<ComposerRoleDefaultResponse, String> {
    let conversation = load_project_conversation(state.inner(), &input.conversation_id).await?;
    let role = routing_role_for_chat_launch(
        "ralphx-general-worker",
        ChatContextType::Project,
        None,
        conversation.agent_mode,
        false,
    );
    resolve_composer_role_default(state.inner(), &conversation.context_id, role).await
}

/// Reset an active Workspace conversation to its complete effective role default.
///
/// Provider/model/effort/speed are returned to the composer. Conversation-owned
/// coordination and persona bindings are validated first and then written in one
/// repository operation so a failed reset cannot expose a partially applied tuple.
#[tauri::command]
pub async fn reset_agent_conversation_role_default<R: Runtime + 'static>(
    input: ResetAgentConversationRoleDefaultInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle<R>,
) -> Result<ComposerRoleDefaultResponse, String> {
    let service = create_chat_service(state.inner(), app, execution_state.inner());
    reset_agent_conversation_role_default_for_state(input, state.inner(), Some(&service)).await
}

#[doc(hidden)]
pub async fn reset_agent_conversation_role_default_for_state(
    input: ResetAgentConversationRoleDefaultInput,
    state: &AppState,
    chat_service: Option<&dyn ChatService>,
) -> Result<ComposerRoleDefaultResponse, String> {
    let conversation = load_project_conversation(state, &input.conversation_id).await?;
    let role = routing_role_for_chat_launch(
        "ralphx-general-worker",
        ChatContextType::Project,
        None,
        conversation.agent_mode,
        false,
    );
    let resolved = resolve_composer_role_default(state, &conversation.context_id, role).await?;
    let value = parse_response(&resolved.value)?;

    crate::application::ensure_provider_spawn_enabled(
        &state.agent_provider_settings_repo,
        value.harness,
        "reset_agent_conversation_role_default",
    )
    .await?;
    let coordination_mode = value.coordination_mode.unwrap_or(CoordinationMode::Solo);
    let codex_ultra_supported =
        crate::application::agent_capability_validation::codex_ultra_support_for_model(
            value.harness,
            value.model.as_deref(),
        );
    crate::application::agent_capability_validation::validate_agent_capability(
        coordination_mode,
        value.harness,
        &state.agent_capability_gate,
        codex_ultra_supported,
    )
    .map_err(|error| error.to_string())?;
    validate_persona(
        state,
        value.persona_id.as_ref(),
        Some(&conversation.context_id),
    )
    .await?;

    let persona_id = value.persona_id.as_ref().map(PersonaId::as_str);
    let persona_changed = conversation.persona_id.as_deref() != persona_id;
    let coordination_changed = conversation.coordination_mode != coordination_mode;
    let running_key = RunningAgentKey::new(
        ChatContextType::Project.to_string(),
        conversation.id.as_str(),
    );
    if state.running_agent_registry.is_running(&running_key).await {
        if persona_changed {
            let service = chat_service.ok_or_else(|| {
                "Cannot reset persona while the conversation agent is running".to_string()
            })?;
            service
                .stop_agent(ChatContextType::Project, &conversation.id.as_str())
                .await
                .map_err(|error| error.to_string())?;
            if state.running_agent_registry.is_running(&running_key).await {
                return Err(
                    "Cannot reset persona while the conversation agent is running".to_string(),
                );
            }
        } else if coordination_changed {
            return Err(
                "Cannot reset coordination while the conversation agent is running".to_string(),
            );
        }
    }

    let clear_provider_session =
        persona_changed && ui_feature_flags_config().persona_switch_forces_fresh_provider_session;
    state
        .chat_conversation_repo
        .update_role_default_bindings(
            &conversation.id,
            coordination_mode,
            persona_id,
            clear_provider_session,
        )
        .await
        .map_err(|error| format!("Failed to reset conversation role default: {error}"))?;

    Ok(resolved)
}

#[tauri::command]
pub async fn update_manual_role_default(
    input: UpdateManualRoleDefaultInput,
    state: State<'_, AppState>,
) -> Result<ManualRoleDefaultResponse, String> {
    update_manual_role_default_for_state(input, state.inner()).await
}

#[doc(hidden)]
pub async fn update_manual_role_default_for_state(
    input: UpdateManualRoleDefaultInput,
    state: &AppState,
) -> Result<ManualRoleDefaultResponse, String> {
    let role = parse_role(&input.role)?;
    let value = parse_input(input.value)?;
    validate_manual_role_default_update(role, &value, &state.agent_capability_gate)?;
    validate_persona(
        state,
        value.persona_id.as_ref(),
        input.project_id.as_deref(),
    )
    .await?;

    let row = match input.project_id {
        Some(project_id) => {
            state
                .manual_role_default_repo
                .upsert_for_project(&project_id, role, &value)
                .await
        }
        None => {
            state
                .manual_role_default_repo
                .upsert_global(role, &value)
                .await
        }
    }
    .map_err(|error| format!("Failed to save manual role default: {error}"))?;
    Ok(response(&row.value))
}

#[tauri::command]
pub async fn clear_manual_role_default(
    input: ClearManualRoleDefaultInput,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let role = parse_role(&input.role)?;
    match input.project_id {
        Some(project_id) => {
            state
                .manual_role_default_repo
                .clear_for_project(&project_id, role)
                .await
        }
        None => state.manual_role_default_repo.clear_global(role).await,
    }
    .map_err(|error| format!("Failed to clear manual role default: {error}"))
}

async fn load_project_conversation(
    state: &AppState,
    conversation_id: &str,
) -> Result<crate::domain::entities::ChatConversation, String> {
    let conversation_id = ChatConversationId::from_string(conversation_id.to_string());
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| format!("Failed to load conversation role default: {error}"))?
        .ok_or_else(|| format!("Conversation not found: {conversation_id}"))?;
    if conversation.context_type != ChatContextType::Project {
        return Err("Role-default reset is available only for project conversations".to_string());
    }
    Ok(conversation)
}

async fn resolve_composer_role_default(
    state: &AppState,
    project_id: &str,
    role: RoutingRole,
) -> Result<ComposerRoleDefaultResponse, String> {
    let root = project_root(state, Some(project_id))
        .await?
        .ok_or_else(|| format!("Project not found: {project_id}"))?;
    let resolved = state
        .resolve_effective_manual_role_default(Some(project_id), Some(&root), role)
        .await
        .map_err(|error| format!("Failed to resolve manual default for {role}: {error}"))?;
    Ok(ComposerRoleDefaultResponse {
        role: role.to_string(),
        source: resolved.source.key().to_string(),
        value: response(&resolved.value),
    })
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
        .get_by_id(&ProjectId::from_string(project_id.to_string()))
        .await
        .map_err(|error| format!("Failed to load project for role defaults: {error}"))?
        .ok_or_else(|| format!("Project not found: {project_id}"))?;
    Ok(Some(PathBuf::from(project.working_directory)))
}

async fn configured_rows(
    state: &AppState,
    project_id: Option<&str>,
) -> Result<HashMap<RoutingRole, StoredManualRoleDefault>, String> {
    let rows = match project_id {
        Some(project_id) => {
            state
                .manual_role_default_repo
                .list_for_project(project_id)
                .await
        }
        None => state.manual_role_default_repo.list_global().await,
    }
    .map_err(|error| format!("Failed to load configured manual role defaults: {error}"))?;
    Ok(rows.into_iter().map(|row| (row.role, row)).collect())
}

async fn validate_persona(
    state: &AppState,
    persona_id: Option<&PersonaId>,
    project_id: Option<&str>,
) -> Result<(), String> {
    let Some(persona_id) = persona_id else {
        return Ok(());
    };
    let project_id = project_id.map(|value| ProjectId::from_string(value.to_string()));
    PersonaService::new(
        state.db.clone(),
        state.persona_repo.clone(),
        state.chat_conversation_repo.clone(),
    )
    .ensure_bindable_to_scope(agent_personas_enabled(), persona_id, project_id.as_ref())
    .await
    .map(|_| ())
    .map_err(|error| error.to_string())
}

pub(super) fn catalog_entry(
    role: RoutingRole,
    configured: Option<ManualRoleDefaultResponse>,
    resolution: crate::error::AppResult<ResolvedManualRoleDefault>,
    personas_enabled: bool,
    capability_gate: &crate::application::agent_capability_gate::AgentCapabilityGate,
) -> ManualRoleCatalogEntryResponse {
    let metadata = role.metadata();
    let (effective, source, diagnostics, provider, model) = match resolution {
        Ok(resolved) => (
            Some(response(&resolved.value)),
            Some(resolved.source.key().to_string()),
            resolved.diagnostics,
            resolved.value.harness,
            resolved.value.model.clone(),
        ),
        Err(error) => (
            None,
            None,
            vec![error.to_string()],
            AgentHarnessKind::Claude,
            None,
        ),
    };
    ManualRoleCatalogEntryResponse {
        role: metadata.key.to_string(),
        display_name: metadata.display_name.to_string(),
        description: metadata.description.to_string(),
        family: metadata.family.key().to_string(),
        family_display_name: metadata.family.display_name().to_string(),
        requires_tasks: metadata.requires_tasks,
        configured,
        effective,
        source,
        diagnostics,
        controls: control_options(
            role,
            provider,
            model.as_deref(),
            personas_enabled,
            capability_gate,
        ),
    }
}

pub(super) fn control_options(
    role: RoutingRole,
    provider: AgentHarnessKind,
    model: Option<&str>,
    personas_enabled: bool,
    capability_gate: &crate::application::agent_capability_gate::AgentCapabilityGate,
) -> RoleControlOptionsResponse {
    let workspace_root = role.metadata().family == RoutingRoleFamily::Workspace;
    let ultra_supported =
        crate::application::agent_capability_validation::codex_ultra_support_for_model(
            provider, model,
        ) == Some(true);
    let ultra_disabled_reason = if !workspace_root {
        "Codex Ultra is available only for Workspace root roles".to_string()
    } else if provider != AgentHarnessKind::Codex {
        "Codex Ultra is available only with the Codex provider.".to_string()
    } else {
        crate::application::agent_capability_validation::AgentCapabilityError::UltraUnavailable
            .to_string()
    };
    let capability = |value: &str, supported: bool, reason: &str| ControlOptionResponse {
        value: value.to_string(),
        enabled: supported,
        disabled_reason: (!supported).then(|| reason.to_string()),
    };
    RoleControlOptionsResponse {
        capabilities: vec![
            capability("solo", true, ""),
            capability(
                "rx_native_team",
                workspace_root && capability_gate.team_enabled(),
                if workspace_root {
                    "Team is disabled. Enable it in Settings > Capabilities."
                } else {
                    "Team is available only for Workspace root roles"
                },
            ),
            capability(
                "rx_native_workflow",
                workspace_root && capability_gate.workflows_enabled(),
                if workspace_root {
                    "Workflows are disabled. Enable them in Settings > Capabilities."
                } else {
                    "Workflow is available only for Workspace root roles"
                },
            ),
            capability(
                "codex_native_ultra",
                workspace_root && ultra_supported,
                &ultra_disabled_reason,
            ),
        ],
        speeds: vec![
            capability("provider_default", true, ""),
            capability("standard", true, ""),
            capability(
                "fast",
                crate::application::agent_capability_validation::codex_fast_support_for_model(
                    provider, model,
                ) == Some(true),
                "Fast requires a supported Codex provider and model",
            ),
        ],
        persona: PersonaControlResponse {
            enabled: workspace_root && personas_enabled,
            disabled_reason: (!(workspace_root && personas_enabled)).then(|| {
                if !workspace_root {
                    "Persona is limited to Workspace Project conversations in V1".to_string()
                } else {
                    "Agent Personas is disabled".to_string()
                }
            }),
        },
    }
}

fn validate_manual_role_default_update(
    role: RoutingRole,
    value: &ManualRoleDefault,
    capability_gate: &crate::application::agent_capability_gate::AgentCapabilityGate,
) -> Result<(), String> {
    validate_role_value(role, value).map_err(|error| error.to_string())?;
    crate::application::agent_capability_validation::validate_manual_role_runtime_capabilities(
        value,
        capability_gate,
    )
}

pub(super) fn parse_input(input: ManualRoleDefaultInput) -> Result<ManualRoleDefault, String> {
    Ok(ManualRoleDefault {
        harness: AgentHarnessKind::from_str(&input.provider)?,
        model: trim(input.model),
        effort: input
            .effort
            .as_deref()
            .map(LogicalEffort::from_str)
            .transpose()?,
        service_tier: ManualServiceTier::from_str(&input.service_tier)?,
        coordination_mode: input
            .coordination_mode
            .as_deref()
            .map(CoordinationMode::from_str)
            .transpose()?,
        persona_id: trim(input.persona_id).map(PersonaId::from),
        approval_policy: trim(input.approval_policy),
        sandbox_mode: trim(input.sandbox_mode),
    })
}

fn parse_response(value: &ManualRoleDefaultResponse) -> Result<ManualRoleDefault, String> {
    parse_input(ManualRoleDefaultInput {
        provider: value.provider.clone(),
        model: value.model.clone(),
        effort: value.effort.clone(),
        service_tier: value.service_tier.clone(),
        coordination_mode: value.coordination_mode.clone(),
        persona_id: value.persona_id.clone(),
        approval_policy: value.approval_policy.clone(),
        sandbox_mode: value.sandbox_mode.clone(),
    })
}

fn parse_role(role: &str) -> Result<RoutingRole, String> {
    RoutingRole::from_str(role)
}

fn trim(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

fn response(value: &ManualRoleDefault) -> ManualRoleDefaultResponse {
    ManualRoleDefaultResponse {
        provider: value.harness.to_string(),
        model: value.model.clone(),
        effort: value.effort.map(|effort| effort.to_string()),
        service_tier: value.service_tier.to_string(),
        coordination_mode: value.coordination_mode.map(|mode| mode.to_string()),
        persona_id: value.persona_id.as_ref().map(ToString::to_string),
        approval_policy: value.approval_policy.clone(),
        sandbox_mode: value.sandbox_mode.clone(),
    }
}

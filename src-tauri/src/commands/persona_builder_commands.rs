use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::application::persona_ingest::{
    ingest_picked_roots, persona_builder_ingest_session_is_live, persona_ingest_conversation_path,
    persona_ingest_storage_path, PersonaIngestManifest,
};
use crate::application::personas::PERSONA_FEATURE_DISABLED_PREFIX;
use crate::application::AppState;
use crate::commands::unified_chat_commands::{
    agent_conversation_response_for_state, AgentConversationResponse,
};
use crate::domain::entities::{AgentConversationWorkspaceMode, ChatConversation, ProjectId};
use crate::infrastructure::agents::claude::agent_personas_enabled;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePersonaBuilderConversationInput {
    pub project_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestPersonaContextInput {
    pub conversation_id: String,
    pub picked_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonaBuilderIngestStatusInput {
    pub conversation_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonaBuilderIngestStatusResponse {
    pub live: bool,
}

/// Create the sole persisted entry point for a PersonaBuilder conversation.
#[tauri::command]
pub async fn create_persona_builder_conversation(
    input: CreatePersonaBuilderConversationInput,
    state: State<'_, AppState>,
) -> Result<AgentConversationResponse, String> {
    create_persona_builder_conversation_for_state(input, state.inner(), agent_personas_enabled())
        .await
}

/// Copy a picked context path into app-owned PersonaBuilder ingest storage.
#[tauri::command]
pub async fn ingest_persona_context<R: tauri::Runtime>(
    input: IngestPersonaContextInput,
    state: State<'_, AppState>,
    app: tauri::AppHandle<R>,
) -> Result<PersonaIngestManifest, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to resolve app data dir: {error}"))?;
    ingest_persona_context_for_state(
        input,
        state.inner(),
        agent_personas_enabled(),
        &app_data_dir,
    )
    .await
}

/// Return whether a PersonaBuilder conversation has app-owned ingested context available.
#[tauri::command]
pub async fn get_persona_builder_ingest_status<R: tauri::Runtime>(
    input: PersonaBuilderIngestStatusInput,
    state: State<'_, AppState>,
    app: tauri::AppHandle<R>,
) -> Result<PersonaBuilderIngestStatusResponse, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Failed to resolve app data dir: {error}"))?;
    get_persona_builder_ingest_status_for_state(
        input,
        state.inner(),
        agent_personas_enabled(),
        &app_data_dir,
    )
    .await
}

#[doc(hidden)]
pub async fn create_persona_builder_conversation_for_state(
    input: CreatePersonaBuilderConversationInput,
    state: &AppState,
    feature_enabled: bool,
) -> Result<AgentConversationResponse, String> {
    if !feature_enabled {
        return Err(crate::error::AppError::FeatureDisabled(format!(
            "{PERSONA_FEATURE_DISABLED_PREFIX} agent personas feature is disabled]"
        ))
        .to_string());
    }

    let mut conversation = ChatConversation::new_project(ProjectId::from_string(input.project_id));
    conversation.set_agent_mode(Some(AgentConversationWorkspaceMode::PersonaBuilder));
    conversation.set_title("Persona builder".to_string());
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .map_err(|error| error.to_string())?;

    agent_conversation_response_for_state(state, conversation).await
}

#[doc(hidden)]
pub async fn ingest_persona_context_for_state(
    input: IngestPersonaContextInput,
    state: &AppState,
    feature_enabled: bool,
    app_data_dir: &Path,
) -> Result<PersonaIngestManifest, String> {
    if !feature_enabled {
        return Err(crate::error::AppError::FeatureDisabled(format!(
            "{PERSONA_FEATURE_DISABLED_PREFIX} agent personas feature is disabled]"
        ))
        .to_string());
    }

    let conversation_id =
        crate::domain::entities::ChatConversationId::from_string(input.conversation_id.clone());
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "PersonaBuilder conversation was not found".to_string())?;
    if conversation.agent_mode != Some(AgentConversationWorkspaceMode::PersonaBuilder) {
        return Err("Persona context ingestion requires a PersonaBuilder conversation".to_string());
    }

    let storage_root = persona_ingest_storage_path(app_data_dir);
    let destination_root = persona_ingest_conversation_path(&storage_root, &input.conversation_id);
    let picked_paths = input
        .picked_paths
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    ingest_picked_roots(&picked_paths, &destination_root).map_err(|error| error.to_string())
}

#[doc(hidden)]
pub async fn get_persona_builder_ingest_status_for_state(
    input: PersonaBuilderIngestStatusInput,
    state: &AppState,
    feature_enabled: bool,
    app_data_dir: &Path,
) -> Result<PersonaBuilderIngestStatusResponse, String> {
    if !feature_enabled {
        return Err(crate::error::AppError::FeatureDisabled(format!(
            "{PERSONA_FEATURE_DISABLED_PREFIX} agent personas feature is disabled]"
        ))
        .to_string());
    }

    let conversation_id =
        crate::domain::entities::ChatConversationId::from_string(input.conversation_id.clone());
    let conversation = state
        .chat_conversation_repo
        .get_by_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "PersonaBuilder conversation was not found".to_string())?;
    if conversation.agent_mode != Some(AgentConversationWorkspaceMode::PersonaBuilder) {
        return Err("Persona context ingestion requires a PersonaBuilder conversation".to_string());
    }

    Ok(PersonaBuilderIngestStatusResponse {
        live: persona_builder_ingest_session_is_live(Some(app_data_dir), &input.conversation_id),
    })
}

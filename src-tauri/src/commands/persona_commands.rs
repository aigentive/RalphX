use serde::Deserialize;
use tauri::State;

use crate::application::personas::{draft_updated_payload, PersonaService, SavePersonaDraftInput};
use crate::application::AppState;
use crate::domain::entities::{Persona, PersonaId};
use crate::infrastructure::agents::claude::ui_feature_flags_config;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPersonasInput {}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonaIdInput {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePersonaDraftInput {
    pub slug: String,
    pub content: String,
    #[serde(default)]
    pub source_session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePersonaInput {
    pub id: String,
    pub content: String,
}

impl From<CreatePersonaDraftInput> for SavePersonaDraftInput {
    fn from(input: CreatePersonaDraftInput) -> Self {
        Self {
            slug: input.slug,
            content: input.content,
            source_session_id: input.source_session_id,
        }
    }
}

#[tauri::command]
pub async fn list_personas(
    _input: ListPersonasInput,
    state: State<'_, AppState>,
) -> Result<Vec<Persona>, String> {
    service(&state)
        .list_personas(enabled())
        .await
        .map_err(to_string)
}

#[tauri::command]
pub async fn get_persona(
    input: PersonaIdInput,
    state: State<'_, AppState>,
) -> Result<Persona, String> {
    service(&state)
        .get_persona(enabled(), &persona_id(input.id)?)
        .await
        .map_err(to_string)
}

#[tauri::command]
pub async fn create_persona_draft(
    input: CreatePersonaDraftInput,
    state: State<'_, AppState>,
) -> Result<Persona, String> {
    let persona = service(&state)
        .create_draft(enabled(), input.into())
        .await
        .map_err(to_string)?;
    emit_draft_updated(&state, &persona);
    Ok(persona)
}

#[tauri::command]
pub async fn update_persona(
    input: UpdatePersonaInput,
    state: State<'_, AppState>,
) -> Result<Persona, String> {
    service(&state)
        .update_persona(enabled(), &persona_id(input.id)?, &input.content)
        .await
        .map_err(to_string)
}

#[tauri::command]
pub async fn approve_persona(
    input: PersonaIdInput,
    state: State<'_, AppState>,
) -> Result<Persona, String> {
    service(&state)
        .approve_persona(enabled(), &persona_id(input.id)?)
        .await
        .map_err(to_string)
}

#[tauri::command]
pub async fn archive_persona(
    input: PersonaIdInput,
    state: State<'_, AppState>,
) -> Result<Persona, String> {
    service(&state)
        .archive_persona(enabled(), &persona_id(input.id)?)
        .await
        .map_err(to_string)
}

#[tauri::command]
pub async fn delete_persona_draft(
    input: PersonaIdInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    service(&state)
        .hard_delete_draft(enabled(), &persona_id(input.id)?)
        .await
        .map_err(to_string)
}

fn service(state: &AppState) -> PersonaService {
    PersonaService::new(
        state.db.clone(),
        state.persona_repo.clone(),
        state.chat_conversation_repo.clone(),
    )
}

fn enabled() -> bool {
    ui_feature_flags_config().agent_personas
}

fn persona_id(id: String) -> Result<PersonaId, String> {
    if id.trim().is_empty() {
        Err("persona id cannot be empty".to_string())
    } else {
        Ok(PersonaId::from(id))
    }
}

pub(crate) fn emit_draft_updated(state: &AppState, persona: &Persona) {
    state
        .events
        .emit("persona:draft_updated", draft_updated_payload(persona));
}

fn to_string(error: crate::error::AppError) -> String {
    error.to_string()
}

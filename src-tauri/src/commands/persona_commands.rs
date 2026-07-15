use ralphx_domain::personas::validation::compose_persona_content;
use serde::Deserialize;
use tauri::State;

use crate::application::personas::{
    draft_applied_payload, draft_updated_payload, PersonaService, SavePersonaDraftInput,
};
use crate::application::AppState;
use crate::domain::entities::{Persona, PersonaId};
use crate::infrastructure::agents::claude::agent_personas_enabled;

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
pub struct ApprovePersonaAsNewInput {
    pub id: String,
    #[serde(default)]
    pub new_slug: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatePersonaDraftInput {
    pub slug: String,
    pub content: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub source_session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePersonaInput {
    pub id: String,
    pub content: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
}

#[tauri::command]
pub async fn list_personas(
    input: ListPersonasInput,
    state: State<'_, AppState>,
) -> Result<Vec<Persona>, String> {
    list_personas_for_state(input, state.inner(), enabled()).await
}

#[doc(hidden)]
pub async fn list_personas_for_state(
    _input: ListPersonasInput,
    state: &AppState,
    feature_enabled: bool,
) -> Result<Vec<Persona>, String> {
    service(state)
        .list_personas(feature_enabled)
        .await
        .map_err(to_string)
}

#[tauri::command]
pub async fn get_persona(
    input: PersonaIdInput,
    state: State<'_, AppState>,
) -> Result<Persona, String> {
    get_persona_for_state(input, state.inner(), enabled()).await
}

#[doc(hidden)]
pub async fn get_persona_for_state(
    input: PersonaIdInput,
    state: &AppState,
    feature_enabled: bool,
) -> Result<Persona, String> {
    service(state)
        .get_persona(feature_enabled, &persona_id(input.id)?)
        .await
        .map_err(to_string)
}

#[tauri::command]
pub async fn create_persona_draft(
    input: CreatePersonaDraftInput,
    state: State<'_, AppState>,
) -> Result<Persona, String> {
    create_persona_draft_for_state(input, state.inner(), enabled()).await
}

#[doc(hidden)]
pub async fn create_persona_draft_for_state(
    input: CreatePersonaDraftInput,
    state: &AppState,
    feature_enabled: bool,
) -> Result<Persona, String> {
    let content =
        resolve_persona_content(&input.slug, input.content, input.description, input.body)?;
    let persona = service(state)
        .create_draft(
            feature_enabled,
            SavePersonaDraftInput {
                slug: input.slug,
                content,
                source_session_id: input.source_session_id,
                source_persona_id: None,
                source_content_hash: None,
            },
        )
        .await
        .map_err(to_string)?;
    emit_draft_updated(state, &persona);
    Ok(persona)
}

#[tauri::command]
pub async fn update_persona(
    input: UpdatePersonaInput,
    state: State<'_, AppState>,
) -> Result<Persona, String> {
    update_persona_for_state(input, state.inner(), enabled()).await
}

#[doc(hidden)]
pub async fn update_persona_for_state(
    input: UpdatePersonaInput,
    state: &AppState,
    feature_enabled: bool,
) -> Result<Persona, String> {
    let id = persona_id(input.id)?;
    let persona_service = service(state);
    let existing = persona_service
        .get_persona(feature_enabled, &id)
        .await
        .map_err(to_string)?;
    let content =
        resolve_persona_content(&existing.slug, input.content, input.description, input.body)?;

    persona_service
        .update_persona(feature_enabled, &id, &content)
        .await
        .map_err(to_string)
}

#[tauri::command]
pub async fn approve_persona(
    input: PersonaIdInput,
    state: State<'_, AppState>,
) -> Result<Persona, String> {
    approve_persona_for_state(input, state.inner(), enabled()).await
}

#[doc(hidden)]
pub async fn approve_persona_for_state(
    input: PersonaIdInput,
    state: &AppState,
    feature_enabled: bool,
) -> Result<Persona, String> {
    let id = persona_id(input.id)?;
    let service = service(state);
    let source_persona_id = service
        .get_draft(feature_enabled, &id)
        .await
        .map_err(to_string)?
        .source_persona_id;
    let persona = service
        .approve_persona(feature_enabled, &id)
        .await
        .map_err(to_string)?;
    if source_persona_id
        .as_ref()
        .is_some_and(|source_id| source_id == &persona.id)
    {
        state.events.emit(
            "persona:draft_applied",
            draft_applied_payload(&id, &persona),
        );
    }
    Ok(persona)
}

#[tauri::command]
pub async fn reseed_persona_draft(
    input: PersonaIdInput,
    state: State<'_, AppState>,
) -> Result<Persona, String> {
    reseed_persona_draft_for_state(input, state.inner(), enabled()).await
}

#[doc(hidden)]
pub async fn reseed_persona_draft_for_state(
    input: PersonaIdInput,
    state: &AppState,
    feature_enabled: bool,
) -> Result<Persona, String> {
    service(state)
        .reseed_persona_draft(feature_enabled, &persona_id(input.id)?)
        .await
        .map_err(to_string)
}

#[tauri::command]
pub async fn approve_persona_as_new(
    input: ApprovePersonaAsNewInput,
    state: State<'_, AppState>,
) -> Result<Persona, String> {
    approve_persona_as_new_for_state(input, state.inner(), enabled()).await
}

#[doc(hidden)]
pub async fn approve_persona_as_new_for_state(
    input: ApprovePersonaAsNewInput,
    state: &AppState,
    feature_enabled: bool,
) -> Result<Persona, String> {
    let id = persona_id(input.id)?;
    service(state)
        .approve_persona_as_new(feature_enabled, &id, input.new_slug.as_deref())
        .await
        .map_err(to_string)
}

#[tauri::command]
pub async fn archive_persona(
    input: PersonaIdInput,
    state: State<'_, AppState>,
) -> Result<Persona, String> {
    archive_persona_for_state(input, state.inner(), enabled()).await
}

#[doc(hidden)]
pub async fn archive_persona_for_state(
    input: PersonaIdInput,
    state: &AppState,
    feature_enabled: bool,
) -> Result<Persona, String> {
    service(state)
        .archive_persona(feature_enabled, &persona_id(input.id)?)
        .await
        .map_err(to_string)
}

#[tauri::command]
pub async fn delete_persona_draft(
    input: PersonaIdInput,
    state: State<'_, AppState>,
) -> Result<(), String> {
    delete_persona_draft_for_state(input, state.inner(), enabled()).await
}

#[doc(hidden)]
pub async fn delete_persona_draft_for_state(
    input: PersonaIdInput,
    state: &AppState,
    feature_enabled: bool,
) -> Result<(), String> {
    service(state)
        .hard_delete_draft(feature_enabled, &persona_id(input.id)?)
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
    agent_personas_enabled()
}

fn persona_id(id: String) -> Result<PersonaId, String> {
    if id.trim().is_empty() {
        Err("persona id cannot be empty".to_string())
    } else {
        Ok(PersonaId::from(id))
    }
}

fn resolve_persona_content(
    slug: &str,
    content: Option<String>,
    description: Option<String>,
    body: Option<String>,
) -> Result<String, String> {
    if let Some(content) = content.filter(|value| !value.trim().is_empty()) {
        return Ok(content);
    }

    let description = description.filter(|value| !value.trim().is_empty());
    let body = body.filter(|value| !value.trim().is_empty());
    match (description, body) {
        (Some(description), Some(body)) => Ok(compose_persona_content(slug, &description, &body)),
        _ => Err("persona content or description+instructions required".to_string()),
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

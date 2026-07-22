use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::json;

use crate::application::personas::{
    builder_draft_updated_payload, PersonaService, SavePersonaDraftInput,
    PERSONA_FEATURE_DISABLED_PREFIX,
};
use crate::domain::entities::{
    ChatContextType, ChatConversation, ChatConversationId, Persona, PersonaId, ProjectId,
};
use crate::error::AppError;
use crate::http_server::handlers::automations::CALLER_SESSION_ID_HEADER;
use crate::http_server::types::{HttpError, HttpServerState};
use crate::infrastructure::agents::agent_personas_enabled;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavePersonaDraftRequest {
    #[serde(default)]
    pub draft_id: Option<String>,
    pub slug: String,
    pub content: String,
    #[serde(default)]
    pub source_session_id: Option<String>,
}

pub async fn save_persona_draft(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<SavePersonaDraftRequest>,
) -> Result<Json<Persona>, HttpError> {
    ensure_enabled()?;
    let service = service(&state);
    let conversation =
        require_persona_builder_caller(&state, &headers, "save_persona_draft").await?;
    if conversation.builder_draft_id.is_none() && conversation.builder_result_persona_id.is_some() {
        return Err(map_app_error(AppError::PersonaAlreadyApproved));
    }

    let persona = match conversation.builder_draft_id.as_deref() {
        Some(bound_id) => {
            if request
                .draft_id
                .as_deref()
                .is_some_and(|requested_id| requested_id != bound_id)
            {
                return Err(HttpError::validation(
                    "PersonaBuilder conversation cannot write outside its bound draft".to_string(),
                ));
            }
            service
                .update_draft_as_agent(true, &PersonaId::from(bound_id), &request.content)
                .await
        }
        None => {
            if request.draft_id.is_some() {
                return Err(HttpError::validation(
                    "PersonaBuilder conversation has no bound draft; omit draft_id to create its bound draft"
                        .to_string(),
                ));
            }
            let project_id = match conversation.context_type {
                ChatContextType::Project => {
                    Some(ProjectId::from_string(conversation.context_id.clone()))
                }
                ChatContextType::Standalone => None,
                _ => {
                    return Err(HttpError::validation(
                        "Persona builder drafts require Project or Standalone conversation context"
                            .to_string(),
                    ))
                }
            };
            service
                .create_bound_draft(
                    true,
                    &conversation.id,
                    SavePersonaDraftInput {
                        project_id,
                        slug: request.slug,
                        content: request.content,
                        source_session_id: Some(conversation.id.as_str()),
                        source_persona_id: None,
                        source_content_hash: None,
                    },
                )
                .await
        }
    }
    .map_err(map_app_error)?;
    let builder_conversation_id = conversation.id.as_str();
    state.app_state.events.emit(
        "persona:draft_updated",
        builder_draft_updated_payload(&persona, &builder_conversation_id),
    );
    Ok(Json(persona))
}

pub async fn get_persona_draft(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Persona>, HttpError> {
    ensure_enabled()?;
    let draft_id = persona_id(id)?;
    let conversation =
        require_persona_builder_caller(&state, &headers, "get_persona_draft").await?;
    if conversation.builder_draft_id.as_deref() != Some(draft_id.as_str()) {
        return Err(HttpError::validation(
            "PersonaBuilder conversation cannot read outside its bound draft".to_string(),
        ));
    }
    service(&state)
        .get_draft(true, &draft_id)
        .await
        .map(Json)
        .map_err(map_app_error)
}

async fn require_persona_builder_caller(
    state: &HttpServerState,
    headers: &HeaderMap,
    operation: &str,
) -> Result<ChatConversation, HttpError> {
    // This caller identity is supplied by the unauthenticated loopback MCP bridge.
    let caller_conversation_id = caller_session_id(headers).ok_or_else(|| {
        HttpError::validation(format!(
            "{operation} requires a valid {CALLER_SESSION_ID_HEADER} caller-session header"
        ))
    })?;
    let conversation = state
        .app_state
        .chat_conversation_repo
        .get_by_id(&ChatConversationId::from_string(caller_conversation_id))
        .await
        .map_err(map_app_error)?
        .ok_or_else(|| {
            HttpError::validation(format!(
                "{operation} caller conversation was not found; start or resume the persona builder conversation and retry"
            ))
        })?;
    if !conversation.is_persona_builder() {
        return Err(HttpError::validation(format!(
            "{operation} caller is not a valid persona builder conversation; use PersonaBuilder mode with Project or Standalone context"
        )));
    }
    Ok(conversation)
}

fn service(state: &HttpServerState) -> PersonaService {
    PersonaService::new(
        state.app_state.db.clone(),
        state.app_state.persona_repo.clone(),
        state.app_state.chat_conversation_repo.clone(),
    )
}

fn ensure_enabled() -> Result<(), HttpError> {
    if agent_personas_enabled() {
        Ok(())
    } else {
        Err(HttpError {
            status: StatusCode::FORBIDDEN,
            message: Some(
                json!({
                    "code": "persona_feature_disabled",
                    "error": format!("{PERSONA_FEATURE_DISABLED_PREFIX} agent personas feature is disabled]"),
                })
                .to_string(),
            ),
        })
    }
}

fn caller_session_id(headers: &HeaderMap) -> Option<String> {
    headers
        .get(CALLER_SESSION_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

fn persona_id(id: String) -> Result<PersonaId, HttpError> {
    if id.trim().is_empty() {
        Err(HttpError::validation(
            "persona id cannot be empty".to_string(),
        ))
    } else {
        Ok(PersonaId::from(id))
    }
}

fn map_app_error(error: AppError) -> HttpError {
    match error {
        AppError::FeatureDisabled(message) => HttpError {
            status: StatusCode::FORBIDDEN,
            message: Some(
                json!({ "code": "persona_feature_disabled", "error": message }).to_string(),
            ),
        },
        AppError::Validation(message) | AppError::PersonaUnavailable(message) => {
            HttpError::validation(message)
        }
        AppError::PersonaAlreadyApproved => {
            HttpError::validation(AppError::PersonaAlreadyApproved.to_string())
        }
        AppError::Conflict(message) => HttpError {
            status: StatusCode::CONFLICT,
            message: Some(message),
        },
        AppError::NotFound(_) => HttpError::from(StatusCode::NOT_FOUND),
        _ => HttpError::from(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

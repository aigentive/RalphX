use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::json;

use crate::application::personas::{
    draft_updated_payload, PersonaService, SavePersonaDraftInput, PERSONA_FEATURE_DISABLED_PREFIX,
};
use crate::domain::entities::{Persona, PersonaId};
use crate::error::AppError;
use crate::http_server::handlers::automations::CALLER_SESSION_ID_HEADER;
use crate::http_server::types::{HttpError, HttpServerState};
use crate::infrastructure::agents::claude::agent_personas_enabled;

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
    let persona = match request.draft_id {
        Some(id) => {
            service
                .update_draft(true, &persona_id(id)?, &request.content)
                .await
        }
        None => {
            service
                .create_draft(
                    true,
                    SavePersonaDraftInput {
                        slug: request.slug,
                        content: request.content,
                        source_session_id: request
                            .source_session_id
                            .or_else(|| caller_session_id(&headers)),
                    },
                )
                .await
        }
    }
    .map_err(map_app_error)?;
    state
        .app_state
        .events
        .emit("persona:draft_updated", draft_updated_payload(&persona));
    Ok(Json(persona))
}

pub async fn get_persona_draft(
    State(state): State<HttpServerState>,
    Path(id): Path<String>,
) -> Result<Json<Persona>, HttpError> {
    ensure_enabled()?;
    service(&state)
        .get_draft(true, &persona_id(id)?)
        .await
        .map(Json)
        .map_err(map_app_error)
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
        AppError::NotFound(_) => HttpError::from(StatusCode::NOT_FOUND),
        _ => HttpError::from(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

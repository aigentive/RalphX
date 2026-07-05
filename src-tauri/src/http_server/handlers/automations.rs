use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use tracing::error;

use crate::commands::automation_commands::{
    automation_service, AutomationDetailResponse, AutomationResponse,
};
use crate::domain::entities::{Automation, AutomationId, ChatConversationId};
use crate::error::AppError;
use crate::http_server::types::{HttpError, HttpServerState};

pub const CALLER_SESSION_ID_HEADER: &str = "x-ralphx-caller-session-id";

#[derive(Debug, Deserialize)]
pub struct UpdateAutomationRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub max_runs: Option<i64>,
    #[serde(default)]
    pub max_consecutive_failures: Option<i64>,
}

pub async fn get_automation(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<AutomationDetailResponse>, HttpError> {
    let automation = resolve_bound_automation(&state, &headers).await?;
    let detail = automation_service(&state.app_state)
        .get_automation_detail(&automation.id)
        .await
        .map_err(map_app_err)?;
    Ok(Json(AutomationDetailResponse::from(detail)))
}

pub async fn update_automation(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<UpdateAutomationRequest>,
) -> Result<Json<AutomationResponse>, HttpError> {
    let automation = resolve_bound_automation(&state, &headers).await?;
    let updated = automation_service(&state.app_state)
        .update_settings(
            crate::application::automation::service::UpdateAutomationSettingsInput {
                id: automation.id,
                name: request.name,
                max_runs: request.max_runs,
                max_consecutive_failures: request.max_consecutive_failures,
            },
        )
        .await
        .map_err(map_app_err)?;
    Ok(Json(AutomationResponse::from(updated)))
}

pub async fn finalize_automation(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<AutomationResponse>, HttpError> {
    let automation = resolve_bound_automation(&state, &headers).await?;
    let finalized = automation_service(&state.app_state)
        .finalize(&automation.id)
        .await
        .map_err(map_app_err)?;
    Ok(Json(AutomationResponse::from(finalized)))
}

async fn resolve_bound_automation(
    state: &HttpServerState,
    headers: &HeaderMap,
) -> Result<Automation, HttpError> {
    let caller_conversation_id = caller_conversation_id(headers)?;
    let conversation = state
        .app_state
        .chat_conversation_repo
        .get_by_id(&caller_conversation_id)
        .await
        .map_err(|error| {
            error!("automation caller conversation lookup failed: {}", error);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or_else(|| {
            automation_forbidden(
                "automation_caller_not_found",
                "Caller conversation is not bound to an automation",
            )
        })?;

    let automation_id = conversation.automation_id.clone().ok_or_else(|| {
        automation_forbidden(
            "automation_caller_unbound",
            "Caller conversation is not bound to an automation",
        )
    })?;

    let automation = automation_service(&state.app_state)
        .get_automation_detail(&automation_id)
        .await
        .map_err(map_app_err)?
        .automation;
    assert_setup_conversation_binding(
        &automation.id,
        automation.setup_conversation_id,
        caller_conversation_id,
    )?;
    Ok(automation)
}

fn caller_conversation_id(headers: &HeaderMap) -> Result<ChatConversationId, HttpError> {
    let Some(value) = headers.get(CALLER_SESSION_ID_HEADER) else {
        return Err(automation_forbidden(
            "automation_caller_missing",
            "Automation tools require injected caller conversation identity",
        ));
    };
    let raw = value.to_str().map_err(|_| {
        automation_forbidden(
            "automation_caller_invalid",
            "Automation caller conversation identity is invalid",
        )
    })?;
    raw.parse::<ChatConversationId>().map_err(|_| {
        automation_forbidden(
            "automation_caller_invalid",
            "Automation caller conversation identity is invalid",
        )
    })
}

fn assert_setup_conversation_binding(
    automation_id: &AutomationId,
    setup_conversation_id: Option<ChatConversationId>,
    caller_conversation_id: ChatConversationId,
) -> Result<(), HttpError> {
    if setup_conversation_id == Some(caller_conversation_id) {
        return Ok(());
    }
    Err(automation_forbidden(
        "automation_conversation_mismatch",
        format!(
            "Caller conversation is not authorized to mutate automation {}",
            automation_id.as_str()
        ),
    ))
}

fn automation_forbidden(code: &str, message: impl Into<String>) -> HttpError {
    HttpError {
        status: StatusCode::FORBIDDEN,
        message: Some(
            json!({
                "code": code,
                "error": message.into(),
            })
            .to_string(),
        ),
    }
}

fn map_app_err(error: AppError) -> HttpError {
    match error {
        AppError::Validation(message) => HttpError::validation(message),
        AppError::NotFound(_) => StatusCode::NOT_FOUND.into(),
        AppError::Conflict(message) => HttpError {
            status: StatusCode::CONFLICT,
            message: Some(message),
        },
        AppError::InvalidTransition { .. } => HttpError {
            status: StatusCode::CONFLICT,
            message: Some(error.to_string()),
        },
        _ => StatusCode::INTERNAL_SERVER_ERROR.into(),
    }
}

#[cfg(test)]
#[path = "automations_tests.rs"]
mod automations_tests;

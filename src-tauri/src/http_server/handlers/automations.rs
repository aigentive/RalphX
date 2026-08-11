use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use tracing::error;

use crate::application::automation::actions::{
    retry_automation_judge_for_state, retry_automation_plan_judge_for_state,
    trigger_automation_run_now_for_state,
};
use crate::application::automation::api::{
    automation_detail_response_for_state, automation_run_response_for_state,
    automation_service_for_state, AutomationDetailResponse, AutomationResponse,
    AutomationRunResponse, AutomationScheduleResponse,
};
use crate::application::automation::decomposition_verifier::{
    AutomationDecompositionVerdict, AutomationDecompositionVerificationOutcome,
    AutomationDecompositionVerifier, HarnessAutomationDecompositionVerifierInvoker,
};
use crate::domain::entities::{
    Automation, AutomationId, AutomationPlanApprovalMode, AutomationPrMergeMode, ChatConversationId,
};
use crate::error::AppError;
use crate::http_server::handlers::agent_workspaces::{
    check_agent_workspace_publish_readiness, get_agent_workspace_publish_status,
    publish_agent_workspace, update_agent_workspace_from_base, AgentWorkspacePublishActionResponse,
    AgentWorkspacePublishReadinessResponse, AgentWorkspacePublishStatusResponse,
    UpdateAgentWorkspaceFromBaseRequest,
};
use crate::http_server::handlers::git::JsonError;
use crate::http_server::types::{HttpError, HttpServerState};
use crate::infrastructure::agents::claude::automations_config;

pub const CALLER_SESSION_ID_HEADER: &str = "x-ralphx-caller-session-id";

#[derive(Debug, Default, Deserialize)]
pub struct UpdateAutomationRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub max_runs: Option<i64>,
    #[serde(default)]
    pub max_consecutive_failures: Option<i64>,
    #[serde(default)]
    pub plan_approval_mode: Option<String>,
    #[serde(default)]
    pub pr_merge_mode: Option<String>,
    #[serde(default)]
    pub plan_deep_verification: Option<bool>,
    #[serde(default)]
    pub goal_prompt: Option<String>,
    #[serde(default)]
    pub first_run_prompt: Option<String>,
    #[serde(default)]
    pub provider_harness: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub logical_effort: Option<String>,
    #[serde(default)]
    pub run_mode: Option<String>,
    #[serde(default)]
    pub base_ref_kind: Option<String>,
    #[serde(default)]
    pub base_ref: Option<String>,
    #[serde(default)]
    pub base_display_name: Option<String>,
    #[serde(default)]
    pub goal_items_json: Option<String>,
    #[serde(default)]
    pub chain_mode: Option<String>,
    #[serde(default)]
    pub completion_signal: Option<String>,
    #[serde(default)]
    pub setup_analysis_summary: Option<String>,
    #[serde(default)]
    pub spec_artifact_id: Option<String>,
    #[serde(default)]
    pub spec_content: Option<String>,
}

impl UpdateAutomationRequest {
    fn has_config_fields(&self) -> bool {
        self.goal_prompt.is_some()
            || self.first_run_prompt.is_some()
            || self.provider_harness.is_some()
            || self.model_id.is_some()
            || self.logical_effort.is_some()
            || self.run_mode.is_some()
            || self.base_ref_kind.is_some()
            || self.base_ref.is_some()
            || self.base_display_name.is_some()
            || self.goal_items_json.is_some()
            || self.chain_mode.is_some()
            || self.completion_signal.is_some()
            || self.setup_analysis_summary.is_some()
            || self.spec_artifact_id.is_some()
            || self.spec_content.is_some()
    }
}

pub async fn get_automation(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<AutomationDetailResponse>, HttpError> {
    let automation = resolve_bound_automation(&state, &headers).await?;
    let detail = automation_service_for_state(&state.app_state)
        .get_automation_detail(&automation.id)
        .await
        .map_err(map_app_err)?;
    let response = automation_detail_response_for_state(detail, &state.app_state)
        .await
        .map_err(map_app_err)?;
    Ok(Json(response))
}

pub async fn update_automation(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(request): Json<UpdateAutomationRequest>,
) -> Result<Json<AutomationResponse>, HttpError> {
    // Ownership resolution keeps the caller-conversation -> automation binding
    // authorization intact before any mutation is applied.
    let automation = resolve_bound_automation(&state, &headers).await?;
    let setup_conversation_id = automation.setup_conversation_id.clone();
    let automation_id = automation.id;
    let apply_config = request.has_config_fields();
    let plan_approval_mode =
        parse_plan_approval_mode(request.plan_approval_mode.as_deref()).map_err(map_app_err)?;
    let pr_merge_mode =
        parse_pr_merge_mode(request.pr_merge_mode.as_deref()).map_err(map_app_err)?;
    let requested_name = request
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    let service = automation_service_for_state(&state.app_state);

    let mut updated = service
        .update_settings(
            crate::application::automation::service::UpdateAutomationSettingsInput {
                id: automation_id.clone(),
                name: request.name,
                max_runs: request.max_runs,
                max_consecutive_failures: request.max_consecutive_failures,
                plan_approval_mode,
                pr_merge_mode,
                plan_deep_verification: request.plan_deep_verification,
            },
        )
        .await
        .map_err(map_app_err)?;

    if apply_config {
        updated = service
            .update_config(
                crate::application::automation::service::UpdateAutomationConfigInput {
                    id: automation_id,
                    goal_prompt: request.goal_prompt,
                    first_run_prompt: request.first_run_prompt,
                    provider_harness: request.provider_harness,
                    model_id: request.model_id,
                    logical_effort: request.logical_effort,
                    run_mode: request.run_mode,
                    base_ref_kind: request.base_ref_kind,
                    base_ref: request.base_ref,
                    base_display_name: request.base_display_name,
                    goal_items_json: request.goal_items_json,
                    chain_mode: request.chain_mode,
                    completion_signal: request.completion_signal,
                    setup_analysis_summary: request.setup_analysis_summary,
                    spec_artifact_id: request.spec_artifact_id,
                    spec_content: request.spec_content,
                },
            )
            .await
            .map_err(map_app_err)?;
    }

    if requested_name.is_some() {
        if let Some(conversation_id) = setup_conversation_id {
            state
                .app_state
                .chat_conversation_repo
                .update_title(&conversation_id, &updated.name)
                .await
                .map_err(map_app_err)?;
        }
    }

    Ok(Json(AutomationResponse::from(updated)))
}

pub async fn finalize_automation(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<AutomationResponse>, HttpError> {
    let automation = resolve_bound_automation(&state, &headers).await?;
    let finalized = automation_service_for_state(&state.app_state)
        .finalize(&automation.id)
        .await
        .map_err(map_app_err)?;
    Ok(Json(AutomationResponse::from(finalized)))
}

pub async fn run_automation_now(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<AutomationScheduleResponse>, HttpError> {
    let automation = resolve_bound_automation(&state, &headers).await?;
    let outcome = trigger_automation_run_now_for_state(&automation.id, &state.app_state)
        .await
        .map_err(map_app_err)?;
    Ok(Json(AutomationScheduleResponse::from(outcome)))
}

pub async fn pause_automation_for_setup_agent(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<AutomationResponse>, HttpError> {
    let automation = resolve_bound_automation(&state, &headers).await?;
    let paused = automation_service_for_state(&state.app_state)
        .pause(&automation.id, "setup_agent_paused", None)
        .await
        .map_err(map_app_err)?;
    Ok(Json(AutomationResponse::from(paused)))
}

pub async fn resume_automation_for_setup_agent(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<AutomationResponse>, HttpError> {
    let automation = resolve_bound_automation(&state, &headers).await?;
    let resumed = automation_service_for_state(&state.app_state)
        .resume(&automation.id)
        .await
        .map_err(map_app_err)?;
    Ok(Json(AutomationResponse::from(resumed)))
}

pub async fn cancel_latest_automation_run(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<AutomationRunResponse>, HttpError> {
    let automation = resolve_bound_automation(&state, &headers).await?;
    let service = automation_service_for_state(&state.app_state);
    let detail = service
        .get_automation_detail(&automation.id)
        .await
        .map_err(map_app_err)?;
    let latest = detail
        .runs
        .last()
        .ok_or_else(|| map_app_err(AppError::Validation("automation has no runs".to_string())))?;
    let cancelled = service
        .cancel_run(&automation.id, &latest.id)
        .await
        .map_err(map_app_err)?;
    let response = automation_run_response_for_state(cancelled, &state.app_state)
        .await
        .map_err(map_app_err)?;
    Ok(Json(response))
}

pub async fn cancel_automation_for_setup_agent(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<AutomationResponse>, HttpError> {
    let automation = resolve_bound_automation(&state, &headers).await?;
    let stopped = automation_service_for_state(&state.app_state)
        .stop(&automation.id)
        .await
        .map_err(map_app_err)?;
    Ok(Json(AutomationResponse::from(stopped)))
}

pub async fn restart_automation_for_setup_agent(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<AutomationScheduleResponse>, HttpError> {
    let automation = resolve_bound_automation(&state, &headers).await?;
    let outcome = automation_service_for_state(&state.app_state)
        .restart(&automation.id)
        .await
        .map_err(map_app_err)?;
    Ok(Json(AutomationScheduleResponse::from(outcome)))
}

pub async fn retry_automation_judge_for_setup_agent(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<AutomationScheduleResponse>, HttpError> {
    let automation = resolve_bound_automation(&state, &headers).await?;
    let outcome = retry_automation_judge_for_state(&automation.id, &state.app_state)
        .await
        .map_err(map_app_err)?;
    Ok(Json(AutomationScheduleResponse::from(outcome)))
}

pub async fn retry_automation_plan_judge_for_setup_agent(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<AutomationScheduleResponse>, HttpError> {
    let automation = resolve_bound_automation(&state, &headers).await?;
    let outcome = retry_automation_plan_judge_for_state(&automation.id, &state.app_state)
        .await
        .map_err(map_app_err)?;
    Ok(Json(AutomationScheduleResponse::from(outcome)))
}

pub async fn skip_latest_automation_judge(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<AutomationScheduleResponse>, HttpError> {
    let automation = resolve_bound_automation(&state, &headers).await?;
    let service = automation_service_for_state(&state.app_state);
    let detail = service
        .get_automation_detail(&automation.id)
        .await
        .map_err(map_app_err)?;
    let latest = detail
        .runs
        .last()
        .ok_or_else(|| map_app_err(AppError::Validation("automation has no runs".to_string())))?;
    let outcome = service
        .skip_judge(&automation.id, &latest.id)
        .await
        .map_err(map_app_err)?;
    Ok(Json(AutomationScheduleResponse::from(outcome)))
}

pub async fn get_automation_publish_status(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<AgentWorkspacePublishStatusResponse>, JsonError> {
    let conversation_id = resolve_bound_publish_conversation_id(&state, &headers).await?;
    get_agent_workspace_publish_status(State(state), Path(conversation_id.to_string())).await
}

pub async fn check_automation_publish_readiness(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<AgentWorkspacePublishReadinessResponse>, JsonError> {
    let conversation_id = resolve_bound_publish_conversation_id(&state, &headers).await?;
    check_agent_workspace_publish_readiness(State(state), Path(conversation_id.to_string())).await
}

pub async fn update_automation_from_base(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<AgentWorkspacePublishActionResponse>, JsonError> {
    let conversation_id = resolve_bound_publish_conversation_id(&state, &headers).await?;
    update_agent_workspace_from_base(
        State(state),
        Path(conversation_id.to_string()),
        Json(UpdateAgentWorkspaceFromBaseRequest {
            base_ref_kind: None,
            base_ref: None,
            base_display_name: None,
            created_by_run_id: None,
        }),
    )
    .await
}

pub async fn publish_automation_workspace(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<AgentWorkspacePublishActionResponse>, JsonError> {
    let conversation_id = resolve_bound_publish_conversation_id(&state, &headers).await?;
    publish_agent_workspace(State(state), Path(conversation_id.to_string())).await
}

#[derive(Debug, serde::Serialize)]
pub struct VerifyAutomationDecompositionResponse {
    pub automation: AutomationResponse,
    pub verdict: AutomationDecompositionVerdict,
}

pub async fn verify_automation_decomposition(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
) -> Result<Json<VerifyAutomationDecompositionResponse>, HttpError> {
    let automation = resolve_bound_automation(&state, &headers).await?;
    let service = automation_service_for_state(&state.app_state);
    let verifier = AutomationDecompositionVerifier::new(
        service,
        Arc::new(HarnessAutomationDecompositionVerifierInvoker::new(
            state.app_state.as_ref().clone(),
        )),
        Duration::from_secs(automations_config().judge_timeout_secs.max(1)),
    );
    let AutomationDecompositionVerificationOutcome {
        automation,
        verdict,
    } = verifier
        .verify_and_finalize(&automation.id)
        .await
        .map_err(map_app_err)?;
    Ok(Json(VerifyAutomationDecompositionResponse {
        automation: AutomationResponse::from(automation),
        verdict,
    }))
}

fn parse_plan_approval_mode(
    value: Option<&str>,
) -> Result<Option<AutomationPlanApprovalMode>, AppError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    AutomationPlanApprovalMode::parse(value)
        .map(Some)
        .ok_or_else(|| AppError::Validation(format!("invalid plan_approval_mode: {value}")))
}

fn parse_pr_merge_mode(value: Option<&str>) -> Result<Option<AutomationPrMergeMode>, AppError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    AutomationPrMergeMode::parse(value)
        .map(Some)
        .ok_or_else(|| AppError::Validation(format!("invalid pr_merge_mode: {value}")))
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

    let automation = automation_service_for_state(&state.app_state)
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

async fn resolve_bound_publish_conversation_id(
    state: &HttpServerState,
    headers: &HeaderMap,
) -> Result<ChatConversationId, JsonError> {
    let automation = resolve_bound_automation(state, headers)
        .await
        .map_err(http_error_as_json)?;
    if let Some(setup_conversation_id) = automation.setup_conversation_id.as_ref() {
        if state
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(setup_conversation_id)
            .await
            .map_err(|error| {
                crate::http_server::handlers::git::json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    error.to_string(),
                    None,
                )
            })?
            .is_some()
        {
            return Ok(setup_conversation_id.clone());
        }
    }

    let detail = automation_service_for_state(&state.app_state)
        .get_automation_detail(&automation.id)
        .await
        .map_err(map_app_err)
        .map_err(http_error_as_json)?;
    for conversation_id in detail
        .runs
        .iter()
        .rev()
        .filter_map(|run| run.conversation_id.as_ref())
    {
        if state
            .app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(conversation_id)
            .await
            .map_err(|error| {
                crate::http_server::handlers::git::json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    error.to_string(),
                    None,
                )
            })?
            .is_some()
        {
            return Ok(conversation_id.clone());
        }
    }
    Err(crate::http_server::handlers::git::json_error(
        StatusCode::NOT_FOUND,
        "Automation has no publishable workspace",
        None,
    ))
}

fn http_error_as_json(error: HttpError) -> JsonError {
    let body = match error.message {
        Some(message) => {
            serde_json::from_str(&message).unwrap_or_else(|_| json!({ "error": message }))
        }
        None => json!({ "error": "Automation workspace request failed" }),
    };
    (error.status, Json(body))
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
use std::sync::Arc;
use std::time::Duration;

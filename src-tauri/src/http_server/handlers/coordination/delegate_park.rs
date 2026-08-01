//! `delegate_park` — durable parent-turn parking for native delegation.
//!
//! A coordinator that spawned delegates registers its outstanding jobs here, receives explicit
//! permission to end its turn, and is resumed later by a hidden `resume_in_place` wake message.
//!
//! Caller identity is transport-owned: the parent conversation and run come from the
//! `x-ralphx-conversation-id` / `x-ralphx-agent-run-id` headers injected by the MCP runtime, never
//! from model-supplied arguments. Missing or non-current run identity fails closed, exactly like
//! `delegate_start`.

use super::*;

use crate::application::delegation_park::ArmParkRequest;
use crate::domain::entities::DelegationWakePolicy;
use crate::http_server::types::{DelegateParkRequest, DelegateParkResponse, ParkedJobSummary};

/// Fail-closed 400 for park calls whose MCP transport carries no run identity. As with
/// `delegate_start`, this signals a spawn-lane injection gap, not a policy denial.
pub const PARK_MISSING_RUN_IDENTITY_ERROR: &str = "delegate_park requires trusted parent agent run context, but this agent's MCP runtime context has no run identity: the spawn lane did not inject --agent-run-id at launch. This is a spawn-lane injection gap (fail-closed by design), not a delegation policy denial for this agent";

pub const PARK_MISSING_CONVERSATION_IDENTITY_ERROR: &str =
    "delegate_park requires trusted parent conversation context from the MCP transport";

fn trimmed_header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn parse_wake_policy(value: Option<&str>) -> Result<DelegationWakePolicy, JsonError> {
    match value {
        None | Some("all") => Ok(DelegationWakePolicy::AllSettled),
        Some("any") => Ok(DelegationWakePolicy::AnySettled),
        Some(other) => Err(json_error(
            StatusCode::BAD_REQUEST,
            format!("Unsupported wake_on value '{other}'; expected \"all\" or \"any\""),
        )),
    }
}

pub async fn park_delegate(
    State(state): State<HttpServerState>,
    headers: HeaderMap,
    Json(req): Json<DelegateParkRequest>,
) -> Result<Json<DelegateParkResponse>, JsonError> {
    let parent_conversation_id = trimmed_header(&headers, "x-ralphx-conversation-id")
        .ok_or_else(|| {
            json_error(
                StatusCode::BAD_REQUEST,
                PARK_MISSING_CONVERSATION_IDENTITY_ERROR,
            )
        })?
        .to_string();
    let parent_agent_run_id = trimmed_header(&headers, "x-ralphx-agent-run-id")
        .ok_or_else(|| json_error(StatusCode::BAD_REQUEST, PARK_MISSING_RUN_IDENTITY_ERROR))?
        .to_string();

    if req.job_ids.is_empty() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "delegate_park requires at least one job_id",
        ));
    }

    let conversation_id = ChatConversationId::from_string(parent_conversation_id.clone());
    let run_id = AgentRunId::from_string(parent_agent_run_id.clone());

    // The claimed run must exist, belong to the claimed conversation, and still be the ACTIVE run.
    // Parking on a stale run would let an old turn register a wake set for a conversation that has
    // already moved on.
    let trusted_run = state
        .app_state
        .agent_run_repo
        .get_by_id(&run_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to validate trusted caller agent run: {error}"),
            )
        })?
        .ok_or_else(|| json_error(StatusCode::NOT_FOUND, "Trusted caller agent run not found"))?;
    if trusted_run.conversation_id != conversation_id {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "Trusted caller agent run does not belong to the caller conversation",
        ));
    }
    let active_run = state
        .app_state
        .agent_run_repo
        .get_active_for_conversation(&conversation_id)
        .await
        .map_err(|error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to resolve current caller agent run: {error}"),
            )
        })?;
    if active_run.as_ref().map(|run| &run.id) != Some(&trusted_run.id) {
        return Err(json_error(
            StatusCode::CONFLICT,
            "Trusted caller agent run is not the active caller run",
        ));
    }

    let wake_policy = parse_wake_policy(req.wake_on.as_deref())?;
    let park = state
        .app_state
        .build_delegation_park_service()
        .arm(
            ArmParkRequest {
                parent_conversation_id: conversation_id,
                parent_agent_run_id: run_id,
                job_ids: req.job_ids.clone(),
                wake_policy,
                wake_on_failure: req.wake_on_failure.unwrap_or(true),
                max_wait_secs: req.max_wait_secs,
            },
            state.delegation_service.as_ref(),
        )
        .await
        .map_err(|error| match error {
            crate::error::AppError::Validation(message) => {
                json_error(StatusCode::BAD_REQUEST, message)
            }
            crate::error::AppError::NotFound(message) => json_error(StatusCode::NOT_FOUND, message),
            other => json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to arm delegation park: {other}"),
            ),
        })?;

    let watched_jobs = park
        .jobs
        .iter()
        .map(|job| ParkedJobSummary {
            job_id: job.job_id.clone(),
            delegated_session_id: job.delegated_session_id.clone(),
        })
        .collect::<Vec<_>>();
    let wake_on = match park.wake_policy {
        DelegationWakePolicy::AllSettled => "all",
        DelegationWakePolicy::AnySettled => "any",
    };
    let failure_guidance = if park.wake_on_failure {
        ", when a delegate fails"
    } else {
        ""
    };
    let guidance = format!(
        "Parked on {} delegate job(s). You may END YOUR TURN now — do not poll. RalphX will resume \
         this conversation automatically when {}{}, or by {} at the latest.",
        watched_jobs.len(),
        if wake_on == "all" {
            "every watched delegate has settled"
        } else {
            "any watched delegate settles"
        },
        failure_guidance,
        park.deadline_at.to_rfc3339(),
    );

    Ok(Json(DelegateParkResponse {
        park_id: park.id.as_str(),
        parked: true,
        wake_on: wake_on.to_string(),
        wake_on_failure: park.wake_on_failure,
        watched_jobs,
        deadline_at: park.deadline_at.to_rfc3339(),
        guidance,
    }))
}

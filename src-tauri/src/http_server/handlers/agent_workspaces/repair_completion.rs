//! Trusted agent-workspace repair completion HTTP handler.

use std::str::FromStr;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};

use super::*;
use crate::application::agent_workspace_publish_repair_state::{
    block_agent_workspace_repair_completion, block_agent_workspace_repair_needs_human,
    classify_agent_workspace_repair_completion_authority,
    continue_agent_workspace_repair_at_boundary, inspect_agent_workspace_repair_completion,
    reacquire_agent_workspace_repair_target_lease_for_continuation,
    record_agent_workspace_repair_validation,
    reopen_agent_workspace_repair_after_validation_failure,
    reserve_agent_workspace_pre_existing_on_base,
    reserve_agent_workspace_repair_completion_validation,
    validate_agent_workspace_repair_target_lease, AgentWorkspaceRepairTransitionOutcome,
    PublishAuthority,
};
use crate::application::publish_resilience::continue_agent_workspace_repair_publish;
use crate::domain::entities::{
    AgentRunId, AgentRunStatus, AgentWorkspacePrAutofixIssueKind, AgentWorkspaceRepairAttempt,
    AgentWorkspaceRepairCompletionAuthority, AgentWorkspaceRepairPhase, AgentWorkspaceRepairSource,
};

#[cfg(feature = "test-utils")]
mod reservation_test_hook {
    use std::sync::{Arc, Mutex, OnceLock};

    use tokio::sync::Barrier;

    static RESERVATION_GATE: OnceLock<Mutex<Option<Arc<Barrier>>>> = OnceLock::new();

    fn reservation_gate() -> &'static Mutex<Option<Arc<Barrier>>> {
        RESERVATION_GATE.get_or_init(|| Mutex::new(None))
    }

    pub(super) fn set(gate: Arc<Barrier>) {
        *reservation_gate()
            .lock()
            .expect("repair completion reservation gate lock") = Some(gate);
    }

    pub(super) fn clear() {
        *reservation_gate()
            .lock()
            .expect("repair completion reservation gate lock") = None;
    }

    pub(super) async fn wait() {
        let gate = reservation_gate()
            .lock()
            .expect("repair completion reservation gate lock")
            .clone();
        if let Some(gate) = gate {
            gate.wait().await;
            gate.wait().await;
        }
    }
}

#[cfg(feature = "test-utils")]
mod completion_phase_test_hook {
    use std::sync::{Arc, Mutex, OnceLock};

    use tokio::sync::Barrier;

    static BLOCKER_GATE: OnceLock<Mutex<Option<Arc<Barrier>>>> = OnceLock::new();
    static SUCCESS_RESERVATION_GATE: OnceLock<Mutex<Option<Arc<Barrier>>>> = OnceLock::new();
    static VALIDATION_GATE: OnceLock<Mutex<Option<Arc<Barrier>>>> = OnceLock::new();
    static CONTINUATION_GATE: OnceLock<Mutex<Option<Arc<Barrier>>>> = OnceLock::new();

    fn blocker_gate() -> &'static Mutex<Option<Arc<Barrier>>> {
        BLOCKER_GATE.get_or_init(|| Mutex::new(None))
    }

    fn continuation_gate() -> &'static Mutex<Option<Arc<Barrier>>> {
        CONTINUATION_GATE.get_or_init(|| Mutex::new(None))
    }

    fn validation_gate() -> &'static Mutex<Option<Arc<Barrier>>> {
        VALIDATION_GATE.get_or_init(|| Mutex::new(None))
    }

    fn success_reservation_gate() -> &'static Mutex<Option<Arc<Barrier>>> {
        SUCCESS_RESERVATION_GATE.get_or_init(|| Mutex::new(None))
    }

    pub(super) fn set_blocker(gate: Arc<Barrier>) {
        *blocker_gate()
            .lock()
            .expect("repair completion blocker gate lock") = Some(gate);
    }

    pub(super) fn clear_blocker() {
        *blocker_gate()
            .lock()
            .expect("repair completion blocker gate lock") = None;
    }

    pub(super) fn set_success_reservation(gate: Arc<Barrier>) {
        *success_reservation_gate()
            .lock()
            .expect("repair completion success reservation gate lock") = Some(gate);
    }

    pub(super) fn clear_success_reservation() {
        *success_reservation_gate()
            .lock()
            .expect("repair completion success reservation gate lock") = None;
    }

    pub(super) fn set_validation(gate: Arc<Barrier>) {
        *validation_gate()
            .lock()
            .expect("repair completion validation gate lock") = Some(gate);
    }

    pub(super) fn clear_validation() {
        *validation_gate()
            .lock()
            .expect("repair completion validation gate lock") = None;
    }

    pub(super) fn set_continuation(gate: Arc<Barrier>) {
        *continuation_gate()
            .lock()
            .expect("repair completion continuation gate lock") = Some(gate);
    }

    pub(super) fn clear_continuation() {
        *continuation_gate()
            .lock()
            .expect("repair completion continuation gate lock") = None;
    }

    async fn wait(gate: &Mutex<Option<Arc<Barrier>>>) {
        let gate = gate
            .lock()
            .expect("repair completion phase gate lock")
            .clone();
        if let Some(gate) = gate {
            gate.wait().await;
            gate.wait().await;
        }
    }

    pub(super) async fn wait_blocker() {
        wait(blocker_gate()).await;
    }

    pub(super) async fn wait_success_reservation() {
        wait(success_reservation_gate()).await;
    }

    pub(super) async fn wait_validation() {
        wait(validation_gate()).await;
    }

    pub(super) async fn wait_continuation() {
        wait(continuation_gate()).await;
    }
}

#[cfg(feature = "test-utils")]
#[doc(hidden)]
pub fn set_agent_workspace_repair_completion_reservation_gate_for_test(
    gate: Arc<tokio::sync::Barrier>,
) {
    reservation_test_hook::set(gate);
}

#[cfg(feature = "test-utils")]
#[doc(hidden)]
pub fn clear_agent_workspace_repair_completion_reservation_gate_for_test() {
    reservation_test_hook::clear();
}

#[cfg(feature = "test-utils")]
#[doc(hidden)]
pub fn set_agent_workspace_repair_completion_blocker_gate_for_test(
    gate: Arc<tokio::sync::Barrier>,
) {
    completion_phase_test_hook::set_blocker(gate);
}

#[cfg(feature = "test-utils")]
#[doc(hidden)]
pub fn clear_agent_workspace_repair_completion_blocker_gate_for_test() {
    completion_phase_test_hook::clear_blocker();
}

#[cfg(feature = "test-utils")]
#[doc(hidden)]
pub fn set_agent_workspace_repair_completion_validation_gate_for_test(
    gate: Arc<tokio::sync::Barrier>,
) {
    completion_phase_test_hook::set_validation(gate);
}

#[cfg(feature = "test-utils")]
#[doc(hidden)]
pub fn clear_agent_workspace_repair_completion_validation_gate_for_test() {
    completion_phase_test_hook::clear_validation();
}

#[cfg(feature = "test-utils")]
#[doc(hidden)]
pub fn set_agent_workspace_repair_completion_success_reservation_gate_for_test(
    gate: Arc<tokio::sync::Barrier>,
) {
    completion_phase_test_hook::set_success_reservation(gate);
}

#[cfg(feature = "test-utils")]
#[doc(hidden)]
pub fn clear_agent_workspace_repair_completion_success_reservation_gate_for_test() {
    completion_phase_test_hook::clear_success_reservation();
}

#[cfg(feature = "test-utils")]
#[doc(hidden)]
pub fn set_agent_workspace_repair_completion_continuation_gate_for_test(
    gate: Arc<tokio::sync::Barrier>,
) {
    completion_phase_test_hook::set_continuation(gate);
}

#[cfg(feature = "test-utils")]
#[doc(hidden)]
pub fn clear_agent_workspace_repair_completion_continuation_gate_for_test() {
    completion_phase_test_hook::clear_continuation();
}

pub(super) fn completion_response(
    status: &str,
    message: impl Into<String>,
) -> Json<CompleteAgentWorkspaceRepairResponse> {
    Json(CompleteAgentWorkspaceRepairResponse {
        success: !matches!(status, "invalid"),
        status: status.to_string(),
        message: message.into(),
        new_status: status.to_string(),
        base_commit: String::new(),
        repair_commit_sha: String::new(),
        auto_publish_status: None,
        auto_publish_error: None,
        pr_number: None,
        pr_url: None,
    })
}

/// Upper bound on `what_happened` / `what_i_did` narrative fields. Chosen to fit a couple of
/// plain-language sentences; over-cap fails closed with a 400 rather than silently truncating.
pub(crate) const MAX_REPAIR_NARRATIVE_CHARS: usize = 480;

/// Shared validator for the two completion narrative fields (`what_happened`, `what_i_did`).
/// Both the trusted `complete-repair` route and the `complete-pr-fix` compatibility route (which
/// forwards into [`complete_agent_workspace_repair_for_trusted_run`]) fold through this one call
/// site, so the two routes cannot drift on trim/empty/cap behavior.
pub(super) fn validate_repair_narrative_field(
    value: Option<String>,
    field_name: &str,
) -> Result<Option<String>, JsonError> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            format!("A repair {field_name} must be non-empty when supplied."),
            None,
        ));
    }
    if trimmed.chars().count() > MAX_REPAIR_NARRATIVE_CHARS {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            format!(
                "A repair {field_name} must be at most {MAX_REPAIR_NARRATIVE_CHARS} characters."
            ),
            None,
        ));
    }
    Ok(Some(trimmed.to_string()))
}

fn trusted_runtime_identity(
    headers: &HeaderMap,
    conversation_id: &ChatConversationId,
) -> Result<AgentRunId, JsonError> {
    let run_id = headers
        .get("x-ralphx-agent-run-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            json_error(
                StatusCode::UNAUTHORIZED,
                "Missing trusted repair-agent run authority",
                None,
            )
        })?;
    let header_conversation_id = headers
        .get("x-ralphx-conversation-id")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            json_error(
                StatusCode::UNAUTHORIZED,
                "Missing trusted repair-agent conversation authority",
                None,
            )
        })?;
    if header_conversation_id != conversation_id.as_str() {
        return Err(json_error(
            StatusCode::UNAUTHORIZED,
            "Repair-agent conversation authority does not match this workspace",
            None,
        ));
    }
    AgentRunId::from_str(run_id).map_err(|_| {
        json_error(
            StatusCode::UNAUTHORIZED,
            "Repair-agent run authority is malformed",
            None,
        )
    })
}

async fn current_authorized_repair_attempt(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    run_id: &AgentRunId,
) -> Result<AgentWorkspaceRepairCompletionAuthority, JsonError> {
    let run = state
        .app_state
        .agent_run_repo
        .get_by_id(run_id)
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    let Some(run) = run else {
        return Ok(AgentWorkspaceRepairCompletionAuthority::Invalid);
    };
    if run.conversation_id != *conversation_id {
        return Ok(AgentWorkspaceRepairCompletionAuthority::Invalid);
    }

    let authority = classify_agent_workspace_repair_completion_authority(
        Arc::clone(&state.app_state.agent_workspace_repair_repo),
        conversation_id,
        run_id,
    )
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?;
    if !matches!(
        authority,
        AgentWorkspaceRepairCompletionAuthority::Current(_)
    ) {
        if matches!(
            authority,
            AgentWorkspaceRepairCompletionAuthority::Superseded
        ) && state
            .app_state
            .agent_workspace_repair_repo
            .get_repair_attempt_for_run(conversation_id, run_id)
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?
            .is_none()
        {
            return Ok(AgentWorkspaceRepairCompletionAuthority::Invalid);
        }
        return Ok(authority);
    }
    if run.status != AgentRunStatus::Running {
        return Ok(AgentWorkspaceRepairCompletionAuthority::Invalid);
    }
    Ok(authority)
}

fn authority_response(
    authority: AgentWorkspaceRepairCompletionAuthority,
) -> Result<Option<Json<CompleteAgentWorkspaceRepairResponse>>, JsonError> {
    match authority {
        AgentWorkspaceRepairCompletionAuthority::Current(_) => Ok(None),
        AgentWorkspaceRepairCompletionAuthority::Superseded => Ok(Some(completion_response(
            "superseded",
            "A newer workspace repair generation is already active.",
        ))),
        AgentWorkspaceRepairCompletionAuthority::AlreadyCompleted => Ok(Some(completion_response(
            "already_completed",
            "This repair completion was already accepted.",
        ))),
        AgentWorkspaceRepairCompletionAuthority::AlreadyBlocked => Ok(Some(completion_response(
            "blocked",
            "This repair generation is blocked. Retry repair from the workspace to start a new repair attempt.",
        ))),
        AgentWorkspaceRepairCompletionAuthority::Invalid => Err(json_error(
            StatusCode::CONFLICT,
            "The repair run is not authorized for the active workspace repair.",
            None,
        )),
    }
}

/// Re-read durable completion authority after an optimistic transition loses its exact snapshot.
/// A same-run handoff may already have recorded a blocker or accepted continuation, while only a
/// different current generation/run is superseded.
pub(super) async fn stale_completion_transition_response(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    run_id: &AgentRunId,
) -> Result<Json<CompleteAgentWorkspaceRepairResponse>, JsonError> {
    let authority = current_authorized_repair_attempt(state, conversation_id, run_id).await?;
    Ok(authority_response(authority)?.unwrap_or_else(|| {
        completion_response(
            "superseded",
            "The repair completion changed before validation; the current workflow remains active.",
        )
    }))
}

/// POST /api/agent-workspaces/{conversation_id}/complete-repair
///
/// The request contains only agent-authored progress text. Conversation/run identity and all Git
/// facts are resolved backend-side, with completion authority reserved before target resolution or
/// any Git probe.
pub async fn complete_agent_workspace_repair(
    State(state): State<HttpServerState>,
    Path(conversation_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<CompleteAgentWorkspaceRepairRequest>,
) -> Result<Json<CompleteAgentWorkspaceRepairResponse>, JsonError> {
    let conversation_id = ChatConversationId::from_string(conversation_id);
    let run_id = trusted_runtime_identity(&headers, &conversation_id)?;
    Box::pin(complete_agent_workspace_repair_for_trusted_run(
        &state,
        conversation_id,
        run_id,
        req,
    ))
    .await
}

/// Completes a durable repair after the transport has authenticated and bound the exact agent run.
/// Compatibility transports may reuse this boundary, but may not settle workspace projections or
/// publish directly.
pub(crate) async fn complete_agent_workspace_repair_for_trusted_run(
    state: &HttpServerState,
    conversation_id: ChatConversationId,
    run_id: AgentRunId,
    req: CompleteAgentWorkspaceRepairRequest,
) -> Result<Json<CompleteAgentWorkspaceRepairResponse>, JsonError> {
    if req.summary.trim().is_empty() {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "A non-empty repair summary is required.",
            None,
        ));
    }
    if req
        .blocker
        .as_deref()
        .is_some_and(|blocker| blocker.trim().is_empty())
    {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "A repair blocker must be non-empty when supplied.",
            None,
        ));
    }
    if matches!(
        req.resolution,
        Some(AgentWorkspacePrFixResolution::TransientCi)
    ) && req.blocker.is_some()
    {
        return Err(json_error(
            StatusCode::BAD_REQUEST,
            "A transient CI rerun request cannot also include a blocker.",
            None,
        ));
    }
    let what_happened = validate_repair_narrative_field(req.what_happened, "what_happened")?;
    let what_i_did = validate_repair_narrative_field(req.what_i_did, "what_i_did")?;

    let authority = current_authorized_repair_attempt(state, &conversation_id, &run_id).await?;
    let (attempt, resurrecting) = match authority {
        AgentWorkspaceRepairCompletionAuthority::Current(attempt) => (*attempt, false),
        AgentWorkspaceRepairCompletionAuthority::AlreadyBlocked if req.blocker.is_none() => {
            let attempt = state
                .app_state
                .agent_workspace_repair_repo
                .get_repair_attempt_for_run(&conversation_id, &run_id)
                .await
                .map_err(|error| {
                    json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
                })?
                .ok_or_else(|| {
                    json_error(
                        StatusCode::CONFLICT,
                        "The repair run is no longer authorized for the active workspace repair.",
                        None,
                    )
                })?;
            // Resurrection applies only when the generation blocked before any validated
            // completion. A recorded repair head means the completion was already accepted and
            // the blocker belongs to the downstream continuation; duplicates stay idempotent.
            if attempt.repair_head_commit.is_some() {
                return Ok(completion_response(
                    "blocked",
                    "This repair generation is blocked. Retry repair from the workspace to start a new repair attempt.",
                ));
            }
            (attempt, true)
        }
        authority => {
            return Ok(authority_response(authority)?.expect("non-current authority responds"));
        }
    };
    let workspace = load_agent_workspace_entity(state.app_state.as_ref(), &conversation_id).await?;

    if matches!(
        req.resolution,
        Some(AgentWorkspacePrFixResolution::TransientCi)
    ) {
        return repair_completion_ci_rerun::request_transient_ci_rerun(
            state,
            &conversation_id,
            &run_id,
            attempt,
            &workspace,
            req.summary.trim(),
            what_happened.as_deref(),
            what_i_did.as_deref(),
        )
        .await;
    }

    if matches!(
        req.resolution,
        Some(AgentWorkspacePrFixResolution::NeedsHuman)
    ) {
        return match block_agent_workspace_repair_needs_human(
            Arc::clone(&state.app_state.agent_workspace_repair_repo),
            Arc::clone(&state.app_state.branch_update_repo),
            attempt,
            req.summary.trim(),
            workspace.pr_auto_merge_current,
            what_happened.as_deref(),
            what_i_did.as_deref(),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        {
            AgentWorkspaceRepairTransitionOutcome::Applied(_) => Ok(completion_response(
                "blocked",
                "RalphX recorded the PR fix as requiring human intervention.",
            )),
            AgentWorkspaceRepairTransitionOutcome::Stale(_)
            | AgentWorkspaceRepairTransitionOutcome::Missing => {
                stale_completion_transition_response(state, &conversation_id, &run_id).await
            }
        };
    }

    if matches!(
        req.resolution,
        Some(AgentWorkspacePrFixResolution::PreExistingOnBase)
    ) {
        if attempt.source != AgentWorkspaceRepairSource::PrAutofix {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "pre_existing_on_base is only valid for PR autofix repair attempts.",
                None,
            ));
        }
        // Fail closed when the backend's own durable facts contradict the claim. This hold can
        // never self-release once entered (its fingerprint cannot change while the repaired head
        // is unpublished), so a wrong classification here is a permanent park. Ordered
        // cheap-durable-first; none of these branches transition the attempt, so the run stays
        // authorized to re-complete honestly.
        if attempt.base_update_head_commit.is_some() {
            return Err(json_error(
                StatusCode::CONFLICT,
                "A base update in this repair attempt already produced a new branch head. Verify the worktree, then report resolution 'fixed' with the current HEAD — the base-update merge commit is a real fix.",
                None,
            ));
        }
        if attempt.pr_autofix_issue_kind == Some(AgentWorkspacePrAutofixIssueKind::Mergeability) {
            return Err(json_error(
                StatusCode::BAD_REQUEST,
                "pre_existing_on_base is not valid for mergeability blockers (behind/conflicting cannot be pre-existing on the base). Run update_agent_workspace_from_base and report 'fixed', or report 'needs_human'.",
                None,
            ));
        }
        if let Some(dispatch_head) = attempt.pr_autofix_dispatch_head_commit.as_deref() {
            // Best effort: on an inspection error the hold is the safe state, and durable
            // base-update evidence still guarantees an eventual redrive.
            match Box::pin(inspect_agent_workspace_repair_completion(
                state.app_state.as_ref(),
                &workspace,
                &attempt.target_base_ref,
                attempt.target_base_commit.as_deref(),
            ))
            .await
            {
                Ok(validation) if validation.repair_head_commit != dispatch_head => {
                    return Err(json_error(
                        StatusCode::CONFLICT,
                        "The branch head moved since this repair was dispatched, so the outcome is not 'pre-existing on base'. Report resolution 'fixed' with the current HEAD.",
                        None,
                    ));
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        conversation_id = conversation_id.as_str(),
                        %error,
                        "Could not verify the branch head before accepting a pre_existing_on_base completion"
                    );
                }
            }
        }
        return match reserve_agent_workspace_pre_existing_on_base(
            Arc::clone(&state.app_state.agent_workspace_repair_repo),
            attempt,
            req.summary.trim(),
            workspace.pr_auto_merge_current,
            what_happened.as_deref(),
            what_i_did.as_deref(),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        {
            AgentWorkspaceRepairTransitionOutcome::Applied(_) => Ok(completion_response(
                "accepted",
                "RalphX recorded that the failure is pre-existing on the base and will wait for changed PR health.",
            )),
            AgentWorkspaceRepairTransitionOutcome::Stale(_)
            | AgentWorkspaceRepairTransitionOutcome::Missing => {
                stale_completion_transition_response(state, &conversation_id, &run_id).await
            }
        };
    }

    if let Some(blocker) = req.blocker.as_deref() {
        #[cfg(feature = "test-utils")]
        completion_phase_test_hook::wait_blocker().await;
        return match block_agent_workspace_repair_completion(
            Arc::clone(&state.app_state.agent_workspace_repair_repo),
            Arc::clone(&state.app_state.branch_update_repo),
            attempt,
            req.summary.trim(),
            blocker.trim(),
            workspace.pr_auto_merge_current,
            what_happened.as_deref(),
            what_i_did.as_deref(),
        )
        .await
        .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
        {
            AgentWorkspaceRepairTransitionOutcome::Applied(_) => Ok(completion_response(
                "blocked",
                "The workspace repair is blocked with the supplied recovery guidance.",
            )),
            AgentWorkspaceRepairTransitionOutcome::Stale(_)
            | AgentWorkspaceRepairTransitionOutcome::Missing => {
                stale_completion_transition_response(state, &conversation_id, &run_id).await
            }
        };
    }

    let attempt = if resurrecting {
        let attempt_id = attempt.id.clone();
        match reacquire_agent_workspace_repair_target_lease_for_continuation(
            state.app_state.as_ref(),
            &workspace,
            attempt,
            AgentWorkspaceRepairPhase::Blocked,
        )
        .await
        {
            Ok(AgentWorkspaceRepairTransitionOutcome::Applied(attempt)) => attempt,
            Ok(
                AgentWorkspaceRepairTransitionOutcome::Stale(_)
                | AgentWorkspaceRepairTransitionOutcome::Missing,
            ) => {
                return stale_completion_transition_response(state, &conversation_id, &run_id)
                    .await;
            }
            Err(error) => {
                tracing::warn!(
                    target: "ralphx_lib::http::agent_workspace_repair",
                    conversation_id = %conversation_id,
                    attempt_id = %attempt_id,
                    error = %error,
                    "Blocked repair completion could not reacquire canonical Git target authority"
                );
                return Ok(completion_response(
                    "blocked",
                    "The repair stayed blocked because RalphX could not reacquire its canonical Git target. Retry repair from the workspace.",
                ));
            }
        }
    } else {
        attempt
    };

    #[cfg(feature = "test-utils")]
    reservation_test_hook::wait().await;
    #[cfg(feature = "test-utils")]
    completion_phase_test_hook::wait_success_reservation().await;
    let reserved = match reserve_agent_workspace_repair_completion_validation(
        Arc::clone(&state.app_state.agent_workspace_repair_repo),
        attempt,
        workspace.pr_auto_merge_current,
    )
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => attempt,
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => {
            return stale_completion_transition_response(state, &conversation_id, &run_id).await;
        }
    };
    Box::pin(complete_reserved_agent_workspace_repair(
        state,
        &conversation_id,
        &run_id,
        &workspace,
        reserved,
        req.summary.trim(),
        req.reported_fix_commit_sha.as_deref(),
        resurrecting,
        what_happened.as_deref(),
        what_i_did.as_deref(),
    ))
    .await
}

#[allow(clippy::too_many_arguments)]
async fn complete_reserved_agent_workspace_repair(
    state: &HttpServerState,
    conversation_id: &ChatConversationId,
    run_id: &AgentRunId,
    workspace: &AgentConversationWorkspace,
    reserved: AgentWorkspaceRepairAttempt,
    summary: &str,
    reported_fix_commit_sha: Option<&str>,
    reblock_validation_failure: bool,
    what_happened: Option<&str>,
    what_i_did: Option<&str>,
) -> Result<Json<CompleteAgentWorkspaceRepairResponse>, JsonError> {
    if let Err(error) = validate_agent_workspace_repair_target_lease(
        state.app_state.branch_update_repo.as_ref(),
        &reserved,
    )
    .await
    {
        tracing::warn!(
            target: "ralphx_lib::http::agent_workspace_repair",
            conversation_id = %conversation_id,
            attempt_id = %reserved.id,
            error = %error,
            "Repair completion lost its canonical Git target lease before validation"
        );
        return match block_agent_workspace_repair_completion(
            Arc::clone(&state.app_state.agent_workspace_repair_repo),
            Arc::clone(&state.app_state.branch_update_repo),
            reserved,
            "Workspace repair completion lost canonical Git target authority.",
            "The repair generation no longer owns its canonical Git target lease. Start a new repair attempt before retrying.",
            workspace.pr_auto_merge_current,
            what_happened,
            what_i_did,
        )
        .await
        .map_err(|block_error| {
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                block_error.to_string(),
                None,
            )
        })? {
            AgentWorkspaceRepairTransitionOutcome::Applied(_) => Err(json_error(
                StatusCode::CONFLICT,
                "The repair completion no longer owns the canonical Git target.",
                None,
            )),
            AgentWorkspaceRepairTransitionOutcome::Stale(_)
            | AgentWorkspaceRepairTransitionOutcome::Missing => {
                stale_completion_transition_response(state, conversation_id, run_id).await
            }
        };
    }
    #[cfg(feature = "test-utils")]
    completion_phase_test_hook::wait_validation().await;
    let validation = match Box::pin(inspect_agent_workspace_repair_completion(
        state.app_state.as_ref(),
        workspace,
        &reserved.target_base_ref,
        reserved.target_base_commit.as_deref(),
    ))
    .await
    {
        Ok(validation) => validation,
        Err(error) => {
            let status = match &error {
                AppError::NotFound(_) => StatusCode::NOT_FOUND,
                AppError::Validation(_) => StatusCode::BAD_REQUEST,
                AppError::Conflict(_) => StatusCode::CONFLICT,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            let validation_error = json_error(status, error.to_string(), None);
            let outcome = if reblock_validation_failure {
                block_agent_workspace_repair_completion(
                    Arc::clone(&state.app_state.agent_workspace_repair_repo),
                    Arc::clone(&state.app_state.branch_update_repo),
                    reserved,
                    "Workspace repair completion could not prove a clean repair.",
                    &error.to_string(),
                    workspace.pr_auto_merge_current,
                    what_happened,
                    what_i_did,
                )
                .await
            } else {
                reopen_agent_workspace_repair_after_validation_failure(
                    Arc::clone(&state.app_state.agent_workspace_repair_repo),
                    reserved,
                    workspace.pr_auto_merge_current,
                )
                .await
            };
            return match outcome.map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })? {
                AgentWorkspaceRepairTransitionOutcome::Applied(_) if reblock_validation_failure => {
                    Ok(completion_response(
                        "blocked",
                        "The repair stayed blocked because RalphX could not verify a clean repair. Retry repair from the workspace.",
                    ))
                }
                AgentWorkspaceRepairTransitionOutcome::Applied(_) => Err(validation_error),
                AgentWorkspaceRepairTransitionOutcome::Stale(_)
                | AgentWorkspaceRepairTransitionOutcome::Missing => {
                    stale_completion_transition_response(state, conversation_id, run_id).await
                }
            };
        }
    };

    if reserved.source == AgentWorkspaceRepairSource::PrAutofix {
        let has_new_commit = reported_fix_commit_sha
            .is_some_and(|reported| reported == validation.repair_head_commit)
            && reserved
                .pr_autofix_dispatch_head_commit
                .as_deref()
                .is_some_and(|dispatch_head| validation.repair_head_commit != dispatch_head);
        if !has_new_commit {
            let error = "PR autofix completion requires a new committed branch head or resolution 'transient_ci', 'pre_existing_on_base', or 'needs_human'. Push a real fix or classify the outcome.";
            let outcome = reopen_agent_workspace_repair_after_validation_failure(
                Arc::clone(&state.app_state.agent_workspace_repair_repo),
                reserved,
                workspace.pr_auto_merge_current,
            )
            .await
            .map_err(|error| {
                json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None)
            })?;
            return match outcome {
                AgentWorkspaceRepairTransitionOutcome::Applied(_) => {
                    Err(json_error(StatusCode::CONFLICT, error, None))
                }
                AgentWorkspaceRepairTransitionOutcome::Stale(_)
                | AgentWorkspaceRepairTransitionOutcome::Missing => {
                    stale_completion_transition_response(state, conversation_id, run_id).await
                }
            };
        }
    }

    let validated = match record_agent_workspace_repair_validation(
        Arc::clone(&state.app_state.agent_workspace_repair_repo),
        reserved,
        &validation.base_ref,
        &validation.base_commit,
        &validation.repair_head_commit,
        summary,
        validation.auto_merge_current,
        what_happened,
        what_i_did,
    )
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(validated) => validated,
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => {
            return stale_completion_transition_response(state, conversation_id, run_id).await;
        }
    };
    #[cfg(feature = "test-utils")]
    completion_phase_test_hook::wait_continuation().await;
    let continuation = match Box::pin(continue_agent_workspace_repair_at_boundary(
        state.app_state.as_ref(),
        validated,
        AgentWorkspaceRepairPhase::Validating,
        summary,
        false,
        PublishAuthority::VerifiedAutomation,
    ))
    .await
    .map_err(|error| json_error(StatusCode::INTERNAL_SERVER_ERROR, error.to_string(), None))?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(attempt) => attempt,
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => {
            return stale_completion_transition_response(state, conversation_id, run_id).await;
        }
    };
    if continuation.phase == AgentWorkspaceRepairPhase::Blocked {
        return Ok(completion_response(
            "blocked",
            "Workspace Review could not start; the durable repair continuation is blocked.",
        ));
    }
    if let Err(error) = Box::pin(continue_agent_workspace_repair_publish(
        state.app_state.as_ref(),
        continuation,
    ))
    .await
    {
        tracing::warn!(
            target: "ralphx_lib::http::agent_workspace_repair",
            conversation_id = %conversation_id,
            error = %error,
            "Repair continuation publication is durably pending after completion"
        );
    }
    Ok(completion_response(
        "accepted",
        "Workspace repair verified; RalphX will continue the stored workflow.",
    ))
}

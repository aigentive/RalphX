//! Workspace-review gate coupling for automation runs.
//!
//! When an automation-owned workspace review resolves to a blocking/failed gate the automation
//! must be paused (user-actionable) rather than allowed to publish or hang on the run wall-clock.
//! This module owns that handoff so all three block sites share one implementation.

use crate::application::automation::api::automation_service_for_state;
use crate::application::AppState;
use crate::domain::entities::{AutomationRun, AutomationRunStatus, ChatConversationId};
use crate::error::AppResult;

/// Pause reason code + terminal run error code used when a workspace review blocks an automation.
///
/// A run terminalized with this code is a user-actionable gate block, NOT an agent failure, so the
/// automation failure counter must exclude it (see `run_is_workspace_review_blocked`).
pub const WORKSPACE_REVIEW_BLOCKED_REASON_CODE: &str = "workspace_review_blocked";

/// True when a run was terminalized because its workspace review blocked the automation.
///
/// The automation `max_consecutive_failures` counter uses this to skip review-gate blocks: they are
/// user-actionable and must not count as agent failures.
pub fn run_is_workspace_review_blocked(run: &AutomationRun) -> bool {
    run.status == AutomationRunStatus::AgentFailed
        && run.error_code.as_deref() == Some(WORKSPACE_REVIEW_BLOCKED_REASON_CODE)
}

/// Pause the automation that owns `conversation_id` because its workspace review is blocked/failed.
///
/// No-ops (returns `Ok(false)`) for non-automation conversations. For an automation-owned run it:
/// 1. transitions the automation to `Paused` with reason `workspace_review_blocked` (validated
///    transition via `AutomationService::pause`), and
/// 2. terminalizes the still-running/provisioning run as `AgentFailed` with the same error code so
///    the run wall-clock cannot false-timeout on resume.
///
/// Failures of either sub-step are logged but do not propagate: this is a best-effort handoff wired
/// into review-completion paths that must not fail the review write itself. Classification is done
/// by the caller using the `AgentWorkspaceReviewGateStatus` enum, never the blocker string.
///
/// # Errors
///
/// Returns an error only when the initial repository lookup for the owning run fails.
pub async fn pause_automation_for_blocked_workspace_review(
    state: &AppState,
    conversation_id: &ChatConversationId,
    review_detail: Option<&str>,
) -> AppResult<bool> {
    let Some(run) = state
        .automation_run_repo
        .find_run_by_conversation_id(conversation_id)
        .await?
    else {
        return Ok(false);
    };

    let automation_id = run.automation_id.clone();
    let service = automation_service_for_state(state);
    if let Err(error) = service
        .pause(
            &automation_id,
            WORKSPACE_REVIEW_BLOCKED_REASON_CODE,
            review_detail,
        )
        .await
    {
        // A conflict here means the automation already left Active (for example already paused by a
        // prior block site). Treat as a soft no-op; do not fail the review write.
        tracing::warn!(
            target: "ralphx_lib::automation::review_gate",
            automation_id = %automation_id,
            conversation_id = %conversation_id,
            error = %error,
            "Could not pause automation for blocked workspace review (already non-active?)"
        );
    }

    // Terminalize the stuck run so its wall-clock cannot false-timeout on resume. The service
    // handles stale/already-terminal runs as soft no-ops and still runs goal-item close sync.
    if let Err(error) = service
        .terminalize_blocked_run(
            &automation_id,
            &run,
            WORKSPACE_REVIEW_BLOCKED_REASON_CODE,
            review_detail.map(str::to_string),
        )
        .await
    {
        tracing::warn!(
            target: "ralphx_lib::automation::review_gate",
            run_id = %run.id,
            conversation_id = %conversation_id,
            error = %error,
            "Failed to terminalize workspace-review-blocked automation run"
        );
    }

    Ok(true)
}

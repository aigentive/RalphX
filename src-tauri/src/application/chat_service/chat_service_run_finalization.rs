use std::sync::Arc;

use crate::domain::entities::{AgentRunId, AgentRunStatus};
use crate::domain::repositories::AgentRunRepository;

use super::chat_service_queue::QueueProcessingOutcome;

/// Fallback error text for a persisted failed run without a stored message.
pub(super) const UNSPECIFIED_RUN_FAILURE: &str = "Agent run failed without a recorded reason";

pub(super) async fn finalize_run_completed_by_id(
    repo: &Arc<dyn AgentRunRepository>,
    run_id: &str,
) -> bool {
    finalize_run_completed(repo, &AgentRunId::from_string(run_id)).await
}

pub(super) async fn finalize_run_completed(
    repo: &Arc<dyn AgentRunRepository>,
    run_id: &AgentRunId,
) -> bool {
    match repo.complete_if_running(run_id).await {
        Ok(true) => return true,
        Ok(false) => {}
        Err(error) => {
            tracing::error!(
                %error,
                run_id = %run_id,
                "Guarded run completion failed"
            );
            return false;
        }
    }

    match repo.complete_if_prune_cancelled(run_id).await {
        Ok(true) => {
            tracing::warn!(
                run_id = %run_id,
                "Repaired prune-cancelled agent run to completed"
            );
            true
        }
        Ok(false) => {
            tracing::warn!(
                run_id = %run_id,
                "Completion lost authority — run already terminal"
            );
            false
        }
        Err(error) => {
            tracing::error!(
                %error,
                run_id = %run_id,
                "Prune-cancelled run completion repair failed"
            );
            false
        }
    }
}

pub(super) async fn run_completed_event_is_authorized(
    repo: &Arc<dyn AgentRunRepository>,
    run_id: &AgentRunId,
) -> bool {
    match repo.get_by_id(run_id).await {
        Ok(Some(run)) => run.status == AgentRunStatus::Completed,
        Ok(None) => {
            tracing::warn!(
                run_id = %run_id,
                "Suppressing run_completed event because the terminal run is missing"
            );
            false
        }
        Err(error) => {
            tracing::error!(
                %error,
                run_id = %run_id,
                "Suppressing run_completed event because terminal authority could not be read"
            );
            false
        }
    }
}

/// Reason to surface as `agent:error` when the terminal run is persisted as failed.
///
/// Returns `None` for every non-`Failed` status, a missing run, and read errors:
/// user cancels are already covered by `agent:stopped`, and an unrepaired
/// prune cancel is the pre-existing crash case rather than a failed run.
pub(super) async fn terminal_failure_reason(
    repo: &Arc<dyn AgentRunRepository>,
    run_id: &AgentRunId,
) -> Option<String> {
    match repo.get_by_id(run_id).await {
        Ok(Some(run)) if run.status == AgentRunStatus::Failed => Some(
            run.error_message
                .unwrap_or_else(|| UNSPECIFIED_RUN_FAILURE.to_string()),
        ),
        Ok(_) => None,
        Err(error) => {
            tracing::error!(
                %error,
                run_id = %run_id,
                "Could not read terminal run status for failure event"
            );
            None
        }
    }
}

/// Structural gate for the no-queue `run_completed` emission.
///
/// `completion_authorized` MUST come from persisted-status authority
/// (`run_completed_event_is_authorized`), not from whether this call happened to
/// apply the completion write.
pub(super) fn run_completed_without_queue_is_authorized(
    completion_authorized: bool,
    skip_post_loop_finalization: bool,
    silent_interactive_exit: bool,
) -> bool {
    completion_authorized && (!skip_post_loop_finalization || silent_interactive_exit)
}

pub(super) async fn queue_run_completed_event_authority(
    repo: &Arc<dyn AgentRunRepository>,
    queue_outcome: &QueueProcessingOutcome,
    parent_run_id: &str,
) -> (String, bool) {
    let terminal_run_id = queue_outcome.terminal_run_id(parent_run_id);
    let authorized =
        run_completed_event_is_authorized(repo, &AgentRunId::from_string(&terminal_run_id)).await;
    (terminal_run_id, authorized)
}

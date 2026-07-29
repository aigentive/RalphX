use std::sync::Arc;

use crate::domain::entities::{AgentRunId, AgentRunStatus};
use crate::domain::repositories::AgentRunRepository;

use super::chat_service_queue::QueueProcessingOutcome;

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

pub(super) fn run_completed_without_queue_is_authorized(
    completion_applied: bool,
    skip_post_loop_finalization: bool,
    silent_interactive_exit: bool,
) -> bool {
    completion_applied && (!skip_post_loop_finalization || silent_interactive_exit)
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

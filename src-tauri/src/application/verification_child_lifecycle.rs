//! Stopping and archiving ideation child sessions.
//!
//! Both the HTTP verification handlers and the command layer need this, and
//! `session_reopen_service` needs it from inside `application`, so it lives
//! here rather than under `http_server/handlers`.

use tracing::error;

use crate::application::{AgentRunCompletedPayload, AppState, InteractiveProcessKey};
use crate::domain::entities::{IdeationSessionId, IdeationSessionStatus};
use crate::domain::services::RunningAgentKey;
use crate::error::AppError;

/// Selects which child sessions to act on.
pub(crate) enum ChildFilter {
    /// All child sessions regardless of purpose.
    AllChildren,
    /// Only verification-purpose child sessions.
    VerificationOnly,
}

/// Stop running child agents and optionally archive the child sessions.
///
/// For each matching child, stops any running agent (emitting `agent:stopped` +
/// `agent:run_completed` events for UI consistency). When `archive_after_stop` is
/// true, also archives the child row via `update_status(Archived)` so it no longer
/// consumes ideation capacity or appears as an orphan.
///
/// Best-effort: errors during stop or archive are logged but do not abort the loop.
pub(crate) async fn stop_and_archive_children(
    session_id: &str,
    app_state: &AppState,
    filter: ChildFilter,
    archive_after_stop: bool,
) -> Result<(), AppError> {
    let session_id_typed = IdeationSessionId::from_string(session_id.to_string());
    let children = match filter {
        ChildFilter::VerificationOnly => {
            app_state
                .ideation_session_repo
                .get_verification_children(&session_id_typed)
                .await?
        }
        ChildFilter::AllChildren => {
            app_state
                .ideation_session_repo
                .get_children(&session_id_typed)
                .await?
        }
    };

    for child in &children {
        let key = RunningAgentKey::new("ideation", child.id.as_str());
        if app_state.running_agent_registry.is_running(&key).await {
            if let Ok(Some(info)) = app_state.running_agent_registry.stop(&key).await {
                // Remove from interactive process registry (closes stdin pipe)
                let ipr_key = InteractiveProcessKey::new("ideation", child.id.as_str());
                app_state
                    .interactive_process_registry
                    .remove(&ipr_key)
                    .await;

                // Mark agent run as failed
                let run_id = crate::domain::entities::AgentRunId::from_string(&info.agent_run_id);
                app_state
                    .agent_run_repo
                    .fail(&run_id, "Verification cancelled")
                    .await
                    .ok();

                app_state.events.emit(
                    "agent:stopped",
                    serde_json::json!({
                        "conversation_id": info.conversation_id.clone(),
                        "agent_run_id": info.agent_run_id,
                        "context_type": "ideation",
                        "context_id": child.id.as_str(),
                    }),
                );
                let completed_payload = AgentRunCompletedPayload::with_provider_session(
                    info.conversation_id.clone(),
                    "ideation".to_string(),
                    child.id.as_str().to_string(),
                    None,
                    None,
                    None,
                );
                if let Err(error) = ralphx_events::emit_serialized(
                    app_state.events.as_ref(),
                    "agent:run_completed",
                    &completed_payload,
                ) {
                    tracing::warn!(
                        event = "agent:run_completed",
                        %error,
                        "Failed to serialize agent run completion event payload"
                    );
                }
            }
        }

        if archive_after_stop {
            if let Err(e) = app_state
                .ideation_session_repo
                .update_status(&child.id, IdeationSessionStatus::Archived)
                .await
            {
                error!(
                    "Failed to archive child session {} after stop: {}",
                    child.id.as_str(),
                    e
                );
            }
        }
    }
    Ok(())
}

/// Stop any running verification child agents for a session.
///
/// Called when verification is skipped or reverted to immediately release the write lock
/// so the parent session can resume plan editing. Best-effort: errors are swallowed so the
/// caller's skip/revert succeeds even if the agent is already dead.
pub(crate) async fn stop_verification_children(
    session_id: &str,
    app_state: &AppState,
) -> Result<(), AppError> {
    stop_and_archive_children(session_id, app_state, ChildFilter::VerificationOnly, true).await
}

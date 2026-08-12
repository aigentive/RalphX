// Tauri IPC wrappers for ideation proposal application.
// Core apply logic lives in application/ideation_apply_service.rs.

use std::sync::Arc;
use tauri::{Manager, State};

use crate::application::{
    session_namer_agent::{spawn_session_namer_agent, SessionNamerTarget},
    spawn_ready_task_scheduler_if_needed, AppState, TaskCleanupService,
};
use crate::application::ideation_apply_service::{
    apply_proposals_core, apply_supervised_proposals_core, ApplyProposalsInput,
};
use crate::application::verification_child_lifecycle::stop_verification_children;
use crate::commands::ExecutionState;
use crate::domain::entities::{ProjectId, TaskId};

use super::ideation_commands_types::ApplyProposalsResultResponse;

// ============================================================================
// Apply and Task Dependency Commands
// ============================================================================

/// Apply selected proposals to the Kanban board as tasks (Tauri IPC command).
///
/// Delegates to [`apply_proposals_core`] and adds Tauri-specific side effects:
/// queue-change events, task scheduler trigger for newly Ready tasks, and
/// ralphx-utility-session-namer re-trigger at acceptance.
/// External HTTP callers use [`crate::http_server::handlers::external_apply_proposals`]
/// instead, which skips the scheduler (external agents poll `get_pipeline_overview`).
#[tauri::command]
pub async fn apply_proposals_to_kanban(
    input: ApplyProposalsInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<crate::commands::ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<ApplyProposalsResultResponse, String> {
    apply_proposals_to_kanban_for_state(input, &state, &execution_state, &app).await
}

#[doc(hidden)]
pub async fn apply_proposals_to_kanban_for_state(
    input: ApplyProposalsInput,
    state: &State<'_, AppState>,
    execution_state: &State<'_, Arc<crate::commands::ExecutionState>>,
    app: &tauri::AppHandle,
) -> Result<ApplyProposalsResultResponse, String> {
    apply_proposals_to_kanban_for_state_inner(input, state, execution_state, app, None).await
}

pub(crate) async fn apply_supervised_proposals_to_kanban_for_state(
    input: ApplyProposalsInput,
    conversation_id: String,
    state: &State<'_, AppState>,
    execution_state: &State<'_, Arc<crate::commands::ExecutionState>>,
    app: &tauri::AppHandle,
) -> Result<ApplyProposalsResultResponse, String> {
    apply_proposals_to_kanban_for_state_inner(
        input,
        state,
        execution_state,
        app,
        Some(conversation_id),
    )
    .await
}

async fn apply_proposals_to_kanban_for_state_inner(
    input: ApplyProposalsInput,
    state: &State<'_, AppState>,
    execution_state: &State<'_, Arc<crate::commands::ExecutionState>>,
    app: &tauri::AppHandle,
    supervised_task_pipeline_conversation_id: Option<String>,
) -> Result<ApplyProposalsResultResponse, String> {
    use crate::commands::emit_queue_changed;

    let result = match supervised_task_pipeline_conversation_id {
        Some(conversation_id) => {
            apply_supervised_proposals_core(
                state.inner(),
                execution_state.inner(),
                input,
                conversation_id,
            )
            .await
        }
        None => apply_proposals_core(state.inner(), execution_state.inner(), input).await,
    }
    .map_err(|e| e.to_string())?;

    // IPR cleanup: stop the ideation session's interactive Claude CLI process
    // now that the session has been accepted (terminal state).
    // Best-effort: if no process is found, GC will eventually clean up.
    if result.session_converted {
        let task_cleanup = TaskCleanupService::new(
            Arc::clone(&state.task_repo),
            Arc::clone(&state.project_repo),
            Arc::clone(&state.running_agent_registry),
            Arc::clone(&state.events),
        )
        .with_interactive_process_registry(Arc::clone(&state.interactive_process_registry));

        let stopped = task_cleanup
            .stop_ideation_session_agent(&result.session_id)
            .await;
        if !stopped {
            tracing::warn!(
                "IPR cleanup: no running process found for accepted session {}",
                result.session_id
            );
        }

        // Stop and archive any running verification child agents (best-effort).
        stop_verification_children(&result.session_id, state.inner())
            .await
            .ok();
    }

    // Re-trigger ralphx-utility-session-namer if title was not manually set by user.
    // At acceptance, proposals are finalized — namer generates a commit-ready title
    // reflecting the actual work (not just the initial user message).
    // Skip if user has set a custom title (title_source == "user").
    if !result.is_user_title {
        let proposals_context = result.proposal_titles.join("; ");
        let session_id_str = result.session_id.clone();
        if let Err(error) = spawn_session_namer_agent(
            state.inner(),
            SessionNamerTarget::accepted_session(session_id_str, proposals_context),
        )
        .await
        {
            tracing::warn!("Failed to prepare session namer at acceptance: {}", error);
        }
    }

    // Emit queue_changed if any tasks were set to Ready status
    if result.any_ready_tasks {
        let project_id = ProjectId::from_string(result.project_id.clone());
        emit_queue_changed(state, &project_id, app).await;

        let execution_state = app.state::<Arc<ExecutionState>>();
        spawn_ready_task_scheduler_if_needed(
            state.inner(),
            Arc::clone(&*execution_state),
            Some(app.clone()),
            true,
        );
    }

    Ok(result.into())
}

/// Get blockers for a task (tasks it depends on)
#[tauri::command]
pub async fn get_task_blockers(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let task_id = TaskId::from_string(task_id);

    state
        .task_dependency_repo
        .get_blockers(&task_id)
        .await
        .map(|blockers| {
            blockers
                .into_iter()
                .map(|id| id.as_str().to_string())
                .collect()
        })
        .map_err(|e| e.to_string())
}

/// Get tasks blocked by a task (tasks that depend on this one)
#[tauri::command]
pub async fn get_blocked_tasks(
    task_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let task_id = TaskId::from_string(task_id);

    state
        .task_dependency_repo
        .get_blocked_by(&task_id)
        .await
        .map(|blocked| {
            blocked
                .into_iter()
                .map(|id| id.as_str().to_string())
                .collect()
        })
        .map_err(|e| e.to_string())
}

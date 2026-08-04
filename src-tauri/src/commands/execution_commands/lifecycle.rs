use super::*;
pub(crate) use crate::application::execution_resume::resume_execution_for_state;

/// Pause execution (stops picking up new tasks and transitions running tasks to Paused)
/// This transitions all agent-active tasks to Paused status via TransitionHandler.
/// Paused is NOT terminal - tasks can be auto-restored on resume using status history.
/// The on_exit handlers will decrement the running count for each task.
/// Phase 82: Optional project_id for per-project scoping.
///
/// ## Pause contract
/// 1. `execution_state.pause()` — gates new task scheduling immediately.
/// 2. `running_agent_registry.stop_all()` — kills all agent processes (even if transition fails).
/// 3. For each task in `AGENT_ACTIVE_STATUSES`:
///    a. Write `PauseReason::UserInitiated { previous_status, paused_at, scope: "global" }`
///       to `task.metadata["pause_reason"]` — this is what `resume_execution` reads back.
///    b. Call `transition_task(id, Paused)` via `TransitionHandler` — triggers `on_exit`
///       which decrements `running_count` and emits `execution:status_changed`.
/// 4. `Paused` state machine rejects all events (resume is command-layer only).
///
/// ## What Pause does NOT affect
/// - `Blocked` tasks: already idle, not agent-active.
/// - `Stopped` tasks: already terminal.
/// - `Paused` tasks: already paused (idempotent — won't double-pause).
#[tauri::command]
pub async fn pause_execution(
    project_id: Option<String>,
    active_project_state: State<'_, Arc<ActiveProjectState>>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app_state: State<'_, AppState>,
) -> Result<ExecutionCommandResponse, String> {
    // Sync runtime quota with persisted project settings for consistency
    let project_id = project_id.map(|id| ProjectId::from_string(id));
    let (effective_project_id, _max_concurrent) = sync_quota_from_project(
        project_id,
        &active_project_state,
        &execution_state,
        &app_state,
    )
    .await?;

    // First pause to prevent new tasks from starting
    execution_state.pause();
    persist_execution_halt_mode(&app_state, ExecutionHaltMode::Paused).await?;

    // Kill all running agent processes immediately via registry
    // This ensures agents are terminated even if transition fails
    app_state.running_agent_registry.stop_all().await;
    // Also clear all interactive process entries — their stdin pipes are now dead
    app_state.interactive_process_registry.clear().await;

    // Build transition service for proper state machine transitions
    let transition_service =
        app_state.build_transition_service_with_execution_state(Arc::clone(&execution_state));

    // Find all tasks in agent-active states (scoped to project if specified)
    let projects_to_process = if let Some(ref pid) = effective_project_id {
        match app_state.project_repo.get_by_id(pid).await {
            Ok(Some(project)) => vec![project],
            Ok(None) => return Err(format!("Project not found: {}", pid.as_str())),
            Err(e) => return Err(e.to_string()),
        }
    } else {
        app_state
            .project_repo
            .get_all()
            .await
            .map_err(|e| e.to_string())?
    };

    for project in projects_to_process {
        let tasks = app_state
            .task_repo
            .get_by_project(&project.id)
            .await
            .map_err(|e| e.to_string())?;

        for task in tasks {
            // Check if task is in an agent-active state
            if AGENT_ACTIVE_STATUSES.contains(&task.internal_status) {
                // Store PauseReason::UserInitiated metadata before transitioning
                let pause_reason = crate::application::chat_service::PauseReason::UserInitiated {
                    previous_status: task.internal_status.to_string(),
                    paused_at: chrono::Utc::now().to_rfc3339(),
                    scope: "global".to_string(),
                };
                let mut updated_task = task.clone();
                updated_task.metadata =
                    Some(pause_reason.write_to_task_metadata(updated_task.metadata.as_deref()));
                updated_task.touch();
                let _ = app_state.task_repo.update(&updated_task).await;

                // Use TransitionHandler to transition to Paused
                // Paused is NOT terminal - can be restored on resume
                // This triggers on_exit handlers which decrement running count
                if let Err(e) = transition_service
                    .transition_task(&task.id, InternalStatus::Paused)
                    .await
                {
                    tracing::warn!(
                        task_id = task.id.as_str(),
                        error = %e,
                        "Failed to transition task to Paused during pause"
                    );
                }
            }
        }
    }

    // Note: running_count is decremented by on_exit handlers in TransitionHandler
    // No manual reset needed here

    // Emit status_changed event for real-time UI update with projectId
    // This reflects the final state after all tasks have been paused
    if let Some(ref handle) = app_state.app_handle {
        let _ = handle.emit(
            "execution:status_changed",
            serde_json::json!({
                "isPaused": execution_state.is_paused(),
                "haltMode": "paused",
                "runningCount": execution_state.running_count(),
                "maxConcurrent": execution_state.max_concurrent(),
                "reason": "paused",
                "projectId": effective_project_id.as_ref().map(|p| p.as_str()),
                "timestamp": chrono::Utc::now().to_rfc3339(),
            }),
        );
    }

    // Get current status
    let status = get_execution_status(
        effective_project_id.map(|p| p.as_str().to_string()),
        active_project_state,
        execution_state,
        app_state,
    )
    .await?;

    Ok(ExecutionCommandResponse {
        success: true,
        status,
    })
}

/// Resume execution (restores Paused tasks and allows picking up new tasks)
/// This restores only Paused tasks (NOT Stopped) to their previous agent-active state.
/// Uses status history to find the pre-pause state and re-runs entry actions.
/// If execution was previously Stopped, this simply reopens the scheduler gate so
/// manually restarted Ready tasks can run again. After restoring, triggers the
/// scheduler to pick up waiting Ready tasks.
/// Phase 82: Optional project_id for per-project scoping.
///
/// ## Resume contract
/// - `Paused` → previous agent-active state (from `pause_reason.previous_status` metadata).
/// - Falls back to `status_history` if `pause_reason` metadata is absent.
/// - Skips tasks whose `previous_status` is not in `AGENT_ACTIVE_STATUSES` (defensive guard).
/// - Respects `execution_state.can_start_task()` — stops restoring once capacity is full.
/// - Calls `execute_entry_actions()` after transition to re-spawn the agent process.
///
/// ## Stopped vs Paused
/// `Stopped` tasks are intentionally excluded. `Stopped` is terminal — requires the user
/// to manually trigger `Retry` (→ `ready`) before the task can re-execute. `resume_execution`
/// may still clear the global halt gate after a stop, but it never auto-restores stopped tasks.
#[tauri::command]
pub async fn resume_execution(
    project_id: Option<String>,
    active_project_state: State<'_, Arc<ActiveProjectState>>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app_state: State<'_, AppState>,
) -> Result<ExecutionCommandResponse, String> {
    resume_execution_for_state(
        project_id,
        active_project_state.inner(),
        execution_state.inner(),
        app_state.inner(),
    )
    .await
}

/// Stop execution (cancels current tasks and pauses)
/// This transitions all agent-active tasks to Stopped status via TransitionHandler.
/// Stopped is a terminal state requiring manual restart - tasks won't auto-resume.
/// The on_exit handlers will decrement the running count for each task.
/// Phase 82: Optional project_id for per-project scoping.
///
/// ## Stop vs Pause
/// | | `stop_execution` | `pause_execution` |
/// |---|---|---|
/// | Result state | `Stopped` (terminal) | `Paused` (non-terminal) |
/// | Auto-resume | ❌ No — user must retry individual tasks | ✅ Yes — via `resume_execution` |
/// | Metadata written | None | `PauseReason::UserInitiated` |
/// | `running_count` | Decremented by `on_exit` | Decremented by `on_exit` |
/// | Global restart path | `resume_execution` reopens scheduling, but stopped tasks still need manual retry | `resume_execution` → previous state |
#[tauri::command]
pub async fn stop_execution(
    project_id: Option<String>,
    active_project_state: State<'_, Arc<ActiveProjectState>>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app_state: State<'_, AppState>,
) -> Result<ExecutionCommandResponse, String> {
    // Sync runtime quota with persisted project settings for consistency
    let project_id = project_id.map(|id| ProjectId::from_string(id));
    let (effective_project_id, _max_concurrent) = sync_quota_from_project(
        project_id,
        &active_project_state,
        &execution_state,
        &app_state,
    )
    .await?;

    // First pause to prevent new tasks from starting
    execution_state.pause();
    persist_execution_halt_mode(&app_state, ExecutionHaltMode::Stopped).await?;

    // Kill all running agent processes immediately via registry
    // This ensures agents are terminated even if transition fails
    app_state.running_agent_registry.stop_all().await;
    // Also clear all interactive process entries — their stdin pipes are now dead
    app_state.interactive_process_registry.clear().await;
    if let Err(error) = clear_paused_chat_queues(effective_project_id.as_ref(), &app_state).await {
        tracing::warn!(error = %error, "Failed to clear queued chat work during stop");
    }

    // Build transition service for proper state machine transitions
    let transition_service =
        app_state.build_transition_service_with_execution_state(Arc::clone(&execution_state));

    // Find all tasks in agent-active states (scoped to project if specified)
    let projects_to_process = if let Some(ref pid) = effective_project_id {
        match app_state.project_repo.get_by_id(pid).await {
            Ok(Some(project)) => vec![project],
            Ok(None) => return Err(format!("Project not found: {}", pid.as_str())),
            Err(e) => return Err(e.to_string()),
        }
    } else {
        app_state
            .project_repo
            .get_all()
            .await
            .map_err(|e| e.to_string())?
    };

    for project in projects_to_process {
        let tasks = app_state
            .task_repo
            .get_by_project(&project.id)
            .await
            .map_err(|e| e.to_string())?;

        for task in tasks {
            // Check if task is in an agent-active state
            if AGENT_ACTIVE_STATUSES.contains(&task.internal_status) {
                // Use TransitionHandler to transition to Stopped
                // Stopped is terminal - requires manual restart via Ready transition
                // This triggers on_exit handlers which decrement running count
                if let Err(e) = transition_service
                    .transition_task(&task.id, InternalStatus::Stopped)
                    .await
                {
                    tracing::warn!(
                        task_id = task.id.as_str(),
                        error = %e,
                        "Failed to transition task to Stopped during stop"
                    );
                }
            }
        }
    }

    // Note: running_count is decremented by on_exit handlers in TransitionHandler
    // No manual reset needed here

    // Emit status_changed event for real-time UI update with projectId
    // This reflects the final state after all tasks have been stopped
    if let Some(ref handle) = app_state.app_handle {
        let _ = handle.emit(
            "execution:status_changed",
            serde_json::json!({
                "isPaused": execution_state.is_paused(),
                "haltMode": "stopped",
                "runningCount": execution_state.running_count(),
                "maxConcurrent": execution_state.max_concurrent(),
                "reason": "stopped",
                "projectId": effective_project_id.as_ref().map(|p| p.as_str()),
                "timestamp": chrono::Utc::now().to_rfc3339(),
            }),
        );
    }

    // Get current status
    let status = get_execution_status(
        effective_project_id.map(|p| p.as_str().to_string()),
        active_project_state,
        execution_state,
        app_state,
    )
    .await?;

    Ok(ExecutionCommandResponse {
        success: true,
        status,
    })
}

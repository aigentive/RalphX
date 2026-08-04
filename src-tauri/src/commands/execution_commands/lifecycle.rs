use super::*;
use crate::domain::state_machine::transition_handler::set_trigger_origin;

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

pub(crate) async fn resume_execution_for_state(
    project_id: Option<String>,
    active_project_state: &Arc<ActiveProjectState>,
    execution_state: &Arc<ExecutionState>,
    app_state: &AppState,
) -> Result<ExecutionCommandResponse, String> {
    let previous_halt_mode = load_execution_halt_mode(app_state).await?;

    // Sync runtime quota with persisted project settings before can_start_task() loops
    let project_id = project_id.map(|id| ProjectId::from_string(id));
    let (effective_project_id, _max_concurrent) =
        sync_quota_from_project(project_id, active_project_state, execution_state, app_state)
            .await?;
    persist_execution_halt_mode(app_state, ExecutionHaltMode::Running).await?;

    // Build transition service for proper state machine transitions
    let transition_service =
        app_state.build_transition_service_with_execution_state(Arc::clone(execution_state));

    // Find all Paused tasks (scoped to project if specified) and restore them
    // Note: Stopped tasks are NOT restored - they require manual restart
    // Local counter tracks tasks queued for restoration during this loop.
    // We cannot use execution_state.can_start_task() here because: (a) the pause
    // flag is still set (cleared after the loop), and (b) running_count hasn't yet
    // been incremented for tasks whose entry actions fire asynchronously.
    let mut restoring_count: u32 = 0;
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
            .get_by_status(&project.id, InternalStatus::Paused)
            .await
            .map_err(|e| e.to_string())?;

        for task in tasks {
            // Determine restore status: prefer valid pause_reason metadata, otherwise
            // fall back to the recorded pre-pause status in status history.
            let restore_status =
                match determine_paused_restore_status(&task, app_state.task_repo.as_ref()).await {
                    Ok(Some(status)) => status,
                    Ok(None) => continue,
                    Err(e) => {
                        tracing::warn!(
                            task_id = task.id.as_str(),
                            error = %e,
                            "Failed to resolve paused restore status"
                        );
                        continue;
                    }
                };

            // Validate that the restore status is a valid agent-active state
            if !AGENT_ACTIVE_STATUSES.contains(&restore_status) {
                tracing::warn!(
                    task_id = task.id.as_str(),
                    restore_status = ?restore_status,
                    "Pre-pause status is not agent-active, skipping restore"
                );
                continue;
            }

            // Check capacity using local counter (running_count not yet incremented
            // for tasks queued this loop; pause flag still set so can_start_task() → false).
            let current = execution_state.running_count() + restoring_count;
            if current >= execution_state.max_concurrent()
                || current >= execution_state.global_max_concurrent()
            {
                tracing::info!(
                    task_id = task.id.as_str(),
                    running = execution_state.running_count(),
                    restoring = restoring_count,
                    "Max concurrent reached, stopping pause recovery"
                );
                break;
            }

            // Transition back to the pre-pause status
            if let Err(e) = transition_service
                .transition_task(&task.id, restore_status)
                .await
            {
                tracing::warn!(
                    task_id = task.id.as_str(),
                    restore_status = ?restore_status,
                    error = %e,
                    "Failed to restore task from Paused"
                );
                continue;
            }

            // Re-run entry actions to respawn the agent
            // Fetch fresh task after transition
            if let Ok(Some(mut restored_task)) = app_state.task_repo.get_by_id(&task.id).await {
                prepare_resumed_task_for_entry_actions(&mut restored_task);
                restored_task.touch();
                let _ = app_state.task_repo.update(&restored_task).await;

                transition_service
                    .execute_entry_actions(&task.id, &restored_task, restore_status)
                    .await;

                restoring_count += 1;
                tracing::info!(
                    task_id = task.id.as_str(),
                    restored_to = ?restore_status,
                    restoring_count,
                    "Successfully restored Paused task"
                );
            }
        }
    }

    // Clear the pause flag now that all paused tasks have been queued for restoration.
    // Doing this after the loop prevents the scheduler from racing with restoration
    // and prevents can_start_task() from returning false during the loop above.
    execution_state.resume();

    // Emit status_changed event for real-time UI update with projectId
    if let Some(ref handle) = app_state.app_handle {
        let _ = handle.emit(
            "execution:status_changed",
            serde_json::json!({
                "isPaused": execution_state.is_paused(),
                "haltMode": "running",
                "runningCount": execution_state.running_count(),
                "maxConcurrent": execution_state.max_concurrent(),
                "reason": if previous_halt_mode == ExecutionHaltMode::Stopped { "started" } else { "resumed" },
                "projectId": effective_project_id.as_ref().map(|p| p.as_str()),
                "timestamp": chrono::Utc::now().to_rfc3339(),
            }),
        );
    }

    // Trigger scheduler to pick up waiting Ready tasks
    let scheduler = Arc::new(app_state.build_task_scheduler_for_runtime(
        Arc::clone(execution_state),
        app_state.app_handle.clone(),
    ));
    scheduler.set_self_ref(Arc::clone(&scheduler) as Arc<dyn TaskScheduler>);
    // Set active project scope before scheduling to prevent cross-project scheduling
    scheduler
        .set_active_project(effective_project_id.clone())
        .await;
    scheduler.try_schedule_ready_tasks().await;

    if app_state.app_handle.is_some() {
        let execution_state_arc = Arc::clone(execution_state);
        if let Err(error) = resume_paused_workspace_queues_with_chat_service(
            effective_project_id.as_ref(),
            app_state,
            &execution_state_arc,
            || {
                Arc::new(
                    app_state
                        .build_chat_service_with_execution_state(Arc::clone(&execution_state_arc)),
                ) as Arc<dyn ChatService>
            },
        )
        .await
        {
            tracing::warn!(
                error = %error,
                "Failed to relaunch paused workspace queues on resume"
            );
        }

        if let Err(error) = resume_paused_slot_consuming_queues_with_chat_service(
            effective_project_id.as_ref(),
            app_state,
            &execution_state_arc,
            || {
                Arc::new(
                    app_state
                        .build_chat_service_with_execution_state(Arc::clone(&execution_state_arc)),
                ) as Arc<dyn ChatService>
            },
        )
        .await
        {
            tracing::warn!(
                error = %error,
                "Failed to relaunch paused task/review/merge queues on resume"
            );
        }

        if let Err(error) = resume_paused_ideation_queues_with_chat_service(
            effective_project_id.as_ref(),
            app_state,
            &execution_state_arc,
            || {
                Arc::new(
                    app_state
                        .build_chat_service_with_execution_state(Arc::clone(&execution_state_arc)),
                ) as Arc<dyn ChatService>
            },
        )
        .await
        {
            tracing::warn!(error = %error, "Failed to relaunch paused ideation queues on resume");
        }

        if let Err(error) = resume_paused_non_slot_chat_queues_with_chat_service(
            effective_project_id.as_ref(),
            app_state,
            || {
                Arc::new(
                    app_state
                        .build_chat_service_with_execution_state(Arc::clone(&execution_state_arc)),
                ) as Arc<dyn ChatService>
            },
        )
        .await
        {
            tracing::warn!(
                error = %error,
                "Failed to relaunch paused task/project chat queues on resume"
            );
        }
    }

    // Get current status
    let status = get_execution_status_for_state(
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

#[doc(hidden)]
pub(crate) fn prepare_resumed_task_for_entry_actions(task: &mut Task) {
    task.metadata = Some(
        crate::application::chat_service::PauseReason::clear_from_task_metadata(
            task.metadata.as_deref(),
        ),
    );
    set_trigger_origin(task, "resume");
}

#[doc(hidden)]
pub(crate) async fn determine_paused_restore_status(
    task: &Task,
    task_repo: &dyn crate::domain::repositories::TaskRepository,
) -> Result<Option<InternalStatus>, crate::error::AppError> {
    if let Some(reason) =
        crate::application::chat_service::PauseReason::from_task_metadata(task.metadata.as_deref())
    {
        match reason.previous_status().parse::<InternalStatus>() {
            Ok(status) => return Ok(Some(status)),
            Err(_) => {
                tracing::warn!(
                    task_id = task.id.as_str(),
                    previous_status = reason.previous_status(),
                    "Invalid previous_status in pause metadata, falling back to status history"
                );
            }
        }
    }

    let status_history = task_repo.get_status_history(&task.id).await?;
    let pause_transition = status_history
        .iter()
        .rev()
        .find(|t| t.to == InternalStatus::Paused);

    if let Some(transition) = pause_transition {
        return Ok(Some(transition.from));
    }

    tracing::warn!(
        task_id = task.id.as_str(),
        "No pause transition found in history or metadata, cannot restore"
    );
    Ok(None)
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

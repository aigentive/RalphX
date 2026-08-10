use super::*;
use ralphx_events::emit_serialized;

/// Set maximum concurrent tasks
/// When capacity increases, triggers the scheduler to pick up waiting Ready tasks.
#[tauri::command]
pub async fn set_max_concurrent(
    max: u32,
    active_project_state: State<'_, Arc<ActiveProjectState>>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app_state: State<'_, AppState>,
) -> Result<ExecutionCommandResponse, String> {
    let old_max = execution_state.max_concurrent();
    execution_state.set_max_concurrent(max);

    // Emit status_changed event for real-time UI update
    execution_state
        .emit_status_changed_to_sink(app_state.events.as_ref(), "max_concurrent_changed");

    // If capacity increased, trigger scheduler to pick up waiting Ready tasks
    if max > old_max {
        // Get active project for scoped scheduling
        let active_project_id = active_project_state.get().await;
        schedule_ready_tasks_for_project(
            &app_state,
            Arc::clone(&execution_state),
            active_project_id,
        )
        .await;
    }

    // Get current status
    let status =
        get_execution_status(None, active_project_state, execution_state, app_state).await?;

    Ok(ExecutionCommandResponse {
        success: true,
        status,
    })
}

/// Get execution settings from database
/// Phase 82: Optional project_id for per-project settings
/// - project_id = Some(id): returns settings for that project (falls back to global if none exist)
/// - project_id = None: returns global default settings
#[tauri::command]
pub async fn get_execution_settings(
    project_id: Option<String>,
    app_state: State<'_, AppState>,
) -> Result<ExecutionSettingsResponse, String> {
    let project_id = project_id.map(|id| ProjectId::from_string(id));
    let settings = app_state
        .execution_settings_repo
        .get_settings(project_id.as_ref())
        .await
        .map_err(|e| e.to_string())?;

    Ok(ExecutionSettingsResponse::from(settings))
}

/// Update execution settings in database and sync ExecutionState
/// Phase 82: Optional project_id for per-project settings
/// - project_id = Some(id): updates/creates settings for that project
/// - project_id = None: updates global default settings
/// When max_concurrent_tasks changes:
/// - Updates the in-memory ExecutionState
/// - If capacity increased, triggers scheduler to pick up waiting Ready tasks
/// Emits settings:execution:updated event for UI updates
#[tauri::command]
pub async fn update_execution_settings(
    project_id: Option<String>,
    input: UpdateExecutionSettingsInput,
    execution_state: State<'_, Arc<ExecutionState>>,
    app_state: State<'_, AppState>,
) -> Result<ExecutionSettingsResponse, String> {
    let project_id = project_id.map(|id| ProjectId::from_string(id));
    let old_max = execution_state.max_concurrent();
    let new_max = input.max_concurrent_tasks;

    // Read old project_ideation_max from DB before persisting.
    // ExecutionState.project_ideation_max() is a single global AtomicU32 that may reflect
    // a different project's value, so we read from DB for the accurate per-project comparison.
    let old_project_ideation_max = app_state
        .execution_settings_repo
        .get_settings(project_id.as_ref())
        .await
        .map(|s| s.project_ideation_max)
        .unwrap_or(input.project_ideation_max); // if read fails, assume unchanged → no drain

    // Build domain settings from input
    let settings = ExecutionSettings {
        max_concurrent_tasks: input.max_concurrent_tasks,
        project_ideation_max: input.project_ideation_max,
        auto_commit: input.auto_commit,
        pause_on_failure: input.pause_on_failure,
        agent_workspace_pr_autofix_default: input.agent_workspace_pr_autofix_default,
        agent_workspace_pr_auto_merge_default: input.agent_workspace_pr_auto_merge_default,
    };

    // Persist to database
    let updated = app_state
        .execution_settings_repo
        .update_settings(project_id.as_ref(), &settings)
        .await
        .map_err(|e| e.to_string())?;

    // Sync ExecutionState if max_concurrent_tasks changed
    if new_max != old_max {
        execution_state.set_max_concurrent(new_max);

        // If capacity increased, trigger scheduler to pick up waiting Ready tasks
        if new_max > old_max {
            schedule_ready_tasks_for_project(
                &app_state,
                Arc::clone(&execution_state),
                project_id.clone(),
            )
            .await;
        }
    }

    // Sync ExecutionState project_ideation_max (per-project setting reflected in global atomic)
    execution_state.set_project_ideation_max(updated.project_ideation_max);

    // If project_ideation_max increased and we have a project scope, wake queued sessions.
    // DB is already persisted above so PendingSessionDrainService will see the new capacity.
    if updated.project_ideation_max > old_project_ideation_max {
        if let Some(ref pid) = project_id {
            let svc =
                app_state.build_chat_service_with_execution_state(Arc::clone(&execution_state));
            let chat_svc: Arc<dyn ChatService> = Arc::new(svc);

            let drain = Arc::new(
                crate::application::pending_session_drain::PendingSessionDrainService::new(
                    Arc::clone(&app_state.ideation_session_repo),
                    Arc::clone(&app_state.project_repo),
                    Arc::clone(&app_state.task_repo),
                    Arc::clone(&app_state.chat_conversation_repo),
                    Arc::clone(&app_state.execution_settings_repo),
                    Arc::clone(&execution_state),
                    Arc::clone(&app_state.running_agent_registry),
                    Arc::clone(&app_state.message_queue),
                    chat_svc,
                ),
            );
            let project_id_str = pid.0.clone();
            tokio::spawn(async move {
                drain.try_drain_pending_for_project(&project_id_str).await;
            });
        }
    }

    // Emit settings:execution:updated event for UI updates (include projectId for per-project)
    let _ = emit_serialized(
        app_state.events.as_ref(),
        "settings:execution:updated",
        &serde_json::json!({
            "project_id": project_id.as_ref().map(|p| p.as_str()),
            "max_concurrent_tasks": updated.max_concurrent_tasks,
            "project_ideation_max": updated.project_ideation_max,
            "auto_commit": updated.auto_commit,
            "pause_on_failure": updated.pause_on_failure,
            "agent_workspace_pr_autofix_default": updated.agent_workspace_pr_autofix_default,
            "agent_workspace_pr_auto_merge_default": updated.agent_workspace_pr_auto_merge_default,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }),
    );

    Ok(ExecutionSettingsResponse::from(updated))
}

// ========================================
// Phase 82: Active Project Management
// ========================================

/// Set the active project for execution scoping.
/// Frontend should call this when switching projects.
/// Commands without explicit project_id will use this active project.
#[tauri::command]
pub async fn set_active_project(
    project_id: Option<String>,
    active_project_state: State<'_, Arc<ActiveProjectState>>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app_state: State<'_, AppState>,
) -> Result<(), String> {
    let project_id = project_id.map(|id| ProjectId::from_string(id));

    // Validate project exists if ID provided
    if let Some(ref pid) = project_id {
        let exists = app_state
            .project_repo
            .get_by_id(pid)
            .await
            .map_err(|e| e.to_string())?
            .is_some();

        if !exists {
            return Err(format!("Project not found: {}", pid.as_str()));
        }
    }

    if !active_project_state
        .set_if_changed(project_id.clone())
        .await
    {
        tracing::debug!(
            project_id = ?project_id.as_ref().map(|p| p.as_str()),
            "Active project set skipped; requested project is already active"
        );
        return Ok(());
    }

    // Persist to DB so it survives app restarts
    app_state
        .app_state_repo
        .set_active_project(project_id.as_ref())
        .await
        .map_err(|e| e.to_string())?;

    // Sync runtime quota immediately after switching active project
    let (_resolved_project_id, _max_concurrent) = sync_quota_from_project(
        project_id.clone(),
        &active_project_state,
        &execution_state,
        &app_state,
    )
    .await?;

    tracing::info!(
        project_id = ?project_id.as_ref().map(|p| p.as_str()),
        "Active project set (in-memory + DB)"
    );

    // Emit events for UI sync
    let _ = emit_serialized(
        app_state.events.as_ref(),
        "execution:active_project_changed",
        &serde_json::json!({
            "projectId": project_id.as_ref().map(|p| p.as_str()),
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }),
    );

    // Emit execution:status_changed after sync so UI updates quota instantly
    let _ = emit_serialized(
        app_state.events.as_ref(),
        "execution:status_changed",
        &serde_json::json!({
            "isPaused": execution_state.is_paused(),
            "runningCount": execution_state.running_count(),
            "maxConcurrent": execution_state.max_concurrent(),
            "globalMaxConcurrent": execution_state.global_max_concurrent(),
            "workspaceMaxConcurrent": execution_state.workspace_max_concurrent(),
            "reason": "active_project_changed",
            "projectId": project_id.as_ref().map(|p| p.as_str()),
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }),
    );

    Ok(())
}

/// Get the current active project ID.
/// Returns None if no project is active.
#[tauri::command]
pub async fn get_active_project(
    active_project_state: State<'_, Arc<ActiveProjectState>>,
) -> Result<Option<String>, String> {
    let project_id = active_project_state.get().await;
    Ok(project_id.map(|p| p.as_str().to_string()))
}

// ========================================
// Phase 82: Global Execution Settings
// ========================================

/// Get global execution settings (cross-project limits)
/// Returns the global_max_concurrent cap that limits total tasks across all projects
#[tauri::command]
pub async fn get_global_execution_settings(
    app_state: State<'_, AppState>,
) -> Result<GlobalExecutionSettingsResponse, String> {
    let settings = app_state
        .global_execution_settings_repo
        .get_settings()
        .await
        .map_err(|e| e.to_string())?;

    Ok(GlobalExecutionSettingsResponse::from(settings))
}

/// Update global execution settings (cross-project limits)
/// global_max_concurrent is capped at 50 (enforced by repository)
/// Syncs in-memory ExecutionState and triggers scheduler if capacity increased
#[tauri::command]
pub async fn update_global_execution_settings(
    input: UpdateGlobalExecutionSettingsInput,
    active_project_state: State<'_, Arc<ActiveProjectState>>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app_state: State<'_, AppState>,
) -> Result<GlobalExecutionSettingsResponse, String> {
    use crate::domain::execution::GlobalExecutionSettings;

    let old_global_max = execution_state.global_max_concurrent();
    let old_workspace_max = execution_state.workspace_max_concurrent();

    let settings = GlobalExecutionSettings {
        global_max_concurrent: input.global_max_concurrent,
        workspace_max_concurrent: input.workspace_max_concurrent,
        global_ideation_max: input.global_ideation_max,
        allow_ideation_borrow_idle_execution: input.allow_ideation_borrow_idle_execution,
    };

    let updated = app_state
        .global_execution_settings_repo
        .update_settings(&settings)
        .await
        .map_err(|e| e.to_string())?;

    // Sync in-memory global cap
    execution_state.set_global_max_concurrent(updated.global_max_concurrent);
    execution_state.set_workspace_max_concurrent(updated.workspace_max_concurrent);
    execution_state.set_global_ideation_max(updated.global_ideation_max);
    execution_state
        .set_allow_ideation_borrow_idle_execution(updated.allow_ideation_borrow_idle_execution);

    if updated.global_max_concurrent > old_global_max
        || updated.workspace_max_concurrent > old_workspace_max
    {
        let chat_service = Arc::new(
            app_state.build_chat_service_with_execution_state(Arc::clone(&execution_state)),
        ) as Arc<dyn ChatService>;
        if let Err(error) = resume_paused_workspace_queues_with_chat_service(
            None,
            &app_state,
            &execution_state,
            || Arc::clone(&chat_service),
        )
        .await
        {
            tracing::warn!(
                error = %error,
                "Failed to relaunch workspace queues after global capacity update"
            );
        }
    }

    // If global capacity increased, trigger scheduler to pick up waiting tasks
    if updated.global_max_concurrent > old_global_max {
        // Get active project for scoped scheduling
        let active_project_id = active_project_state.get().await;
        schedule_ready_tasks_for_project(
            &app_state,
            Arc::clone(&execution_state),
            active_project_id,
        )
        .await;
    }

    // Emit event for UI sync
    let _ = emit_serialized(
        app_state.events.as_ref(),
        "settings:global_execution:updated",
        &serde_json::json!({
            "global_max_concurrent": updated.global_max_concurrent,
            "workspace_max_concurrent": updated.workspace_max_concurrent,
            "global_ideation_max": updated.global_ideation_max,
            "allow_ideation_borrow_idle_execution": updated.allow_ideation_borrow_idle_execution,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }),
    );

    Ok(GlobalExecutionSettingsResponse::from(updated))
}

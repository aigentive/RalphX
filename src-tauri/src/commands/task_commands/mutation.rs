// Mutation (write) handlers for task_commands module

use super::helpers::{emit_queue_changed, emit_task_lifecycle_event};
use super::types::{
    AnswerUserQuestionInput, AnswerUserQuestionResponse, CreateTaskInput, InjectTaskInput,
    InjectTaskResponse, TaskResponse, UnblockTaskResponse, UpdateTaskInput,
};
use crate::application::chat_service::PauseReason;
use crate::application::{AppState, TaskTransitionService};
use crate::commands::execution_commands::prepare_resumed_task_for_entry_actions;
use crate::commands::execution_commands::project_has_execution_capacity_for_state;
use crate::commands::ExecutionState;
use crate::domain::entities::{
    ExecutionRecoveryMetadata, ExecutionRecoveryState, InternalStatus, ProjectId, Task,
    TaskCategory, TaskId, TaskStepStatus,
};
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::transition_handler::metadata_builder::build_restart_metadata;
use crate::domain::state_machine::transition_handler::{parse_metadata, set_trigger_origin};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

async fn clear_failed_steps_for_failed_restart(
    task_step_repo: &Arc<dyn crate::domain::repositories::TaskStepRepository>,
    task_id: &TaskId,
) -> Result<u32, String> {
    let steps = task_step_repo
        .get_by_task(task_id)
        .await
        .map_err(|e| e.to_string())?;

    let mut cleared = 0u32;
    for mut step in steps {
        if step.status != TaskStepStatus::Failed {
            continue;
        }

        step.status = TaskStepStatus::Pending;
        step.started_at = None;
        step.completed_at = None;
        step.completion_note = None;
        task_step_repo
            .update(&step)
            .await
            .map_err(|e| e.to_string())?;
        cleared += 1;
    }

    Ok(cleared)
}

fn validate_update_task_input(input: &UpdateTaskInput) -> Result<(), String> {
    if input.internal_status.is_some() {
        return Err(
            "Task status updates must use move_task or the dedicated lifecycle commands"
                .to_string(),
        );
    }

    Ok(())
}

fn build_transition_service(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    app_handle: Option<&AppHandle>,
) -> TaskTransitionService {
    state.build_transition_service_for_runtime(
        Arc::clone(execution_state),
        app_handle.cloned().or_else(|| state.app_handle.clone()),
    )
}

fn build_task_scheduler(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    app: &AppHandle,
) -> Arc<dyn TaskScheduler> {
    let scheduler_concrete = Arc::new(
        state.build_task_scheduler_for_runtime(Arc::clone(execution_state), Some(app.clone())),
    );
    scheduler_concrete.set_self_ref(Arc::clone(&scheduler_concrete) as Arc<dyn TaskScheduler>);
    scheduler_concrete as Arc<dyn TaskScheduler>
}

/// Create a new task
#[tauri::command]
pub async fn create_task(
    input: CreateTaskInput,
    state: State<'_, AppState>,
) -> Result<TaskResponse, String> {
    let project_id = ProjectId::from_string(input.project_id);
    let category: TaskCategory = input
        .category
        .as_deref()
        .unwrap_or("regular")
        .parse()
        .unwrap_or(TaskCategory::Regular);

    let mut task = Task::new_with_category(project_id, input.title, category);

    if let Some(desc) = input.description {
        task.description = Some(desc);
    }
    if let Some(priority) = input.priority {
        task.priority = priority;
    }

    // Create the task first
    let created_task = state
        .task_repo
        .create(task)
        .await
        .map_err(|e| e.to_string())?;

    // If steps are provided, create TaskSteps for each
    if let Some(step_titles) = input.steps {
        if !step_titles.is_empty() {
            use crate::domain::entities::TaskStep;

            let steps: Vec<TaskStep> = step_titles
                .into_iter()
                .enumerate()
                .map(|(idx, title)| {
                    TaskStep::new(
                        created_task.id.clone(),
                        title,
                        idx as i32,
                        "user".to_string(),
                    )
                })
                .collect();

            // Use bulk_create for efficiency
            state
                .task_step_repo
                .bulk_create(steps)
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(TaskResponse::from(created_task))
}

/// Update an existing task
#[tauri::command]
pub async fn update_task(
    task_id: String,
    input: UpdateTaskInput,
    state: State<'_, AppState>,
) -> Result<TaskResponse, String> {
    validate_update_task_input(&input)?;

    let task_id = TaskId::from_string(task_id);

    // Get existing task
    let mut task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;

    // Apply updates
    if let Some(title) = input.title {
        task.title = title;
    }
    if let Some(desc) = input.description {
        task.description = Some(desc);
    }
    if let Some(category_str) = input.category {
        task.category = category_str.parse().unwrap_or(TaskCategory::Regular);
    }
    if let Some(priority) = input.priority {
        task.priority = priority;
    }
    task.touch();

    state
        .task_repo
        .update(&task)
        .await
        .map_err(|e| e.to_string())?;

    Ok(TaskResponse::from(task))
}

/// Delete a task
#[tauri::command]
pub async fn delete_task(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let task_id = TaskId::from_string(id);
    state
        .task_repo
        .delete(&task_id)
        .await
        .map_err(|e| e.to_string())
}

/// Move a task to a new status (for Kanban drag-drop)
///
/// This command uses the TaskTransitionService to properly trigger state machine
/// entry actions, such as spawning worker agents when moving to "executing" status.
///
/// # Arguments
/// * `task_id` - The task ID (camelCase for frontend compatibility)
/// * `to_status` - The target status string (e.g., "ready", "executing", "approved")
///
/// # Returns
/// * `TaskResponse` - The updated task
#[tauri::command]
pub async fn move_task(
    task_id: String,
    to_status: String,
    agent_variant: Option<String>,
    note: Option<String>,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<TaskResponse, String> {
    tracing::info!(task_id = %task_id, to_status = %to_status, "move_task command invoked");

    let task_id = TaskId::from_string(task_id);

    // Parse the target status
    let new_status: InternalStatus = to_status
        .parse()
        .map_err(|_| format!("Invalid status: {}", to_status))?;

    // Get the old task to know its current status before transition
    let old_task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;

    let old_status = old_task.internal_status;
    let project_id = old_task.project_id.clone();

    // Terminal→Ready restart: clear stale git refs, reset execution_recovery, set trigger_origin,
    // and update agent_variant — all in a single atomic task_repo.update().
    //
    // IMPORTANT: This write MUST happen before transition_task_with_metadata / transition_task
    // below, because both paths re-fetch the task from DB before running on_enter. The cleared
    // git refs must be visible to on_enter so it creates a fresh branch instead of failing with
    // ExecutionBlocked on a deleted branch.
    if old_status.is_terminal() && new_status == InternalStatus::Ready {
        let mut task_mut = old_task.clone();

        // 1. Set trigger_origin (existing behavior)
        set_trigger_origin(&mut task_mut, "retry");

        // 2. Clear stale git refs so on_enter creates a fresh branch on next execution.
        //    merge_commit_sha is also cleared: on restart the task enters a full new execution
        //    cycle, so the old merge SHA is irrelevant.
        task_mut.task_branch = None;
        task_mut.worktree_path = None;
        task_mut.merge_commit_sha = None;

        // 3. Selectively reset execution_recovery: clear transient state but PRESERVE
        //    auto_recovery_count so the reconciler circuit-breaker stays intact.
        if let Ok(Some(mut recovery)) =
            ExecutionRecoveryMetadata::from_task_metadata(task_mut.metadata.as_deref())
        {
            recovery.stop_retrying = false;
            recovery.last_state = ExecutionRecoveryState::Retrying;
            recovery.events.clear();
            recovery.unrecoverable_reason = None;
            // auto_recovery_count intentionally preserved
            if let Ok(updated_meta) = recovery.update_task_metadata(task_mut.metadata.as_deref()) {
                task_mut.metadata = Some(updated_meta);
            }
        }

        // 4. Update agent_variant here to avoid the metadata clobber from block 2.
        //    Block 2 calls parse_metadata(&old_task) — the pre-write snapshot — then
        //    update_metadata() which overwrites the entire metadata string, erasing the
        //    execution_recovery reset above. Handle agent_variant in this block instead.
        let mut meta = parse_metadata(&task_mut).unwrap_or_else(|| serde_json::json!({}));
        if let Some(obj) = meta.as_object_mut() {
            if old_status == InternalStatus::Failed {
                obj.insert("preserve_steps".to_string(), serde_json::json!(true));
            }
            match agent_variant.as_deref() {
                Some(variant) if !variant.is_empty() => {
                    obj.insert("agent_variant".to_string(), serde_json::json!(variant));
                }
                _ => {
                    obj.remove("agent_variant");
                }
            }
        }
        task_mut.metadata = Some(meta.to_string());

        if let Err(e) = state.task_repo.update(&task_mut).await {
            tracing::error!(
                task_id = task_id.as_str(),
                error = %e,
                "Failed to clear git refs and reset execution_recovery on task restart"
            );
        }

        if old_status == InternalStatus::Failed {
            match clear_failed_steps_for_failed_restart(&state.task_step_repo, &task_id).await {
                Ok(cleared) if cleared > 0 => {
                    tracing::info!(
                        task_id = task_id.as_str(),
                        cleared,
                        "Cleared failed steps while preserving completed progress for failed-task restart"
                    );
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        task_id = task_id.as_str(),
                        error = %error,
                        "Failed to clear failed steps for failed-task restart"
                    );
                }
            }
        }
    }

    // Always update agent_variant in metadata for ready/executing transitions
    // so that switching from team→solo properly clears the stale "team" value.
    // SKIP for terminal→Ready: agent_variant is already handled in the block above
    // to prevent it from clobbering the execution_recovery reset via update_metadata().
    if matches!(
        new_status,
        InternalStatus::Ready | InternalStatus::Executing
    ) && !(old_status.is_terminal() && new_status == InternalStatus::Ready)
    {
        let mut meta = parse_metadata(&old_task).unwrap_or_else(|| serde_json::json!({}));
        if let Some(obj) = meta.as_object_mut() {
            match agent_variant.as_deref() {
                Some(variant) if !variant.is_empty() => {
                    obj.insert("agent_variant".to_string(), serde_json::json!(variant));
                }
                _ => {
                    obj.remove("agent_variant");
                }
            }
        }
        if let Err(e) = state
            .task_repo
            .update_metadata(&task_id, Some(meta.to_string()))
            .await
        {
            tracing::error!(
                task_id = task_id.as_str(),
                error = %e,
                "Failed to update agent_variant in metadata"
            );
        }
    }

    // Create the task scheduler for auto-scheduling Ready tasks
    let task_scheduler = build_task_scheduler(&state, &execution_state, &app);

    // Create the transition service with all required dependencies
    let is_team_mode = agent_variant.as_deref() == Some("team");
    let mut transition_service =
        build_transition_service(&state, &execution_state, Some(&app))
            .with_task_scheduler(Arc::clone(&task_scheduler));

    // ALWAYS set team_mode based on explicit UI selection so it overrides
    // env var defaults and stale metadata. Some(true) = team, Some(false) = solo.
    transition_service = transition_service.with_team_mode(is_team_mode);
    transition_service = transition_service.with_step_repo(Arc::clone(&state.task_step_repo));

    // Transition the task - this triggers entry actions like spawning workers!
    // When a note is provided and the task is moving to Ready (restart/reopen flow),
    // store it as restart_note in metadata so the re-executing agent can read it.
    let task = if note.is_some() && new_status == InternalStatus::Ready {
        let restart_metadata = build_restart_metadata(note.as_deref());
        transition_service
            .transition_task_with_metadata(&task_id, new_status, Some(restart_metadata))
            .await
            .map_err(|e| e.to_string())?
    } else {
        transition_service
            .transition_task(&task_id, new_status)
            .await
            .map_err(|e| e.to_string())?
    };

    // If the task was already Ready and we requested Ready (Start button on Ready task),
    // transition_task is a no-op. Explicitly trigger the scheduler so plan_merge and
    // other Ready tasks get picked up.
    if old_status == InternalStatus::Ready && new_status == InternalStatus::Ready {
        tracing::info!(
            task_id = task_id.as_str(),
            "Ready→Ready self-transition detected, triggering scheduler"
        );
        task_scheduler.try_schedule_ready_tasks().await;
    }

    // Emit queue_changed event if the move affects Ready status
    if old_status == InternalStatus::Ready || new_status == InternalStatus::Ready {
        emit_queue_changed(&state, &project_id, &app).await;
    }

    Ok(TaskResponse::from(task))
}

/// Inject a task mid-loop
///
/// Allows users to add tasks during execution. Tasks can be sent to:
/// - **Backlog** (deferred): Task is created with Backlog status
/// - **Planned** (immediate queue): Task is created with Ready status at correct priority
///
/// If `make_next` is true and target is "planned", the task gets the highest
/// priority (max existing priority + 1000) to ensure it executes next.
///
/// # Arguments
/// * `input` - The inject input containing project_id, title, target, and make_next options
/// * `app` - Tauri app handle for event emission
///
/// # Returns
/// * `InjectTaskResponse` - Contains the created task, target, priority, and whether make_next was applied
#[tauri::command]
pub async fn inject_task(
    input: InjectTaskInput,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<InjectTaskResponse, String> {
    let project_id = ProjectId::from_string(input.project_id.clone());
    let category: TaskCategory = input
        .category
        .as_deref()
        .unwrap_or("regular")
        .parse()
        .unwrap_or(TaskCategory::Regular);

    // Create the new task
    let mut task = Task::new_with_category(project_id.clone(), input.title, category);

    if let Some(desc) = input.description {
        task.description = Some(desc);
    }

    // Determine initial status and priority based on target
    let (status, priority, make_next_applied) = match input.target.as_str() {
        "planned" => {
            if input.make_next {
                // Get max priority among Ready tasks and add 1000 for safe margin
                let ready_tasks = state
                    .task_repo
                    .get_by_status(&project_id, InternalStatus::Ready)
                    .await
                    .map_err(|e| e.to_string())?;

                let max_priority = ready_tasks.iter().map(|t| t.priority).max().unwrap_or(0);

                (InternalStatus::Ready, max_priority + 1000, true)
            } else {
                // Insert at default priority (0) - will be ordered by created_at
                (InternalStatus::Ready, 0, false)
            }
        }
        _ => {
            // Default to backlog
            (InternalStatus::Backlog, 0, false)
        }
    };

    task.internal_status = status;
    task.priority = priority;

    // Save the task
    let created = state
        .task_repo
        .create(task)
        .await
        .map_err(|e| e.to_string())?;

    // Emit task:created event
    let created_payload = serde_json::json!({
        "taskId": created.id.as_str(),
        "projectId": created.project_id.as_str(),
        "title": created.title,
        "status": created.internal_status.as_str(),
        "priority": created.priority,
        "injected": true,
    });
    if let Some(throttled) = app.try_state::<std::sync::Arc<crate::application::ThrottledEmitter>>()
    {
        throttled.emit("task:created", created_payload);
    } else {
        let _ = app.emit("task:created", created_payload);
    }

    let target = if input.target == "planned" {
        // Emit queue_changed since we're adding a task to Ready status
        emit_queue_changed(&state, &project_id, &app).await;
        "planned".to_string()
    } else {
        "backlog".to_string()
    };

    Ok(InjectTaskResponse {
        task: TaskResponse::from(created),
        target,
        priority,
        make_next_applied,
    })
}

/// Answer a user question from an agent
///
/// When an agent asks a question via the AskUserQuestion tool, the task
/// transitions to Blocked status. This command accepts the user's answer
/// and resumes the task by transitioning it to Ready status.
///
/// # Arguments
/// * `input` - The answer input containing task_id, selected_options, and optional custom_response
///
/// # Returns
/// * `AnswerUserQuestionResponse` - Contains the task_id, new status, and confirmation
///
/// # Errors
/// * Task not found
/// * Task is not in Blocked status
#[tauri::command]
pub async fn answer_user_question(
    input: AnswerUserQuestionInput,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<AnswerUserQuestionResponse, String> {
    let task_id = TaskId::from_string(input.task_id.clone());

    // Get the task
    let task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;

    // Verify task is in Blocked status
    if task.internal_status != InternalStatus::Blocked {
        return Err(format!(
            "Task {} is not in Blocked status (current: {})",
            task_id.as_str(),
            task.internal_status
        ));
    }

    let task_scheduler = build_task_scheduler(&state, &execution_state, &app);

    let transition_service =
        build_transition_service(&state, &execution_state, Some(&app))
            .with_task_scheduler(task_scheduler);

    let updated_task = transition_service
        .transition_task(&task_id, InternalStatus::Ready)
        .await
        .map_err(|e| e.to_string())?;

    // Note: The answer data (selected_options, custom_response) is not persisted to the database.
    // The frontend passes answers directly to the agent via the MCP protocol when resuming execution.
    // This keeps the backend stateless and avoids coupling task state to agent communication details.

    Ok(AnswerUserQuestionResponse {
        task_id: input.task_id,
        resumed_status: updated_task.internal_status.as_str().to_string(),
        answer_recorded: true,
    })
}

/// Archive a task (soft delete)
///
/// Sets the archived_at timestamp to now, effectively removing the task from
/// normal views while preserving it for potential restore.
///
/// # Arguments
/// * `task_id` - The task ID to archive
/// * `app` - Tauri app handle for event emission
///
/// # Returns
/// * `TaskResponse` - The archived task
///
/// # Events
/// * Emits 'task:archived' with { task_id, project_id }
#[tauri::command]
pub async fn archive_task(
    task_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<TaskResponse, String> {
    let task_id_obj = TaskId::from_string(task_id.clone());

    // Archive the task via repository
    let archived_task = state
        .task_repo
        .archive(&task_id_obj)
        .await
        .map_err(|e| e.to_string())?;

    // Emit event for real-time UI updates
    emit_task_lifecycle_event(
        &app,
        "task:archived",
        archived_task.id.as_str(),
        archived_task.project_id.as_str(),
    );

    Ok(TaskResponse::from(archived_task))
}

/// Restore an archived task
///
/// Clears the archived_at timestamp, making the task visible in normal views again.
///
/// # Arguments
/// * `task_id` - The task ID to restore
/// * `app` - Tauri app handle for event emission
///
/// # Returns
/// * `TaskResponse` - The restored task
///
/// # Events
/// * Emits 'task:restored' with { task_id, project_id }
#[tauri::command]
pub async fn restore_task(
    task_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<TaskResponse, String> {
    let task_id_obj = TaskId::from_string(task_id.clone());

    // Restore the task via repository
    let restored_task = state
        .task_repo
        .restore(&task_id_obj)
        .await
        .map_err(|e| e.to_string())?;

    // Emit event for real-time UI updates
    emit_task_lifecycle_event(
        &app,
        "task:restored",
        restored_task.id.as_str(),
        restored_task.project_id.as_str(),
    );

    Ok(TaskResponse::from(restored_task))
}

/// Block a task with an optional reason
///
/// Transitions the task to Blocked status and optionally records why it's blocked.
/// The blocked reason is displayed on the task card and can help track dependencies
/// or external blockers.
///
/// # Arguments
/// * `task_id` - The task ID to block
/// * `reason` - Optional reason why the task is blocked
/// * `app` - Tauri app handle for event emission
///
/// # Returns
/// * `TaskResponse` - The blocked task with updated status and reason
///
/// # Errors
/// * Task not found
/// * Invalid state transition (task cannot transition to Blocked from current status)
#[tauri::command]
pub async fn block_task(
    task_id: String,
    reason: Option<String>,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<TaskResponse, String> {
    tracing::info!(task_id = %task_id, reason = ?reason, "block_task command invoked");

    let task_id_obj = TaskId::from_string(task_id.clone());

    // Get the task first to capture project_id for events
    let task = state
        .task_repo
        .get_by_id(&task_id_obj)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id))?;

    let project_id = task.project_id.clone();

    // Create the task scheduler for auto-scheduling Ready tasks
    let task_scheduler = build_task_scheduler(&state, &execution_state, &app);

    // Create the transition service
    let transition_service =
        build_transition_service(&state, &execution_state, Some(&app))
            .with_task_scheduler(task_scheduler);

    // Transition to Blocked status
    let mut blocked_task = transition_service
        .transition_task(&task_id_obj, InternalStatus::Blocked)
        .await
        .map_err(|e| e.to_string())?;

    // Set the blocked reason (must update separately after transition)
    blocked_task.blocked_reason = reason;
    blocked_task.touch();

    state
        .task_repo
        .update(&blocked_task)
        .await
        .map_err(|e| e.to_string())?;

    // Emit queue_changed since the task was likely in Ready status
    emit_queue_changed(&state, &project_id, &app).await;

    Ok(TaskResponse::from(blocked_task))
}

/// Unblock a task
///
/// Transitions the task from Blocked to Ready status and clears the blocked reason.
/// If the task has dependencies in Failed status, the operation still succeeds but
/// the response includes a `warning` field so the caller can prompt the user.
///
/// # Arguments
/// * `task_id` - The task ID to unblock
/// * `app` - Tauri app handle for event emission
///
/// # Returns
/// * `UnblockTaskResponse` - The unblocked task with Ready status, plus an optional warning
///
/// # Errors
/// * Task not found
/// * Invalid state transition (task must be in Blocked status)
#[tauri::command]
pub async fn unblock_task(
    task_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<UnblockTaskResponse, String> {
    tracing::info!(task_id = %task_id, "unblock_task command invoked");

    let task_id_obj = TaskId::from_string(task_id.clone());

    // Get the task first to verify it's blocked and capture project_id
    let task = state
        .task_repo
        .get_by_id(&task_id_obj)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id))?;

    if task.internal_status != InternalStatus::Blocked {
        return Err(format!(
            "Task {} is not in Blocked status (current: {}). Cannot unblock.",
            task_id, task.internal_status
        ));
    }

    let project_id = task.project_id.clone();

    // Check for failed dependencies and prepare a warning if any exist.
    // The unblock still proceeds — this is a manual override — but the caller
    // should surface the warning so the user knows they may be building on broken output.
    let failed_dep_warning = {
        let blockers = state
            .task_dependency_repo
            .get_blockers(&task_id_obj)
            .await
            .unwrap_or_default();
        let mut failed_titles: Vec<String> = Vec::new();
        for blocker_id in blockers {
            if let Ok(Some(blocker)) = state.task_repo.get_by_id(&blocker_id).await {
                if blocker.internal_status == InternalStatus::Failed {
                    failed_titles.push(blocker.title);
                }
            }
        }
        if failed_titles.is_empty() {
            None
        } else {
            let names = failed_titles
                .iter()
                .map(|n| format!("\"{}\"", n))
                .collect::<Vec<_>>()
                .join(", ");
            let dep_word = if failed_titles.len() == 1 {
                "dependency"
            } else {
                "dependencies"
            };
            Some(format!(
                "Task has failed {dep_word}: {names}. Proceeding may produce broken output."
            ))
        }
    };

    // Create the task scheduler for auto-scheduling Ready tasks
    let task_scheduler = build_task_scheduler(&state, &execution_state, &app);

    // Create the transition service
    let transition_service =
        build_transition_service(&state, &execution_state, Some(&app))
            .with_task_scheduler(task_scheduler);

    // Transition to Ready status
    let mut unblocked_task = transition_service
        .transition_task(&task_id_obj, InternalStatus::Ready)
        .await
        .map_err(|e| e.to_string())?;

    // Clear the blocked reason
    unblocked_task.blocked_reason = None;
    unblocked_task.touch();

    state
        .task_repo
        .update(&unblocked_task)
        .await
        .map_err(|e| e.to_string())?;

    // Emit queue_changed since we're adding a task to Ready status
    emit_queue_changed(&state, &project_id, &app).await;

    Ok(UnblockTaskResponse {
        task: TaskResponse::from(unblocked_task),
        warning: failed_dep_warning,
    })
}

/// Clean archive a single task: force-stop agent if active, cleanup branch/worktree, archive in DB, emit events
///
/// This does not require the task to be archived first.
/// It handles full cleanup including stopping active agents and removing git resources.
/// Active tasks are transitioned to Stopped to trigger proper on_exit side effects.
///
/// # Arguments
/// * `task_id` - The task ID to clean archive
///
/// # Events
/// * Emits 'task:archived' with { task_id, project_id }
#[tauri::command]
pub async fn cleanup_task(
    task_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    use crate::application::TaskCleanupService;

    let task_id_obj = TaskId::from_string(task_id.clone());

    // Get task once — passed by reference to service to avoid double fetch
    let task = state
        .task_repo
        .get_by_id(&task_id_obj)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id))?;

    let project_id_str = task.project_id.as_str().to_string();

    let stopper = build_task_stopper(&state, &execution_state, &app);
    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        Some(app.clone()),
    )
    .with_interactive_process_registry(Arc::clone(&state.interactive_process_registry))
    .with_task_stopper(stopper);

    service
        .cleanup_task_ref(&task)
        .await
        .map_err(|e| e.to_string())?;

    emit_task_lifecycle_event(&app, "task:archived", &task_id, &project_id_str);

    Ok(())
}

/// Clean delete all tasks in a group: force-stop agents, cleanup branches, delete from DB, emit events
///
/// group_kind: "status" | "session" | "uncategorized"
/// group_id: the status name (e.g. "ready") or session ID (for "session"), ignored for "uncategorized"
/// project_id: required for all group kinds
///
/// Skips plan_merge tasks (system-managed).
/// Active tasks are transitioned to Stopped to trigger proper on_exit side effects.
///
/// # Events
/// * Emits 'task:list_changed' with { project_id } after bulk deletion
#[tauri::command]
pub async fn cleanup_tasks_in_group(
    group_kind: String,
    group_id: String,
    project_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<super::types::CleanupReportResponse, String> {
    use crate::application::{TaskCleanupService, TaskGroup};

    let group = match group_kind.as_str() {
        "status" => TaskGroup::Status {
            status: group_id,
            project_id: project_id.clone(),
        },
        "session" => TaskGroup::Session {
            session_id: group_id,
            project_id: project_id.clone(),
        },
        "uncategorized" => TaskGroup::Uncategorized {
            project_id: project_id.clone(),
        },
        _ => {
            return Err(format!(
                "Invalid group_kind: {}. Expected 'status', 'session', or 'uncategorized'",
                group_kind
            ))
        }
    };

    let stopper = build_task_stopper(&state, &execution_state, &app);
    let service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        Some(app.clone()),
    )
    .with_interactive_process_registry(Arc::clone(&state.interactive_process_registry))
    .with_task_stopper(stopper);

    let report = service
        .cleanup_tasks_in_group(group)
        .await
        .map_err(|e| e.to_string())?;

    // Emit task:list_changed for UI refresh
    let _ = app.emit(
        "task:list_changed",
        serde_json::json!({
            "projectId": project_id,
        }),
    );

    Ok(super::types::CleanupReportResponse {
        archived_count: report.archived_count(),
        failed_count: report.failed_count(),
        stopped_agents: report.stopped_agents(),
    })
}

// --- TaskStopper implementation backed by TaskTransitionService ---

use crate::application::TaskStopper;
use crate::error::AppResult;
use async_trait::async_trait;

/// Wraps a TaskTransitionService to implement the TaskStopper trait.
struct TransitionTaskStopper {
    transition_service: TaskTransitionService,
}

#[async_trait]
impl TaskStopper for TransitionTaskStopper {
    async fn transition_to_stopped(&self, task_id: &TaskId) -> AppResult<()> {
        self.transition_service
            .transition_task(task_id, InternalStatus::Stopped)
            .await
            .map(|_| ())
    }

    async fn transition_to_stopped_with_context(
        &self,
        task_id: &TaskId,
        from_status: InternalStatus,
        reason: Option<String>,
    ) -> AppResult<()> {
        self.transition_service
            .transition_to_stopped_with_context(task_id, from_status, reason)
            .await
            .map(|_| ())
    }
}

#[cfg(test)]
mod update_task_validation_tests {
    use super::{validate_update_task_input, UpdateTaskInput};

    #[test]
    fn rejects_status_edits_through_update_task() {
        let input = UpdateTaskInput {
            title: Some("Updated".to_string()),
            description: None,
            category: None,
            priority: None,
            internal_status: Some("ready".to_string()),
        };

        let error = validate_update_task_input(&input).expect_err("status edits must be rejected");
        assert!(error.contains("move_task"));
    }

    #[test]
    fn allows_regular_field_edits_through_update_task() {
        let input = UpdateTaskInput {
            title: Some("Updated".to_string()),
            description: Some("Desc".to_string()),
            category: Some("bug".to_string()),
            priority: Some(3),
            internal_status: None,
        };

        validate_update_task_input(&input).expect("non-status edits should remain allowed");
    }
}

/// Build a TaskStopper from the standard Tauri state dependencies.
fn build_task_stopper(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    app: &tauri::AppHandle,
) -> Arc<dyn TaskStopper> {
    let transition_service = build_transition_service(state, execution_state, Some(app));

    Arc::new(TransitionTaskStopper { transition_service })
}

/// Pause a specific task
/// Transitions the task to Paused state, which can be resumed later
#[tauri::command]
pub async fn pause_task(
    task_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
) -> Result<TaskResponse, String> {
    let task_id = TaskId::from_string(task_id);

    // Verify task exists
    let task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;

    // Store PauseReason::UserInitiated metadata before transitioning
    let pause_reason = crate::application::chat_service::PauseReason::UserInitiated {
        previous_status: task.internal_status.to_string(),
        paused_at: chrono::Utc::now().to_rfc3339(),
        scope: "task".to_string(),
    };
    let mut task_to_update = task.clone();
    task_to_update.metadata =
        Some(pause_reason.write_to_task_metadata(task_to_update.metadata.as_deref()));
    task_to_update.touch();
    let _ = state.task_repo.update(&task_to_update).await;

    // Build transition service
    let transition_service = build_transition_service(&state, &execution_state, None);

    // Transition to Paused
    let updated_task = transition_service
        .transition_task(&task_id, InternalStatus::Paused)
        .await
        .map_err(|e| e.to_string())?;

    // Emit lifecycle event
    if let Some(ref app) = state.app_handle {
        emit_task_lifecycle_event(
            app,
            "task:paused",
            updated_task.id.as_str(),
            updated_task.project_id.as_str(),
        );
    }

    Ok(TaskResponse::from(updated_task))
}

/// Stop a specific task
/// Transitions the task to Stopped state (terminal, requires manual restart)
///
/// # Arguments
/// * `task_id` - The task ID
/// * `reason` - Optional reason for stopping (captured in stop metadata for smart resume)
///
/// # Returns
/// * `TaskResponse` - The stopped task
#[tauri::command]
pub async fn stop_task(
    task_id: String,
    reason: Option<String>,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
) -> Result<TaskResponse, String> {
    let task_id = TaskId::from_string(task_id);

    // Get task to capture current status before stopping
    let task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;

    let from_status = task.internal_status;

    // Build transition service
    let transition_service = build_transition_service(&state, &execution_state, None);

    // Transition to Stopped with context capture
    let updated_task = transition_service
        .transition_to_stopped_with_context(&task_id, from_status, reason.clone())
        .await
        .map_err(|e| e.to_string())?;

    // Emit lifecycle event with stop context
    if let Some(ref app) = state.app_handle {
        app.emit(
            "task:stopped",
            serde_json::json!({
                "taskId": updated_task.id.as_str(),
                "projectId": updated_task.project_id.as_str(),
                "stoppedFromStatus": from_status.as_str(),
                "stopReason": reason,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            }),
        )
        .map_err(|e| format!("Failed to emit task:stopped event: {}", e))?;
    }

    Ok(TaskResponse::from(updated_task))
}

/// Cancel all tasks in a group (group_kind: "status" | "session" | "uncategorized")
///
/// Transitions all non-terminal tasks in the group to Cancelled status.
/// This is a non-destructive alternative to cleanup_tasks_in_group.
/// Returns count of cancelled tasks.
#[tauri::command]
pub async fn cancel_tasks_in_group(
    group_kind: String,
    group_id: String,
    project_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<super::types::BulkCancelResponse, String> {
    let project_id_obj = ProjectId::from_string(project_id.clone());

    // Determine the group and fetch tasks
    let tasks = match group_kind.as_str() {
        "status" => {
            let internal_status: InternalStatus = group_id
                .parse()
                .map_err(|_| format!("Invalid status: {}", group_id))?;
            state
                .task_repo
                .get_by_status(&project_id_obj, internal_status)
                .await
                .map_err(|e| e.to_string())?
        }
        "session" => {
            let session_id = crate::domain::entities::IdeationSessionId::from_string(group_id);
            state
                .task_repo
                .get_by_ideation_session(&session_id)
                .await
                .map_err(|e| e.to_string())?
        }
        "uncategorized" => {
            let all_tasks = state
                .task_repo
                .get_by_project(&project_id_obj)
                .await
                .map_err(|e| e.to_string())?;
            all_tasks
                .into_iter()
                .filter(|t| t.ideation_session_id.is_none())
                .collect()
        }
        _ => {
            return Err(format!(
                "Invalid group_kind: {}. Expected 'status', 'session', or 'uncategorized'",
                group_kind
            ))
        }
    };

    // Build transition service
    let transition_service = build_transition_service(&state, &execution_state, Some(&app));

    let mut cancelled_count = 0;

    // Cancel each non-terminal task
    for task in tasks {
        if task.internal_status.is_terminal() {
            continue; // Skip already-terminal tasks
        }

        match transition_service
            .transition_task(&task.id, InternalStatus::Cancelled)
            .await
        {
            Ok(cancelled_task) => {
                emit_task_lifecycle_event(
                    &app,
                    "task:cancelled",
                    cancelled_task.id.as_str(),
                    cancelled_task.project_id.as_str(),
                );
                cancelled_count += 1;
            }
            Err(e) => {
                tracing::warn!(
                    task_id = %task.id,
                    error = %e,
                    "Failed to cancel task in group"
                );
                // Continue with next task rather than failing completely
            }
        }
    }

    // Emit task:list_changed for UI refresh
    let _ = app.emit(
        "task:list_changed",
        serde_json::json!({
            "projectId": project_id,
        }),
    );

    Ok(super::types::BulkCancelResponse { cancelled_count })
}

/// Resume a single paused task back to its pre-pause status.
///
/// Reads pause_reason metadata to determine the previous status, falls back to
/// status_history lookup. Clears pause metadata and re-executes entry actions
/// to respawn the agent.
///
/// # Arguments
/// * `task_id` - The task ID to resume
///
/// # Returns
/// * `TaskResponse` - The resumed task
#[tauri::command]
pub async fn resume_task(
    task_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<TaskResponse, String> {
    let task_id = TaskId::from_string(task_id);

    // Get the paused task
    let task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;

    if task.internal_status != InternalStatus::Paused {
        return Err(format!(
            "Task {} is not paused (current status: {})",
            task_id.as_str(),
            task.internal_status.as_str()
        ));
    }

    // Determine restore status: prefer pause_reason metadata, fall back to status_history
    let restore_status =
        if let Some(reason) = PauseReason::from_task_metadata(task.metadata.as_deref()) {
            match reason.previous_status().parse::<InternalStatus>() {
                Ok(status) => status,
                Err(_) => {
                    tracing::warn!(
                        task_id = task_id.as_str(),
                        previous_status = reason.previous_status(),
                        "Invalid previous_status in pause metadata, falling back to history"
                    );
                    // Fall back to status history
                    get_restore_status_from_history(&state, &task_id).await?
                }
            }
        } else {
            get_restore_status_from_history(&state, &task_id).await?
        };

    if !execution_state.can_start_any_execution_context() {
        return Err("Cannot resume: max concurrent task limit reached".to_string());
    }
    if !project_has_execution_capacity_for_state(&state, &execution_state, &task.project_id).await?
    {
        return Err("Cannot resume: project execution capacity reached".to_string());
    }

    // Build transition service
    let task_scheduler = build_task_scheduler(&state, &execution_state, &app);

    let transition_service = build_transition_service(&state, &execution_state, Some(&app))
        .with_task_scheduler(task_scheduler);

    // Transition to restore status
    transition_service
        .transition_task(&task_id, restore_status)
        .await
        .map_err(|e| e.to_string())?;

    // Fetch fresh task, clear metadata, execute entry actions
    let mut restored_task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found after transition: {}", task_id.as_str()))?;

    // Mark this as a continuation resume before re-running entry actions so
    // fresh execution/merge spawns preserve prior harness lineage.
    prepare_resumed_task_for_entry_actions(&mut restored_task);
    restored_task.touch();
    state
        .task_repo
        .update(&restored_task)
        .await
        .map_err(|e| e.to_string())?;

    // Re-execute entry actions to respawn agent
    transition_service
        .execute_entry_actions(&task_id, &restored_task, restore_status)
        .await;

    // Emit lifecycle event
    emit_task_lifecycle_event(
        &app,
        "task:resumed",
        restored_task.id.as_str(),
        restored_task.project_id.as_str(),
    );

    tracing::info!(
        task_id = task_id.as_str(),
        restored_to = ?restore_status,
        "Successfully resumed paused task"
    );

    Ok(TaskResponse::from(restored_task))
}

/// Pause all tasks in a group (group_kind: "status" | "session" | "uncategorized")
///
/// Transitions all non-terminal, non-paused tasks to Paused status.
/// Writes PauseReason::UserInitiated metadata before each transition.
/// Returns count of paused tasks.
#[tauri::command]
pub async fn pause_tasks_in_group(
    group_kind: String,
    group_id: String,
    project_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<super::types::BulkPauseResponse, String> {
    let project_id_obj = ProjectId::from_string(project_id.clone());

    let tasks = match group_kind.as_str() {
        "status" => {
            let internal_status: InternalStatus = group_id
                .parse()
                .map_err(|_| format!("Invalid status: {}", group_id))?;
            state
                .task_repo
                .get_by_status(&project_id_obj, internal_status)
                .await
                .map_err(|e| e.to_string())?
        }
        "session" => {
            let session_id = crate::domain::entities::IdeationSessionId::from_string(group_id);
            state
                .task_repo
                .get_by_ideation_session(&session_id)
                .await
                .map_err(|e| e.to_string())?
        }
        "uncategorized" => {
            let all_tasks = state
                .task_repo
                .get_by_project(&project_id_obj)
                .await
                .map_err(|e| e.to_string())?;
            all_tasks
                .into_iter()
                .filter(|t| t.ideation_session_id.is_none())
                .collect()
        }
        _ => {
            return Err(format!(
                "Invalid group_kind: {}. Expected 'status', 'session', or 'uncategorized'",
                group_kind
            ))
        }
    };

    let transition_service = build_transition_service(&state, &execution_state, Some(&app));

    let mut paused_count = 0;

    for task in tasks {
        if task.internal_status.is_terminal() || task.internal_status == InternalStatus::Paused {
            continue;
        }

        let pause_reason = crate::application::chat_service::PauseReason::UserInitiated {
            previous_status: task.internal_status.to_string(),
            paused_at: chrono::Utc::now().to_rfc3339(),
            scope: "task".to_string(),
        };
        let mut task_to_update = task.clone();
        task_to_update.metadata =
            Some(pause_reason.write_to_task_metadata(task_to_update.metadata.as_deref()));
        task_to_update.touch();
        let _ = state.task_repo.update(&task_to_update).await;

        match transition_service
            .transition_task(&task.id, InternalStatus::Paused)
            .await
        {
            Ok(paused_task) => {
                emit_task_lifecycle_event(
                    &app,
                    "task:paused",
                    paused_task.id.as_str(),
                    paused_task.project_id.as_str(),
                );
                paused_count += 1;
            }
            Err(e) => {
                tracing::warn!(
                    task_id = %task.id,
                    error = %e,
                    "Failed to pause task in group"
                );
            }
        }
    }

    let _ = app.emit(
        "task:list_changed",
        serde_json::json!({ "projectId": project_id }),
    );

    Ok(super::types::BulkPauseResponse { paused_count })
}

/// Resume all paused tasks in a group (group_kind: "status" | "session" | "uncategorized")
///
/// Transitions all Paused tasks back to their pre-pause status.
/// Reads PauseReason metadata to determine previous status, falls back to status history.
/// Returns count of resumed tasks.
#[tauri::command]
pub async fn resume_tasks_in_group(
    group_kind: String,
    group_id: String,
    project_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<super::types::BulkResumeResponse, String> {
    use crate::application::chat_service::PauseReason;

    let project_id_obj = ProjectId::from_string(project_id.clone());

    let tasks = match group_kind.as_str() {
        "status" => {
            let internal_status: InternalStatus = group_id
                .parse()
                .map_err(|_| format!("Invalid status: {}", group_id))?;
            state
                .task_repo
                .get_by_status(&project_id_obj, internal_status)
                .await
                .map_err(|e| e.to_string())?
        }
        "session" => {
            let session_id = crate::domain::entities::IdeationSessionId::from_string(group_id);
            state
                .task_repo
                .get_by_ideation_session(&session_id)
                .await
                .map_err(|e| e.to_string())?
        }
        "uncategorized" => {
            let all_tasks = state
                .task_repo
                .get_by_project(&project_id_obj)
                .await
                .map_err(|e| e.to_string())?;
            all_tasks
                .into_iter()
                .filter(|t| t.ideation_session_id.is_none())
                .collect()
        }
        _ => {
            return Err(format!(
                "Invalid group_kind: {}. Expected 'status', 'session', or 'uncategorized'",
                group_kind
            ))
        }
    };

    let paused_tasks: Vec<_> = tasks
        .into_iter()
        .filter(|t| t.internal_status == InternalStatus::Paused)
        .collect();

    let task_scheduler = build_task_scheduler(&state, &execution_state, &app);

    let transition_service = build_transition_service(&state, &execution_state, Some(&app))
        .with_task_scheduler(task_scheduler);

    let mut resumed_count = 0;

    for task in paused_tasks {
        if !execution_state.can_start_any_execution_context() {
            tracing::warn!(task_id = %task.id, "Skipping resume: max concurrent task limit reached");
            continue;
        }
        if !project_has_execution_capacity_for_state(&state, &execution_state, &task.project_id)
            .await?
        {
            tracing::warn!(
                task_id = %task.id,
                project_id = %task.project_id,
                "Skipping resume: project execution capacity reached"
            );
            continue;
        }

        let restore_status =
            if let Some(reason) = PauseReason::from_task_metadata(task.metadata.as_deref()) {
                match reason.previous_status().parse::<InternalStatus>() {
                    Ok(status) => status,
                    Err(_) => match get_restore_status_from_history(&state, &task.id).await {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!(task_id = %task.id, error = %e, "Cannot restore task");
                            continue;
                        }
                    },
                }
            } else {
                match get_restore_status_from_history(&state, &task.id).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(task_id = %task.id, error = %e, "Cannot restore task");
                        continue;
                    }
                }
            };

        if let Err(e) = transition_service
            .transition_task(&task.id, restore_status)
            .await
        {
            tracing::warn!(task_id = %task.id, error = %e, "Failed to resume task in group");
            continue;
        }

        let updated_task = match state.task_repo.get_by_id(&task.id).await {
            Ok(Some(t)) => t,
            _ => continue,
        };

        let mut restored_task = updated_task;
        prepare_resumed_task_for_entry_actions(&mut restored_task);
        restored_task.touch();
        let _ = state.task_repo.update(&restored_task).await;

        transition_service
            .execute_entry_actions(&task.id, &restored_task, restore_status)
            .await;

        emit_task_lifecycle_event(
            &app,
            "task:resumed",
            restored_task.id.as_str(),
            restored_task.project_id.as_str(),
        );

        resumed_count += 1;
    }

    let _ = app.emit(
        "task:list_changed",
        serde_json::json!({ "projectId": project_id }),
    );

    Ok(super::types::BulkResumeResponse { resumed_count })
}

/// Archive all tasks in a group (group_kind: "status" | "session" | "uncategorized")
///
/// Archives all non-archived tasks in the group.
/// Returns count of archived tasks.
#[tauri::command]
pub async fn archive_tasks_in_group(
    group_kind: String,
    group_id: String,
    project_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<super::types::BulkArchiveResponse, String> {
    let project_id_obj = ProjectId::from_string(project_id.clone());

    let tasks = match group_kind.as_str() {
        "status" => {
            let internal_status: InternalStatus = group_id
                .parse()
                .map_err(|_| format!("Invalid status: {}", group_id))?;
            state
                .task_repo
                .get_by_status(&project_id_obj, internal_status)
                .await
                .map_err(|e| e.to_string())?
        }
        "session" => {
            let session_id = crate::domain::entities::IdeationSessionId::from_string(group_id);
            state
                .task_repo
                .get_by_ideation_session(&session_id)
                .await
                .map_err(|e| e.to_string())?
        }
        "uncategorized" => {
            let all_tasks = state
                .task_repo
                .get_by_project(&project_id_obj)
                .await
                .map_err(|e| e.to_string())?;
            all_tasks
                .into_iter()
                .filter(|t| t.ideation_session_id.is_none())
                .collect()
        }
        _ => {
            return Err(format!(
                "Invalid group_kind: {}. Expected 'status', 'session', or 'uncategorized'",
                group_kind
            ))
        }
    };

    let mut archived_count = 0;

    for task in tasks {
        if task.archived_at.is_some() {
            continue;
        }

        match state.task_repo.archive(&task.id).await {
            Ok(archived_task) => {
                emit_task_lifecycle_event(
                    &app,
                    "task:archived",
                    archived_task.id.as_str(),
                    archived_task.project_id.as_str(),
                );
                archived_count += 1;
            }
            Err(e) => {
                tracing::warn!(
                    task_id = %task.id,
                    error = %e,
                    "Failed to archive task in group"
                );
            }
        }
    }

    let _ = app.emit(
        "task:list_changed",
        serde_json::json!({ "projectId": project_id }),
    );

    Ok(super::types::BulkArchiveResponse { archived_count })
}

/// Helper: get restore status from status_history for a paused task
async fn get_restore_status_from_history(
    state: &AppState,
    task_id: &TaskId,
) -> Result<InternalStatus, String> {
    let status_history = state
        .task_repo
        .get_status_history(task_id)
        .await
        .map_err(|e| e.to_string())?;

    let pause_transition = status_history
        .iter()
        .rev()
        .find(|t| t.to == InternalStatus::Paused);

    match pause_transition {
        Some(transition) => Ok(transition.from),
        None => Err(format!(
            "No pause transition found in history for task {}",
            task_id.as_str()
        )),
    }
}

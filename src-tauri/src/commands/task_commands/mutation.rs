// Mutation (write) handlers for task_commands module

use super::helpers::{emit_queue_changed, emit_task_lifecycle_event};
use super::types::{
    AnswerUserQuestionInput, AnswerUserQuestionResponse, CreateTaskInput, InjectTaskInput,
    InjectTaskResponse, TaskResponse, UnblockTaskResponse, UpdateTaskInput,
};
use crate::application::execution_control::project_has_execution_capacity_for_state;
use crate::application::task_restart::build_terminal_ready_restart_plan;
use crate::application::task_resume_execution::{
    branch_update_status, build_task_scheduler, build_transition_service, resume_task_for_state,
    resume_tasks_in_group_for_state,
};
use crate::application::{AppState, TaskTransitionService};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    BranchUpdatePhase, ChatContextType,
    ExecutionPlanId, IdeationSessionId, InternalStatus, ProjectId, Task, TaskCategory, TaskId,
};
use crate::domain::repositories::{
    BranchUpdateCasOutcome, PauseBranchUpdate, RetryBranchUpdate,
    StopBranchUpdate,
};
use crate::domain::services::{QueueKey, RunningAgentKey};
use crate::domain::state_machine::transition_handler::metadata_builder::build_restart_metadata;
use std::sync::Arc;
use tauri::{Emitter, Manager, State};

fn validate_update_task_input(input: &UpdateTaskInput) -> Result<(), String> {
    if input.internal_status.is_some() {
        return Err(
            "Task status updates must use move_task or the dedicated lifecycle commands"
                .to_string(),
        );
    }

    Ok(())
}

fn non_empty_input(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

async fn stop_branch_update_runtime(state: &AppState, task_id: &TaskId) {
    let context = ChatContextType::BranchUpdate;
    let ipr_key = crate::application::interactive_process_registry::InteractiveProcessKey::new(
        context.to_string(),
        task_id.as_str(),
    );
    state.interactive_process_registry.remove(&ipr_key).await;
    let running_key = RunningAgentKey::new(context.to_string(), task_id.as_str());
    let _ = state.running_agent_registry.stop(&running_key).await;
    let queue_key = QueueKey::new(context, task_id.as_str());
    state.message_queue.clear_with_key(&queue_key);
    if let Err(error) = state.queued_message_repo.clear(&queue_key).await {
        tracing::warn!(
            task_id = task_id.as_str(),
            error = %error,
            "Failed to clear durable branch-update queue after control transition"
        );
    }
}

async fn attach_create_task_plan_scope(
    task: &mut Task,
    input: &CreateTaskInput,
    state: &AppState,
) -> Result<(), String> {
    if let Some(execution_plan_id_str) = non_empty_input(&input.execution_plan_id) {
        let execution_plan_id = ExecutionPlanId::from_string(execution_plan_id_str.to_string());
        let execution_plan = state
            .execution_plan_repo
            .get_by_id(&execution_plan_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Execution plan not found: {}", execution_plan_id.as_str()))?;

        if let Some(requested_session_id) = non_empty_input(&input.ideation_session_id) {
            if requested_session_id != execution_plan.session_id.as_str() {
                return Err(format!(
                    "Execution plan {} belongs to session {}, not {}",
                    execution_plan_id.as_str(),
                    execution_plan.session_id.as_str(),
                    requested_session_id
                ));
            }
        }

        let session = state
            .ideation_session_repo
            .get_by_id(&execution_plan.session_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| {
                format!(
                    "Ideation session not found for execution plan {}: {}",
                    execution_plan_id.as_str(),
                    execution_plan.session_id.as_str()
                )
            })?;

        if session.project_id.as_str() != task.project_id.as_str() {
            return Err(format!(
                "Execution plan {} belongs to project {}, not {}",
                execution_plan_id.as_str(),
                session.project_id.as_str(),
                task.project_id.as_str()
            ));
        }

        task.ideation_session_id = Some(execution_plan.session_id.clone());
        task.execution_plan_id = Some(execution_plan.id.clone());
        task.plan_artifact_id = session
            .plan_artifact_id
            .clone()
            .or_else(|| session.inherited_plan_artifact_id.clone());
        return Ok(());
    }

    if let Some(session_id_str) = non_empty_input(&input.ideation_session_id) {
        let session_id = IdeationSessionId::from_string(session_id_str.to_string());
        let session = state
            .ideation_session_repo
            .get_by_id(&session_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Ideation session not found: {}", session_id.as_str()))?;

        if session.project_id.as_str() != task.project_id.as_str() {
            return Err(format!(
                "Ideation session {} belongs to project {}, not {}",
                session_id.as_str(),
                session.project_id.as_str(),
                task.project_id.as_str()
            ));
        }

        task.ideation_session_id = Some(session.id.clone());
        task.plan_artifact_id = session
            .plan_artifact_id
            .clone()
            .or_else(|| session.inherited_plan_artifact_id.clone());
    }

    Ok(())
}

async fn authorize_task_mutation(state: &AppState, task: &Task) -> Result<(), String> {
    crate::application::tasks_feature_policy::TasksFeaturePolicy::from_state(state)
        .authorize_session(
            task.ideation_session_id.as_ref(),
            crate::domain::ideation::TasksFeatureAction::HistoryMutation,
        )
        .await
        .map_err(|error| error.to_string())
}

/// Create a new task
#[tauri::command]
pub async fn create_task(
    input: CreateTaskInput,
    state: State<'_, AppState>,
) -> Result<TaskResponse, String> {
    let project_id = ProjectId::from_string(input.project_id.clone());
    let category: TaskCategory = input
        .category
        .as_deref()
        .unwrap_or("regular")
        .parse()
        .unwrap_or(TaskCategory::Regular);

    let mut task = Task::new_with_category(project_id, input.title.clone(), category);

    if let Some(desc) = &input.description {
        task.description = Some(desc.clone());
    }
    if let Some(priority) = input.priority {
        task.priority = priority;
    }
    attach_create_task_plan_scope(&mut task, &input, &state).await?;
    authorize_task_mutation(&state, &task).await?;

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
    authorize_task_mutation(&state, &task).await?;

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
    let task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;
    authorize_task_mutation(&state, &task).await?;
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
    let feature_action = if matches!(
        new_status,
        InternalStatus::Paused | InternalStatus::Stopped | InternalStatus::Cancelled
    ) {
        crate::domain::ideation::TasksFeatureAction::Quiesce
    } else {
        crate::domain::ideation::TasksFeatureAction::Progress
    };
    crate::application::tasks_feature_policy::TasksFeaturePolicy::from_state(&state)
        .authorize_session(old_task.ideation_session_id.as_ref(), feature_action)
        .await
        .map_err(|error| error.to_string())?;

    let old_status = old_task.internal_status;
    let project_id = old_task.project_id.clone();

    // Terminal→Ready restarts are planned without writes, then committed below with
    // cleanup, failed-step resets, Ready status, and history in one repository transaction.
    let terminal_restart_plan = if old_status.is_terminal() && new_status == InternalStatus::Ready {
        build_terminal_ready_restart_plan(&state.task_step_repo, &old_task)
            .await
            .map_err(|error| format!("Failed to prepare task restart: {error}"))?
    } else {
        None
    };

    // Create the task scheduler for auto-scheduling Ready tasks
    let task_scheduler = build_task_scheduler(&state, &execution_state, &app);

    // Create the transition service with all required dependencies
    let mut transition_service = build_transition_service(&state, &execution_state, Some(&app))
        .with_task_scheduler(Arc::clone(&task_scheduler));
    transition_service = transition_service.with_step_repo(Arc::clone(&state.task_step_repo));

    // Transition the task - this triggers entry actions like spawning workers!
    // When a note is provided and the task is moving to Ready (restart/reopen flow),
    // store it as restart_note in metadata so the re-executing agent can read it.
    let task = if let Some(plan) = terminal_restart_plan {
        transition_service
            .restart_terminal_task_to_ready(plan, Some(build_restart_metadata(note.as_deref())))
            .await
            .map_err(|e| e.to_string())?
    } else if note.is_some() && new_status == InternalStatus::Ready {
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
    crate::application::tasks_feature_policy::TasksFeaturePolicy::from_state(&state)
        .authorize_session(None, crate::domain::ideation::TasksFeatureAction::Progress)
        .await
        .map_err(|error| error.to_string())?;
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

    let transition_service = build_transition_service(&state, &execution_state, Some(&app))
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
    let task = state
        .task_repo
        .get_by_id(&task_id_obj)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id_obj.as_str()))?;
    authorize_task_mutation(&state, &task).await?;

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
    let task = state
        .task_repo
        .get_by_id(&task_id_obj)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id_obj.as_str()))?;
    authorize_task_mutation(&state, &task).await?;

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
    let transition_service = build_transition_service(&state, &execution_state, Some(&app))
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
    let transition_service = build_transition_service(&state, &execution_state, Some(&app))
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
    use super::{
        attach_create_task_plan_scope, validate_update_task_input, CreateTaskInput, TaskResponse,
        UpdateTaskInput,
    };
    use crate::application::AppState;
    use crate::domain::entities::{
        ExecutionPlan, IdeationSession, IdeationSessionId, Project, ProjectId, Task,
    };
    use crate::domain::repositories::ProjectRepository;
    use crate::infrastructure::memory::{MemoryProjectRepository, MemoryTaskRepository};
    use std::sync::Arc;

    async fn setup_test_state() -> AppState {
        let task_repo = Arc::new(MemoryTaskRepository::new());
        let project_repo = Arc::new(MemoryProjectRepository::new());
        let project = Project::new("Test Project".to_string(), "/test/path".to_string());
        project_repo.create(project).await.unwrap();

        AppState::with_repos(task_repo, project_repo)
    }

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

    #[tokio::test]
    async fn attaches_execution_plan_scope() {
        let state = setup_test_state().await;
        let project_id = ProjectId::from_string("test-project".to_string());
        let session = state
            .ideation_session_repo
            .create(IdeationSession::new(project_id.clone()))
            .await
            .unwrap();
        let execution_plan = state
            .execution_plan_repo
            .create(ExecutionPlan::new(session.id.clone()))
            .await
            .unwrap();
        let mut task = Task::new(project_id, "Scoped task".to_string());
        let input = CreateTaskInput {
            project_id: "test-project".to_string(),
            title: "Scoped task".to_string(),
            category: None,
            description: None,
            priority: None,
            steps: None,
            ideation_session_id: None,
            execution_plan_id: Some(execution_plan.id.as_str().to_string()),
        };

        attach_create_task_plan_scope(&mut task, &input, &state)
            .await
            .unwrap();

        assert_eq!(task.ideation_session_id, Some(session.id));
        assert_eq!(task.execution_plan_id, Some(execution_plan.id));
    }

    #[tokio::test]
    async fn rejects_execution_plan_session_mismatch() {
        let state = setup_test_state().await;
        let project_id = ProjectId::from_string("test-project".to_string());
        let session = state
            .ideation_session_repo
            .create(IdeationSession::new(project_id.clone()))
            .await
            .unwrap();
        let other_session_id = IdeationSessionId::new();
        let execution_plan = state
            .execution_plan_repo
            .create(ExecutionPlan::new(session.id))
            .await
            .unwrap();
        let mut task = Task::new(project_id, "Scoped task".to_string());
        let input = CreateTaskInput {
            project_id: "test-project".to_string(),
            title: "Scoped task".to_string(),
            category: None,
            description: None,
            priority: None,
            steps: None,
            ideation_session_id: Some(other_session_id.as_str().to_string()),
            execution_plan_id: Some(execution_plan.id.as_str().to_string()),
        };

        let err = attach_create_task_plan_scope(&mut task, &input, &state)
            .await
            .unwrap_err();

        assert!(err.contains("belongs to session"));
    }

    #[tokio::test]
    async fn rejects_execution_plan_project_mismatch() {
        let state = setup_test_state().await;
        let session_project_id = ProjectId::from_string("test-project".to_string());
        let task_project_id = ProjectId::from_string("other-project".to_string());
        let session = state
            .ideation_session_repo
            .create(IdeationSession::new(session_project_id))
            .await
            .unwrap();
        let execution_plan = state
            .execution_plan_repo
            .create(ExecutionPlan::new(session.id))
            .await
            .unwrap();
        let mut task = Task::new(task_project_id, "Scoped task".to_string());
        let input = CreateTaskInput {
            project_id: "other-project".to_string(),
            title: "Scoped task".to_string(),
            category: None,
            description: None,
            priority: None,
            steps: None,
            ideation_session_id: None,
            execution_plan_id: Some(execution_plan.id.as_str().to_string()),
        };

        let err = attach_create_task_plan_scope(&mut task, &input, &state)
            .await
            .unwrap_err();

        assert!(err.contains("belongs to project"));
    }

    #[tokio::test]
    async fn attaches_session_only_scope() {
        let state = setup_test_state().await;
        let project_id = ProjectId::from_string("test-project".to_string());
        let session = state
            .ideation_session_repo
            .create(IdeationSession::new(project_id.clone()))
            .await
            .unwrap();
        let mut task = Task::new(project_id, "Scoped task".to_string());
        let input = CreateTaskInput {
            project_id: "test-project".to_string(),
            title: "Scoped task".to_string(),
            category: None,
            description: None,
            priority: None,
            steps: None,
            ideation_session_id: Some(session.id.as_str().to_string()),
            execution_plan_id: None,
        };

        attach_create_task_plan_scope(&mut task, &input, &state)
            .await
            .unwrap();

        assert_eq!(task.ideation_session_id, Some(session.id));
        assert!(task.execution_plan_id.is_none());
    }

    #[test]
    fn task_response_includes_execution_plan_id() {
        let project_id = ProjectId::from_string("test-project".to_string());
        let session = IdeationSession::new(project_id.clone());
        let execution_plan = ExecutionPlan::new(session.id.clone());
        let mut task = Task::new(project_id, "Scoped task".to_string());
        task.ideation_session_id = Some(session.id);
        task.execution_plan_id = Some(execution_plan.id.clone());

        let response = TaskResponse::from(task);

        assert_eq!(
            response.execution_plan_id,
            Some(execution_plan.id.as_str().to_string())
        );
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

    if matches!(
        task.internal_status,
        InternalStatus::UpdatingPlanBranch | InternalStatus::UpdatingTaskBranch
    ) {
        let operation = state
            .branch_update_repo
            .get_active_operation(&task.id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Branch-update task has no active operation".to_string())?;
        let expected_status = branch_update_status(operation.direction);
        if task.internal_status != expected_status {
            return Err("Branch-update direction/status authority mismatch".to_string());
        }
        let lease = state
            .branch_update_repo
            .get_target_lease(&operation.target_identity)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Branch-update target authority is missing".to_string())?;
        let outcome = state
            .branch_update_repo
            .pause_operation(PauseBranchUpdate {
                operation_id: operation.id,
                task_id: task.id.clone(),
                originating_history_id: operation.originating_history_id,
                update_status: expected_status,
                owner: lease.owner().clone(),
                fencing_epoch: lease.fencing_epoch(),
                history_id: uuid::Uuid::new_v4().to_string(),
                task_metadata: None,
            })
            .await
            .map_err(|error| error.to_string())?;
        if outcome != BranchUpdateCasOutcome::Applied {
            return Err(format!("Branch-update pause lost authority: {outcome:?}"));
        }
        stop_branch_update_runtime(&state, &task.id).await;
        let paused = state
            .task_repo
            .get_by_id(&task.id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Paused branch-update task disappeared".to_string())?;
        if let Some(ref app) = state.app_handle {
            emit_task_lifecycle_event(
                app,
                "task:paused",
                paused.id.as_str(),
                paused.project_id.as_str(),
            );
        }
        return Ok(TaskResponse::from(paused));
    }

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

    if matches!(
        from_status,
        InternalStatus::UpdatingPlanBranch | InternalStatus::UpdatingTaskBranch
    ) {
        let operation = state
            .branch_update_repo
            .get_active_operation(&task.id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Branch-update task has no active operation".to_string())?;
        let expected_status = branch_update_status(operation.direction);
        if from_status != expected_status {
            return Err("Branch-update direction/status authority mismatch".to_string());
        }
        let lease = state
            .branch_update_repo
            .get_target_lease(&operation.target_identity)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Branch-update target authority is missing".to_string())?;
        let outcome = state
            .branch_update_repo
            .stop_operation(StopBranchUpdate {
                operation_id: operation.id,
                task_id: task.id.clone(),
                originating_history_id: operation.originating_history_id,
                update_status: expected_status,
                owner: lease.owner().clone(),
                fencing_epoch: lease.fencing_epoch(),
                history_id: uuid::Uuid::new_v4().to_string(),
                reason: reason.clone(),
            })
            .await
            .map_err(|error| error.to_string())?;
        if outcome != BranchUpdateCasOutcome::Applied {
            return Err(format!("Branch-update stop lost authority: {outcome:?}"));
        }
        stop_branch_update_runtime(&state, &task.id).await;
        let stopped = state
            .task_repo
            .get_by_id(&task.id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Stopped branch-update task disappeared".to_string())?;
        if let Some(ref app) = state.app_handle {
            app.emit(
                "task:stopped",
                serde_json::json!({
                    "taskId": stopped.id.as_str(),
                    "projectId": stopped.project_id.as_str(),
                    "stoppedFromStatus": from_status.as_str(),
                    "stopReason": reason,
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                }),
            )
            .map_err(|error| format!("Failed to emit task:stopped event: {error}"))?;
        }
        return Ok(TaskResponse::from(stopped));
    }

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
    resume_task_for_state(task_id, &state, &execution_state, app).await
}

#[tauri::command]
pub async fn retry_branch_update(
    task_id: String,
    state: State<'_, AppState>,
    execution_state: State<'_, Arc<ExecutionState>>,
    app: tauri::AppHandle,
) -> Result<TaskResponse, String> {
    let task_id = TaskId::from_string(task_id);
    let task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;
    if task.internal_status != InternalStatus::BranchUpdateBlocked {
        return Err(format!(
            "Task {} is not blocked on a branch update",
            task_id.as_str()
        ));
    }
    crate::application::tasks_feature_policy::TasksFeaturePolicy::from_state(&state)
        .authorize_session(
            task.ideation_session_id.as_ref(),
            crate::domain::ideation::TasksFeatureAction::Progress,
        )
        .await
        .map_err(|error| error.to_string())?;
    if !execution_state.can_start_any_execution_context() {
        return Err("Cannot retry: max concurrent task limit reached".to_string());
    }
    if !project_has_execution_capacity_for_state(&state, &execution_state, &task.project_id).await?
    {
        return Err("Cannot retry: project execution capacity reached".to_string());
    }
    let operation = state
        .branch_update_repo
        .get_active_operation(&task.id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Blocked task has no active branch-update operation".to_string())?;
    if operation.phase != BranchUpdatePhase::Blocked {
        return Err("Branch-update operation is not blocked".to_string());
    }
    let lease = state
        .branch_update_repo
        .get_target_lease(&operation.target_identity)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Branch-update target authority is missing".to_string())?;
    let update_status = branch_update_status(operation.direction);
    let new_operation_id = crate::domain::entities::BranchUpdateOperationId::new();
    let outcome = state
        .branch_update_repo
        .retry_operation(RetryBranchUpdate {
            operation_id: operation.id,
            new_operation_id: new_operation_id.clone(),
            task_id: task.id.clone(),
            originating_history_id: operation.originating_history_id,
            update_status,
            owner: lease.owner().clone(),
            fencing_epoch: lease.fencing_epoch(),
            history_id: uuid::Uuid::new_v4().to_string(),
        })
        .await
        .map_err(|error| error.to_string())?;
    if outcome != BranchUpdateCasOutcome::Applied {
        return Err(format!("Branch-update retry lost authority: {outcome:?}"));
    }
    let retry = state
        .branch_update_repo
        .get_operation(&new_operation_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Branch-update retry operation disappeared".to_string())?;
    let transition_service = build_transition_service(&state, &execution_state, Some(&app));
    if retry.phase == BranchUpdatePhase::Programmatic {
        let project = state
            .project_repo
            .get_by_id(&task.project_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Project not found for branch-update retry".to_string())?;
        match crate::application::branch_update_executor::execute_programmatic_branch_update(
            Arc::clone(&state.branch_update_repo),
            Arc::clone(&state.task_repo),
            std::path::Path::new(&project.working_directory),
            &retry,
            update_status,
            retry.target_lease_epoch,
        )
        .await
        .map_err(|error| error.to_string())?
        {
            crate::application::branch_update_executor::BranchUpdateExecutionOutcome::Completed {
                destination,
            } => {
                let continued = state
                    .task_repo
                    .get_by_id(&task.id)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "Retried branch-update task disappeared".to_string())?;
                transition_service
                    .execute_entry_actions(&continued.id, &continued, destination)
                    .await;
            }
            crate::application::branch_update_executor::BranchUpdateExecutionOutcome::ContinuationPending => {
                let pending = state
                    .branch_update_repo
                    .get_operation(&new_operation_id)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "Publication retry operation disappeared".to_string())?;
                crate::application::branch_update_executor::publish_post_merge_branch_update(
                    Arc::clone(&state.branch_update_repo),
                    std::path::Path::new(&project.working_directory),
                    &pending,
                    update_status,
                )
                .await
                .map_err(|error| error.to_string())?;
            }
            crate::application::branch_update_executor::BranchUpdateExecutionOutcome::NeedsAgent => {
                let resolving = state
                    .task_repo
                    .get_by_id(&task.id)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "Resolving branch-update task disappeared".to_string())?;
                transition_service
                    .execute_entry_actions(&resolving.id, &resolving, update_status)
                    .await;
            }
            crate::application::branch_update_executor::BranchUpdateExecutionOutcome::Blocked => {}
        }
    } else {
        let resolving = state
            .task_repo
            .get_by_id(&task.id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Resolving branch-update task disappeared".to_string())?;
        transition_service
            .execute_entry_actions(&resolving.id, &resolving, update_status)
            .await;
    }
    let updated = state
        .task_repo
        .get_by_id(&task.id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "Retried branch-update task disappeared".to_string())?;
    Ok(TaskResponse::from(updated))
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
    resume_tasks_in_group_for_state(
        group_kind,
        group_id,
        project_id,
        &state,
        &execution_state,
        app,
    )
    .await
}

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

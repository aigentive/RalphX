pub(crate) async fn resume_task_for_state(
    task_id: String,
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
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

    if let Some(operation) = state
        .branch_update_repo
        .get_active_operation(&task.id)
        .await
        .map_err(|error| error.to_string())?
    {
        let update_status = branch_update_status(operation.direction);
        if !execution_state.can_start_any_execution_context() {
            return Err("Cannot resume: max concurrent task limit reached".to_string());
        }
        if !project_has_execution_capacity_for_state(state, execution_state, &task.project_id)
            .await?
        {
            return Err("Cannot resume: project execution capacity reached".to_string());
        }
        let lease = state
            .branch_update_repo
            .get_target_lease(&operation.target_identity)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Branch-update target authority is missing".to_string())?;
        let outcome = state
            .branch_update_repo
            .resume_operation(ResumeBranchUpdate {
                operation_id: operation.id.clone(),
                task_id: task.id.clone(),
                originating_history_id: operation.originating_history_id.clone(),
                update_status,
                owner: lease.owner().clone(),
                fencing_epoch: lease.fencing_epoch(),
                history_id: uuid::Uuid::new_v4().to_string(),
            })
            .await
            .map_err(|error| error.to_string())?;
        if outcome != BranchUpdateCasOutcome::Applied {
            return Err(format!("Branch-update resume lost authority: {outcome:?}"));
        }

        let transition_service = build_transition_service(state, execution_state);
        if matches!(
            operation.phase,
            BranchUpdatePhase::ContinuationPending | BranchUpdatePhase::ContinuationInProgress
        ) {
            let next_status = if operation.continuation
                == BranchUpdateContinuation::FinalizePostMergePrPublication
            {
                let project = state
                    .project_repo
                    .get_by_id(&task.project_id)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "Project not found for publication resume".to_string())?;
                crate::application::branch_update_executor::publish_post_merge_branch_update(
                    Arc::clone(&state.branch_update_repo),
                    std::path::Path::new(&project.working_directory),
                    &operation,
                    update_status,
                )
                .await
                .map_err(|error| error.to_string())?
            } else {
                crate::application::branch_update_executor::resume_branch_update_continuation(
                    Arc::clone(&state.branch_update_repo),
                    &operation,
                    update_status,
                )
                .await
                .map_err(|error| error.to_string())?
            };
            if next_status != InternalStatus::Merged {
                let continued = state
                    .task_repo
                    .get_by_id(&task.id)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| "Resumed branch-update task disappeared".to_string())?;
                transition_service
                    .execute_entry_actions(&continued.id, &continued, next_status)
                    .await;
            }
        } else {
            let resumed = state
                .task_repo
                .get_by_id(&task.id)
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| "Resumed branch-update task disappeared".to_string())?;
            transition_service
                .execute_entry_actions(&resumed.id, &resumed, update_status)
                .await;
        }
        let resumed = state
            .task_repo
            .get_by_id(&task.id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Resumed branch-update task disappeared".to_string())?;
        emit_task_lifecycle_event_to_sink(
            state.events.as_ref(),
            "task:resumed",
            resumed.id.as_str(),
            resumed.project_id.as_str(),
        );
        return Ok(TaskResponse::from(resumed));
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
                    get_restore_status_from_history(state, &task_id).await?
                }
            }
        } else {
            get_restore_status_from_history(state, &task_id).await?
        };

    if !execution_state.can_start_any_execution_context() {
        return Err("Cannot resume: max concurrent task limit reached".to_string());
    }
    if !project_has_execution_capacity_for_state(state, execution_state, &task.project_id).await? {
        return Err("Cannot resume: project execution capacity reached".to_string());
    }

    // Build transition service
    let task_scheduler = build_task_scheduler(state, execution_state);

    let transition_service =
        build_transition_service(state, execution_state).with_task_scheduler(task_scheduler);

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
    emit_task_lifecycle_event_to_sink(
        state.events.as_ref(),
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

pub(crate) async fn resume_tasks_in_group_for_state(
    group_kind: String,
    group_id: String,
    project_id: String,
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> Result<BulkResumeResponse, String> {
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

    let task_scheduler = build_task_scheduler(state, execution_state);

    let transition_service =
        build_transition_service(state, execution_state).with_task_scheduler(task_scheduler);

    let mut resumed_count = 0;

    for task in paused_tasks {
        if !execution_state.can_start_any_execution_context() {
            tracing::warn!(task_id = %task.id, "Skipping resume: max concurrent task limit reached");
            continue;
        }
        if !project_has_execution_capacity_for_state(state, execution_state, &task.project_id)
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
                    Err(_) => match get_restore_status_from_history(state, &task.id).await {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!(task_id = %task.id, error = %e, "Cannot restore task");
                            continue;
                        }
                    },
                }
            } else {
                match get_restore_status_from_history(state, &task.id).await {
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

        emit_task_lifecycle_event_to_sink(
            state.events.as_ref(),
            "task:resumed",
            restored_task.id.as_str(),
            restored_task.project_id.as_str(),
        );

        resumed_count += 1;
    }

    let _ = ralphx_events::emit_serialized(
        state.events.as_ref(),
        "task:list_changed",
        &serde_json::json!({ "projectId": project_id }),
    );

    Ok(BulkResumeResponse { resumed_count })
}

/// Archive all tasks in a group (group_kind: "status" | "session" | "uncategorized")
///
/// Archives all non-archived tasks in the group.
/// Returns count of archived tasks.
#[doc(hidden)]
pub(crate) fn prepare_resumed_task_for_entry_actions(task: &mut Task) {
    task.metadata = Some(
        crate::application::chat_service::PauseReason::clear_from_task_metadata(
            task.metadata.as_deref(),
        ),
    );
    set_trigger_origin(task, "resume");
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
pub(crate) fn build_transition_service(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> TaskTransitionService {
    state.build_transition_service_for_runtime(Arc::clone(execution_state), None)
}

pub(crate) fn build_task_scheduler(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> Arc<dyn TaskScheduler> {
    let scheduler_concrete =
        Arc::new(state.build_task_scheduler_for_runtime(Arc::clone(execution_state), None));
    scheduler_concrete.set_self_ref(Arc::clone(&scheduler_concrete) as Arc<dyn TaskScheduler>);
    scheduler_concrete as Arc<dyn TaskScheduler>
}
pub(crate) fn branch_update_status(direction: BranchUpdateDirection) -> InternalStatus {
    match direction {
        BranchUpdateDirection::PlanBranch => InternalStatus::UpdatingPlanBranch,
        BranchUpdateDirection::TaskBranch => InternalStatus::UpdatingTaskBranch,
    }
}
use std::sync::Arc;

use crate::application::chat_service::PauseReason;
use crate::application::execution_control::project_has_execution_capacity_for_state;
use crate::application::execution_state::ExecutionState;
use crate::application::task_command_types::{BulkResumeResponse, TaskResponse};
use crate::application::{AppState, TaskTransitionService};
use crate::domain::entities::{
    BranchUpdateContinuation, BranchUpdateDirection, BranchUpdatePhase, InternalStatus, ProjectId,
    Task, TaskId,
};
use crate::domain::repositories::{BranchUpdateCasOutcome, ResumeBranchUpdate};
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::transition_handler::set_trigger_origin;

use super::task_lifecycle_events::emit_task_lifecycle_event_to_sink;

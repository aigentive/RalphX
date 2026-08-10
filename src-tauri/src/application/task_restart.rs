use std::path::Path;
use std::sync::Arc;

use crate::application::execution_recovery::{
    build_transition_service_for_recovery, categorize_resume_state, restart_transition_target,
    validate_resume, RestartDisposition, RestartResult, ResumeCategory, ResumeValidationWarning,
};
use crate::application::execution_state::ExecutionState;
use crate::application::git_service::GitService;
use crate::application::task_diff_base::ensure_task_has_non_empty_captured_diff;
use crate::application::validation_service::validation_run_proves_current_completion;
use crate::application::AppState;
use crate::domain::entities::{
    AgentRunStatus, ChatContextType, ExecutionRecoveryMetadata, ExecutionRecoveryState,
    InternalStatus, Task, TaskId, TaskStep, TaskStepStatus, ValidationCacheMetadata,
};
use crate::domain::repositories::{TaskRepository, TaskStepRepository};
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::transition_handler::{parse_metadata, set_trigger_origin};
use crate::error::{AppError, AppResult};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReadyRestartPreparation {
    pub cleared_failed_steps: u32,
}

#[derive(Debug, Clone)]
pub struct TerminalReadyRestartPlan {
    pub task: Task,
    pub failed_steps: Vec<TaskStep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedRecoveryEvidence {
    pub agent_run_id: String,
    pub validation_run_id: String,
    pub promoted_commit_sha: String,
    pub episode_entered_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedRecoveryWarning {
    pub code: String,
    pub message: String,
}

impl FailedRecoveryWarning {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailedRestartClassification {
    RecoverToReview(FailedRecoveryEvidence),
    RestartRequired(Vec<FailedRecoveryWarning>),
    Blocked(Vec<FailedRecoveryWarning>),
}

pub async fn classify_failed_restart(state: &AppState, task: &Task) -> FailedRestartClassification {
    if task.internal_status != InternalStatus::Failed {
        return FailedRestartClassification::Blocked(vec![FailedRecoveryWarning::new(
            "task_not_failed",
            format!(
                "Task is '{}' rather than failed",
                task.internal_status.as_str()
            ),
        )]);
    }

    let executing_at = match state
        .task_repo
        .get_status_last_entered_at(&task.id, InternalStatus::Executing)
        .await
    {
        Ok(value) => value,
        Err(error) => return recovery_read_block("execution_episode_read_failed", error),
    };
    let re_executing_at = match state
        .task_repo
        .get_status_last_entered_at(&task.id, InternalStatus::ReExecuting)
        .await
    {
        Ok(value) => value,
        Err(error) => return recovery_read_block("execution_episode_read_failed", error),
    };
    let Some(episode_entered_at) = executing_at.into_iter().chain(re_executing_at).max() else {
        return recovery_authority_block(
            "missing_execution_episode",
            "No execution episode proves which failed attempt owns the work",
        );
    };

    let steps = match state.task_step_repo.get_by_task(&task.id).await {
        Ok(steps) => steps,
        Err(error) => return recovery_read_block("task_steps_read_failed", error),
    };
    if steps.is_empty()
        || steps.iter().any(|step| {
            !matches!(
                step.status,
                TaskStepStatus::Completed | TaskStepStatus::Skipped
            )
        })
    {
        return restart_required(
            "steps_not_complete",
            "The current attempt does not have a complete persisted step set",
        );
    }

    let conversation = match state
        .chat_conversation_repo
        .get_active_for_context(ChatContextType::TaskExecution, task.id.as_str())
        .await
    {
        Ok(Some(conversation)) => conversation,
        Ok(None) => {
            return recovery_authority_block(
                "missing_execution_conversation",
                "No task-execution conversation proves the current attempt",
            )
        }
        Err(error) => return recovery_read_block("execution_conversation_read_failed", error),
    };
    let agent_run = match state
        .agent_run_repo
        .get_latest_for_conversation(&conversation.id)
        .await
    {
        Ok(Some(run)) => run,
        Ok(None) => {
            return recovery_authority_block(
                "missing_agent_run",
                "No agent run proves the current attempt completed",
            )
        }
        Err(error) => return recovery_read_block("agent_run_read_failed", error),
    };
    if agent_run.started_at < episode_entered_at {
        return recovery_authority_block(
            "agent_run_not_current",
            "The latest task-execution run predates the current execution episode",
        );
    }
    if agent_run.status != AgentRunStatus::Completed {
        return restart_required(
            "agent_run_not_completed",
            "The current task-execution run did not complete successfully",
        );
    }

    let project = match state.project_repo.get_by_id(&task.project_id).await {
        Ok(Some(project)) => project,
        Ok(None) => {
            return FailedRestartClassification::Blocked(vec![FailedRecoveryWarning::new(
                "project_missing",
                "The task project no longer exists",
            )])
        }
        Err(error) => return recovery_read_block("project_read_failed", error),
    };
    let Some(stored_worktree) = task.worktree_path.as_deref() else {
        return restart_required(
            "missing_worktree",
            "The failed attempt has no preserved worktree",
        );
    };
    let Some(task_branch) = task.task_branch.as_deref() else {
        return restart_required(
            "missing_task_branch",
            "The failed attempt has no preserved task branch",
        );
    };
    let expected_worktree = project.task_worktree_path(task.id.as_str());
    if Path::new(stored_worktree) != expected_worktree {
        return FailedRestartClassification::Blocked(vec![FailedRecoveryWarning::new(
            "worktree_path_mismatch",
            "The preserved worktree is not the process-owned path for this task",
        )]);
    }
    let worktree = match crate::utils::path_safety::validate_absolute_non_root_path(
        &expected_worktree,
        "failed task recovery worktree",
    ) {
        Ok(path) => path,
        Err(error) => return recovery_read_block("unsafe_worktree_path", error),
    };
    let worktree_root =
        match crate::application::agent_conversation_workspace::expand_worktree_parent_public(
            project.worktree_parent_or_default(),
        )
        .and_then(|path| {
            crate::utils::path_safety::validate_absolute_non_root_path(
                &path,
                "failed task recovery worktree root",
            )
        }) {
            Ok(path) => path,
            Err(error) => return recovery_read_block("unsafe_worktree_root", error),
        };
    let worktree_exists =
        match crate::utils::path_safety::checked_exists(&worktree, "failed task recovery worktree")
        {
            Ok(exists) => exists,
            Err(error) => return recovery_read_block("worktree_existence_read_failed", error),
        };
    if !worktree_exists {
        return restart_required(
            "missing_worktree",
            "The preserved worktree no longer exists",
        );
    }
    let canonical_root = match worktree_root.canonicalize() {
        Ok(path) => path,
        Err(error) => return recovery_read_block("worktree_root_resolution_failed", error),
    };
    let canonical_worktree = match worktree.canonicalize() {
        Ok(path) => path,
        Err(error) => return recovery_read_block("worktree_resolution_failed", error),
    };
    if !canonical_worktree.starts_with(&canonical_root) || canonical_worktree == canonical_root {
        return FailedRestartClassification::Blocked(vec![FailedRecoveryWarning::new(
            "worktree_root_escape",
            "The preserved worktree resolves outside the configured worktree root",
        )]);
    }
    match GitService::has_uncommitted_changes(&worktree).await {
        Ok(true) => return FailedRestartClassification::Blocked(vec![FailedRecoveryWarning::new("dirty_worktree", "The preserved worktree has uncommitted changes; recovery and restart are blocked to avoid losing work")]),
        Ok(false) => {}
        Err(error) => return recovery_read_block("worktree_status_read_failed", error),
    }
    match GitService::get_current_branch(&worktree).await {
        Ok(branch) if branch == task_branch => {}
        Ok(_) => {
            return FailedRestartClassification::Blocked(vec![FailedRecoveryWarning::new(
                "task_branch_mismatch",
                "The preserved worktree is checked out on a different branch",
            )])
        }
        Err(error) => return recovery_read_block("task_branch_read_failed", error),
    }
    let head_sha = match GitService::get_head_sha(&worktree).await {
        Ok(sha) => sha,
        Err(error) => return recovery_read_block("task_head_read_failed", error),
    };
    if let Err(error) =
        ensure_task_has_non_empty_captured_diff(task, &project, "failed_restart_recovery").await
    {
        let message = error.to_string();
        if message.contains("empty_task_diff_missing_captured_base")
            || message.contains("empty_task_diff_against_captured_base")
        {
            return restart_required("task_diff_not_recoverable", message);
        }
        return recovery_read_block("task_diff_read_failed", message);
    }

    let validation = match state
        .validation_run_repo
        .latest_non_baseline_run_with_results_for_task(&task.id)
        .await
    {
        Ok(Some(validation)) => Some(validation),
        Ok(None) => None,
        Err(error) => return recovery_read_block("validation_evidence_read_failed", error),
    };
    let validation_run_id = if let Some(validation) = validation {
        if !validation_run_proves_current_completion(&validation, &head_sha, episode_entered_at) {
            return restart_required(
                "validation_evidence_not_current",
                "Validation is not green, current-attempt, test-bearing evidence promoted to the preserved HEAD",
            );
        }
        validation.run.id
    } else {
        let cache = match ValidationCacheMetadata::from_task_metadata(task.metadata.as_deref()) {
            Ok(Some(cache)) => cache,
            Ok(None) => return restart_required("missing_validation_evidence", "No non-baseline validation run or compatible validation cache proves the preserved commit"),
            Err(error) => return recovery_read_block("legacy_validation_cache_read_failed", error),
        };
        if cache.commit_sha != head_sha
            || !cache.tests_ran
            || !cache.tests_passed
            || cache.captured_at < episode_entered_at
        {
            return restart_required(
                "validation_evidence_not_current",
                "Legacy validation evidence is not green, test-bearing, current-episode proof for the preserved HEAD",
            );
        }
        "legacy_validation_cache".to_string()
    };

    FailedRestartClassification::RecoverToReview(FailedRecoveryEvidence {
        agent_run_id: agent_run.id.as_str().to_string(),
        validation_run_id,
        promoted_commit_sha: head_sha,
        episode_entered_at,
    })
}

async fn schedule_ready_tasks_for_project(
    app_state: &AppState,
    execution_state: Arc<crate::application::ExecutionState>,
    project_id: Option<crate::domain::entities::ProjectId>,
) {
    let scheduler =
        Arc::new(app_state.build_task_scheduler_for_runtime(Arc::clone(&execution_state), None));
    scheduler.set_self_ref(Arc::clone(&scheduler) as Arc<dyn TaskScheduler>);
    scheduler.set_active_project(project_id).await;
    scheduler.try_schedule_ready_tasks().await;
}

fn restart_required(code: &str, message: impl Into<String>) -> FailedRestartClassification {
    FailedRestartClassification::RestartRequired(vec![FailedRecoveryWarning::new(code, message)])
}

fn recovery_authority_block(code: &str, message: impl Into<String>) -> FailedRestartClassification {
    FailedRestartClassification::Blocked(vec![FailedRecoveryWarning::new(code, message)])
}

fn recovery_read_block(code: &str, error: impl std::fmt::Display) -> FailedRestartClassification {
    FailedRestartClassification::Blocked(vec![FailedRecoveryWarning::new(
        code,
        format!("Recovery authority could not be read safely: {error}"),
    )])
}

pub async fn prepare_terminal_task_for_ready_restart(
    task_repo: &Arc<dyn TaskRepository>,
    task_step_repo: &Arc<dyn TaskStepRepository>,
    old_task: &Task,
) -> AppResult<ReadyRestartPreparation> {
    let Some(plan) = build_terminal_ready_restart_plan(task_step_repo, old_task).await? else {
        return Ok(ReadyRestartPreparation::default());
    };

    if !task_repo
        .update_with_expected_status(&plan.task, old_task.internal_status)
        .await?
    {
        return Err(AppError::Validation(format!(
            "Task {} changed concurrently; restart preparation did not clear preserved work",
            old_task.id.as_str()
        )));
    }

    let cleared_failed_steps = reset_failed_steps(task_step_repo, plan.failed_steps).await?;

    Ok(ReadyRestartPreparation {
        cleared_failed_steps,
    })
}

pub async fn build_terminal_ready_restart_plan(
    task_step_repo: &Arc<dyn TaskStepRepository>,
    old_task: &Task,
) -> AppResult<Option<TerminalReadyRestartPlan>> {
    if !old_task.internal_status.is_terminal() {
        return Ok(None);
    }
    ensure_restart_worktree_is_safe_to_clear(old_task).await?;

    let mut task_mut = old_task.clone();

    set_trigger_origin(&mut task_mut, "retry");

    task_mut.task_branch = None;
    task_mut.worktree_path = None;
    task_mut.merge_commit_sha = None;

    if let Ok(Some(mut recovery)) =
        ExecutionRecoveryMetadata::from_task_metadata(task_mut.metadata.as_deref())
    {
        recovery.stop_retrying = false;
        recovery.last_state = ExecutionRecoveryState::Retrying;
        recovery.events.clear();
        recovery.unrecoverable_reason = None;
        if let Ok(updated_meta) = recovery.update_task_metadata(task_mut.metadata.as_deref()) {
            task_mut.metadata = Some(updated_meta);
        }
    }

    let mut meta = parse_metadata(&task_mut).unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = meta.as_object_mut() {
        if old_task.internal_status == InternalStatus::Failed {
            obj.insert("preserve_steps".to_string(), serde_json::json!(true));
        }
    }
    task_mut.metadata = Some(meta.to_string());

    let failed_steps = if old_task.internal_status == InternalStatus::Failed {
        task_step_repo
            .get_by_task(&old_task.id)
            .await?
            .into_iter()
            .filter(|step| step.status == TaskStepStatus::Failed)
            .collect()
    } else {
        Vec::new()
    };

    Ok(Some(TerminalReadyRestartPlan {
        task: task_mut,
        failed_steps,
    }))
}

pub async fn clear_failed_steps_for_failed_restart(
    task_step_repo: &Arc<dyn TaskStepRepository>,
    task_id: &TaskId,
) -> AppResult<u32> {
    let steps = task_step_repo.get_by_task(task_id).await?;

    reset_failed_steps(
        task_step_repo,
        steps
            .into_iter()
            .filter(|step| step.status == TaskStepStatus::Failed)
            .collect(),
    )
    .await
}

async fn reset_failed_steps(
    task_step_repo: &Arc<dyn TaskStepRepository>,
    steps: Vec<TaskStep>,
) -> AppResult<u32> {
    let mut cleared = 0u32;
    for mut step in steps {
        step.status = TaskStepStatus::Pending;
        step.started_at = None;
        step.completed_at = None;
        step.completion_note = None;
        task_step_repo.update(&step).await?;
        cleared += 1;
    }

    Ok(cleared)
}

async fn ensure_restart_worktree_is_safe_to_clear(task: &Task) -> AppResult<()> {
    let Some(worktree_path) = task.worktree_path.as_deref() else {
        return Ok(());
    };
    let worktree = crate::utils::path_safety::validate_absolute_non_root_path(
        std::path::Path::new(worktree_path),
        "task restart worktree",
    )?;
    if !crate::utils::path_safety::checked_exists(&worktree, "task restart worktree")? {
        return Ok(());
    }
    if GitService::has_uncommitted_changes(&worktree).await? {
        return Err(AppError::Validation(format!(
            "Cannot restart task {} safely because worktree '{}' has uncommitted changes",
            task.id.as_str(),
            worktree.display()
        )));
    }
    Ok(())
}

pub(crate) async fn restart_task_for_state(
    task_id: String,
    force: bool,
    note: Option<String>,
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> Result<RestartResult, String> {
    use crate::domain::state_machine::transition_handler::metadata_builder::{
        build_restart_metadata, parse_stop_metadata,
    };

    let task_id = TaskId::from_string(task_id);

    // 1. Get the task
    let task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;
    crate::application::tasks_feature_policy::TasksFeaturePolicy::from_state(state)
        .authorize_session(
            task.ideation_session_id.as_ref(),
            crate::domain::ideation::TasksFeatureAction::Progress,
        )
        .await
        .map_err(|error| error.to_string())?;

    if task.internal_status == InternalStatus::Failed {
        let classification = classify_failed_restart(state, &task).await;
        match classification {
            FailedRestartClassification::RecoverToReview(_) => {
                // Repeat the complete proof immediately before the corrective CAS so a
                // worktree/validation change during the first preflight cannot advance review.
                let current_task = state
                    .task_repo
                    .get_by_id(&task_id)
                    .await
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| format!("Task not found: {}", task_id.as_str()))?;
                let FailedRestartClassification::RecoverToReview(evidence) =
                    classify_failed_restart(state, &current_task).await
                else {
                    return Ok(RestartResult::Blocked {
                        warnings: vec![ResumeValidationWarning {
                            code: "recovery_authority_changed".to_string(),
                            message: "Recovery evidence changed during preflight; no task state was mutated".to_string(),
                        }],
                    });
                };
                let transition_service =
                    build_transition_service_for_recovery(state, Arc::clone(execution_state));
                let updated_task = transition_service
                    .recover_failed_completed_task_to_review(&task_id, &evidence)
                    .await
                    .map_err(|error| error.to_string())?;
                return Ok(RestartResult::Success {
                    task: serde_json::to_value(&updated_task).map_err(|error| error.to_string())?,
                    category: ResumeCategory::Redirect,
                    resumed_to_status: InternalStatus::PendingReview.as_str().to_string(),
                    disposition: Some(RestartDisposition::RecoveredToReview),
                });
            }
            FailedRestartClassification::RestartRequired(_) => {
                let plan = build_terminal_ready_restart_plan(&state.task_step_repo, &task)
                    .await
                    .map_err(|error| format!("Failed to prepare task restart: {error}"))?
                    .ok_or_else(|| {
                        "Failed task restart did not produce a terminal plan".to_string()
                    })?;
                let transition_service =
                    build_transition_service_for_recovery(state, Arc::clone(execution_state));
                let updated_task = transition_service
                    .restart_terminal_task_to_ready(
                        plan,
                        Some(build_restart_metadata(note.as_deref())),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                schedule_ready_tasks_for_project(
                    state,
                    Arc::clone(execution_state),
                    Some(updated_task.project_id.clone()),
                )
                .await;
                return Ok(RestartResult::Success {
                    task: serde_json::to_value(&updated_task).map_err(|error| error.to_string())?,
                    category: ResumeCategory::Direct,
                    resumed_to_status: InternalStatus::Ready.as_str().to_string(),
                    disposition: Some(RestartDisposition::RestartedToReady),
                });
            }
            FailedRestartClassification::Blocked(warnings) => {
                return Ok(RestartResult::Blocked {
                    warnings: warnings
                        .into_iter()
                        .map(|warning| ResumeValidationWarning {
                            code: warning.code,
                            message: warning.message,
                        })
                        .collect(),
                });
            }
        }
    }

    // 2. Verify task is in Stopped status
    if task.internal_status != InternalStatus::Stopped {
        return Err(format!(
            "Task is not in Stopped status (current: {})",
            task.internal_status.as_str()
        ));
    }

    // 3. Parse stop metadata
    let stop_metadata = parse_stop_metadata(task.metadata.as_deref())
        .ok_or_else(|| "Task has no stop metadata - cannot smart resume".to_string())?;

    let stopped_from_status = stop_metadata.parse_from_status().ok_or_else(|| {
        format!(
            "Invalid stopped_from_status: {}",
            stop_metadata.stopped_from_status
        )
    })?;

    tracing::info!(
        task_id = task_id.as_str(),
        stopped_from = stopped_from_status.as_str(),
        reason = ?stop_metadata.stop_reason,
        "Smart restarting task"
    );

    // 4. Categorize the resume state
    let categorized = categorize_resume_state(stopped_from_status);

    // 5. For Validated category, run validation (unless forced)
    if categorized.category == ResumeCategory::Validated && !force {
        let validation_result = validate_resume(&task, state).await;
        if !validation_result.passed {
            return Ok(RestartResult::ValidationFailed {
                warnings: validation_result.warnings,
                stopped_from_status: stopped_from_status.as_str().to_string(),
            });
        }
    }

    // 6. Build transition service
    let transition_service =
        build_transition_service_for_recovery(state, Arc::clone(execution_state));

    let transition_target = restart_transition_target(stopped_from_status);
    if !task.internal_status.can_transition_to(transition_target) {
        return Ok(RestartResult::ValidationFailed {
            warnings: vec![ResumeValidationWarning {
                code: "unsupported_restart_target".to_string(),
                message: format!(
                    "Stopped task from '{}' cannot safely restart directly to '{}'",
                    stopped_from_status.as_str(),
                    transition_target.as_str()
                ),
            }],
            stopped_from_status: stopped_from_status.as_str().to_string(),
        });
    }
    let terminal_restart_plan =
        if transition_target == InternalStatus::Ready && task.internal_status.is_terminal() {
            build_terminal_ready_restart_plan(&state.task_step_repo, &task)
                .await
                .map_err(|e| format!("Failed to prepare task restart: {e}"))?
        } else {
            None
        };

    // 7. Transition to target status: clear stop metadata and optionally store restart_note
    let restart_metadata = build_restart_metadata(note.as_deref());
    let updated_task = if let Some(plan) = terminal_restart_plan {
        transition_service
            .restart_terminal_task_to_ready(plan, Some(restart_metadata))
            .await
            .map_err(|e| e.to_string())?
    } else {
        transition_service
            .transition_task_with_metadata(&task_id, transition_target, Some(restart_metadata))
            .await
            .map_err(|e| e.to_string())?
    };

    if transition_target == InternalStatus::Ready {
        schedule_ready_tasks_for_project(
            state,
            Arc::clone(execution_state),
            Some(updated_task.project_id.clone()),
        )
        .await;
    }

    tracing::info!(
        task_id = task_id.as_str(),
        category = ?categorized.category,
        target = transition_target.as_str(),
        stopped_from = stopped_from_status.as_str(),
        "Task restarted successfully"
    );

    // 8. Emit lifecycle event
    let _ = ralphx_events::emit_serialized(
        state.events.as_ref(),
        "task:restarted",
        &serde_json::json!({
                "taskId": updated_task.id.as_str(),
                "projectId": updated_task.project_id.as_str(),
                "resumedToStatus": transition_target.as_str(),
                "stoppedFromStatus": stopped_from_status.as_str(),
                "category": categorized.category,
                "stopReason": stop_metadata.stop_reason,
                "timestamp": chrono::Utc::now().to_rfc3339(),
        }),
    );

    // 9. Return success result
    // Serialize task to JSON Value for flexible response
    let task_json = serde_json::to_value(&updated_task).map_err(|e| e.to_string())?;

    Ok(RestartResult::Success {
        task: task_json,
        category: categorized.category,
        resumed_to_status: transition_target.as_str().to_string(),
        disposition: None,
    })
}
#[cfg(test)]
#[path = "task_restart_tests.rs"]
mod tests;

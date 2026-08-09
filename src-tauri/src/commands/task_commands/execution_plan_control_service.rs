use std::sync::Arc;

use tauri::AppHandle;

use crate::application::interactive_process_registry::InteractiveProcessKey;
use crate::application::AppState;
use crate::commands::execution_commands::{
    determine_paused_restore_status, prepare_resumed_task_for_entry_actions,
    project_has_execution_capacity_for_state, AGENT_ACTIVE_STATUSES,
};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    ChatContextType, ExecutionPlan, ExecutionPlanHaltMode, ExecutionPlanId, IdeationSessionId,
    InternalStatus, ProjectId, Task, TaskId,
};
use crate::domain::services::{QueueKey, RunningAgentKey};
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::transition_handler::metadata_builder::MetadataUpdate;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone)]
pub struct ExecutionPlanControlScope {
    pub project_id: ProjectId,
    pub session_id: IdeationSessionId,
    pub execution_plan_id: Option<ExecutionPlanId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlanControlOutcome {
    pub execution_plan_id: ExecutionPlanId,
    pub affected_count: usize,
}

pub struct ExecutionPlanControlService<'a> {
    state: &'a AppState,
    execution_state: Arc<ExecutionState>,
}

impl<'a> ExecutionPlanControlService<'a> {
    pub fn new(
        state: &'a AppState,
        execution_state: Arc<ExecutionState>,
        _app_handle: Option<AppHandle>,
    ) -> Self {
        Self {
            state,
            execution_state,
        }
    }

    pub async fn pause_plan(
        &self,
        scope: ExecutionPlanControlScope,
    ) -> AppResult<ExecutionPlanControlOutcome> {
        let plan = self.resolve_execution_plan(&scope).await?;
        let previous_halt_mode = plan.halt_mode;
        self.state
            .execution_plan_repo
            .set_halt_mode(&plan.id, ExecutionPlanHaltMode::Paused)
            .await?;
        let tasks = self.current_plan_tasks(&scope.project_id, &plan).await?;

        let transition_service = self.build_transition_service();
        let mut paused_count = 0usize;
        for task in tasks {
            if !AGENT_ACTIVE_STATUSES.contains(&task.internal_status) {
                continue;
            }

            if let Err(error) = self.pause_active_task(&transition_service, &task).await {
                self.restore_halt_mode_if_no_tasks_changed(
                    &plan.id,
                    previous_halt_mode,
                    paused_count,
                    "pause",
                )
                .await;
                return Err(error);
            }
            paused_count += 1;
        }

        Ok(ExecutionPlanControlOutcome {
            execution_plan_id: plan.id,
            affected_count: paused_count,
        })
    }

    pub async fn resume_plan(
        &self,
        scope: ExecutionPlanControlScope,
    ) -> AppResult<ExecutionPlanControlOutcome> {
        let plan = self.resolve_execution_plan(&scope).await?;
        let tasks = self.current_plan_tasks(&scope.project_id, &plan).await?;
        self.state
            .execution_plan_repo
            .set_halt_mode(&plan.id, ExecutionPlanHaltMode::Running)
            .await?;

        let scheduler = Arc::new(
            self.state
                .build_task_scheduler_for_runtime(Arc::clone(&self.execution_state), None),
        );
        scheduler.set_self_ref(Arc::clone(&scheduler) as Arc<dyn TaskScheduler>);
        scheduler
            .set_active_project(Some(scope.project_id.clone()))
            .await;
        scheduler
            .set_active_execution_plan(Some(plan.id.clone()))
            .await;

        let transition_service = self
            .build_transition_service()
            .with_task_scheduler(Arc::clone(&scheduler) as Arc<dyn TaskScheduler>);
        let mut resumed_count = 0usize;

        for task in tasks {
            if task.internal_status != InternalStatus::Paused {
                continue;
            }

            let restore_status =
                match determine_paused_restore_status(&task, self.state.task_repo.as_ref()).await {
                    Ok(Some(status)) => status,
                    Ok(None) => continue,
                    Err(error) => {
                        tracing::warn!(
                            task_id = task.id.as_str(),
                            error = %error,
                            "Failed to resolve execution-plan paused restore status"
                        );
                        continue;
                    }
                };

            if !AGENT_ACTIVE_STATUSES.contains(&restore_status) {
                tracing::warn!(
                    task_id = task.id.as_str(),
                    restore_status = restore_status.as_str(),
                    "Skipping execution-plan scoped resume for non-agent-active restore status"
                );
                continue;
            }
            if !self.execution_state.can_start_any_execution_context() {
                tracing::info!(
                    task_id = task.id.as_str(),
                    "Stopping execution-plan scoped resume: global capacity reached"
                );
                break;
            }
            if !project_has_execution_capacity_for_state(
                self.state,
                &self.execution_state,
                &task.project_id,
            )
            .await
            .map_err(AppError::Validation)?
            {
                tracing::info!(
                    task_id = task.id.as_str(),
                    project_id = task.project_id.as_str(),
                    "Stopping execution-plan scoped resume: project capacity reached"
                );
                break;
            }

            if let Err(error) = transition_service
                .transition_task(&task.id, restore_status)
                .await
            {
                tracing::warn!(
                    task_id = task.id.as_str(),
                    restore_status = restore_status.as_str(),
                    error = %error,
                    "Failed to resume task for execution-plan scoped resume"
                );
                continue;
            }

            let Some(mut restored_task) = self.state.task_repo.get_by_id(&task.id).await? else {
                continue;
            };
            prepare_resumed_task_for_entry_actions(&mut restored_task);
            restored_task.touch();
            self.state.task_repo.update(&restored_task).await?;
            transition_service
                .execute_entry_actions(&task.id, &restored_task, restore_status)
                .await;
            resumed_count += 1;
        }

        scheduler.try_schedule_ready_tasks().await;

        Ok(ExecutionPlanControlOutcome {
            execution_plan_id: plan.id,
            affected_count: resumed_count,
        })
    }

    pub async fn stop_plan(
        &self,
        scope: ExecutionPlanControlScope,
    ) -> AppResult<ExecutionPlanControlOutcome> {
        let plan = self.resolve_execution_plan(&scope).await?;
        let previous_halt_mode = plan.halt_mode;
        self.state
            .execution_plan_repo
            .set_halt_mode(&plan.id, ExecutionPlanHaltMode::Stopped)
            .await?;
        let tasks = self.current_plan_tasks(&scope.project_id, &plan).await?;

        let transition_service = self.build_transition_service();
        let mut stopped_count = 0usize;
        for task in &tasks {
            if !AGENT_ACTIVE_STATUSES.contains(&task.internal_status) {
                continue;
            }

            if let Err(error) = self.stop_active_task(&transition_service, task).await {
                self.restore_halt_mode_if_no_tasks_changed(
                    &plan.id,
                    previous_halt_mode,
                    stopped_count,
                    "stop",
                )
                .await;
                return Err(error);
            }
            stopped_count += 1;
        }

        for task in tasks
            .iter()
            .filter(|task| !AGENT_ACTIVE_STATUSES.contains(&task.internal_status))
        {
            self.clear_task_queues(&task.id).await?;
        }

        Ok(ExecutionPlanControlOutcome {
            execution_plan_id: plan.id,
            affected_count: stopped_count,
        })
    }

    fn build_transition_service(&self) -> crate::application::TaskTransitionService {
        self.state
            .build_transition_service_for_runtime(Arc::clone(&self.execution_state), None)
    }

    async fn pause_active_task(
        &self,
        transition_service: &crate::application::TaskTransitionService,
        task: &Task,
    ) -> AppResult<()> {
        let paused_task = transition_service
            .transition_task_with_metadata(
                &task.id,
                InternalStatus::Paused,
                Some(Self::pause_metadata(task)?),
            )
            .await?;
        Self::ensure_transition_result(
            &task.id,
            paused_task.internal_status,
            InternalStatus::Paused,
            "pause",
        )?;
        self.stop_task_runtime_contexts(&task.id).await;
        Ok(())
    }

    async fn stop_active_task(
        &self,
        transition_service: &crate::application::TaskTransitionService,
        task: &Task,
    ) -> AppResult<()> {
        let stopped_task = transition_service
            .transition_to_stopped_with_context(
                &task.id,
                task.internal_status,
                Some("Stopped from accepted plan controls".to_string()),
            )
            .await?;
        Self::ensure_transition_result(
            &task.id,
            stopped_task.internal_status,
            InternalStatus::Stopped,
            "stop",
        )?;
        self.stop_task_runtime_contexts(&task.id).await;
        self.clear_task_queues(&task.id).await
    }

    fn pause_metadata(task: &Task) -> AppResult<MetadataUpdate> {
        let pause_reason = crate::application::chat_service::PauseReason::UserInitiated {
            previous_status: task.internal_status.to_string(),
            paused_at: chrono::Utc::now().to_rfc3339(),
            scope: "execution_plan".to_string(),
        };
        let value = serde_json::to_value(pause_reason).map_err(|error| {
            AppError::Validation(format!("Failed to serialize pause metadata: {error}"))
        })?;
        Ok(MetadataUpdate::new().with_value("pause_reason", value))
    }

    fn ensure_transition_result(
        task_id: &TaskId,
        actual: InternalStatus,
        expected: InternalStatus,
        operation: &str,
    ) -> AppResult<()> {
        if actual == expected {
            return Ok(());
        }

        Err(AppError::Validation(format!(
            "Execution-plan scoped {operation} for task {} did not reach expected status {} (actual: {})",
            task_id.as_str(),
            expected.as_str(),
            actual.as_str()
        )))
    }

    async fn restore_halt_mode_if_no_tasks_changed(
        &self,
        plan_id: &ExecutionPlanId,
        previous_halt_mode: ExecutionPlanHaltMode,
        affected_count: usize,
        operation: &str,
    ) {
        if affected_count != 0 {
            return;
        }

        if let Err(error) = self
            .state
            .execution_plan_repo
            .set_halt_mode(plan_id, previous_halt_mode)
            .await
        {
            tracing::warn!(
                execution_plan_id = plan_id.as_str(),
                previous_halt_mode = previous_halt_mode.to_db_string(),
                error = %error,
                "Failed to restore execution-plan halt mode after scoped {operation} failed"
            );
        }
    }

    async fn resolve_execution_plan(
        &self,
        scope: &ExecutionPlanControlScope,
    ) -> AppResult<ExecutionPlan> {
        let session = self
            .state
            .ideation_session_repo
            .get_by_id(&scope.session_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Ideation session not found: {}",
                    scope.session_id.as_str()
                ))
            })?;
        if session.project_id != scope.project_id {
            return Err(AppError::Validation(format!(
                "Ideation session {} belongs to project {}, not {}",
                scope.session_id.as_str(),
                session.project_id.as_str(),
                scope.project_id.as_str()
            )));
        }

        let plan = self
            .state
            .execution_plan_repo
            .get_active_for_session(&scope.session_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Active execution plan not found for session {}",
                    scope.session_id.as_str()
                ))
            })?;

        if let Some(requested_id) = scope.execution_plan_id.as_ref() {
            if requested_id != &plan.id {
                return Err(AppError::Validation(format!(
                    "Execution plan {} is not the active execution plan for session {}",
                    requested_id.as_str(),
                    scope.session_id.as_str()
                )));
            }
        }

        if plan.session_id != scope.session_id {
            return Err(AppError::Validation(format!(
                "Execution plan {} belongs to session {}, not {}",
                plan.id.as_str(),
                plan.session_id.as_str(),
                scope.session_id.as_str()
            )));
        }

        Ok(plan)
    }

    async fn current_plan_tasks(
        &self,
        project_id: &ProjectId,
        plan: &ExecutionPlan,
    ) -> AppResult<Vec<Task>> {
        let tasks = self
            .state
            .task_repo
            .get_by_ideation_session(&plan.session_id)
            .await?;
        Ok(tasks
            .into_iter()
            .filter(|task| task.project_id == *project_id)
            .filter(|task| task.execution_plan_id.as_ref() == Some(&plan.id))
            .collect())
    }

    async fn stop_task_runtime_contexts(&self, task_id: &TaskId) -> bool {
        let mut stopped_any = false;
        for context_type in ["task_execution", "review", "merge"] {
            let ipr_key = InteractiveProcessKey::new(context_type, task_id.as_str());
            self.state
                .interactive_process_registry
                .remove(&ipr_key)
                .await;

            let key = RunningAgentKey::new(context_type, task_id.as_str());
            if self
                .state
                .running_agent_registry
                .stop(&key)
                .await
                .ok()
                .flatten()
                .is_some()
            {
                stopped_any = true;
            }
        }
        stopped_any
    }

    async fn clear_task_queues(&self, task_id: &TaskId) -> AppResult<()> {
        for context_type in [
            ChatContextType::TaskExecution,
            ChatContextType::Review,
            ChatContextType::Merge,
            ChatContextType::Task,
        ] {
            let key = QueueKey::new(context_type, task_id.as_str());
            self.state.message_queue.clear_with_key(&key);
            self.state.queued_message_repo.clear(&key).await?;
        }
        Ok(())
    }
}

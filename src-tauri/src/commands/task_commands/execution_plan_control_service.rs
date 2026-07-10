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
    app_handle: Option<AppHandle>,
}

impl<'a> ExecutionPlanControlService<'a> {
    pub fn new(
        state: &'a AppState,
        execution_state: Arc<ExecutionState>,
        app_handle: Option<AppHandle>,
    ) -> Self {
        Self {
            state,
            execution_state,
            app_handle,
        }
    }

    pub async fn pause_plan(
        &self,
        scope: ExecutionPlanControlScope,
    ) -> AppResult<ExecutionPlanControlOutcome> {
        let plan = self.resolve_execution_plan(&scope).await?;
        self.state
            .execution_plan_repo
            .set_halt_mode(&plan.id, ExecutionPlanHaltMode::Paused)
            .await?;

        let transition_service = self.build_transition_service();
        let mut paused_count = 0usize;
        for task in self.current_plan_tasks(&scope.project_id, &plan).await? {
            if !AGENT_ACTIVE_STATUSES.contains(&task.internal_status) {
                continue;
            }

            self.stop_task_runtime_contexts(&task.id).await;
            self.write_pause_reason(&task).await?;

            match transition_service
                .transition_task(&task.id, InternalStatus::Paused)
                .await
            {
                Ok(_) => paused_count += 1,
                Err(error) => {
                    tracing::warn!(
                        task_id = task.id.as_str(),
                        error = %error,
                        "Failed to pause task for execution-plan scoped pause"
                    );
                }
            }
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
        self.state
            .execution_plan_repo
            .set_halt_mode(&plan.id, ExecutionPlanHaltMode::Running)
            .await?;

        let scheduler = Arc::new(self.state.build_task_scheduler_for_runtime(
            Arc::clone(&self.execution_state),
            self.app_handle.clone(),
        ));
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

        for task in self.current_plan_tasks(&scope.project_id, &plan).await? {
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
        self.state
            .execution_plan_repo
            .set_halt_mode(&plan.id, ExecutionPlanHaltMode::Stopped)
            .await?;

        let transition_service = self.build_transition_service();
        let mut stopped_count = 0usize;
        for task in self.current_plan_tasks(&scope.project_id, &plan).await? {
            self.clear_task_queues(&task.id).await?;

            if !AGENT_ACTIVE_STATUSES.contains(&task.internal_status) {
                continue;
            }

            self.stop_task_runtime_contexts(&task.id).await;
            match transition_service
                .transition_to_stopped_with_context(
                    &task.id,
                    task.internal_status,
                    Some("Stopped from accepted plan controls".to_string()),
                )
                .await
            {
                Ok(_) => stopped_count += 1,
                Err(error) => {
                    tracing::warn!(
                        task_id = task.id.as_str(),
                        error = %error,
                        "Failed to stop task for execution-plan scoped stop"
                    );
                }
            }
        }

        Ok(ExecutionPlanControlOutcome {
            execution_plan_id: plan.id,
            affected_count: stopped_count,
        })
    }

    fn build_transition_service(&self) -> crate::application::TaskTransitionService {
        self.state.build_transition_service_for_runtime(
            Arc::clone(&self.execution_state),
            self.app_handle.clone(),
        )
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

        let plan = if let Some(id) = scope.execution_plan_id.as_ref() {
            self.state
                .execution_plan_repo
                .get_by_id(id)
                .await?
                .ok_or_else(|| {
                    AppError::NotFound(format!("Execution plan not found: {}", id.as_str()))
                })?
        } else {
            self.state
                .execution_plan_repo
                .get_active_for_session(&scope.session_id)
                .await?
                .ok_or_else(|| {
                    AppError::NotFound(format!(
                        "Active execution plan not found for session {}",
                        scope.session_id.as_str()
                    ))
                })?
        };

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

    async fn write_pause_reason(&self, task: &Task) -> AppResult<()> {
        let pause_reason = crate::application::chat_service::PauseReason::UserInitiated {
            previous_status: task.internal_status.to_string(),
            paused_at: chrono::Utc::now().to_rfc3339(),
            scope: "execution_plan".to_string(),
        };
        let mut task_to_update = task.clone();
        task_to_update.metadata =
            Some(pause_reason.write_to_task_metadata(task_to_update.metadata.as_deref()));
        task_to_update.touch();
        self.state.task_repo.update(&task_to_update).await
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

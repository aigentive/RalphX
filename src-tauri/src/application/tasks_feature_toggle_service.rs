use std::collections::HashSet;
use tauri::{AppHandle, Emitter};

use crate::application::chat_service::PauseReason;
use crate::application::task_cleanup_service::{is_agent_active_status, TaskCleanupService};
use crate::application::{AppState, TaskTransitionService};
use crate::domain::entities::{InternalStatus, Task};
use crate::domain::ideation::{IdeationSettings, TasksFeatureState};
use crate::domain::repositories::{BranchUpdateCasOutcome, PauseBranchUpdate};
use crate::domain::state_machine::transition_handler::metadata_builder::MetadataUpdate;
use crate::error::{AppError, AppResult};

pub const TASKS_DRAIN_INCOMPLETE_ERROR_CODE: &str = "ralphx:tasks_drain_incomplete";

#[derive(Debug, Clone, serde::Serialize)]
pub struct TasksDisableImpact {
    pub active_standalone_tasks: usize,
    pub active_attached_agent_workspaces: usize,
    pub paused_or_blocked_tasks: usize,
    pub active_branch_update_operations: usize,
    pub affected_task_ids: Vec<String>,
    pub affected_conversation_ids: Vec<String>,
    pub affected_project_ids: Vec<String>,
}

pub(crate) struct TasksFeatureToggleService<'a> {
    state: &'a AppState,
    transition_service: TaskTransitionService,
    cleanup: TaskCleanupService,
    app_handle: Option<AppHandle>,
}

impl<'a> TasksFeatureToggleService<'a> {
    pub(crate) fn new(
        state: &'a AppState,
        transition_service: TaskTransitionService,
        cleanup: TaskCleanupService,
        app_handle: Option<AppHandle>,
    ) -> Self {
        Self {
            state,
            transition_service,
            cleanup,
            app_handle,
        }
    }

    pub(crate) async fn get_disable_impact(&self) -> AppResult<TasksDisableImpact> {
        let projects = self
            .state
            .project_repo
            .get_all()
            .await
            .map_err(|error| AppError::Database(error.to_string()))?;
        let mut active_standalone_tasks = 0;
        let mut attached_conversations = HashSet::new();
        let mut paused_or_blocked_tasks = 0;
        let mut affected_task_ids = Vec::new();
        let mut affected_projects = HashSet::new();
        let mut feature_disabled_paused_task_ids = HashSet::new();

        for project in projects {
            let tasks = self
                .state
                .task_repo
                .get_by_project_filtered(&project.id, true)
                .await?;
            for task in tasks {
                if has_feature_disabled_pause_marker(&task) {
                    feature_disabled_paused_task_ids.insert(task.id.clone());
                }
                if matches!(
                    task.internal_status,
                    InternalStatus::Paused
                        | InternalStatus::Blocked
                        | InternalStatus::BranchUpdateBlocked
                ) {
                    paused_or_blocked_tasks += 1;
                }
                if !is_agent_active_status(task.internal_status) {
                    continue;
                }
                affected_task_ids.push(task.id.as_str().to_string());
                affected_projects.insert(project.id.as_str().to_string());
                let workspace = match task.ideation_session_id.as_ref() {
                    Some(session_id) => self
                        .state
                        .agent_conversation_workspace_repo
                        .get_by_task_pipeline_session_id(session_id)
                        .await
                        .map_err(|error| AppError::Database(error.to_string()))?,
                    None => None,
                };
                if let Some(workspace) = workspace {
                    attached_conversations.insert(workspace.conversation_id.as_str().to_string());
                } else {
                    active_standalone_tasks += 1;
                }
            }
        }

        let active_branch_update_operations = self
            .state
            .branch_update_repo
            .list_active_operations()
            .await?
            .into_iter()
            .filter(|operation| !feature_disabled_paused_task_ids.contains(&operation.task_id))
            .count();
        let mut affected_conversation_ids = attached_conversations.into_iter().collect::<Vec<_>>();
        affected_conversation_ids.sort();
        let mut affected_project_ids = affected_projects.into_iter().collect::<Vec<_>>();
        affected_project_ids.sort();
        affected_task_ids.sort();

        Ok(TasksDisableImpact {
            active_standalone_tasks,
            active_attached_agent_workspaces: affected_conversation_ids.len(),
            paused_or_blocked_tasks,
            active_branch_update_operations,
            affected_task_ids,
            affected_conversation_ids,
            affected_project_ids,
        })
    }

    pub(crate) async fn set_tasks_enabled(&self, enabled: bool) -> AppResult<IdeationSettings> {
        let current = self
            .state
            .ideation_settings_repo
            .get_settings()
            .await
            .map_err(|error| AppError::Database(error.to_string()))?;

        if enabled {
            match current.tasks_feature_state {
                TasksFeatureState::Enabled => return Ok(current),
                TasksFeatureState::Draining => {
                    return Err(drain_incomplete_error(
                        &["shutdown-in-progress".to_string()],
                    ));
                }
                TasksFeatureState::Disabled => {}
            }
            if !self
                .state
                .ideation_settings_repo
                .compare_and_set_tasks_feature_state(
                    TasksFeatureState::Disabled,
                    TasksFeatureState::Enabled,
                )
                .await
                .map_err(|error| AppError::Database(error.to_string()))?
            {
                return Err(AppError::Conflict(
                    "Tasks feature state changed while enabling".to_string(),
                ));
            }
            self.reconcile_missing_assessments();
            return self.current_settings().await;
        }

        match current.tasks_feature_state {
            TasksFeatureState::Disabled => {
                let failures = self.drain_active_tasks().await;
                if failures.is_empty() {
                    return Ok(current);
                }
                return Err(drain_incomplete_error(&failures));
            }
            TasksFeatureState::Enabled => {
                if !self
                    .state
                    .ideation_settings_repo
                    .compare_and_set_tasks_feature_state(
                        TasksFeatureState::Enabled,
                        TasksFeatureState::Draining,
                    )
                    .await
                    .map_err(|error| AppError::Database(error.to_string()))?
                {
                    return Err(AppError::Conflict(
                        "Tasks feature state changed while disabling".to_string(),
                    ));
                }
            }
            TasksFeatureState::Draining => {}
        }

        let failures = self.drain_active_tasks().await;
        if !failures.is_empty() {
            tracing::error!(task_ids = ?failures, "Tasks drain remains incomplete");
            return Err(drain_incomplete_error(&failures));
        }
        if !self
            .state
            .ideation_settings_repo
            .compare_and_set_tasks_feature_state(
                TasksFeatureState::Draining,
                TasksFeatureState::Disabled,
            )
            .await
            .map_err(|error| AppError::Database(error.to_string()))?
        {
            return Err(AppError::Conflict(
                "Tasks feature state changed before shutdown completed".to_string(),
            ));
        }
        self.current_settings().await
    }

    async fn current_settings(&self) -> AppResult<IdeationSettings> {
        self.state
            .ideation_settings_repo
            .get_settings()
            .await
            .map_err(|error| AppError::Database(error.to_string()))
    }

    fn reconcile_missing_assessments(&self) {
        let Some(app_handle) = self.app_handle.clone() else {
            return;
        };
        let db = self.state.db.clone();
        tauri::async_runtime::spawn(async move {
            let pending = db
                .run(|conn| {
                    crate::application::plan_complexity_assessment::list_missing_plan_complexity_assessments_sync(
                        conn, 8,
                    )
                })
                .await;
            match pending {
                Ok(pending) => {
                    for (session_id, artifact_id, artifact_version) in pending {
                        crate::application::plan_complexity_assessment::spawn_plan_complexity_assessor_from_app_handle(
                            app_handle.clone(),
                            session_id,
                            artifact_id,
                            artifact_version,
                        );
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "Failed to reconcile plan complexity assessments after Tasks re-enable"
                    );
                }
            }
        });
    }

    pub(crate) async fn drain_active_tasks(&self) -> Vec<String> {
        let projects = match self.state.project_repo.get_all().await {
            Ok(projects) => projects,
            Err(error) => {
                tracing::error!(error = %error, "Failed to enumerate projects for Tasks OFF drain");
                return vec!["project-enumeration".to_string()];
            }
        };

        let mut failures = Vec::new();
        for project in projects {
            let tasks = match self
                .state
                .task_repo
                .get_by_project_filtered(&project.id, true)
                .await
            {
                Ok(tasks) => tasks,
                Err(error) => {
                    tracing::error!(
                        project_id = project.id.as_str(),
                        error = %error,
                        "Failed to load project Tasks for OFF drain"
                    );
                    failures.push(format!("project:{}", project.id.as_str()));
                    continue;
                }
            };

            for task in tasks {
                let has_active_branch_update = match self
                    .state
                    .branch_update_repo
                    .get_active_operation(&task.id)
                    .await
                {
                    Ok(operation) => operation.is_some(),
                    Err(error) => {
                        tracing::error!(
                            task_id = task.id.as_str(),
                            error = %error,
                            "Failed to inspect branch-update authority during Tasks OFF drain"
                        );
                        failures.push(task.id.as_str().to_string());
                        continue;
                    }
                };
                if !is_agent_active_status(task.internal_status)
                    && !has_active_branch_update
                    && !has_feature_disabled_pause_marker(&task)
                {
                    continue;
                }
                if self
                    .drain_one_task(&self.transition_service, &self.cleanup, &task)
                    .await
                    .is_err()
                {
                    failures.push(task.id.as_str().to_string());
                }
            }
        }
        failures
    }

    async fn drain_one_task(
        &self,
        transition_service: &crate::application::TaskTransitionService,
        cleanup: &TaskCleanupService,
        candidate: &Task,
    ) -> AppResult<()> {
        let Some(current) = self.state.task_repo.get_by_id(&candidate.id).await? else {
            return Ok(());
        };
        if !is_agent_active_status(current.internal_status)
            && has_feature_disabled_pause_marker(&current)
        {
            cleanup
                .stop_task_runtime_contexts_strict(&current.id)
                .await?;
            return Ok(());
        }
        if !is_agent_active_status(current.internal_status) {
            let has_active_branch_update = self
                .state
                .branch_update_repo
                .get_active_operation(&current.id)
                .await?
                .is_some();
            if !has_active_branch_update {
                return Ok(());
            }
        }

        if let Some(operation) = self
            .state
            .branch_update_repo
            .get_active_operation(&current.id)
            .await?
        {
            let lease = self
                .state
                .branch_update_repo
                .get_target_lease(&operation.target_identity)
                .await?
                .ok_or_else(|| {
                    AppError::Conflict(format!(
                        "Branch-update target authority is missing for task {}",
                        current.id.as_str()
                    ))
                })?;
            let outcome = self
                .state
                .branch_update_repo
                .pause_operation(PauseBranchUpdate {
                    operation_id: operation.id,
                    task_id: current.id.clone(),
                    originating_history_id: operation.originating_history_id,
                    update_status: current.internal_status,
                    owner: lease.owner().clone(),
                    fencing_epoch: lease.fencing_epoch(),
                    history_id: uuid::Uuid::new_v4().to_string(),
                    task_metadata: Some(
                        feature_disabled_pause_metadata(&current)?
                            .merge_into(current.metadata.as_deref()),
                    ),
                })
                .await?;
            if outcome != BranchUpdateCasOutcome::Applied {
                return Err(AppError::Conflict(format!(
                    "Branch-update pause lost authority for task {}: {outcome:?}",
                    current.id.as_str()
                )));
            }
            cleanup
                .stop_task_runtime_contexts_strict(&current.id)
                .await?;
            if let Some(app_handle) = self.app_handle.as_ref() {
                let _ = app_handle.emit(
                    "task:paused",
                    serde_json::json!({
                        "taskId": current.id.as_str(),
                        "projectId": current.project_id.as_str(),
                    }),
                );
            }
            return Ok(());
        }

        let pause_metadata = feature_disabled_pause_metadata(&current)?;
        let paused = if current.archived_at.is_some() {
            transition_service
                .transition_archived_task_to_paused_with_metadata(&current.id, pause_metadata)
                .await?
        } else {
            transition_service
                .transition_task_with_metadata(
                    &current.id,
                    InternalStatus::Paused,
                    Some(pause_metadata),
                )
                .await?
        };
        if paused.internal_status != InternalStatus::Paused {
            if is_agent_active_status(paused.internal_status) {
                return Err(AppError::Conflict(format!(
                    "Task {} remained active during Tasks OFF drain",
                    paused.id.as_str()
                )));
            }
            return Ok(());
        }

        cleanup
            .stop_task_runtime_contexts_strict(&current.id)
            .await?;
        Ok(())
    }
}

fn drain_incomplete_error(failures: &[String]) -> AppError {
    AppError::FeatureDisabled(format!(
        "{TASKS_DRAIN_INCOMPLETE_ERROR_CODE}: Tasks shutdown must be retried for: {}",
        failures.join(", ")
    ))
}

fn has_feature_disabled_pause_marker(task: &Task) -> bool {
    task.internal_status == InternalStatus::Paused
        && task
            .metadata
            .as_deref()
            .and_then(|metadata| serde_json::from_str::<serde_json::Value>(metadata).ok())
            .and_then(|metadata| metadata.get("tasks_feature_disabled").cloned())
            .is_some()
}

fn feature_disabled_pause_metadata(task: &Task) -> AppResult<MetadataUpdate> {
    let paused_at = chrono::Utc::now().to_rfc3339();
    let pause_reason = PauseReason::UserInitiated {
        previous_status: task.internal_status.to_string(),
        paused_at: paused_at.clone(),
        scope: "tasks_feature_disabled".to_string(),
    };
    let pause_reason = serde_json::to_value(pause_reason).map_err(|error| {
        AppError::Validation(format!(
            "Failed to serialize Tasks OFF pause reason: {error}"
        ))
    })?;
    Ok(MetadataUpdate::new()
        .with_value("pause_reason", pause_reason)
        .with_value(
            "tasks_feature_disabled",
            serde_json::json!({
                "previous_status": task.internal_status.as_str(),
                "paused_at": paused_at,
            }),
        ))
}

use tauri::AppHandle;

use crate::application::chat_service::PauseReason;
use crate::application::task_cleanup_service::{is_agent_active_status, TaskCleanupService};
use crate::application::tasks_feature_policy::TasksFeaturePolicy;
use crate::application::{AppState, TaskTransitionService};
use crate::domain::entities::{InternalStatus, Task};
use crate::domain::ideation::IdeationSettings;
use crate::domain::state_machine::transition_handler::metadata_builder::MetadataUpdate;
use crate::error::{AppError, AppResult};

pub const TASKS_DRAIN_INCOMPLETE_ERROR_CODE: &str = "ralphx:tasks_drain_incomplete";

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

    pub(crate) async fn update_settings(
        &self,
        settings: IdeationSettings,
    ) -> AppResult<IdeationSettings> {
        let previous = self
            .state
            .ideation_settings_repo
            .get_settings()
            .await
            .map_err(|error| AppError::Database(error.to_string()))?;
        let updated = self
            .state
            .ideation_settings_repo
            .update_settings(&settings)
            .await
            .map_err(|error| AppError::Database(error.to_string()))?;

        if updated.tasks_enabled {
            if !previous.tasks_enabled {
                self.reconcile_missing_assessments();
            }
            return Ok(updated);
        }

        let failures = self.drain_unentitled_active_tasks().await;
        if failures.is_empty() {
            return Ok(updated);
        }

        tracing::error!(
            task_ids = ?failures,
            was_enabled = previous.tasks_enabled,
            "Tasks stayed disabled but some active runtime contexts could not be drained"
        );
        Err(AppError::FeatureDisabled(format!(
            "{TASKS_DRAIN_INCOMPLETE_ERROR_CODE}: Tasks are disabled, but cleanup must be retried for task(s): {}",
            failures.join(", ")
        )))
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

    pub(crate) async fn drain_unentitled_active_tasks(&self) -> Vec<String> {
        let policy = TasksFeaturePolicy::from_state(self.state);

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
                .get_by_project_filtered(&project.id, false)
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
                if !is_agent_active_status(task.internal_status) {
                    continue;
                }
                if self
                    .drain_one_task(&policy, &self.transition_service, &self.cleanup, &task)
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
        policy: &TasksFeaturePolicy,
        transition_service: &crate::application::TaskTransitionService,
        cleanup: &TaskCleanupService,
        candidate: &Task,
    ) -> AppResult<()> {
        let Some(current) = self.state.task_repo.get_by_id(&candidate.id).await? else {
            return Ok(());
        };
        if !is_agent_active_status(current.internal_status)
            || policy
                .is_session_authorized(current.ideation_session_id.as_ref())
                .await
        {
            return Ok(());
        }

        let paused = transition_service
            .transition_task_with_metadata(
                &current.id,
                InternalStatus::Paused,
                Some(feature_disabled_pause_metadata(&current)?),
            )
            .await?;
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

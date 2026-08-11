use std::sync::Arc;

use crate::application::AppState;
use crate::domain::entities::IdeationSessionId;
use crate::domain::ideation::{TasksFeatureAction, TasksFeatureState};
use crate::domain::repositories::IdeationSettingsRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::sqlite_ideation_settings_repo;

pub const TASKS_DISABLED_ERROR_CODE: &str = "ralphx:tasks_disabled";
pub const TASKS_DISABLED_MESSAGE: &str =
    "ralphx:tasks_disabled: Tasks are disabled in Planning & Verification settings";

pub(crate) struct TasksFeaturePolicy {
    settings_repo: Arc<dyn IdeationSettingsRepository>,
}

impl TasksFeaturePolicy {
    pub(crate) fn new(settings_repo: Arc<dyn IdeationSettingsRepository>) -> Self {
        Self { settings_repo }
    }

    pub(crate) fn from_state(state: &AppState) -> Self {
        Self::new(Arc::clone(&state.ideation_settings_repo))
    }

    pub(crate) async fn authorize_session(
        &self,
        _session_id: Option<&IdeationSessionId>,
        action: TasksFeatureAction,
    ) -> AppResult<()> {
        if action == TasksFeatureAction::Quiesce {
            return Ok(());
        }
        let settings = self
            .settings_repo
            .get_settings()
            .await
            .map_err(|error| disabled_error(format!("settings could not be read: {error}")))?;
        if settings.tasks_feature_state == TasksFeatureState::Enabled {
            return Ok(());
        }
        Err(disabled_error(format!(
            "{:?} is blocked while Tasks are {}",
            action,
            settings.tasks_feature_state.as_str()
        )))
    }
}

pub(crate) fn authorize_tasks_session_sync(
    conn: &rusqlite::Connection,
    session_id: Option<&str>,
    action: TasksFeatureAction,
) -> AppResult<()> {
    sqlite_ideation_settings_repo::authorize_tasks_session_sync(conn, session_id, action)
}

fn disabled_error(detail: impl AsRef<str>) -> AppError {
    AppError::FeatureDisabled(format!(
        "{TASKS_DISABLED_ERROR_CODE}: Tasks are disabled in Planning & Verification settings ({})",
        detail.as_ref()
    ))
}

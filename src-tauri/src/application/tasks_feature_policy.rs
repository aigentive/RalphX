use std::sync::Arc;

use crate::application::AppState;
use crate::domain::entities::{AgentConversationWorkspaceStatus, IdeationSessionId};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, IdeationSessionRepository, IdeationSettingsRepository,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::sqlite_ideation_settings_repo;

pub const TASKS_DISABLED_ERROR_CODE: &str = "ralphx:tasks_disabled";
pub const TASKS_DISABLED_MESSAGE: &str =
    "ralphx:tasks_disabled: Tasks are disabled in Planning & Verification settings";

pub(crate) struct TasksFeaturePolicy {
    settings_repo: Arc<dyn IdeationSettingsRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    session_repo: Arc<dyn IdeationSessionRepository>,
}

impl TasksFeaturePolicy {
    pub(crate) fn new(
        settings_repo: Arc<dyn IdeationSettingsRepository>,
        workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
        session_repo: Arc<dyn IdeationSessionRepository>,
    ) -> Self {
        Self {
            settings_repo,
            workspace_repo,
            session_repo,
        }
    }

    pub(crate) fn from_state(state: &AppState) -> Self {
        Self::new(
            Arc::clone(&state.ideation_settings_repo),
            Arc::clone(&state.agent_conversation_workspace_repo),
            Arc::clone(&state.ideation_session_repo),
        )
    }

    pub(crate) async fn authorize_session(
        &self,
        session_id: Option<&IdeationSessionId>,
    ) -> AppResult<()> {
        let settings = self
            .settings_repo
            .get_settings()
            .await
            .map_err(|error| disabled_error(format!("settings could not be read: {error}")))?;
        if settings.tasks_enabled {
            return Ok(());
        }

        let session_id = session_id.ok_or_else(|| disabled_error("standalone Task"))?;
        let workspace = self
            .workspace_repo
            .get_by_task_pipeline_session_id(session_id)
            .await
            .map_err(|error| disabled_error(format!("attachment could not be read: {error}")))?
            .ok_or_else(|| disabled_error("Task is not attached to an Agent pipeline"))?;
        if workspace.status != AgentConversationWorkspaceStatus::Active
            || workspace.task_pipeline_session_id.as_ref() != Some(session_id)
        {
            return Err(disabled_error("Agent pipeline is not active"));
        }

        let session = self
            .session_repo
            .get_by_id(session_id)
            .await
            .map_err(|error| {
                disabled_error(format!("pipeline session could not be read: {error}"))
            })?
            .ok_or_else(|| disabled_error("pipeline session is missing"))?;
        if session.project_id != workspace.project_id {
            return Err(disabled_error(
                "pipeline project does not match its workspace",
            ));
        }

        Ok(())
    }

    pub(crate) async fn is_session_authorized(
        &self,
        session_id: Option<&IdeationSessionId>,
    ) -> bool {
        self.authorize_session(session_id).await.is_ok()
    }
}

pub(crate) fn authorize_tasks_session_sync(
    conn: &rusqlite::Connection,
    session_id: Option<&str>,
) -> AppResult<()> {
    sqlite_ideation_settings_repo::authorize_tasks_session_sync(conn, session_id)
}

fn disabled_error(detail: impl AsRef<str>) -> AppError {
    AppError::FeatureDisabled(format!(
        "{TASKS_DISABLED_ERROR_CODE}: Tasks are disabled in Planning & Verification settings ({})",
        detail.as_ref()
    ))
}

use std::sync::Arc;

use async_trait::async_trait;

use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversationId,
    IdeationAnalysisBaseRefKind, IdeationSession, InternalStatus, Project, Task,
};
use crate::domain::ideation::IdeationSettings;
use crate::domain::repositories::ProjectRepository;
use crate::error::{AppError, AppResult};

struct FailingProjectRepository;

#[async_trait]
impl ProjectRepository for FailingProjectRepository {
    async fn create(&self, _project: Project) -> AppResult<Project> {
        Err(AppError::Database(
            "injected project repository failure".into(),
        ))
    }

    async fn get_by_id(
        &self,
        _id: &crate::domain::entities::ProjectId,
    ) -> AppResult<Option<Project>> {
        Err(AppError::Database(
            "injected project repository failure".into(),
        ))
    }

    async fn get_all(&self) -> AppResult<Vec<Project>> {
        Err(AppError::Database(
            "injected project enumeration failure".into(),
        ))
    }

    async fn update(&self, _project: &Project) -> AppResult<()> {
        Err(AppError::Database(
            "injected project repository failure".into(),
        ))
    }

    async fn delete(&self, _id: &crate::domain::entities::ProjectId) -> AppResult<()> {
        Err(AppError::Database(
            "injected project repository failure".into(),
        ))
    }

    async fn get_by_working_directory(&self, _path: &str) -> AppResult<Option<Project>> {
        Err(AppError::Database(
            "injected project repository failure".into(),
        ))
    }

    async fn archive(&self, _id: &crate::domain::entities::ProjectId) -> AppResult<Project> {
        Err(AppError::Database(
            "injected project repository failure".into(),
        ))
    }
}

#[tokio::test]
async fn disabling_tasks_pauses_only_active_unentitled_tasks_and_keeps_off() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Policy project".to_string(),
            "/tmp/policy-project".to_string(),
        ))
        .await
        .unwrap();
    let session = IdeationSession::new(project.id.clone());
    state
        .ideation_session_repo
        .create(session.clone())
        .await
        .unwrap();

    let mut workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("conversation-1"),
        project.id.clone(),
        AgentConversationWorkspaceMode::Tasks,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/conversation-1".to_string(),
        "/tmp/policy-worktree".to_string(),
    );
    workspace.task_pipeline_session_id = Some(session.id.clone());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .unwrap();

    let mut standalone_active = Task::new(project.id.clone(), "Standalone active".to_string());
    standalone_active.internal_status = InternalStatus::Executing;
    let standalone_active = state.task_repo.create(standalone_active).await.unwrap();

    let standalone_ready = state
        .task_repo
        .create(Task::new(
            project.id.clone(),
            "Standalone ready".to_string(),
        ))
        .await
        .unwrap();

    let mut attached_active = Task::new(project.id.clone(), "Attached active".to_string());
    attached_active.internal_status = InternalStatus::Reviewing;
    attached_active.ideation_session_id = Some(session.id.clone());
    let attached_active = state.task_repo.create(attached_active).await.unwrap();

    state
        .build_tasks_feature_toggle_service_for_test()
        .update_settings(IdeationSettings::default())
        .await
        .expect("OFF drain should succeed");

    let paused = state
        .task_repo
        .get_by_id(&standalone_active.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(paused.internal_status, InternalStatus::Paused);
    assert!(paused
        .metadata
        .as_deref()
        .is_some_and(|metadata| metadata.contains("tasks_feature_disabled")));
    assert_eq!(
        state
            .task_repo
            .get_by_id(&standalone_ready.id)
            .await
            .unwrap()
            .unwrap()
            .internal_status,
        InternalStatus::Backlog
    );
    assert_eq!(
        state
            .task_repo
            .get_by_id(&attached_active.id)
            .await
            .unwrap()
            .unwrap()
            .internal_status,
        InternalStatus::Reviewing
    );
    assert!(
        !state
            .ideation_settings_repo
            .get_settings()
            .await
            .unwrap()
            .tasks_enabled
    );
}

#[tokio::test]
async fn disabling_tasks_keeps_off_when_drain_cannot_enumerate_projects() {
    let mut state = AppState::new_test();
    state.project_repo = Arc::new(FailingProjectRepository);

    let error = state
        .build_tasks_feature_toggle_service_for_test()
        .update_settings(IdeationSettings::default())
        .await
        .expect_err("drain failure must be reported after committing OFF");

    assert!(error
        .to_string()
        .starts_with("ralphx:tasks_drain_incomplete"));
    assert!(
        !state
            .ideation_settings_repo
            .get_settings()
            .await
            .unwrap()
            .tasks_enabled
    );
}

#[tokio::test]
async fn enabling_tasks_persists_the_setting_without_an_app_handle() {
    let state = AppState::new_test();

    let updated = state
        .build_tasks_feature_toggle_service_for_test()
        .update_settings(IdeationSettings {
            tasks_enabled: true,
            ..Default::default()
        })
        .await
        .expect("re-enabling Tasks without the desktop app handle must persist the setting");

    assert!(updated.tasks_enabled);
    assert!(
        state
            .ideation_settings_repo
            .get_settings()
            .await
            .unwrap()
            .tasks_enabled
    );
}

use super::remote_resume_commands::*;
use crate::application::AppState;
use crate::domain::entities::RemoteExecutionResumeRequest;
use crate::domain::entities::{InternalStatus, Project, RemoteResumeRequestStatus, Task};
use crate::domain::repositories::RemoteExecutionResumeRequestRepository;
use crate::error::{AppError, AppResult};
use async_trait::async_trait;
use std::sync::Arc;
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Manager;

struct FailingExecutionResumeRepo;

#[async_trait]
impl RemoteExecutionResumeRequestRepository for FailingExecutionResumeRepo {
    async fn create_execution_resume_request(
        &self,
        _: RemoteExecutionResumeRequest,
    ) -> AppResult<RemoteExecutionResumeRequest> {
        panic!("create must not run after failed dedupe read")
    }
    async fn get(&self, _: &str) -> AppResult<Option<RemoteExecutionResumeRequest>> {
        Err(AppError::Database("offline".into()))
    }
    async fn find_unsettled(
        &self,
        _: Option<&crate::domain::entities::ProjectId>,
    ) -> AppResult<Option<RemoteExecutionResumeRequest>> {
        Err(AppError::Database("offline".into()))
    }
    async fn claim_pending(
        &self,
        _: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<Option<RemoteExecutionResumeRequest>> {
        Err(AppError::Database("offline".into()))
    }
    async fn complete(
        &self,
        _: &str,
        _: serde_json::Value,
        _: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<()> {
        Err(AppError::Database("offline".into()))
    }
    async fn fail(&self, _: &str, _: &str, _: chrono::DateTime<chrono::Utc>) -> AppResult<()> {
        Err(AppError::Database("offline".into()))
    }
    async fn fail_stale(
        &self,
        _: chrono::DateTime<chrono::Utc>,
        _: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<u64> {
        Err(AppError::Database("offline".into()))
    }
}

async fn seed_project(state: &AppState) -> Project {
    state
        .project_repo
        .create(Project::new(
            "Remote resume".into(),
            "/tmp/remote-resume".into(),
        ))
        .await
        .expect("seed project")
}

async fn seed_task(state: &AppState, status: InternalStatus) -> Task {
    let project = seed_project(state).await;
    let mut task = Task::new(project.id, "Resume me".into());
    task.internal_status = status;
    state.task_repo.create(task).await.expect("seed task")
}

#[tokio::test]
async fn execution_resume_validates_then_persists_and_deduplicates() {
    let state = AppState::new_test();
    let project = seed_project(&state).await;
    let input = || RequestRemoteExecutionResumeInput {
        project_id: Some(project.id.as_str().to_string()),
    };
    let first = request_remote_execution_resume_for_state(&state, input())
        .await
        .expect("persist");
    let second = request_remote_execution_resume_for_state(&state, input())
        .await
        .expect("dedupe");
    assert_eq!(first.status, RemoteResumeRequestStatus::Pending);
    assert_eq!(first.request_id, second.request_id);
    assert!(second.deduplicated);
}

#[tokio::test]
async fn execution_resume_rejects_missing_project_without_persisting() {
    let state = AppState::new_test();
    let error = request_remote_execution_resume_for_state(
        &state,
        RequestRemoteExecutionResumeInput {
            project_id: Some("missing".into()),
        },
    )
    .await
    .expect_err("reject");
    assert_eq!(error, REMOTE_RESUME_PROJECT_NOT_FOUND);
    assert!(state
        .remote_execution_resume_request_repo
        .find_unsettled(Some(&crate::domain::entities::ProjectId::from_string(
            "missing".to_string()
        )))
        .await
        .expect("read")
        .is_none());
}

#[tokio::test]
async fn execution_resume_repo_error_fails_closed_before_create() {
    let mut state = AppState::new_test();
    state.remote_execution_resume_request_repo = Arc::new(FailingExecutionResumeRepo);
    let error = request_remote_execution_resume_for_state(
        &state,
        RequestRemoteExecutionResumeInput { project_id: None },
    )
    .await
    .expect_err("repo outage rejects");
    assert_eq!(error, REMOTE_RESUME_LOOKUP_FAILED);
}

#[tokio::test]
async fn task_resume_requires_paused_and_persists_last() {
    let state = AppState::new_test();
    let task = seed_task(&state, InternalStatus::Ready).await;
    let error = request_remote_task_resume_for_state(
        &state,
        RequestRemoteTaskResumeInput {
            task_id: task.id.as_str().to_string(),
        },
    )
    .await
    .expect_err("reject");
    assert_eq!(error, REMOTE_RESUME_TASK_NOT_PAUSED);
    assert!(state
        .remote_task_action_request_repo
        .find_unsettled_for_task(&task.id)
        .await
        .expect("read")
        .is_none());
    let mut paused = task;
    paused.internal_status = InternalStatus::Paused;
    state.task_repo.update(&paused).await.expect("pause");
    let response = request_remote_task_resume_for_state(
        &state,
        RequestRemoteTaskResumeInput {
            task_id: paused.id.as_str().to_string(),
        },
    )
    .await
    .expect("persist");
    assert_eq!(response.status, RemoteResumeRequestStatus::Pending);
}

#[tokio::test]
async fn restart_and_group_are_distinct_actions_in_one_task_queue() {
    let state = AppState::new_test();
    let stopped = seed_task(&state, InternalStatus::Stopped).await;
    let restart = request_remote_task_restart_for_state(
        &state,
        RequestRemoteTaskRestartInput {
            task_id: stopped.id.as_str().to_string(),
            force: true,
            note: Some("retry".into()),
        },
    )
    .await
    .expect("restart");
    let stored = state
        .remote_task_action_request_repo
        .get(&restart.request_id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(
        stored.action,
        crate::domain::entities::RemoteTaskAction::Restart
    );
    assert!(stored.force);
    let project = state
        .project_repo
        .get_by_id(&stopped.project_id)
        .await
        .expect("read project")
        .expect("project");
    let group = request_remote_group_resume_for_state(
        &state,
        RequestRemoteGroupResumeInput {
            group_kind: "status".into(),
            group_id: "paused".into(),
            project_id: project.id.as_str().to_string(),
        },
    )
    .await
    .expect("group");
    let stored = state
        .remote_task_action_request_repo
        .get(&group.request_id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(
        stored.action,
        crate::domain::entities::RemoteTaskAction::GroupResume
    );
}

#[tokio::test]
async fn execution_and_restart_wrappers_match_their_state_seams() {
    let state = AppState::new_test();
    let task = seed_task(&state, InternalStatus::Ready).await;
    let execution = Arc::new(crate::commands::ExecutionState::new());
    let active = Arc::new(crate::commands::ActiveProjectState::new());
    let app = mock_builder()
        .manage(state)
        .manage(Arc::clone(&execution))
        .manage(Arc::clone(&active))
        .build(mock_context(noop_assets()))
        .expect("mock app");

    let wrapper_execution = crate::commands::execution_commands::resume_execution(
        Some("missing".to_string()),
        app.state::<Arc<crate::commands::ActiveProjectState>>(),
        app.state::<Arc<crate::commands::ExecutionState>>(),
        app.state::<AppState>(),
    )
    .await
    .expect_err("wrapper rejects");
    let seam_execution = crate::commands::execution_commands::resume_execution_for_state(
        Some("missing".to_string()),
        &active,
        &execution,
        app.state::<AppState>().inner(),
    )
    .await
    .expect_err("seam rejects");
    assert_eq!(wrapper_execution, seam_execution);

    let wrapper_restart = crate::commands::execution_commands::restart_task(
        task.id.as_str().to_string(),
        false,
        None,
        app.state::<AppState>(),
        app.state::<Arc<crate::commands::ExecutionState>>(),
    )
    .await
    .expect_err("wrapper rejects");
    let seam_restart = crate::commands::execution_commands::restart_task_for_state(
        task.id.as_str().to_string(),
        false,
        None,
        app.state::<AppState>().inner(),
        &execution,
    )
    .await
    .expect_err("seam rejects");
    assert_eq!(wrapper_restart, seam_restart);
}

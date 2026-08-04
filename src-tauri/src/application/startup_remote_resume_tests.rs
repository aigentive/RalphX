use std::sync::Arc;

use chrono::{Duration, Utc};

use super::startup_background::{
    claim_and_revalidate_remote_task_action, dispatch_one_remote_execution_resume,
};
use super::AppState;
use crate::commands::remote_resume_commands::{
    request_remote_execution_resume_for_state, request_remote_task_resume_for_state,
    RequestRemoteExecutionResumeInput, RequestRemoteTaskResumeInput,
    REMOTE_RESUME_AUTHORITY_CHANGED,
};
use crate::commands::{ActiveProjectState, ExecutionState};
use crate::domain::entities::{InternalStatus, Project, RemoteResumeRequestStatus, Task};

#[tokio::test]
async fn execution_dispatch_executes_once_and_persists_terminal_result() {
    let state = AppState::new_test();
    let requested = request_remote_execution_resume_for_state(
        &state,
        RequestRemoteExecutionResumeInput { project_id: None },
    )
    .await
    .expect("request");
    let active = Arc::new(ActiveProjectState::new());
    let execution = Arc::new(ExecutionState::new());

    dispatch_one_remote_execution_resume(&state, &active, &execution)
        .await
        .expect("first dispatch");
    let completed = state
        .remote_execution_resume_request_repo
        .get(&requested.request_id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(completed.status, RemoteResumeRequestStatus::Completed);
    assert!(completed.result.is_some());

    dispatch_one_remote_execution_resume(&state, &active, &execution)
        .await
        .expect("re-entry is idle");
    let after_reentry = state
        .remote_execution_resume_request_repo
        .get(&requested.request_id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(after_reentry, completed);
}

#[tokio::test]
async fn claimed_execution_intent_is_revalidated_and_fails_closed() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new("Claim race".into(), "/tmp/claim-race".into()))
        .await
        .expect("seed");
    let requested = request_remote_execution_resume_for_state(
        &state,
        RequestRemoteExecutionResumeInput {
            project_id: Some(project.id.as_str().to_string()),
        },
    )
    .await
    .expect("request");
    state
        .project_repo
        .delete(&project.id)
        .await
        .expect("delete after request");
    dispatch_one_remote_execution_resume(
        &state,
        &Arc::new(ActiveProjectState::new()),
        &Arc::new(ExecutionState::new()),
    )
    .await
    .expect("dispatch");
    let row = state
        .remote_execution_resume_request_repo
        .get(&requested.request_id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(row.status, RemoteResumeRequestStatus::Failed);
    assert_eq!(
        row.error_code.as_deref(),
        Some(REMOTE_RESUME_AUTHORITY_CHANGED)
    );
}

#[tokio::test]
async fn stale_claims_become_failed_stale_and_are_never_claimed_again() {
    let state = AppState::new_test();
    let requested = request_remote_execution_resume_for_state(
        &state,
        RequestRemoteExecutionResumeInput { project_id: None },
    )
    .await
    .expect("request");
    let claimed_at = Utc::now() - Duration::minutes(10);
    state
        .remote_execution_resume_request_repo
        .claim_pending(claimed_at)
        .await
        .expect("claim")
        .expect("row");
    assert_eq!(
        state
            .remote_execution_resume_request_repo
            .fail_stale(Utc::now() - Duration::minutes(5), Utc::now())
            .await
            .expect("sweep"),
        1
    );
    assert!(state
        .remote_execution_resume_request_repo
        .claim_pending(Utc::now())
        .await
        .expect("claim again")
        .is_none());
    let row = state
        .remote_execution_resume_request_repo
        .get(&requested.request_id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(row.status, RemoteResumeRequestStatus::FailedStale);
}

#[tokio::test]
async fn task_claim_revalidates_wrong_state_and_suppresses_execution_authority() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Task claim race".into(),
            "/tmp/task-claim-race".into(),
        ))
        .await
        .expect("seed project");
    let mut task = Task::new(project.id, "Resume race".into());
    task.internal_status = InternalStatus::Paused;
    let mut task = state.task_repo.create(task).await.expect("seed task");
    let requested = request_remote_task_resume_for_state(
        &state,
        RequestRemoteTaskResumeInput {
            task_id: task.id.as_str().to_string(),
        },
    )
    .await
    .expect("request");

    task.internal_status = InternalStatus::Ready;
    state.task_repo.update(&task).await.expect("race status");
    let claimed = claim_and_revalidate_remote_task_action(&state)
        .await
        .expect("revalidate");
    assert!(claimed.is_none(), "rejected row must not reach host seam");
    let row = state
        .remote_task_action_request_repo
        .get(&requested.request_id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(row.status, RemoteResumeRequestStatus::Failed);
    assert_eq!(
        row.error_code.as_deref(),
        Some(REMOTE_RESUME_AUTHORITY_CHANGED)
    );
    let unchanged = state
        .task_repo
        .get_by_id(&task.id)
        .await
        .expect("read task")
        .expect("task");
    assert_eq!(unchanged.internal_status, InternalStatus::Ready);
}

#[tokio::test]
async fn valid_task_claim_is_returned_once_and_duplicate_claim_is_absent() {
    let state = AppState::new_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Valid task claim".into(),
            "/tmp/valid-task-claim".into(),
        ))
        .await
        .expect("seed project");
    let mut task = Task::new(project.id, "Resume once".into());
    task.internal_status = InternalStatus::Paused;
    let task = state.task_repo.create(task).await.expect("seed task");
    let requested = request_remote_task_resume_for_state(
        &state,
        RequestRemoteTaskResumeInput {
            task_id: task.id.as_str().to_string(),
        },
    )
    .await
    .expect("request");

    let claimed = claim_and_revalidate_remote_task_action(&state)
        .await
        .expect("claim")
        .expect("valid authority");
    assert_eq!(claimed.id, requested.request_id);
    assert_eq!(claimed.status, RemoteResumeRequestStatus::Starting);
    assert!(claim_and_revalidate_remote_task_action(&state)
        .await
        .expect("duplicate claim")
        .is_none());
}

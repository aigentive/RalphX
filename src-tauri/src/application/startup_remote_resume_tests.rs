use std::sync::Arc;

use chrono::{Duration, Utc};

use super::startup_background::dispatch_one_remote_execution_resume;
use super::AppState;
use crate::commands::remote_resume_commands::{
    request_remote_execution_resume_for_state, RequestRemoteExecutionResumeInput,
    REMOTE_RESUME_AUTHORITY_CHANGED,
};
use crate::commands::{ActiveProjectState, ExecutionState};
use crate::domain::entities::{Project, RemoteResumeRequestStatus};

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

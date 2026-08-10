use chrono::{Duration, Utc};

use super::remote_finalize_decision_intent::{
    request_remote_finalize_decision_for_state, RequestRemoteFinalizeDecisionInput,
    REMOTE_FINALIZE_AUTHORITY_CHANGED, REMOTE_FINALIZE_NOT_PENDING,
};
use super::startup_background::dispatch_one_remote_finalize_decision;
use super::{AppState, ExecutionState};
use crate::domain::entities::{
    AcceptanceStatus, IdeationSession, IdeationSessionStatus, Priority, Project, ProjectId,
    ProposalCategory, RemoteFinalizeDecision, RemoteFinalizeDecisionRequestStatus, TaskProposal,
};
use std::sync::Arc;

async fn pending_session(state: &AppState) -> IdeationSession {
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(ProjectId::from_string(
            "remote-finalize-project".into(),
        )))
        .await
        .expect("seed");
    state
        .ideation_session_repo
        .update_acceptance_status(&session.id, None, Some(AcceptanceStatus::Pending))
        .await
        .expect("mark pending");
    state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("reload")
        .expect("session")
}

async fn request(
    state: &AppState,
    session: &IdeationSession,
    decision: RemoteFinalizeDecision,
) -> String {
    request_remote_finalize_decision_for_state(
        state,
        RequestRemoteFinalizeDecisionInput {
            session_id: session.id.as_str().to_string(),
            decision,
        },
    )
    .await
    .expect("request")
    .request_id
}

async fn task_count(state: &AppState, session: &IdeationSession) -> u32 {
    state
        .task_repo
        .count_tasks(&session.project_id, true, Some(session.id.as_str()), None)
        .await
        .expect("count tasks")
}

#[tokio::test]
async fn remote_finalize_accept_creates_tasks_once_and_reentry_is_idle() {
    let state = AppState::new_sqlite_for_apply_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Remote finalize project".to_string(),
            "/tmp/ralphx-remote-finalize-project".to_string(),
        ))
        .await
        .expect("seed project");
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id))
        .await
        .expect("seed session");
    state
        .ideation_session_repo
        .update_acceptance_status(&session.id, None, Some(AcceptanceStatus::Pending))
        .await
        .expect("mark pending");
    state
        .task_proposal_repo
        .create(TaskProposal::new(
            session.id.clone(),
            "Remote accepted task",
            ProposalCategory::Feature,
            Priority::Medium,
        ))
        .await
        .expect("seed proposal");
    let id = request(&state, &session, RemoteFinalizeDecision::Accept).await;

    dispatch_one_remote_finalize_decision(&state, &Arc::new(ExecutionState::new()))
        .await
        .expect("dispatch accept");
    let completed = state
        .remote_finalize_decision_request_repo
        .get(&id)
        .await
        .expect("read request")
        .expect("request row");
    assert_eq!(
        completed.status,
        RemoteFinalizeDecisionRequestStatus::Completed
    );
    let created_count = task_count(&state, &session).await;
    assert_eq!(
        created_count, 2,
        "proposal task and merge task are created once"
    );

    dispatch_one_remote_finalize_decision(&state, &Arc::new(ExecutionState::new()))
        .await
        .expect("re-entry is idle");
    assert_eq!(
        task_count(&state, &session).await,
        created_count,
        "re-entry must create no task"
    );
    assert_eq!(
        state
            .remote_finalize_decision_request_repo
            .get(&id)
            .await
            .expect("read")
            .expect("row"),
        completed,
        "settled request must not change"
    );
}

#[tokio::test]
async fn remote_finalize_accept_lost_pending_cas_creates_zero_tasks() {
    let state = AppState::new_sqlite_for_apply_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Remote finalize race".to_string(),
            "/tmp/ralphx-remote-finalize-race".to_string(),
        ))
        .await
        .expect("seed project");
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id))
        .await
        .expect("seed session");
    state
        .ideation_session_repo
        .update_acceptance_status(&session.id, None, Some(AcceptanceStatus::Pending))
        .await
        .expect("mark pending");
    let id = request(&state, &session, RemoteFinalizeDecision::Accept).await;
    state
        .ideation_session_repo
        .update_acceptance_status(&session.id, Some(AcceptanceStatus::Pending), None)
        .await
        .expect("consume acceptance elsewhere");

    dispatch_one_remote_finalize_decision(&state, &Arc::new(ExecutionState::new()))
        .await
        .expect("dispatch lost race");
    let failed = state
        .remote_finalize_decision_request_repo
        .get(&id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(
        failed.error_code.as_deref(),
        Some(REMOTE_FINALIZE_NOT_PENDING)
    );
    assert_eq!(
        task_count(&state, &session).await,
        0,
        "lost CAS must create no tasks"
    );
}

#[tokio::test]
async fn remote_finalize_accept_repository_error_leaves_claim_unsettled() {
    let state = AppState::new_sqlite_for_apply_test();
    let session = pending_session(&state).await;
    let id = request(&state, &session, RemoteFinalizeDecision::Accept).await;
    state
        .db
        .run(|conn| {
            conn.execute("DROP TABLE task_proposals", [])?;
            Ok(())
        })
        .await
        .expect("inject proposal repository failure");

    dispatch_one_remote_finalize_decision(&state, &Arc::new(ExecutionState::new()))
        .await
        .expect_err("repository failure must propagate");
    let starting = state
        .remote_finalize_decision_request_repo
        .get(&id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(
        starting.status,
        RemoteFinalizeDecisionRequestStatus::Starting
    );
    assert!(
        starting.result.is_none(),
        "repository error must not write a result"
    );
    assert!(
        starting.error_code.is_none(),
        "repository error must not write a terminal code"
    );
    assert_eq!(task_count(&state, &session).await, 0);
}

#[tokio::test]
async fn remote_finalize_rejects_once_and_reentry_is_idle() {
    let state = AppState::new_test();
    let session = pending_session(&state).await;
    let id = request(&state, &session, RemoteFinalizeDecision::Reject).await;
    dispatch_one_remote_finalize_decision(&state, &Arc::new(ExecutionState::new()))
        .await
        .expect("dispatch");
    let completed = state
        .remote_finalize_decision_request_repo
        .get(&id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(
        completed.status,
        RemoteFinalizeDecisionRequestStatus::Completed
    );
    assert!(state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("read session")
        .expect("session")
        .acceptance_status
        .is_none());
    dispatch_one_remote_finalize_decision(&state, &Arc::new(ExecutionState::new()))
        .await
        .expect("idle");
    assert_eq!(
        state
            .remote_finalize_decision_request_repo
            .get(&id)
            .await
            .expect("read")
            .expect("row"),
        completed
    );
}

#[tokio::test]
async fn remote_finalize_revalidation_and_stale_claim_fail_closed() {
    let state = AppState::new_test();
    let session = pending_session(&state).await;
    let id = request(&state, &session, RemoteFinalizeDecision::Reject).await;
    state
        .ideation_session_repo
        .update_status(&session.id, IdeationSessionStatus::Archived)
        .await
        .expect("archive");
    dispatch_one_remote_finalize_decision(&state, &Arc::new(ExecutionState::new()))
        .await
        .expect("dispatch");
    assert_eq!(
        state
            .remote_finalize_decision_request_repo
            .get(&id)
            .await
            .expect("read")
            .expect("row")
            .error_code
            .as_deref(),
        Some(REMOTE_FINALIZE_AUTHORITY_CHANGED)
    );

    let state = AppState::new_test();
    let session = pending_session(&state).await;
    let id = request(&state, &session, RemoteFinalizeDecision::Accept).await;
    state
        .remote_finalize_decision_request_repo
        .claim_pending(Utc::now() - Duration::minutes(10))
        .await
        .expect("claim");
    assert_eq!(
        state
            .remote_finalize_decision_request_repo
            .fail_stale(Utc::now() - Duration::minutes(5), Utc::now())
            .await
            .expect("sweep"),
        1
    );
    dispatch_one_remote_finalize_decision(&state, &Arc::new(ExecutionState::new()))
        .await
        .expect("idle");
    let stale = state
        .remote_finalize_decision_request_repo
        .get(&id)
        .await
        .expect("read")
        .expect("row");
    assert_eq!(
        stale.status,
        RemoteFinalizeDecisionRequestStatus::FailedStale
    );
    assert!(stale.result.is_none());
}

#[tokio::test]
async fn remote_finalize_reject_lost_pending_cas_settles_not_pending() {
    let state = AppState::new_test();
    let session = pending_session(&state).await;
    let id = request(&state, &session, RemoteFinalizeDecision::Reject).await;
    state
        .ideation_session_repo
        .update_acceptance_status(&session.id, Some(AcceptanceStatus::Pending), None)
        .await
        .expect("race");
    dispatch_one_remote_finalize_decision(&state, &Arc::new(ExecutionState::new()))
        .await
        .expect("dispatch");
    assert_eq!(
        state
            .remote_finalize_decision_request_repo
            .get(&id)
            .await
            .expect("read")
            .expect("row")
            .error_code
            .as_deref(),
        Some(REMOTE_FINALIZE_NOT_PENDING)
    );
}

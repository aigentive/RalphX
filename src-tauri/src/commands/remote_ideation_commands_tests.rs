use super::remote_ideation_commands::*;
use crate::application::AppState;
use crate::domain::entities::{
    AcceptanceStatus, IdeationSession, ProjectId, RemoteFinalizeDecision,
};

async fn pending_session(state: &AppState) -> IdeationSession {
    let mut session =
        IdeationSession::new(ProjectId::from_string("remote-finalize-project".into()));
    session.acceptance_status = Some(AcceptanceStatus::Pending);
    state
        .ideation_session_repo
        .create(session)
        .await
        .expect("seed session")
}

fn input(
    session: &IdeationSession,
    decision: RemoteFinalizeDecision,
) -> RequestRemoteFinalizeDecisionInput {
    RequestRemoteFinalizeDecisionInput {
        session_id: session.id.as_str().to_string(),
        decision,
    }
}

#[tokio::test]
async fn remote_ideation_finalize_persists_both_decisions_dedups_same_and_rejects_flip() {
    for decision in [
        RemoteFinalizeDecision::Accept,
        RemoteFinalizeDecision::Reject,
    ] {
        let state = AppState::new_test();
        let session = pending_session(&state).await;
        let first = request_remote_finalize_decision_for_state(&state, input(&session, decision))
            .await
            .expect("persist");
        let duplicate =
            request_remote_finalize_decision_for_state(&state, input(&session, decision))
                .await
                .expect("dedupe");
        assert_eq!(duplicate.request_id, first.request_id);
        assert!(duplicate.deduplicated);
        let other = if decision == RemoteFinalizeDecision::Accept {
            RemoteFinalizeDecision::Reject
        } else {
            RemoteFinalizeDecision::Accept
        };
        assert_eq!(
            request_remote_finalize_decision_for_state(&state, input(&session, other))
                .await
                .expect_err("verdict flip refused"),
            REMOTE_FINALIZE_AUTHORITY_CHANGED
        );
    }
}

#[tokio::test]
async fn remote_ideation_finalize_validation_is_fail_closed_and_leaves_no_row() {
    let state = AppState::new_test();
    let session = IdeationSession::new(ProjectId::from_string("remote-finalize-project".into()));
    let session = state
        .ideation_session_repo
        .create(session)
        .await
        .expect("seed session");
    assert_eq!(
        request_remote_finalize_decision_for_state(
            &state,
            input(&session, RemoteFinalizeDecision::Accept)
        )
        .await
        .expect_err("pending required"),
        REMOTE_FINALIZE_NOT_PENDING
    );
    assert!(state
        .remote_finalize_decision_request_repo
        .find_unsettled_for_session(&session.id)
        .await
        .expect("read")
        .is_none());
    assert_eq!(
        request_remote_finalize_decision_for_state(
            &state,
            RequestRemoteFinalizeDecisionInput {
                session_id: "missing".into(),
                decision: RemoteFinalizeDecision::Reject
            }
        )
        .await
        .expect_err("missing"),
        REMOTE_FINALIZE_SESSION_NOT_FOUND
    );
}

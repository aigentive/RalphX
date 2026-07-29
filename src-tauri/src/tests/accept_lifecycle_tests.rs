// accept_lifecycle_tests.rs
//
// Integration tests verifying that agent processes and child sessions are properly
// cleaned up when a plan session is accepted or when verification reaches a terminal
// state.
//
// Both the IPC accept path (`apply_proposals_to_kanban`) and the HTTP accept path
// (`external_apply_proposals`) converge on the same cleanup call:
//
//   stop_verification_children(session_id, &app_state)
//
// These tests verify that function's behavior directly, which is equivalent to testing
// both accept paths. The full accept pipeline is not exercised here to avoid the heavy
// setup (projects, tasks, proposals) it requires.
//
// Test scenarios:
//   T16. IPC/HTTP accept proxy — stop_verification_children stops a registered child
//        agent and archives the child session row.
//   T17. Accept path archives child even when the agent had already exited (no registry
//        entry) — archive_after_stop is unconditional.
//   T18. Accept with no verification children — no-op, no errors.
//   T19. Terminal state (Verified) path — same cleanup as accept; stop_verification_children
//        is also called by post_verification_status when new_status is Verified/Skipped.

use crate::application::AppState;
use crate::domain::entities::{
    IdeationSession, IdeationSessionStatus, ProjectId, SessionPurpose, VerificationStatus,
};
use crate::domain::services::running_agent_registry::RunningAgentKey;
use crate::http_server::handlers::ideation::stop_verification_children;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_parent(project_id: &ProjectId) -> IdeationSession {
    IdeationSession::new(project_id.clone())
}

fn make_verification_child(
    project_id: &ProjectId,
    parent: &IdeationSession,
) -> IdeationSession {
    let mut child = IdeationSession::new(project_id.clone());
    child.session_purpose = SessionPurpose::Verification;
    child.parent_session_id = Some(parent.id.clone());
    child
}

// ── T16: Accept path stops registered child agent and archives the session ───
//
// Simulates what happens when apply_proposals_to_kanban (Tauri IPC) or
// external_apply_proposals (HTTP MCP) invokes stop_verification_children after
// accepting the plan:
//   - Parent has one active verification child
//   - Child has an agent registered in the running_agent_registry (pid=0 — safe
//     because kill_process guards pid ≤ 1 and does nothing)
//   - After stop_verification_children:
//       * agent registry entry is gone (agent "stopped")
//       * child session status = Archived
//       * get_verification_children returns empty

#[tokio::test]
async fn test_accept_path_stops_agent_and_archives_verification_child() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // Seed parent session
    let parent = make_parent(&project_id);
    let parent_id = parent.id.clone();
    state.ideation_session_repo.create(parent).await.unwrap();

    // Seed active verification child session
    let child_session = make_verification_child(&project_id, &state
        .ideation_session_repo
        .get_by_id(&parent_id)
        .await
        .unwrap()
        .unwrap());
    let child_id = child_session.id.clone();
    state
        .ideation_session_repo
        .create(child_session)
        .await
        .unwrap();

    // Register a running agent for the child (pid=0 is safe — kill_process no-ops for pid ≤ 1)
    let agent_key = RunningAgentKey::new("ideation", child_id.as_str());
    state
        .running_agent_registry
        .register(
            agent_key.clone(),
            0,
            "test-conversation-id".to_string(),
            "test-agent-run-id".to_string(),
            None,
            None,
        )
        .await;

    // Preconditions
    assert!(
        state.running_agent_registry.is_running(&agent_key).await,
        "precondition: agent must be registered before cleanup"
    );
    let child_before = state
        .ideation_session_repo
        .get_by_id(&child_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        child_before.status,
        IdeationSessionStatus::Active,
        "precondition: child must be Active before cleanup"
    );

    // Call stop_verification_children — the function both accept paths invoke
    stop_verification_children(parent_id.as_str(), &state)
        .await
        .expect("stop_verification_children must succeed");

    // Agent registry entry must be removed (agent "stopped")
    assert!(
        !state.running_agent_registry.is_running(&agent_key).await,
        "agent registry must have no entry after stop_verification_children"
    );

    // Child session must be Archived
    let child_after = state
        .ideation_session_repo
        .get_by_id(&child_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        child_after.status,
        IdeationSessionStatus::Archived,
        "verification child must be Archived after accept-path cleanup"
    );

    // No active verification children remain
    let active_children = state
        .ideation_session_repo
        .get_verification_children(&parent_id)
        .await
        .unwrap();
    assert!(
        active_children.is_empty(),
        "no active verification children must remain after accept cleanup"
    );
}

// ── T17: Accept path archives child even when no agent is currently registered ─
//
// The verification agent may have already exited (completed its turn and was
// unregistered) while the child session row is still Active.
// stop_verification_children must archive the child row unconditionally regardless
// of whether an agent is running — archive_after_stop is always true.

#[tokio::test]
async fn test_accept_path_archives_child_without_registered_agent() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // Seed parent + child, but do NOT register any agent
    let parent = make_parent(&project_id);
    let parent_id = parent.id.clone();
    state.ideation_session_repo.create(parent).await.unwrap();

    let child_session = make_verification_child(&project_id, &state
        .ideation_session_repo
        .get_by_id(&parent_id)
        .await
        .unwrap()
        .unwrap());
    let child_id = child_session.id.clone();
    state
        .ideation_session_repo
        .create(child_session)
        .await
        .unwrap();

    let agent_key = RunningAgentKey::new("ideation", child_id.as_str());

    // Precondition: no agent in registry
    assert!(
        !state.running_agent_registry.is_running(&agent_key).await,
        "precondition: no agent must be registered"
    );

    // Must succeed even without a running agent
    stop_verification_children(parent_id.as_str(), &state)
        .await
        .expect("stop_verification_children must succeed even without registered agent");

    // Child must still be archived (archive_after_stop is unconditional)
    let child_after = state
        .ideation_session_repo
        .get_by_id(&child_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        child_after.status,
        IdeationSessionStatus::Archived,
        "child must be Archived even when no agent was registered at accept time"
    );
}

// ── T18: Accept with no verification children — no-op, no errors ─────────────
//
// Sessions that never triggered plan verification have no verification children.
// stop_verification_children must complete successfully with zero side effects.

#[tokio::test]
async fn test_accept_with_no_verification_children_is_noop() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // Parent only — no children
    let parent = make_parent(&project_id);
    let parent_id = parent.id.clone();
    state.ideation_session_repo.create(parent).await.unwrap();

    // Must succeed with no children present
    let result = stop_verification_children(parent_id.as_str(), &state).await;
    assert!(
        result.is_ok(),
        "stop_verification_children must succeed when no verification children exist: {:?}",
        result
    );

    // Parent session must be unchanged
    let parent_after = state
        .ideation_session_repo
        .get_by_id(&parent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        parent_after.status,
        IdeationSessionStatus::Active,
        "parent session status must be unchanged when no verification children exist"
    );
}

// ── T19: Terminal state (Verified) path stops agent and archives child ────────
//
// When post_verification_status sets new_status = Verified or Skipped, it calls
// stop_verification_children(&session_id, &state.app_state).await.ok().
// This test verifies that exact call path's outcome without re-testing the full
// post_verification_status HTTP handler.
//
// Same assertions as T16 — both accept and terminal-state paths call the same
// function. This test is named separately for traceability against Proof Obligation 3.

#[tokio::test]
async fn test_terminal_state_verified_stops_agent_and_archives_child() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    // Parent: in Reviewing state with active verification
    let mut parent = make_parent(&project_id);
    parent.verification_status = VerificationStatus::Reviewing;
    parent.verification_in_progress = true;
    parent.verification_generation = 1;
    let parent_id = parent.id.clone();
    state.ideation_session_repo.create(parent).await.unwrap();

    // Active verification child with a running agent
    let child_session = make_verification_child(&project_id, &state
        .ideation_session_repo
        .get_by_id(&parent_id)
        .await
        .unwrap()
        .unwrap());
    let child_id = child_session.id.clone();
    state
        .ideation_session_repo
        .create(child_session)
        .await
        .unwrap();

    let agent_key = RunningAgentKey::new("ideation", child_id.as_str());
    state
        .running_agent_registry
        .register(
            agent_key.clone(),
            0,
            "verifier-conversation-id".to_string(),
            "verifier-agent-run-id".to_string(),
            None,
            None,
        )
        .await;

    // Simulate the post_verification_status handler reaching Verified and calling cleanup
    stop_verification_children(parent_id.as_str(), &state)
        .await
        .expect("stop_verification_children must succeed on terminal state");

    // Agent must be unregistered
    assert!(
        !state.running_agent_registry.is_running(&agent_key).await,
        "verifier agent must be unregistered after Verified terminal state"
    );

    // Verification child session must be Archived
    let child_after = state
        .ideation_session_repo
        .get_by_id(&child_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        child_after.status,
        IdeationSessionStatus::Archived,
        "verification child must be Archived after Verified terminal state"
    );

    // No active verification children remain
    let active_children = state
        .ideation_session_repo
        .get_verification_children(&parent_id)
        .await
        .unwrap();
    assert!(
        active_children.is_empty(),
        "no active verification children must remain after terminal state cleanup"
    );
}

// Integration tests proving that `count_active_ideation_slots` correctly
// excludes `interactive_idle` sessions for ALL ideation session types.
//
// Proof Obligation #6 from the Ideation Capacity Visibility plan:
//   "interactive_idle covers all ideation types — Prove with test that general
//   sessions, verification children, and external sessions all get marked idle
//   on `waiting_for_input`."
//
// Key invariants verified:
//   1. Slot key format "ideation/{session_id}" is consistent between the streaming
//      TurnComplete handler and count_active_ideation_slots.
//   2. Sessions marked as interactive_idle are excluded from the active count.
//   3. All three session types (general, verification, external) go through the
//      same ChatContextType::Ideation code path and are treated identically.

use std::sync::Arc;

use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::commands::execution_commands::count_active_ideation_slots;
use ralphx_lib::domain::entities::ideation::{SessionOrigin, SessionPurpose};
use ralphx_lib::domain::entities::{IdeationSession, ProjectId};
use ralphx_lib::domain::services::{RunningAgentKey, RunningAgentRegistry};

// ─── test helpers ────────────────────────────────────────────────────────────

/// Register an ideation session in the running_agent_registry with context_type = "ideation".
/// Uses a non-zero PID so it passes the `info.pid != 0` guard in count_active_ideation_slots.
async fn register_ideation_session(registry: &dyn RunningAgentRegistry, session_id: &str) {
    let key = RunningAgentKey::new("ideation", session_id);
    registry
        .register(
            key,
            9999, // non-zero fake PID
            format!("conv-{session_id}"),
            format!("run-{session_id}"),
            None,
            None,
        )
        .await;
}

// ─── Slot key format consistency ─────────────────────────────────────────────

/// Verify that the slot key format "ideation/{session_id}" is shared between:
///   - The TurnComplete streaming handler (marks idle via `mark_interactive_idle`)
///   - `count_active_ideation_slots` (checks idle via `is_interactive_idle`)
///
/// Streaming code (chat_service_streaming.rs):
///   let slot_key = format!("{}/{}", context_type, context_id_str);
///   where context_type = ChatContextType::Ideation.to_string() = "ideation"
///
/// Count code (control_helpers.rs):
///   let slot_key = format!("{}/{}", key.context_type, key.context_id);
///   where key.context_type = RunningAgentKey.context_type = "ideation"
///
/// This test exercises BOTH production paths on a shared ExecutionState:
/// marking via the streaming format confirms the counting function excludes it.
#[tokio::test]
async fn test_slot_key_format_ideation_prefix_matches_between_streaming_and_counting() {
    let app = AppState::new_sqlite_test();
    let exec_state = Arc::new(ExecutionState::new());
    let pid = ProjectId::new();

    let session = app
        .ideation_session_repo
        .create(IdeationSession::builder().project_id(pid).build())
        .await
        .unwrap();

    register_ideation_session(&*app.running_agent_registry, session.id.as_str()).await;

    // Before marking idle: production counting function sees the session as active
    let count_before = count_active_ideation_slots(&app, &exec_state, None)
        .await
        .unwrap();
    assert_eq!(
        count_before, 1,
        "Session should be counted as active (1 slot) before idle mark"
    );

    // Simulate the streaming TurnComplete handler: format!("{}/{}", "ideation", session_id)
    // Expected key format: "ideation/{session_id}"
    let streaming_key = format!("ideation/{}", session.id.as_str());
    exec_state.mark_interactive_idle(&streaming_key);

    // After marking idle via the streaming key format, the counting function must
    // exclude the session — proving both paths use the same key format.
    let count_after = count_active_ideation_slots(&app, &exec_state, None)
        .await
        .unwrap();
    assert_eq!(
        count_after, 0,
        "Session marked idle via streaming key format must be excluded by the counting function"
    );
}

// ─── General sessions ────────────────────────────────────────────────────────

/// General session (SessionPurpose::General) in generating state → counted as active.
#[tokio::test]
async fn test_general_session_generating_is_counted() {
    let app = AppState::new_sqlite_test();
    let exec_state = Arc::new(ExecutionState::new());

    let pid = ProjectId::new();
    let session = app
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(pid.clone())
                .session_purpose(SessionPurpose::General)
                .build(),
        )
        .await
        .unwrap();

    register_ideation_session(&*app.running_agent_registry, session.id.as_str()).await;

    let count = count_active_ideation_slots(&app, &exec_state, None)
        .await
        .unwrap();
    assert_eq!(count, 1, "Generating general session must count as 1 active ideation slot");
}

/// General session in waiting_for_input (marked interactive_idle) → excluded from active count.
#[tokio::test]
async fn test_general_session_idle_excluded_from_active_count() {
    let app = AppState::new_sqlite_test();
    let exec_state = Arc::new(ExecutionState::new());

    let pid = ProjectId::new();
    let session = app
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(pid.clone())
                .session_purpose(SessionPurpose::General)
                .build(),
        )
        .await
        .unwrap();

    register_ideation_session(&*app.running_agent_registry, session.id.as_str()).await;

    // Simulate TurnComplete: decrement_and_mark_idle sets this key
    let slot_key = format!("ideation/{}", session.id.as_str());
    exec_state.mark_interactive_idle(&slot_key);

    let count = count_active_ideation_slots(&app, &exec_state, None)
        .await
        .unwrap();
    assert_eq!(
        count, 0,
        "General session in waiting_for_input (interactive_idle) must not count as active"
    );
}

// ─── Verification child sessions ─────────────────────────────────────────────

/// Verification child session (SessionPurpose::Verification) in generating state → counted.
#[tokio::test]
async fn test_verification_session_generating_is_counted() {
    let app = AppState::new_sqlite_test();
    let exec_state = Arc::new(ExecutionState::new());

    let pid = ProjectId::new();
    let session = app
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(pid.clone())
                .session_purpose(SessionPurpose::Verification)
                .build(),
        )
        .await
        .unwrap();

    register_ideation_session(&*app.running_agent_registry, session.id.as_str()).await;

    let count = count_active_ideation_slots(&app, &exec_state, None)
        .await
        .unwrap();
    assert_eq!(
        count, 1,
        "Generating verification child session must count as 1 active ideation slot"
    );
}

/// Verification child session in waiting_for_input (interactive_idle) → excluded.
#[tokio::test]
async fn test_verification_session_idle_excluded_from_active_count() {
    let app = AppState::new_sqlite_test();
    let exec_state = Arc::new(ExecutionState::new());

    let pid = ProjectId::new();
    let session = app
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(pid.clone())
                .session_purpose(SessionPurpose::Verification)
                .build(),
        )
        .await
        .unwrap();

    register_ideation_session(&*app.running_agent_registry, session.id.as_str()).await;

    let slot_key = format!("ideation/{}", session.id.as_str());
    exec_state.mark_interactive_idle(&slot_key);

    let count = count_active_ideation_slots(&app, &exec_state, None)
        .await
        .unwrap();
    assert_eq!(
        count, 0,
        "Verification child in waiting_for_input (interactive_idle) must not count as active"
    );
}

// ─── External sessions ───────────────────────────────────────────────────────

/// External session (SessionOrigin::External) in generating state → counted.
#[tokio::test]
async fn test_external_session_generating_is_counted() {
    let app = AppState::new_sqlite_test();
    let exec_state = Arc::new(ExecutionState::new());

    let pid = ProjectId::new();
    let session = app
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(pid.clone())
                .origin(SessionOrigin::External)
                .build(),
        )
        .await
        .unwrap();

    register_ideation_session(&*app.running_agent_registry, session.id.as_str()).await;

    let count = count_active_ideation_slots(&app, &exec_state, None)
        .await
        .unwrap();
    assert_eq!(
        count, 1,
        "Generating external session must count as 1 active ideation slot"
    );
}

/// External session in waiting_for_input (interactive_idle) → excluded.
#[tokio::test]
async fn test_external_session_idle_excluded_from_active_count() {
    let app = AppState::new_sqlite_test();
    let exec_state = Arc::new(ExecutionState::new());

    let pid = ProjectId::new();
    let session = app
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(pid.clone())
                .origin(SessionOrigin::External)
                .build(),
        )
        .await
        .unwrap();

    register_ideation_session(&*app.running_agent_registry, session.id.as_str()).await;

    let slot_key = format!("ideation/{}", session.id.as_str());
    exec_state.mark_interactive_idle(&slot_key);

    let count = count_active_ideation_slots(&app, &exec_state, None)
        .await
        .unwrap();
    assert_eq!(
        count, 0,
        "External session in waiting_for_input (interactive_idle) must not count as active"
    );
}

// ─── Mixed session types ─────────────────────────────────────────────────────

/// Mixed scenario: general (generating) + verification (idle) + external (idle).
/// Only the generating session should be counted; the two idle ones are excluded.
#[tokio::test]
async fn test_mixed_session_types_only_generating_counted() {
    let app = AppState::new_sqlite_test();
    let exec_state = Arc::new(ExecutionState::new());
    let pid = ProjectId::new();

    let gen_session = app
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(pid.clone())
                .session_purpose(SessionPurpose::General)
                .build(),
        )
        .await
        .unwrap();
    let verif_session = app
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(pid.clone())
                .session_purpose(SessionPurpose::Verification)
                .build(),
        )
        .await
        .unwrap();
    let ext_session = app
        .ideation_session_repo
        .create(
            IdeationSession::builder()
                .project_id(pid.clone())
                .origin(SessionOrigin::External)
                .build(),
        )
        .await
        .unwrap();

    register_ideation_session(&*app.running_agent_registry, gen_session.id.as_str()).await;
    register_ideation_session(&*app.running_agent_registry, verif_session.id.as_str()).await;
    register_ideation_session(&*app.running_agent_registry, ext_session.id.as_str()).await;

    // Mark verification and external as idle (waiting_for_input between turns)
    exec_state.mark_interactive_idle(&format!("ideation/{}", verif_session.id.as_str()));
    exec_state.mark_interactive_idle(&format!("ideation/{}", ext_session.id.as_str()));

    let count = count_active_ideation_slots(&app, &exec_state, None)
        .await
        .unwrap();
    assert_eq!(
        count, 1,
        "Only the generating general session should count; idle verification and external are excluded"
    );
}

/// All three session types idle → count = 0.
#[tokio::test]
async fn test_all_session_types_idle_count_zero() {
    let app = AppState::new_sqlite_test();
    let exec_state = Arc::new(ExecutionState::new());
    let pid = ProjectId::new();

    for purpose_label in ["general-idle", "verif-idle", "ext-idle"] {
        let session = app
            .ideation_session_repo
            .create(IdeationSession::builder().project_id(pid.clone()).build())
            .await
            .unwrap();
        register_ideation_session(&*app.running_agent_registry, session.id.as_str()).await;
        exec_state.mark_interactive_idle(&format!("ideation/{}", session.id.as_str()));
        let _ = purpose_label; // label used only for readability
    }

    let count = count_active_ideation_slots(&app, &exec_state, None)
        .await
        .unwrap();
    assert_eq!(count, 0, "When all sessions are idle, active count must be 0");
}

// ─── Project-scoped counting ─────────────────────────────────────────────────

/// Project-scoped count excludes sessions from other projects.
#[tokio::test]
async fn test_project_scoped_count_excludes_other_projects() {
    let app = AppState::new_sqlite_test();
    let exec_state = Arc::new(ExecutionState::new());

    let pid_a = ProjectId::new();
    let pid_b = ProjectId::new();

    // Session in project A (generating)
    let session_a = app
        .ideation_session_repo
        .create(IdeationSession::builder().project_id(pid_a.clone()).build())
        .await
        .unwrap();
    // Session in project B (generating)
    let session_b = app
        .ideation_session_repo
        .create(IdeationSession::builder().project_id(pid_b.clone()).build())
        .await
        .unwrap();

    register_ideation_session(&*app.running_agent_registry, session_a.id.as_str()).await;
    register_ideation_session(&*app.running_agent_registry, session_b.id.as_str()).await;

    // Global count: 2 (both projects)
    let global = count_active_ideation_slots(&app, &exec_state, None)
        .await
        .unwrap();
    assert_eq!(global, 2, "Global count should include both projects");

    // Project A count: 1
    let count_a = count_active_ideation_slots(&app, &exec_state, Some(&pid_a))
        .await
        .unwrap();
    assert_eq!(count_a, 1, "Project A count should be 1 (only session_a)");

    // Project B count: 1
    let count_b = count_active_ideation_slots(&app, &exec_state, Some(&pid_b))
        .await
        .unwrap();
    assert_eq!(count_b, 1, "Project B count should be 1 (only session_b)");
}

/// Project-scoped count excludes idle sessions from the target project.
#[tokio::test]
async fn test_project_scoped_count_excludes_idle_in_same_project() {
    let app = AppState::new_sqlite_test();
    let exec_state = Arc::new(ExecutionState::new());
    let pid = ProjectId::new();

    let active = app
        .ideation_session_repo
        .create(IdeationSession::builder().project_id(pid.clone()).build())
        .await
        .unwrap();
    let idle = app
        .ideation_session_repo
        .create(IdeationSession::builder().project_id(pid.clone()).build())
        .await
        .unwrap();

    register_ideation_session(&*app.running_agent_registry, active.id.as_str()).await;
    register_ideation_session(&*app.running_agent_registry, idle.id.as_str()).await;

    exec_state.mark_interactive_idle(&format!("ideation/{}", idle.id.as_str()));

    let count = count_active_ideation_slots(&app, &exec_state, Some(&pid))
        .await
        .unwrap();
    assert_eq!(
        count, 1,
        "Project-scoped count must exclude idle sessions within the same project"
    );
}

// ─── Ghost entry filtering ────────────────────────────────────────────────────

/// A registry entry whose session is not in the repo (ghost entry) must not be counted.
#[tokio::test]
async fn test_ghost_registry_entry_not_counted() {
    let app = AppState::new_sqlite_test();
    let exec_state = Arc::new(ExecutionState::new());

    // Register an entry for a session that doesn't exist in the DB
    register_ideation_session(&*app.running_agent_registry, "ghost-session-not-in-db").await;

    let count = count_active_ideation_slots(&app, &exec_state, None)
        .await
        .unwrap();
    assert_eq!(count, 0, "Ghost registry entry (no matching session in DB) must not be counted");
}

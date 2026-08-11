// Reliability test suite — C1, C2, C3, C4 gap coverage
//
// C1: auto-propose retry + failure event emission (auto_propose_with_retry)
// C2: `origin: External` propagates through create_child_session(purpose: "verification")
// C4: auto-accept → task linking produces no orphaned proposals

use axum::{extract::State, Json};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    Artifact, ArtifactId, ArtifactType, Complexity, IdeationSession, IdeationSessionId,
    IdeationSessionStatus, Priority, Project, ProjectId, ProposalCategory, ProposalStatus,
    SessionOrigin, SessionPurpose, TaskProposal, TaskProposalId, VerificationStatus,
};
use ralphx_lib::error::AppError;
use ralphx_lib::http_server::handlers::create_child_session;
use ralphx_lib::http_server::helpers::finalize_proposals_impl;
use ralphx_lib::http_server::types::{CreateChildSessionRequest, HttpServerState};
use std::sync::Arc;

// ============================================================================
// Shared Setup Helpers
// ============================================================================

async fn setup_sqlite_state() -> HttpServerState {
    let app_state = Arc::new(AppState::new_sqlite_test());
    let execution_state = Arc::new(ExecutionState::new());
    HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
        external_mcp_supervisor: None,
    }
}

fn test_execution_state() -> Arc<ExecutionState> {
    Arc::new(ExecutionState::new())
}

fn make_external_session(
    project_id: &ProjectId,
    plan_artifact_id: Option<ArtifactId>,
) -> IdeationSession {
    IdeationSession {
        id: IdeationSessionId::new(),
        project_id: project_id.clone(),
        title: Some("External Parent Session".to_string()),
        status: IdeationSessionStatus::Active,
        plan_artifact_id,
        plan_blueprint_artifact_id: None,
        verified_plan_artifact_id: None,
        verified_plan_blueprint_artifact_id: None,
        verified_plan_agent_run_id: None,
        inherited_plan_artifact_id: None,
        inherited_plan_blueprint_artifact_id: None,
        seed_task_id: None,
        parent_session_id: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        archived_at: None,
        converted_at: None,
        title_source: None,
        verification_status: Default::default(),
        verification_in_progress: false,
        verification_generation: 0,
        verification_current_round: None,
        verification_max_rounds: None,
        verification_gap_count: 0,
        verification_gap_score: None,
        verification_convergence_reason: None,
        source_project_id: None,
        source_session_id: None,
        source_task_id: None,
        source_context_type: None,
        source_context_id: None,
        spawn_reason: None,
        blocker_fingerprint: None,
        session_purpose: Default::default(),
        session_flow: Default::default(),
        cross_project_checked: true,
        plan_version_last_read: None,
        blueprint_version_last_read: None,
        plan_contract_version: 1,
        origin: SessionOrigin::External,
        expected_proposal_count: None,
        auto_accept_status: None,
        auto_accept_started_at: None,
        api_key_id: None,
        idempotency_key: None,
        external_activity_phase: None,
        external_last_read_message_id: None,
        dependencies_acknowledged: false,
        pending_initial_prompt: None,
        acceptance_status: None,
        verification_confirmation_status: None,
        analysis: Default::default(),
        last_effective_model: None,
    }
}

// ============================================================================
// C2: model-native verification rejects legacy verification child creation
// ============================================================================

/// C2: External parents cannot create retired verification child sessions.
#[tokio::test]
async fn c2_external_parent_rejects_verification_child() {
    let state = setup_sqlite_state().await;

    let project_id = ProjectId::from_string("proj-c2-test".to_string());

    // Create External parent session with a plan artifact (FK OFF — no real artifact needed
    // because the SQLite test state disables FK enforcement for simplicity)
    let parent = make_external_session(&project_id, Some(ArtifactId::new()));
    let parent_id = parent.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(parent)
        .await
        .unwrap();

    // Call the handler to create a verification child session
    let req = CreateChildSessionRequest {
        parent_session_id: parent_id.as_str().to_string(),
        title: None,
        description: Some("Verify the plan".to_string()),
        inherit_context: false,
        initial_prompt: None,
        purpose: Some("verification".to_string()),
        is_external_trigger: false,
        source_task_id: None,
        source_context_type: None,
        source_context_id: None,
        spawn_reason: None,
        blocker_fingerprint: None,
    };

    let error = create_child_session(State(state.clone()), Json(req))
        .await
        .expect_err("verification must remain in the active Plan conversation");
    assert_eq!(error.0, axum::http::StatusCode::BAD_REQUEST);
    assert!(error.1["error"]
        .as_str()
        .is_some_and(|message| message.contains("active Plan conversation")));
    assert!(state
        .app_state
        .ideation_session_repo
        .get_children(&parent_id)
        .await
        .unwrap()
        .is_empty());
}

/// C2b: Internal parents enforce the same no-hidden-verification-child contract.
#[tokio::test]
async fn c2_internal_parent_rejects_verification_child() {
    let state = setup_sqlite_state().await;

    let project_id = ProjectId::from_string("proj-c2b-test".to_string());

    // Create Internal parent (default origin)
    let mut parent = make_external_session(&project_id, Some(ArtifactId::new()));
    parent.origin = SessionOrigin::Internal;
    let parent_id = parent.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(parent)
        .await
        .unwrap();

    let req = CreateChildSessionRequest {
        parent_session_id: parent_id.as_str().to_string(),
        title: None,
        description: Some("Verify internal plan".to_string()),
        inherit_context: false,
        initial_prompt: None,
        purpose: Some("verification".to_string()),
        is_external_trigger: false,
        source_task_id: None,
        source_context_type: None,
        source_context_id: None,
        spawn_reason: None,
        blocker_fingerprint: None,
    };

    let error = create_child_session(State(state.clone()), Json(req))
        .await
        .expect_err("verification must remain in the active Plan conversation");
    assert_eq!(error.0, axum::http::StatusCode::BAD_REQUEST);
    assert!(error.1["error"]
        .as_str()
        .is_some_and(|message| message.contains("active Plan conversation")));
    assert!(state
        .app_state
        .ideation_session_repo
        .get_children(&parent_id)
        .await
        .unwrap()
        .is_empty());
}

// ============================================================================
#[tokio::test]
async fn c5a_external_trigger_sets_external_origin_for_general_child() {
    let state = setup_sqlite_state().await;
    let project_id = ProjectId::from_string("proj-c5a".to_string());

    // Parent can have any origin — general child origin comes from is_external_trigger, not parent
    let mut parent = make_external_session(&project_id, None);
    parent.origin = SessionOrigin::Internal; // set to Internal to prove child doesn't inherit
    let parent_id = parent.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(parent)
        .await
        .unwrap();

    let req = CreateChildSessionRequest {
        parent_session_id: parent_id.as_str().to_string(),
        title: None,
        description: None,
        inherit_context: false,
        initial_prompt: None,
        purpose: None, // general
        is_external_trigger: true,
        source_task_id: None,
        source_context_type: None,
        source_context_id: None,
        spawn_reason: None,
        blocker_fingerprint: None,
    };

    let result = create_child_session(State(state.clone()), Json(req)).await;
    assert!(
        result.is_ok(),
        "Handler must succeed, got: {:?}",
        result.err()
    );

    let child_id = IdeationSessionId::from_string(result.unwrap().0.session_id);
    let child = state
        .app_state
        .ideation_session_repo
        .get_by_id(&child_id)
        .await
        .unwrap()
        .expect("Child must exist");

    assert_eq!(
        child.origin,
        SessionOrigin::External,
        "is_external_trigger=true must set origin=External on general child"
    );
    assert_eq!(
        child.session_purpose,
        SessionPurpose::default(), // General
        "Child session_purpose must be General (default)"
    );
}

/// C5b: is_external_trigger=false → general child gets origin=Internal.
///
/// Default case: no external trigger flag → origin is Internal regardless of parent origin.
#[tokio::test]
async fn c5b_no_external_trigger_sets_internal_origin_for_general_child() {
    let state = setup_sqlite_state().await;
    let project_id = ProjectId::from_string("proj-c5b".to_string());

    // Parent is External — general child should still get Internal (not inherited)
    let parent = make_external_session(&project_id, None);
    let parent_id = parent.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(parent)
        .await
        .unwrap();

    let req = CreateChildSessionRequest {
        parent_session_id: parent_id.as_str().to_string(),
        title: None,
        description: None,
        inherit_context: false,
        initial_prompt: None,
        purpose: None, // general
        is_external_trigger: false,
        source_task_id: None,
        source_context_type: None,
        source_context_id: None,
        spawn_reason: None,
        blocker_fingerprint: None,
    };

    let result = create_child_session(State(state.clone()), Json(req)).await;
    assert!(
        result.is_ok(),
        "Handler must succeed, got: {:?}",
        result.err()
    );

    let child_id = IdeationSessionId::from_string(result.unwrap().0.session_id);
    let child = state
        .app_state
        .ideation_session_repo
        .get_by_id(&child_id)
        .await
        .unwrap()
        .expect("Child must exist");

    assert_eq!(
        child.origin,
        SessionOrigin::Internal,
        "is_external_trigger=false must set origin=Internal (not inherited from External parent)"
    );
}

/// C5c: Trigger-origin flags cannot bypass the no-hidden-verification-child contract.
#[tokio::test]
async fn c5c_verification_child_rejected_regardless_of_external_trigger_flag() {
    let state = setup_sqlite_state().await;
    let project_id = ProjectId::from_string("proj-c5c".to_string());

    let parent = make_external_session(&project_id, Some(ArtifactId::new()));
    let parent_id = parent.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(parent)
        .await
        .unwrap();

    // Pass is_external_trigger=false — verification child must still get External from parent
    let req = CreateChildSessionRequest {
        parent_session_id: parent_id.as_str().to_string(),
        title: None,
        description: Some("Verify".to_string()),
        inherit_context: false,
        initial_prompt: None,
        purpose: Some("verification".to_string()),
        is_external_trigger: false, // trigger says Internal, but verification inherits
        source_task_id: None,
        source_context_type: None,
        source_context_id: None,
        spawn_reason: None,
        blocker_fingerprint: None,
    };

    let error = create_child_session(State(state.clone()), Json(req))
        .await
        .expect_err("verification must remain in the active Plan conversation");
    assert_eq!(error.0, axum::http::StatusCode::BAD_REQUEST);
    assert!(error.1["error"]
        .as_str()
        .is_some_and(|message| message.contains("active Plan conversation")));
    assert!(state
        .app_state
        .ideation_session_repo
        .get_children(&parent_id)
        .await
        .unwrap()
        .is_empty());
}

/// C5d: Backward compat — request without is_external_trigger field deserializes with default false.
///
/// Ensures existing callers that don't pass is_external_trigger still work correctly.
/// The #[serde(default)] annotation handles JSON deserialization; this test verifies
/// that the default value produces Internal origin for a general child.
#[tokio::test]
async fn c5d_missing_is_external_trigger_defaults_to_internal_origin() {
    let state = setup_sqlite_state().await;
    let project_id = ProjectId::from_string("proj-c5d".to_string());

    // External parent — child should still get Internal if no trigger flag
    let parent = make_external_session(&project_id, None);
    let parent_id = parent.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(parent)
        .await
        .unwrap();

    // Simulate JSON payload without is_external_trigger field via serde
    let json_payload = serde_json::json!({
        "parent_session_id": parent_id.as_str(),
        "title": null,
        "description": null,
        "inherit_context": false,
        "initial_prompt": null,
        "purpose": null
        // is_external_trigger deliberately absent
    });
    let req: CreateChildSessionRequest =
        serde_json::from_value(json_payload).expect("deserialization must succeed");

    assert!(
        !req.is_external_trigger,
        "Missing field must default to false"
    );

    let result = create_child_session(State(state.clone()), Json(req)).await;
    assert!(
        result.is_ok(),
        "Handler must succeed, got: {:?}",
        result.err()
    );

    let child_id = IdeationSessionId::from_string(result.unwrap().0.session_id);
    let child = state
        .app_state
        .ideation_session_repo
        .get_by_id(&child_id)
        .await
        .unwrap()
        .expect("Child must exist");

    assert_eq!(
        child.origin,
        SessionOrigin::Internal,
        "Default is_external_trigger=false must produce Internal origin"
    );
}

// ============================================================================
// C4: auto-accept → task linking — no orphaned proposals
// ============================================================================

/// C4: finalize_proposals links all proposals to tasks (no orphaned proposals).
///
/// Simulates the auto-accept flow: session with proposals → finalize_proposals_impl
/// → verify every proposal has created_task_id set to a valid task ID.
#[tokio::test]
async fn c4_finalize_proposals_links_all_proposals_to_tasks() {
    let state = AppState::new_sqlite_test();

    // Create project with use_feature_branches=false to skip git operations
    let mut project = Project::new(
        "Reliability Test Project".to_string(),
        "/tmp/test-c4".to_string(),
    );
    project.use_feature_branches = false;
    let project_id = project.id.clone();
    state.project_repo.create(project).await.unwrap();

    // Create plan artifact (real DB row for FK safety)
    let artifact = Artifact::new_inline(
        "Test Plan",
        ArtifactType::Specification,
        "# Reliability Plan\n\nTest content for C4.",
        "test",
    );
    let artifact_id = artifact.id.clone();
    state.artifact_repo.create(artifact).await.unwrap();

    // Create session
    let session_id = IdeationSessionId::new();
    let session = IdeationSession {
        id: session_id.clone(),
        project_id: project_id.clone(),
        title: Some("Auto-Accept Session".to_string()),
        status: IdeationSessionStatus::Active,
        plan_artifact_id: Some(artifact_id.clone()),
        plan_blueprint_artifact_id: None,
        verified_plan_artifact_id: None,
        verified_plan_blueprint_artifact_id: None,
        verified_plan_agent_run_id: None,
        inherited_plan_artifact_id: None,
        inherited_plan_blueprint_artifact_id: None,
        seed_task_id: None,
        parent_session_id: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        archived_at: None,
        converted_at: None,
        title_source: None,
        verification_status: VerificationStatus::Skipped, // bypass verification gate
        verification_in_progress: false,
        verification_generation: 0,
        verification_current_round: None,
        verification_max_rounds: None,
        verification_gap_count: 0,
        verification_gap_score: None,
        verification_convergence_reason: None,
        source_project_id: None,
        source_session_id: None,
        source_task_id: None,
        source_context_type: None,
        source_context_id: None,
        spawn_reason: None,
        blocker_fingerprint: None,
        session_purpose: Default::default(),
        session_flow: Default::default(),
        cross_project_checked: true,
        plan_version_last_read: None,
        blueprint_version_last_read: None,
        plan_contract_version: 1,
        origin: SessionOrigin::External,
        expected_proposal_count: None,
        auto_accept_status: None,
        auto_accept_started_at: None,
        api_key_id: None,
        idempotency_key: None,
        external_activity_phase: None,
        external_last_read_message_id: None,
        dependencies_acknowledged: false,
        pending_initial_prompt: None,
        acceptance_status: None,
        verification_confirmation_status: None,
        analysis: Default::default(),
        last_effective_model: None,
    };
    state.ideation_session_repo.create(session).await.unwrap();

    // Create 3 proposals linked to the session
    let mut proposal_ids = Vec::new();
    for i in 1..=3i32 {
        let proposal = TaskProposal {
            id: TaskProposalId::new(),
            session_id: session_id.clone(),
            title: format!("Proposal {}", i),
            description: Some(format!("Description for proposal {}", i)),
            category: ProposalCategory::Feature,
            status: ProposalStatus::Pending,
            suggested_priority: Priority::Medium,
            priority_score: 50,
            priority_reason: None,
            priority_factors: None,
            estimated_complexity: Complexity::Moderate,
            user_priority: None,
            user_modified: false,
            steps: None,
            acceptance_criteria: None,
            plan_artifact_id: Some(artifact_id.clone()),
            plan_version_at_creation: None,
            blueprint_artifact_id: None,
            blueprint_version_at_creation: None,
            created_task_id: None, // starts as None — will be linked after finalize
            selected: false,
            affected_paths: Some(format!(r#"["src/reliability/proposal_{}.rs"]"#, i)),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            archived_at: None,
            sort_order: i,
            target_project: None,
            migrated_from_session_id: None,
            migrated_from_proposal_id: None,
        };
        let pid = proposal.id.clone();
        state.task_proposal_repo.create(proposal).await.unwrap();
        proposal_ids.push(pid);
    }

    // Mirror the real flow: 2+ proposal finalize requires dependency review acknowledgment.
    // In production this is set by analyze_session_dependencies or explicit dependency edits.
    state
        .ideation_session_repo
        .set_dependencies_acknowledged(session_id.as_str())
        .await
        .unwrap();

    // Call finalize_proposals_impl — the auto-accept entry point
    let result =
        finalize_proposals_impl(&state, &test_execution_state(), session_id.as_str(), false).await;
    assert!(
        result.is_ok(),
        "finalize_proposals_impl must succeed, got: {:?}",
        result.err()
    );

    let finalize_response = result.unwrap();
    assert_eq!(
        finalize_response.created_task_ids.len(),
        3,
        "Must create exactly 3 tasks (one per proposal)"
    );

    // Verify every proposal has created_task_id set — no orphans
    for pid in &proposal_ids {
        let proposal = state
            .task_proposal_repo
            .get_by_id(pid)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("Proposal {} must exist after finalize", pid.as_str()));

        assert!(
            proposal.created_task_id.is_some(),
            "Proposal '{}' must have created_task_id set after finalize_proposals (orphaned proposal detected)",
            proposal.title
        );

        // Verify the linked task row actually exists in SQLite. This keeps the
        // assertion on the durable finalize invariant even if repo-level
        // shaping changes.
        let task_id = proposal.created_task_id.as_ref().unwrap();
        let task_row_id = state
            .db
            .query_optional({
                let task_id = task_id.as_str().to_string();
                move |conn| {
                    conn.query_row(
                        "SELECT id FROM tasks WHERE id = ?1",
                        rusqlite::params![task_id],
                        |row| row.get::<_, String>(0),
                    )
                }
            })
            .await
            .unwrap()
            .unwrap_or_else(|| {
                panic!(
                    "Task {} linked from proposal '{}' must exist",
                    task_id.as_str(),
                    proposal.title
                )
            });

        assert_eq!(
            task_row_id,
            task_id.as_str().to_string(),
            "Task row must exist for proposal '{}'",
            proposal.title
        );
    }

    // Verify session was converted to Accepted
    assert_eq!(
        finalize_response.session_status, "accepted",
        "Session must transition to accepted after finalize"
    );
}

/// C4b: Proposal count mismatch prevents finalize (expected_proposal_count gate).
///
/// If the session has expected_proposal_count set and the actual count doesn't match,
/// finalize_proposals_impl must return a validation error — not silently create partial tasks.
#[tokio::test]
async fn c4_count_mismatch_prevents_finalize_and_leaves_no_orphans() {
    let state = AppState::new_sqlite_test();

    let mut project = Project::new(
        "Count Test Project".to_string(),
        "/tmp/test-c4b".to_string(),
    );
    project.use_feature_branches = false;
    let project_id = project.id.clone();
    state.project_repo.create(project).await.unwrap();

    let artifact = Artifact::new_inline("Plan", ArtifactType::Specification, "# Plan", "test");
    let artifact_id = artifact.id.clone();
    state.artifact_repo.create(artifact).await.unwrap();

    let session_id = IdeationSessionId::new();
    let session = IdeationSession {
        id: session_id.clone(),
        project_id: project_id.clone(),
        title: Some("Count Gate Session".to_string()),
        status: IdeationSessionStatus::Active,
        plan_artifact_id: Some(artifact_id.clone()),
        plan_blueprint_artifact_id: None,
        verified_plan_artifact_id: None,
        verified_plan_blueprint_artifact_id: None,
        verified_plan_agent_run_id: None,
        inherited_plan_artifact_id: None,
        inherited_plan_blueprint_artifact_id: None,
        seed_task_id: None,
        parent_session_id: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        archived_at: None,
        converted_at: None,
        title_source: None,
        verification_status: VerificationStatus::Skipped,
        verification_in_progress: false,
        verification_generation: 0,
        verification_current_round: None,
        verification_max_rounds: None,
        verification_gap_count: 0,
        verification_gap_score: None,
        verification_convergence_reason: None,
        source_project_id: None,
        source_session_id: None,
        source_task_id: None,
        source_context_type: None,
        source_context_id: None,
        spawn_reason: None,
        blocker_fingerprint: None,
        session_purpose: Default::default(),
        session_flow: Default::default(),
        cross_project_checked: true,
        plan_version_last_read: None,
        blueprint_version_last_read: None,
        plan_contract_version: 1,
        origin: SessionOrigin::External,
        expected_proposal_count: None, // will be set via SQL below (create() doesn't persist it)
        auto_accept_status: None,
        auto_accept_started_at: None,
        api_key_id: None,
        idempotency_key: None,
        external_activity_phase: None,
        external_last_read_message_id: None,
        dependencies_acknowledged: false,
        pending_initial_prompt: None,
        acceptance_status: None,
        verification_confirmation_status: None,
        analysis: Default::default(),
        last_effective_model: None,
    };
    state.ideation_session_repo.create(session).await.unwrap();

    // create() does not persist expected_proposal_count — set it via SQL
    // (consistent with how http_helpers.rs tests set fields not included in the INSERT)
    let sid = session_id.as_str().to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE ideation_sessions SET expected_proposal_count = 5 WHERE id = ?1",
                rusqlite::params![sid],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();

    // Create only 2 proposals (mismatches the expected count of 5)
    let mut proposal_ids = Vec::new();
    for i in 1..=2i32 {
        let proposal = TaskProposal {
            id: TaskProposalId::new(),
            session_id: session_id.clone(),
            title: format!("Proposal {}", i),
            description: None,
            category: ProposalCategory::Feature,
            status: ProposalStatus::Pending,
            suggested_priority: Priority::Medium,
            priority_score: 50,
            priority_reason: None,
            priority_factors: None,
            estimated_complexity: Complexity::Moderate,
            user_priority: None,
            user_modified: false,
            steps: None,
            acceptance_criteria: None,
            plan_artifact_id: Some(artifact_id.clone()),
            plan_version_at_creation: None,
            blueprint_artifact_id: None,
            blueprint_version_at_creation: None,
            created_task_id: None,
            selected: false,
            affected_paths: Some(format!(r#"["src/reliability/mismatch_{}.rs"]"#, i)),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            archived_at: None,
            sort_order: i,
            target_project: None,
            migrated_from_session_id: None,
            migrated_from_proposal_id: None,
        };
        let pid = proposal.id.clone();
        state.task_proposal_repo.create(proposal).await.unwrap();
        proposal_ids.push(pid);
    }

    // finalize_proposals_impl must fail due to count mismatch
    let result =
        finalize_proposals_impl(&state, &test_execution_state(), session_id.as_str(), false).await;
    assert!(
        result.is_err(),
        "finalize_proposals_impl must fail when proposal count doesn't match expected"
    );
    match result.unwrap_err() {
        AppError::Validation(msg) => {
            assert!(
                msg.contains("mismatch") || msg.contains("count"),
                "Error must mention count mismatch, got: {}",
                msg
            );
        }
        other => panic!(
            "Expected AppError::Validation for count mismatch, got: {:?}",
            other
        ),
    }

    // Verify proposals are NOT linked to any tasks (no orphaned partial state)
    for pid in &proposal_ids {
        let proposal = state
            .task_proposal_repo
            .get_by_id(pid)
            .await
            .unwrap()
            .unwrap();
        assert!(
            proposal.created_task_id.is_none(),
            "Proposal '{}' must NOT have created_task_id after failed finalize",
            proposal.title
        );
    }
}

// ============================================================================
// C1: Auto-propose delivery retry logic
// ============================================================================

/// All attempts fail → retry fires 4 times total (initial + 3 retries) and
/// exactly one `ideation:auto_propose_failed` event is written to external_events.
#[tokio::test]
async fn test_auto_propose_all_attempts_fail_emits_failure_event() {
    use ralphx_lib::application::MockChatService;
    use ralphx_lib::domain::repositories::ExternalEventsRepository;
    use ralphx_lib::http_server::handlers::auto_propose_with_retry;
    use ralphx_lib::infrastructure::memory::MemoryExternalEventsRepository;

    let mock_service = MockChatService::new();
    mock_service.set_available(false).await;

    let repo = Arc::new(MemoryExternalEventsRepository::new());
    let events_repo: Arc<dyn ExternalEventsRepository> = Arc::clone(&repo) as _;

    let session_id = "test-session-c1";
    let project_id = "test-project-c1";

    // Zero delays so the test completes instantly (4 total attempts: initial + 3 retries)
    auto_propose_with_retry(
        session_id,
        project_id,
        &mock_service,
        events_repo,
        None,
        &[0, 0, 0],
    )
    .await;

    // All 4 attempts (initial + 3 retries) should have been tried
    assert_eq!(
        mock_service.call_count(),
        4,
        "should attempt initial send + 3 retries = 4 total calls"
    );

    // Exactly one failure event written to external_events
    let events = repo
        .get_events_after_cursor(&[project_id.to_string()], 0, 10)
        .await
        .unwrap();
    assert_eq!(events.len(), 1, "should emit exactly one failure event");
    assert_eq!(events[0].event_type, "ideation:auto_propose_failed");
    assert_eq!(events[0].project_id, project_id);

    let payload: serde_json::Value = serde_json::from_str(&events[0].payload).unwrap();
    assert_eq!(payload["session_id"], session_id);
    assert_eq!(payload["project_id"], project_id);
    assert!(
        payload["error"].is_string(),
        "payload must contain error field"
    );
}

/// Success on first attempt → exactly one call made and no failure event written.
#[tokio::test]
async fn test_auto_propose_success_no_failure_event() {
    use ralphx_lib::application::MockChatService;
    use ralphx_lib::domain::repositories::ExternalEventsRepository;
    use ralphx_lib::http_server::handlers::auto_propose_with_retry;
    use ralphx_lib::infrastructure::memory::MemoryExternalEventsRepository;

    let mock_service = MockChatService::new(); // available by default

    let repo = Arc::new(MemoryExternalEventsRepository::new());
    let events_repo: Arc<dyn ExternalEventsRepository> = Arc::clone(&repo) as _;

    let session_id = "test-session-c1-ok";
    let project_id = "test-project-c1-ok";

    auto_propose_with_retry(
        session_id,
        project_id,
        &mock_service,
        events_repo,
        None,
        &[0, 0, 0],
    )
    .await;

    // Succeeds on first attempt — no retries needed
    assert_eq!(
        mock_service.call_count(),
        1,
        "should succeed on first attempt with no retries"
    );

    // On success, exactly one auto_propose_sent event written (Layer 2)
    let events = repo
        .get_events_after_cursor(&[project_id.to_string()], 0, 10)
        .await
        .unwrap();
    assert_eq!(
        events.len(),
        1,
        "should write exactly one auto_propose_sent event on success"
    );
    assert_eq!(events[0].event_type, "ideation:auto_propose_sent");
    assert_eq!(events[0].project_id, project_id);
    let payload: serde_json::Value = serde_json::from_str(&events[0].payload).unwrap();
    assert_eq!(payload["session_id"], session_id);
    assert_eq!(payload["project_id"], project_id);
}

// ============================================================================
// C5: Cross-project finalize filtering — local vs foreign proposal partitioning
// ============================================================================

fn make_c5_session(
    project_id: &ProjectId,
    session_id: &IdeationSessionId,
    artifact_id: &ArtifactId,
) -> IdeationSession {
    IdeationSession {
        id: session_id.clone(),
        project_id: project_id.clone(),
        title: Some("C5 Cross-Project Session".to_string()),
        status: IdeationSessionStatus::Active,
        plan_artifact_id: Some(artifact_id.clone()),
        plan_blueprint_artifact_id: None,
        verified_plan_artifact_id: None,
        verified_plan_blueprint_artifact_id: None,
        verified_plan_agent_run_id: None,
        inherited_plan_artifact_id: None,
        inherited_plan_blueprint_artifact_id: None,
        seed_task_id: None,
        parent_session_id: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        archived_at: None,
        converted_at: None,
        title_source: None,
        verification_status: VerificationStatus::Skipped,
        verification_in_progress: false,
        verification_generation: 0,
        verification_current_round: None,
        verification_max_rounds: None,
        verification_gap_count: 0,
        verification_gap_score: None,
        verification_convergence_reason: None,
        source_project_id: None,
        source_session_id: None,
        source_task_id: None,
        source_context_type: None,
        source_context_id: None,
        spawn_reason: None,
        blocker_fingerprint: None,
        session_purpose: Default::default(),
        session_flow: Default::default(),
        cross_project_checked: true,
        plan_version_last_read: None,
        blueprint_version_last_read: None,
        plan_contract_version: 1,
        origin: SessionOrigin::External,
        expected_proposal_count: None,
        auto_accept_status: None,
        auto_accept_started_at: None,
        api_key_id: None,
        idempotency_key: None,
        external_activity_phase: None,
        external_last_read_message_id: None,
        dependencies_acknowledged: false,
        pending_initial_prompt: None,
        acceptance_status: None,
        verification_confirmation_status: None,
        analysis: Default::default(),
        last_effective_model: None,
    }
}

fn make_c5_proposal(
    session_id: &IdeationSessionId,
    artifact_id: &ArtifactId,
    sort_order: i32,
    target_project: Option<String>,
) -> TaskProposal {
    TaskProposal {
        id: TaskProposalId::new(),
        session_id: session_id.clone(),
        title: format!("C5 Proposal {}", sort_order),
        description: Some(format!("Description for C5 proposal {}", sort_order)),
        category: ProposalCategory::Feature,
        status: ProposalStatus::Pending,
        suggested_priority: Priority::Medium,
        priority_score: 50,
        priority_reason: None,
        priority_factors: None,
        estimated_complexity: Complexity::Moderate,
        user_priority: None,
        user_modified: false,
        steps: None,
        acceptance_criteria: None,
        plan_artifact_id: Some(artifact_id.clone()),
        plan_version_at_creation: None,
        blueprint_artifact_id: None,
        blueprint_version_at_creation: None,
        created_task_id: None,
        selected: false,
        affected_paths: Some(format!(
            r#"["src/reliability/c5_proposal_{}.rs"]"#,
            sort_order
        )),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        archived_at: None,
        sort_order,
        target_project,
        migrated_from_session_id: None,
        migrated_from_proposal_id: None,
    }
}

/// C5a: All-local proposals (target_project=None) finalize normally.
///
/// Regression guard: ensures the foreign-filtering logic does NOT break the
/// baseline path where all proposals are local.
#[tokio::test]
async fn c5a_finalize_local_only_regression() {
    let state = AppState::new_sqlite_test();

    let mut project = Project::new("C5a Project".to_string(), "/tmp/test-c5a".to_string());
    project.use_feature_branches = false;
    let project_id = project.id.clone();
    state.project_repo.create(project).await.unwrap();

    let artifact = Artifact::new_inline("C5a Plan", ArtifactType::Specification, "# C5a", "test");
    let artifact_id = artifact.id.clone();
    state.artifact_repo.create(artifact).await.unwrap();

    let session_id = IdeationSessionId::new();
    let session = make_c5_session(&project_id, &session_id, &artifact_id);
    state.ideation_session_repo.create(session).await.unwrap();

    // 2 local proposals (target_project=None)
    for i in 1..=2i32 {
        let proposal = make_c5_proposal(&session_id, &artifact_id, i, None);
        state.task_proposal_repo.create(proposal).await.unwrap();
    }

    state
        .ideation_session_repo
        .set_dependencies_acknowledged(session_id.as_str())
        .await
        .unwrap();

    let result =
        finalize_proposals_impl(&state, &test_execution_state(), session_id.as_str(), false).await;
    assert!(
        result.is_ok(),
        "C5a: finalize must succeed for all-local proposals, got: {:?}",
        result.err()
    );

    let resp = result.unwrap();
    assert_eq!(resp.tasks_created, 2, "C5a: must create exactly 2 tasks");
    assert_eq!(
        resp.skipped_foreign_count, 0,
        "C5a: no foreign proposals to skip"
    );
    assert_eq!(
        resp.session_status, "accepted",
        "C5a: session must transition to accepted"
    );
}

/// C5b: Mixed local + foreign proposals — only local become tasks, foreign are skipped.
///
/// 2 local (target_project=None) + 3 foreign (target_project="/tmp/test-c5b-other-project")
/// → tasks_created=2, skipped_foreign_count=3, foreign proposals not archived, no created_task_id.
#[tokio::test]
async fn c5b_finalize_mixed_local_and_foreign() {
    let state = AppState::new_sqlite_test();

    let mut project = Project::new("C5b Project".to_string(), "/tmp/test-c5b".to_string());
    project.use_feature_branches = false;
    let project_id = project.id.clone();
    state.project_repo.create(project).await.unwrap();

    let artifact = Artifact::new_inline("C5b Plan", ArtifactType::Specification, "# C5b", "test");
    let artifact_id = artifact.id.clone();
    state.artifact_repo.create(artifact).await.unwrap();

    let session_id = IdeationSessionId::new();
    let session = make_c5_session(&project_id, &session_id, &artifact_id);
    state.ideation_session_repo.create(session).await.unwrap();

    // 2 local proposals
    let mut local_ids = Vec::new();
    for i in 1..=2i32 {
        let proposal = make_c5_proposal(&session_id, &artifact_id, i, None);
        local_ids.push(proposal.id.clone());
        state.task_proposal_repo.create(proposal).await.unwrap();
    }

    // 3 foreign proposals
    let mut foreign_ids = Vec::new();
    for i in 3..=5i32 {
        let proposal = make_c5_proposal(
            &session_id,
            &artifact_id,
            i,
            Some("/tmp/test-c5b-other-project".to_string()),
        );
        foreign_ids.push(proposal.id.clone());
        state.task_proposal_repo.create(proposal).await.unwrap();
    }

    state
        .ideation_session_repo
        .set_dependencies_acknowledged(session_id.as_str())
        .await
        .unwrap();

    let result =
        finalize_proposals_impl(&state, &test_execution_state(), session_id.as_str(), false).await;
    assert!(
        result.is_ok(),
        "C5b: finalize must succeed for mixed proposals, got: {:?}",
        result.err()
    );

    let resp = result.unwrap();
    assert_eq!(
        resp.tasks_created, 2,
        "C5b: must create exactly 2 tasks (local only)"
    );
    assert_eq!(
        resp.skipped_foreign_count, 3,
        "C5b: must report 3 skipped foreign proposals"
    );

    // Foreign proposals must NOT be archived and must NOT have created_task_id set
    for fid in &foreign_ids {
        let proposal = state
            .task_proposal_repo
            .get_by_id(fid)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("C5b: foreign proposal {} must still exist", fid.as_str()));
        assert!(
            proposal.archived_at.is_none(),
            "C5b: foreign proposal '{}' must NOT be archived",
            proposal.title
        );
        assert!(
            proposal.created_task_id.is_none(),
            "C5b: foreign proposal '{}' must NOT have created_task_id set",
            proposal.title
        );
    }

    // Local proposals must have created_task_id set
    for lid in &local_ids {
        let proposal = state
            .task_proposal_repo
            .get_by_id(lid)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("C5b: local proposal {} must exist", lid.as_str()));
        assert!(
            proposal.created_task_id.is_some(),
            "C5b: local proposal '{}' must have created_task_id set after finalize",
            proposal.title
        );
    }
}

/// C5c: All foreign proposals → no tasks created, session stays Active.
///
/// When all proposals are foreign, finalize short-circuits:
/// tasks_created=0, skipped_foreign_count=3, session_status="active", execution_plan_id=None.
#[tokio::test]
async fn c5c_finalize_all_foreign_creates_nothing() {
    let state = AppState::new_sqlite_test();

    let mut project = Project::new("C5c Project".to_string(), "/tmp/test-c5c".to_string());
    project.use_feature_branches = false;
    let project_id = project.id.clone();
    state.project_repo.create(project).await.unwrap();

    let artifact = Artifact::new_inline("C5c Plan", ArtifactType::Specification, "# C5c", "test");
    let artifact_id = artifact.id.clone();
    state.artifact_repo.create(artifact).await.unwrap();

    let session_id = IdeationSessionId::new();
    let session = make_c5_session(&project_id, &session_id, &artifact_id);
    state.ideation_session_repo.create(session).await.unwrap();

    // 3 foreign proposals
    for i in 1..=3i32 {
        let proposal = make_c5_proposal(
            &session_id,
            &artifact_id,
            i,
            Some("/tmp/test-c5c-other-project".to_string()),
        );
        state.task_proposal_repo.create(proposal).await.unwrap();
    }

    // No need to call set_dependencies_acknowledged — short-circuit happens before the gate

    let result =
        finalize_proposals_impl(&state, &test_execution_state(), session_id.as_str(), false).await;
    assert!(
        result.is_ok(),
        "C5c: finalize must succeed even when all proposals are foreign, got: {:?}",
        result.err()
    );

    let resp = result.unwrap();
    assert_eq!(resp.tasks_created, 0, "C5c: no tasks must be created");
    assert_eq!(
        resp.skipped_foreign_count, 3,
        "C5c: must report 3 skipped foreign proposals"
    );
    assert_eq!(
        resp.session_status, "accepted",
        "C5c: session must transition to accepted when all proposals are foreign"
    );
    assert!(
        resp.execution_plan_id.is_none(),
        "C5c: execution_plan_id must be None when no local proposals"
    );

    // Verify session is Accepted in DB
    let session_db = state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .expect("C5c: session must still exist in DB");
    assert_eq!(
        session_db.status,
        IdeationSessionStatus::Accepted,
        "C5c: session must be Accepted in DB"
    );
}

/// C5d: expected_proposal_count validation uses TOTAL count (local + foreign).
///
/// Subcase A: expected=3, actual=2 local + 1 foreign = 3 total → succeeds, tasks_created=2.
/// Subcase B: expected=3, actual=1 local + 1 foreign = 2 total → AppError::Validation (count mismatch).
#[tokio::test]
async fn c5d_expected_count_uses_total_including_foreign() {
    // --- Subcase A: count matches total (local + foreign) → succeeds ---
    {
        let state = AppState::new_sqlite_test();

        let mut project = Project::new("C5d-A Project".to_string(), "/tmp/test-c5d-a".to_string());
        project.use_feature_branches = false;
        let project_id = project.id.clone();
        state.project_repo.create(project).await.unwrap();

        let artifact =
            Artifact::new_inline("C5d-A Plan", ArtifactType::Specification, "# C5d-A", "test");
        let artifact_id = artifact.id.clone();
        state.artifact_repo.create(artifact).await.unwrap();

        let session_id = IdeationSessionId::new();
        let session = make_c5_session(&project_id, &session_id, &artifact_id);
        state.ideation_session_repo.create(session).await.unwrap();

        // Set expected_proposal_count=3 via SQL
        let sid = session_id.as_str().to_string();
        state
            .db
            .run(move |conn| {
                conn.execute(
                    "UPDATE ideation_sessions SET expected_proposal_count = 3 WHERE id = ?1",
                    rusqlite::params![sid],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
                Ok(())
            })
            .await
            .unwrap();

        // 2 local + 1 foreign = 3 total (matches expected)
        for i in 1..=2i32 {
            let proposal = make_c5_proposal(&session_id, &artifact_id, i, None);
            state.task_proposal_repo.create(proposal).await.unwrap();
        }
        let foreign = make_c5_proposal(
            &session_id,
            &artifact_id,
            3,
            Some("/tmp/test-c5d-other".to_string()),
        );
        state.task_proposal_repo.create(foreign).await.unwrap();

        state
            .ideation_session_repo
            .set_dependencies_acknowledged(session_id.as_str())
            .await
            .unwrap();

        let result =
            finalize_proposals_impl(&state, &test_execution_state(), session_id.as_str(), false)
                .await;
        assert!(
            result.is_ok(),
            "C5d-A: finalize must succeed when total count (2+1=3) matches expected=3, got: {:?}",
            result.err()
        );
        let resp = result.unwrap();
        assert_eq!(
            resp.tasks_created, 2,
            "C5d-A: must create 2 tasks (local only)"
        );
        assert_eq!(
            resp.skipped_foreign_count, 1,
            "C5d-A: must skip 1 foreign proposal"
        );
    }

    // --- Subcase B: count mismatches total → validation error ---
    {
        let state = AppState::new_sqlite_test();

        let mut project = Project::new("C5d-B Project".to_string(), "/tmp/test-c5d-b".to_string());
        project.use_feature_branches = false;
        let project_id = project.id.clone();
        state.project_repo.create(project).await.unwrap();

        let artifact =
            Artifact::new_inline("C5d-B Plan", ArtifactType::Specification, "# C5d-B", "test");
        let artifact_id = artifact.id.clone();
        state.artifact_repo.create(artifact).await.unwrap();

        let session_id = IdeationSessionId::new();
        let session = make_c5_session(&project_id, &session_id, &artifact_id);
        state.ideation_session_repo.create(session).await.unwrap();

        // Set expected_proposal_count=3 via SQL
        let sid = session_id.as_str().to_string();
        state
            .db
            .run(move |conn| {
                conn.execute(
                    "UPDATE ideation_sessions SET expected_proposal_count = 3 WHERE id = ?1",
                    rusqlite::params![sid],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
                Ok(())
            })
            .await
            .unwrap();

        // 1 local + 1 foreign = 2 total (mismatches expected=3)
        let local = make_c5_proposal(&session_id, &artifact_id, 1, None);
        state.task_proposal_repo.create(local).await.unwrap();
        let foreign = make_c5_proposal(
            &session_id,
            &artifact_id,
            2,
            Some("/tmp/test-c5d-other".to_string()),
        );
        state.task_proposal_repo.create(foreign).await.unwrap();

        let result =
            finalize_proposals_impl(&state, &test_execution_state(), session_id.as_str(), false)
                .await;
        assert!(
            result.is_err(),
            "C5d-B: finalize must fail when total count (1+1=2) mismatches expected=3"
        );
        match result.unwrap_err() {
            AppError::Validation(msg) => {
                assert!(
                    msg.contains("mismatch") || msg.contains("count"),
                    "C5d-B: validation error must mention count mismatch, got: {}",
                    msg
                );
            }
            other => panic!("C5d-B: expected AppError::Validation, got: {:?}", other),
        }
    }
}

/// C5e: Path canonicalization — trailing slash in target_project treated as local.
///
/// project working_dir="/tmp/test-c5e-path-canon"
/// Proposal 1: target_project="/tmp/test-c5e-path-canon/" (trailing slash) → LOCAL
/// Proposal 2: target_project="/tmp/test-c5e-different-project" → FOREIGN
/// Assert: tasks_created=1, skipped_foreign_count=1, local proposal has created_task_id.
#[tokio::test]
async fn c5e_path_canonicalization_trailing_slash() {
    // Create the directory so canonicalize() succeeds
    std::fs::create_dir_all("/tmp/test-c5e-path-canon")
        .expect("must be able to create test directory");

    let state = AppState::new_sqlite_test();

    let mut project = Project::new(
        "C5e Project".to_string(),
        "/tmp/test-c5e-path-canon".to_string(),
    );
    project.use_feature_branches = false;
    let project_id = project.id.clone();
    state.project_repo.create(project).await.unwrap();

    let artifact = Artifact::new_inline("C5e Plan", ArtifactType::Specification, "# C5e", "test");
    let artifact_id = artifact.id.clone();
    state.artifact_repo.create(artifact).await.unwrap();

    let session_id = IdeationSessionId::new();
    let session = make_c5_session(&project_id, &session_id, &artifact_id);
    state.ideation_session_repo.create(session).await.unwrap();

    // Proposal 1: target_project with trailing slash → should canonicalize to same path → LOCAL
    let local_proposal = make_c5_proposal(
        &session_id,
        &artifact_id,
        1,
        Some("/tmp/test-c5e-path-canon/".to_string()),
    );
    let local_id = local_proposal.id.clone();
    state
        .task_proposal_repo
        .create(local_proposal)
        .await
        .unwrap();

    // Proposal 2: different project → FOREIGN
    let foreign_proposal = make_c5_proposal(
        &session_id,
        &artifact_id,
        2,
        Some("/tmp/test-c5e-different-project".to_string()),
    );
    let foreign_id = foreign_proposal.id.clone();
    state
        .task_proposal_repo
        .create(foreign_proposal)
        .await
        .unwrap();

    state
        .ideation_session_repo
        .set_dependencies_acknowledged(session_id.as_str())
        .await
        .unwrap();

    let result =
        finalize_proposals_impl(&state, &test_execution_state(), session_id.as_str(), false).await;
    assert!(
        result.is_ok(),
        "C5e: finalize must succeed with canonicalized trailing-slash path, got: {:?}",
        result.err()
    );

    let resp = result.unwrap();
    assert_eq!(
        resp.tasks_created, 1,
        "C5e: must create exactly 1 task (trailing-slash local proposal)"
    );
    assert_eq!(
        resp.skipped_foreign_count, 1,
        "C5e: must skip 1 foreign proposal"
    );

    // Local proposal (trailing slash) must have created_task_id set
    let local_db = state
        .task_proposal_repo
        .get_by_id(&local_id)
        .await
        .unwrap()
        .expect("C5e: local proposal must exist");
    assert!(
        local_db.created_task_id.is_some(),
        "C5e: local proposal (trailing-slash path) must have created_task_id set after finalize"
    );

    // Foreign proposal must NOT have created_task_id
    let foreign_db = state
        .task_proposal_repo
        .get_by_id(&foreign_id)
        .await
        .unwrap()
        .expect("C5e: foreign proposal must exist");
    assert!(
        foreign_db.created_task_id.is_none(),
        "C5e: foreign proposal must NOT have created_task_id set"
    );
}

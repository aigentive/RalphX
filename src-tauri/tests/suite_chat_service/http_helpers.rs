use ralphx_lib::application::{
    AppState, CreateProposalOptions, UpdateProposalOptions, UpdateSource,
};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    compute_validation_hint, Artifact, ArtifactId, ArtifactType, IdeationSession, IdeationSessionId,
    IdeationSessionStatus,
    InternalStatus, Priority, Project, ProjectId, ProposalCategory, Task, TaskId, TaskProposalId,
    ValidationCacheDecision, ValidationCommandCategory, ValidationCommandResult,
    ValidationCommandSource, ValidationCommandStatus, ValidationContextType, ValidationPurpose,
    ValidationRun, ValidationRunMode, ValidationRunStatus,
};
use ralphx_lib::error::AppError;
use ralphx_lib::http_server::helpers::*;

// -------------------------------------------------------------------------
// assert_session_mutable tests
// -------------------------------------------------------------------------

#[test]
fn test_assert_session_mutable_active_ok() {
    let session = IdeationSession::new_with_title(ProjectId::new(), "Active Session");
    assert_eq!(session.status, IdeationSessionStatus::Active);
    assert!(assert_session_mutable(&session).is_ok());
}

#[test]
fn test_assert_session_mutable_archived_err() {
    let session = IdeationSession::builder()
        .project_id(ProjectId::new())
        .title("Archived Session")
        .status(IdeationSessionStatus::Archived)
        .build();
    let result = assert_session_mutable(&session);
    assert!(result.is_err());
    match result.unwrap_err() {
        AppError::Validation(msg) => {
            assert!(msg.contains("archived"), "Expected 'archived' in: {}", msg);
            assert!(msg.contains("Reopen"), "Expected 'Reopen' in: {}", msg);
        }
        other => panic!("Expected Validation error, got: {:?}", other),
    }
}

#[test]
fn test_assert_session_mutable_accepted_err() {
    let session = IdeationSession::builder()
        .project_id(ProjectId::new())
        .title("Accepted Session")
        .status(IdeationSessionStatus::Accepted)
        .build();
    let result = assert_session_mutable(&session);
    assert!(result.is_err());
    match result.unwrap_err() {
        AppError::Validation(msg) => {
            assert!(msg.contains("accepted"), "Expected 'accepted' in: {}", msg);
        }
        other => panic!("Expected Validation error, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_create_proposal_without_plan_artifact_returns_validation_error() {
    let state = AppState::new_sqlite_test();
    let project_id = ProjectId::new();

    // Create a session WITHOUT a plan artifact
    let session = IdeationSession::new_with_title(project_id.clone(), "Test Session");
    let session_id = session.id.clone();
    state.ideation_session_repo.create(session).await.unwrap();

    let options = CreateProposalOptions {
        title: "Test Proposal".to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![],
        expected_proposal_count: None,
    };

    let result = create_proposal_impl(&state, session_id, options).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    match &err {
        AppError::Validation(msg) => {
            assert!(
                msg.contains("plan artifact"),
                "Error message should mention plan artifact, got: {}",
                msg
            );
        }
        other => panic!("Expected AppError::Validation, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_create_proposal_with_plan_artifact_succeeds_and_auto_links() {
    let state = AppState::new_sqlite_test();
    let project_id = ProjectId::new();

    // Create artifact first
    let artifact = Artifact::new_inline(
        "Test Plan",
        ArtifactType::Specification,
        "# Plan content",
        "test",
    );
    let artifact_id = artifact.id.clone();
    state.artifact_repo.create(artifact).await.unwrap();
    let blueprint = Artifact::new_inline(
        "Test Blueprint",
        ArtifactType::Specification,
        "# Implementation blueprint",
        "test",
    );
    let blueprint_id = blueprint.id.clone();
    state.artifact_repo.create(blueprint).await.unwrap();

    // Create a session WITH a plan artifact
    let mut session = IdeationSession::builder()
        .project_id(project_id.clone())
        .title("Test Session")
        .plan_artifact_id(artifact_id.clone())
        .blueprint_version_last_read(1)
        .cross_project_checked(true)
        .build();
    session.plan_blueprint_artifact_id = Some(blueprint_id);
    let session_id = session.id.clone();
    state.ideation_session_repo.create(session).await.unwrap();
    mark_plan_bundle_read(&state, &session_id).await;

    let options = CreateProposalOptions {
        title: "Test Proposal".to_string(),
        description: Some("A test proposal".to_string()),
        category: ProposalCategory::Feature,
        suggested_priority: Priority::High,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![],
        expected_proposal_count: None,
    };

    let result = create_proposal_impl(&state, session_id, options).await;
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result.err());

    let (proposal, _dep_errors, _) = result.unwrap();
    assert_eq!(
        proposal.plan_artifact_id,
        Some(artifact_id),
        "Proposal should have plan_artifact_id auto-set from session"
    );
}

#[tokio::test]
async fn test_create_proposal_sets_plan_version_at_creation() {
    let state = AppState::new_sqlite_test();
    let project_id = ProjectId::new();

    // Create artifact with version 1 (default)
    let artifact = Artifact::new_inline(
        "Test Plan",
        ArtifactType::Specification,
        "# Plan v1",
        "test",
    );
    let artifact_id = artifact.id.clone();
    state.artifact_repo.create(artifact).await.unwrap();
    let blueprint = Artifact::new_inline(
        "Test Blueprint",
        ArtifactType::Specification,
        "# Implementation blueprint",
        "test",
    );
    let blueprint_id = blueprint.id.clone();
    state.artifact_repo.create(blueprint).await.unwrap();

    // Create session with plan artifact
    let mut session = IdeationSession::builder()
        .project_id(project_id.clone())
        .title("Test Session")
        .plan_artifact_id(artifact_id.clone())
        .blueprint_version_last_read(1)
        .cross_project_checked(true)
        .build();
    session.plan_blueprint_artifact_id = Some(blueprint_id);
    let session_id = session.id.clone();
    state.ideation_session_repo.create(session).await.unwrap();
    mark_plan_bundle_read(&state, &session_id).await;

    let options = CreateProposalOptions {
        title: "Versioned Proposal".to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![],
        expected_proposal_count: None,
    };

    let (proposal, _dep_errors, _) = create_proposal_impl(&state, session_id, options)
        .await
        .unwrap();

    assert_eq!(
        proposal.plan_version_at_creation,
        Some(1),
        "Proposal should have plan_version_at_creation set to artifact's current version"
    );
}

// ============================================================================
// Verification Gate Integration Tests — Scenarios 10-25
// ============================================================================

/// Shared setup: create artifact + session with a plan and optionally seed the legacy
/// verification-status/proposal-gate fields retained for database compatibility.
async fn setup_session_with_gate(
    state: &AppState,
    verification_status: &str,
    gate_enabled: bool,
) -> (IdeationSession, ArtifactId) {
    let project_id = ProjectId::new();
    let project = ralphx_lib::domain::entities::Project::new(
        "Gate Test Project".to_string(),
        "/tmp/gate-test-project".to_string(),
    );
    let mut project = project;
    project.id = project_id.clone();
    state.project_repo.create(project).await.unwrap();

    let artifact = Artifact::new_inline("Plan", ArtifactType::Specification, "# Plan", "test");
    let artifact_id = artifact.id.clone();
    state.artifact_repo.create(artifact).await.unwrap();

    let blueprint = Artifact::new_inline(
        "Blueprint",
        ArtifactType::Specification,
        "# Implementation blueprint",
        "test",
    );
    let blueprint_id = blueprint.id.clone();
    state.artifact_repo.create(blueprint).await.unwrap();

    let mut session = IdeationSession::builder()
        .project_id(project_id)
        .title("Gate Test Session")
        .plan_artifact_id(artifact_id.clone())
        .cross_project_checked(true)
        .build();
    session.plan_blueprint_artifact_id = Some(blueprint_id);
    state
        .ideation_session_repo
        .create(session.clone())
        .await
        .unwrap();

    // Model the read receipt written by get_session_plan before proposal creation.
    let sid = session.id.as_str().to_string();
    let session_id_for_read = session.id.as_str().to_string();
    let status = verification_status.to_string();
    let gate: i64 = if gate_enabled { 1 } else { 0 };
    state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE ideation_sessions SET verification_status = ?1 WHERE id = ?2",
                rusqlite::params![status, sid],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            conn.execute(
                "UPDATE ideation_sessions
                 SET plan_version_last_read = 1, blueprint_version_last_read = 1
                 WHERE id = ?1",
                rusqlite::params![session_id_for_read],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            conn.execute(
                "UPDATE ideation_settings SET require_verification_for_proposals = ?1 WHERE id = 1",
                rusqlite::params![gate],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();

    (session, artifact_id)
}

/// Shared helper: create a proposal in a verified session (gate off) for use in update/delete tests.
async fn create_test_proposal(state: &AppState, session_id: &IdeationSessionId) -> TaskProposalId {
    let options = CreateProposalOptions {
        title: "Test Proposal".to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![],
        expected_proposal_count: None,
    };
    create_proposal_impl(state, session_id.clone(), options)
        .await
        .expect("test proposal creation should succeed")
        .0
        .id
}

/// Model the paired read receipts written by `get_session_plan` for a v2 plan bundle.
async fn mark_plan_bundle_read(state: &AppState, session_id: &IdeationSessionId) {
    let session_id = session_id.as_str().to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE ideation_sessions
                 SET plan_version_last_read = 1, blueprint_version_last_read = 1
                 WHERE id = ?1",
                rusqlite::params![session_id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();
}

// Scenario 10: The retired proposal gate cannot block draft proposal creation.
#[tokio::test]
async fn test_create_ignores_legacy_proposal_gate_when_unverified() {
    let state = AppState::new_sqlite_test();
    let (session, artifact_id) = setup_session_with_gate(&state, "unverified", true).await;

    let options = CreateProposalOptions {
        title: "Blocked Proposal".to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![],
        expected_proposal_count: None,
    };

    let (created, dependency_errors, ready_to_finalize) =
        create_proposal_impl(&state, session.id.clone(), options)
            .await
            .expect("legacy proposal-gate fields must not block draft creation");
    assert_eq!(created.title, "Blocked Proposal");
    assert_eq!(created.session_id, session.id);
    assert_eq!(created.plan_artifact_id, Some(artifact_id));
    assert!(dependency_errors.is_empty());
    assert!(!ready_to_finalize);
}

// Scenario 11: Tauri IPC and HTTP retain the same ungated draft-creation semantics
// because both use create_proposal_impl.
#[tokio::test]
async fn test_create_ipc_http_parity_when_legacy_gate_is_enabled() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "unverified", true).await;

    // Both IPC and HTTP call create_proposal_impl — the single enforcement point.
    let ipc_options = CreateProposalOptions {
        title: "IPC Proposal".to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![],
        expected_proposal_count: None,
    };
    let http_options = CreateProposalOptions {
        title: "HTTP Proposal".to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![],
        expected_proposal_count: None,
    };

    let ipc_result = create_proposal_impl(&state, session.id.clone(), ipc_options).await;
    let http_result = create_proposal_impl(&state, session.id.clone(), http_options).await;

    let (ipc_created, ipc_dependency_errors, ipc_ready) =
        ipc_result.expect("IPC-backed draft creation must ignore the legacy gate");
    let (http_created, http_dependency_errors, http_ready) =
        http_result.expect("HTTP-backed draft creation must ignore the legacy gate");
    assert_eq!(ipc_created.title, "IPC Proposal");
    assert_eq!(http_created.title, "HTTP Proposal");
    assert_eq!(ipc_created.session_id, session.id);
    assert_eq!(http_created.session_id, session.id);
    assert_ne!(ipc_created.id, http_created.id);
    assert!(ipc_dependency_errors.is_empty());
    assert!(http_dependency_errors.is_empty());
    assert!(!ipc_ready);
    assert!(!http_ready);
}

// Scenario 12: Legacy Reviewing state cannot block proposal draft updates.
#[tokio::test]
async fn test_update_ignores_legacy_reviewing_gate() {
    let state = AppState::new_sqlite_test();
    // First create with gate off to get a proposal
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;
    let proposal_id = create_test_proposal(&state, &session.id).await;

    // Now enable gate and set status to Reviewing
    let sid = session.id.as_str().to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE ideation_sessions SET verification_status = 'reviewing' WHERE id = ?1",
                rusqlite::params![sid],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            conn.execute(
                "UPDATE ideation_settings SET require_verification_for_proposals = 1 WHERE id = 1",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();

    let options = UpdateProposalOptions {
        title: Some("Updated".to_string()),
        source: UpdateSource::Api,
        ..Default::default()
    };
    let (updated, dependency_errors) = update_proposal_impl(&state, &proposal_id, options)
        .await
        .expect("legacy Reviewing state must not block draft updates");
    assert_eq!(updated.title, "Updated");
    assert_eq!(updated.id, proposal_id);
    assert!(dependency_errors.is_empty());
    let persisted = state
        .task_proposal_repo
        .get_by_id(&proposal_id)
        .await
        .unwrap()
        .expect("updated proposal must remain persisted");
    assert_eq!(persisted.title, "Updated");
}

// Scenario 13: Legacy NeedsRevision state cannot block proposal draft archival.
#[tokio::test]
async fn test_archive_ignores_legacy_needs_revision_gate() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;
    let proposal_id = create_test_proposal(&state, &session.id).await;

    // Enable gate and set status to NeedsRevision
    let sid = session.id.as_str().to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE ideation_sessions SET verification_status = 'needs_revision' WHERE id = ?1",
                rusqlite::params![sid],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            conn.execute(
                "UPDATE ideation_settings SET require_verification_for_proposals = 1 WHERE id = 1",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();

    let archived_session_id = archive_proposal_impl(&state, proposal_id.clone())
        .await
        .expect("legacy NeedsRevision state must not block draft archival");
    assert_eq!(archived_session_id, session.id);
    let archived = state
        .task_proposal_repo
        .get_by_id(&proposal_id)
        .await
        .unwrap()
        .expect("archived proposal must remain queryable");
    assert!(archived.archived_at.is_some());
}

// Scenario 14: Single event per operation — verify proposal appears exactly once in DB
// (no duplicate rows, confirming no double-emit causes duplicate writes).
// app_handle=None in tests, so events are silently skipped; impl is called once → one row.
#[tokio::test]
async fn test_create_proposal_inserted_exactly_once() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;

    let options = CreateProposalOptions {
        title: "Single Proposal".to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![],
        expected_proposal_count: None,
    };

    let (proposal, _dep_errors, _) = create_proposal_impl(&state, session.id.clone(), options)
        .await
        .unwrap();

    // Verify exactly one proposal exists in DB
    let proposals = state
        .task_proposal_repo
        .get_by_session(&session.id)
        .await
        .unwrap();
    assert_eq!(proposals.len(), 1, "Exactly one proposal should exist");
    assert_eq!(proposals[0].id, proposal.id);
}

#[tokio::test]
async fn test_create_proposal_persists_affected_paths() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;

    let options = CreateProposalOptions {
        title: "Scoped Proposal".to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: Some(
            serde_json::to_string(&vec![
                "src-tauri/src/http_server".to_string(),
                "src/components/execution".to_string(),
            ])
            .unwrap(),
        ),
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![],
        expected_proposal_count: None,
    };

    let (proposal, _dep_errors, _) = create_proposal_impl(&state, session.id.clone(), options)
        .await
        .unwrap();

    let persisted = state
        .task_proposal_repo
        .get_by_id(&proposal.id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        persisted.affected_paths.as_deref(),
        Some("[\"src-tauri/src/http_server\",\"src/components/execution\"]")
    );
}

// Scenario 15: IPC update sets user_modified=true on changed fields.
#[tokio::test]
async fn test_update_ipc_sets_user_modified() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;
    let proposal_id = create_test_proposal(&state, &session.id).await;

    let options = UpdateProposalOptions {
        title: Some("IPC Updated Title".to_string()),
        source: UpdateSource::TauriIpc,
        ..Default::default()
    };
    let (updated, _dep_errors) = update_proposal_impl(&state, &proposal_id, options)
        .await
        .unwrap();

    assert!(
        updated.user_modified,
        "IPC update must set user_modified=true"
    );
}

// Scenario 16: API update does NOT set user_modified (agent-originated, no field tracking).
#[tokio::test]
async fn test_update_api_does_not_set_user_modified() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;
    let proposal_id = create_test_proposal(&state, &session.id).await;

    // Verify proposal starts with user_modified=false
    let initial = state
        .task_proposal_repo
        .get_by_id(&proposal_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !initial.user_modified,
        "Proposal should start with user_modified=false"
    );

    let options = UpdateProposalOptions {
        title: Some("API Updated Title".to_string()),
        source: UpdateSource::Api,
        ..Default::default()
    };
    let (updated, _dep_errors) = update_proposal_impl(&state, &proposal_id, options)
        .await
        .unwrap();

    assert!(
        !updated.user_modified,
        "API update must NOT set user_modified"
    );
}

#[tokio::test]
async fn test_update_proposal_persists_affected_paths() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;
    let proposal_id = create_test_proposal(&state, &session.id).await;

    let options = UpdateProposalOptions {
        affected_paths: Some(Some(
            serde_json::to_string(&vec![
                "src-tauri/src/application/chat_service".to_string(),
                "src-tauri/tests/chat_service_streaming.rs".to_string(),
            ])
            .unwrap(),
        )),
        source: UpdateSource::Api,
        ..Default::default()
    };
    let (updated, _dep_errors) = update_proposal_impl(&state, &proposal_id, options)
        .await
        .unwrap();

    assert_eq!(
        updated.affected_paths.as_deref(),
        Some(
            "[\"src-tauri/src/application/chat_service\",\"src-tauri/tests/chat_service_streaming.rs\"]"
        )
    );
}

// Scenario 17: estimated_complexity roundtrip — create with complexity → stored correctly.
#[tokio::test]
async fn test_create_proposal_with_estimated_complexity_roundtrip() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;

    let options = CreateProposalOptions {
        title: "Complexity Proposal".to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: Some("complex".to_string()),
        target_project: None,
        depends_on: vec![],
        expected_proposal_count: None,
    };
    let (proposal, _dep_errors, _) = create_proposal_impl(&state, session.id.clone(), options)
        .await
        .unwrap();

    // Reload from DB and verify complexity is stored
    let reloaded = state
        .task_proposal_repo
        .get_by_id(&proposal.id)
        .await
        .unwrap()
        .expect("Proposal must be in DB");
    assert_eq!(
        reloaded.estimated_complexity.to_string().to_lowercase(),
        "complex",
        "Estimated complexity must round-trip correctly"
    );
}

// Scenario 18: assert_session_mutable on update — Archived session blocks update_proposal_impl.
#[tokio::test]
async fn test_update_proposal_on_archived_session_blocked() {
    let state = AppState::new_sqlite_test();
    let project_id = ProjectId::new();

    let artifact = Artifact::new_inline("Plan", ArtifactType::Specification, "# Plan", "test");
    let artifact_id = artifact.id.clone();
    state.artifact_repo.create(artifact).await.unwrap();

    // Create as Active, create proposal, then Archive the session
    let blueprint = Artifact::new_inline(
        "Plan Blueprint",
        ArtifactType::Specification,
        "# Implementation blueprint",
        "test",
    );
    let blueprint_id = blueprint.id.clone();
    state.artifact_repo.create(blueprint).await.unwrap();
    let mut session = IdeationSession::builder()
        .project_id(project_id)
        .title("Archived Session")
        .plan_artifact_id(artifact_id)
        .blueprint_version_last_read(1)
        .cross_project_checked(true)
        .build();
    session.plan_blueprint_artifact_id = Some(blueprint_id);
    let session_id = session.id.clone();
    state.ideation_session_repo.create(session).await.unwrap();
    mark_plan_bundle_read(&state, &session_id).await;
    let proposal_id = create_test_proposal(&state, &session_id).await;

    // Archive the session via SQL
    let sid = session_id.as_str().to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE ideation_sessions SET status = 'archived' WHERE id = ?1",
                rusqlite::params![sid],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();

    let options = UpdateProposalOptions {
        title: Some("Should Fail".to_string()),
        source: UpdateSource::Api,
        ..Default::default()
    };
    let result = update_proposal_impl(&state, &proposal_id, options).await;
    assert!(result.is_err(), "Update on Archived session must fail");
    assert!(
        matches!(result.unwrap_err(), AppError::Validation(_)),
        "Must return Validation error for Archived session"
    );
}

// Scenario 19: assert_session_mutable on delete — Accepted session blocks archive_proposal_impl.
// Also validates the Phase 1 bug fix: HTTP delete now guards via archive_proposal_impl.
#[tokio::test]
async fn test_archive_proposal_on_accepted_session_blocked() {
    let state = AppState::new_sqlite_test();
    let project_id = ProjectId::new();

    let artifact = Artifact::new_inline("Plan", ArtifactType::Specification, "# Plan", "test");
    let artifact_id = artifact.id.clone();
    state.artifact_repo.create(artifact).await.unwrap();

    let blueprint = Artifact::new_inline(
        "Plan Blueprint",
        ArtifactType::Specification,
        "# Implementation blueprint",
        "test",
    );
    let blueprint_id = blueprint.id.clone();
    state.artifact_repo.create(blueprint).await.unwrap();
    let mut session = IdeationSession::builder()
        .project_id(project_id)
        .title("Accepted Session")
        .plan_artifact_id(artifact_id)
        .blueprint_version_last_read(1)
        .cross_project_checked(true)
        .build();
    session.plan_blueprint_artifact_id = Some(blueprint_id);
    let session_id = session.id.clone();
    state.ideation_session_repo.create(session).await.unwrap();
    mark_plan_bundle_read(&state, &session_id).await;
    let proposal_id = create_test_proposal(&state, &session_id).await;

    // Accept the session via SQL
    let sid = session_id.as_str().to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE ideation_sessions SET status = 'accepted' WHERE id = ?1",
                rusqlite::params![sid],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();

    let result = archive_proposal_impl(&state, proposal_id).await;
    assert!(result.is_err(), "Archive on Accepted session must fail");
    assert!(
        matches!(result.unwrap_err(), AppError::Validation(_)),
        "Must return Validation error for Accepted session"
    );
}

#[tokio::test]
async fn test_archive_proposal_clears_dependency_rows() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;

    let dependency_target_id = create_test_proposal(&state, &session.id).await;
    let dependent_id = create_test_proposal(&state, &session.id).await;

    let (_, dep_errors) = update_proposal_impl(
        &state,
        &dependent_id,
        UpdateProposalOptions {
            add_depends_on: vec![dependency_target_id.as_str().to_string()],
            source: UpdateSource::Api,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(
        dep_errors.is_empty(),
        "dependency setup should succeed: {dep_errors:?}"
    );

    archive_proposal_impl(&state, dependency_target_id.clone())
        .await
        .unwrap();

    let proposal_id = dependency_target_id.as_str().to_string();
    let stale_count: i64 = state
        .db
        .run(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM proposal_dependencies
                 WHERE proposal_id = ?1 OR depends_on_proposal_id = ?1",
                rusqlite::params![proposal_id],
                |row| row.get(0),
            )?;
            Ok(count)
        })
        .await
        .unwrap();

    assert_eq!(
        stale_count, 0,
        "archiving must remove related dependency rows"
    );
}

// Scenario 20: Settings — require_verification_for_proposals roundtrip.
// Validates that the settings field persists and is read correctly.
#[tokio::test]
async fn test_settings_require_proposals_roundtrip_via_db() {
    let state = AppState::new_sqlite_test();

    // Enable require_verification_for_proposals
    state
        .db
        .run(|conn| {
            conn.execute(
                "UPDATE ideation_settings SET require_verification_for_proposals = 1 WHERE id = 1",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();

    // Read back via get_settings_sync inside a closure
    let proposals_enabled = state
        .db
        .run(|conn| {
            use ralphx_lib::infrastructure::sqlite::sqlite_ideation_settings_repo::get_settings_sync;
            let s = get_settings_sync(conn)?;
            Ok(s.require_verification_for_proposals)
        })
        .await
        .unwrap();

    assert!(
        proposals_enabled,
        "require_verification_for_proposals must persist as true"
    );

    // Disable and re-verify
    state
        .db
        .run(|conn| {
            conn.execute(
                "UPDATE ideation_settings SET require_verification_for_proposals = 0 WHERE id = 1",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();

    let proposals_disabled = state
        .db
        .run(|conn| {
            use ralphx_lib::infrastructure::sqlite::sqlite_ideation_settings_repo::get_settings_sync;
            let s = get_settings_sync(conn)?;
            Ok(s.require_verification_for_proposals)
        })
        .await
        .unwrap();

    assert!(
        !proposals_disabled,
        "require_verification_for_proposals must persist as false"
    );
}

// Scenario 21: Both verification fields are independent — setting one doesn't affect the other.
#[tokio::test]
async fn test_settings_both_verification_fields_independent() {
    let state = AppState::new_sqlite_test();

    // Set accept=true, proposals=false
    state
        .db
        .run(|conn| {
            conn.execute(
                "UPDATE ideation_settings SET require_verification_for_accept = 1, require_verification_for_proposals = 0 WHERE id = 1",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();

    let (accept, proposals) = state
        .db
        .run(|conn| {
            use ralphx_lib::infrastructure::sqlite::sqlite_ideation_settings_repo::get_settings_sync;
            let s = get_settings_sync(conn)?;
            Ok((s.require_verification_for_accept, s.require_verification_for_proposals))
        })
        .await
        .unwrap();

    assert!(accept, "require_verification_for_accept must be true");
    assert!(
        !proposals,
        "require_verification_for_proposals must be false"
    );

    // Flip: accept=false, proposals=true
    state
        .db
        .run(|conn| {
            conn.execute(
                "UPDATE ideation_settings SET require_verification_for_accept = 0, require_verification_for_proposals = 1 WHERE id = 1",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();

    let (accept2, proposals2) = state
        .db
        .run(|conn| {
            use ralphx_lib::infrastructure::sqlite::sqlite_ideation_settings_repo::get_settings_sync;
            let s = get_settings_sync(conn)?;
            Ok((s.require_verification_for_accept, s.require_verification_for_proposals))
        })
        .await
        .unwrap();

    assert!(!accept2, "require_verification_for_accept must be false");
    assert!(
        proposals2,
        "require_verification_for_proposals must be true"
    );
}

// Scenario 22: require_verification_for_accept roundtrip — validates the hardcoded-false bug fix.
// The settings repo used to return hardcoded `false` for this field regardless of DB value.
#[tokio::test]
async fn test_settings_require_accept_roundtrip_true_case() {
    let state = AppState::new_sqlite_test();

    state
        .db
        .run(|conn| {
            conn.execute(
                "UPDATE ideation_settings SET require_verification_for_accept = 1 WHERE id = 1",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();

    let accept_enabled = state
        .db
        .run(|conn| {
            use ralphx_lib::infrastructure::sqlite::sqlite_ideation_settings_repo::get_settings_sync;
            let s = get_settings_sync(conn)?;
            Ok(s.require_verification_for_accept)
        })
        .await
        .unwrap();

    assert!(
        accept_enabled,
        "require_verification_for_accept must round-trip as true (validates bug fix)"
    );
}

// Scenario 23: Gate allows all ops when gate is off — create always succeeds regardless of status.
#[tokio::test]
async fn test_create_gate_off_allows_any_status() {
    let state = AppState::new_sqlite_test();
    // Gate off (default), session in Reviewing state
    let (session, _) = setup_session_with_gate(&state, "reviewing", false).await;

    let options = CreateProposalOptions {
        title: "Gate Off Proposal".to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![],
        expected_proposal_count: None,
    };

    let result = create_proposal_impl(&state, session.id.clone(), options).await;
    assert!(
        result.is_ok(),
        "Create must succeed when gate=off, got: {:?}",
        result.err()
    );
}

// Scenario 24: Concurrent creates produce unique sort_orders (TOCTOU prevention via transaction lock).
#[tokio::test]
async fn test_concurrent_creates_produce_unique_sort_orders() {
    use std::collections::HashSet;
    use std::sync::Arc;

    let state = Arc::new(AppState::new_sqlite_test());
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;
    let session_id = session.id.clone();

    let make_options = |n: u32| CreateProposalOptions {
        title: format!("Concurrent Proposal {}", n),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![],
        expected_proposal_count: None,
    };

    // Run 3 creates concurrently via tokio::join! (they share the same mutex-serialized DB conn)
    let (r1, r2, r3) = tokio::join!(
        create_proposal_impl(&state, session_id.clone(), make_options(1)),
        create_proposal_impl(&state, session_id.clone(), make_options(2)),
        create_proposal_impl(&state, session_id.clone(), make_options(3)),
    );

    let (p1, _, _) = r1.expect("concurrent create 1 must succeed");
    let (p2, _, _) = r2.expect("concurrent create 2 must succeed");
    let (p3, _, _) = r3.expect("concurrent create 3 must succeed");

    // All sort_orders must be unique (no TOCTOU duplicates)
    let orders: HashSet<i32> = [p1.sort_order, p2.sort_order, p3.sort_order]
        .iter()
        .cloned()
        .collect();
    assert_eq!(
        orders.len(),
        3,
        "All 3 sort_orders must be unique, got: {:?}",
        orders
    );

    // Verify all 3 proposals exist in DB
    let all_proposals = state
        .task_proposal_repo
        .get_by_session(&session_id)
        .await
        .unwrap();
    assert_eq!(all_proposals.len(), 3, "All 3 proposals must be in DB");
}

// =========================================================================
// Inline dependency tests — depends_on / add_depends_on / add_blocks
// =========================================================================

// Scenario 26: create with valid depends_on inserts the dependency and returns no dep_errors.
#[tokio::test]
async fn test_create_with_valid_depends_on_inserts_dependency() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;

    // Create proposal A (no deps)
    let a_id = create_test_proposal(&state, &session.id).await;

    // Create proposal B with depends_on=[A]
    let options = CreateProposalOptions {
        title: "Proposal B".to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![a_id.as_str().to_string()],
        expected_proposal_count: None,
    };
    let (b_proposal, dep_errors, _) = create_proposal_impl(&state, session.id.clone(), options)
        .await
        .expect("create with valid dep should succeed");

    assert!(
        dep_errors.is_empty(),
        "Expected no dep errors, got: {:?}",
        dep_errors
    );

    // Verify dep was inserted: B should have 1 dependency (A)
    let dep_count = state
        .proposal_dependency_repo
        .count_dependencies(&b_proposal.id)
        .await
        .expect("count_dependencies should succeed");
    assert_eq!(dep_count, 1, "B should have exactly 1 dependency (A)");
}

// Scenario 27: create with nonexistent dep → partial failure (proposal created, dep_errors non-empty).
#[tokio::test]
async fn test_create_with_nonexistent_dep_partial_failure() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;

    let options = CreateProposalOptions {
        title: "Proposal with bad dep".to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: None,
        depends_on: vec!["nonexistent-proposal-id".to_string()],
        expected_proposal_count: None,
    };
    let (proposal, dep_errors, _) = create_proposal_impl(&state, session.id.clone(), options)
        .await
        .expect("proposal itself should be created despite bad dep");

    // Proposal was created
    let in_db = state
        .task_proposal_repo
        .get_by_id(&proposal.id)
        .await
        .unwrap();
    assert!(
        in_db.is_some(),
        "Proposal should be in DB despite dep error"
    );

    // dep_errors has one entry for the nonexistent dep
    assert_eq!(
        dep_errors.len(),
        1,
        "Expected one dep error, got: {:?}",
        dep_errors
    );
    assert!(
        dep_errors[0].contains("not found"),
        "Error should mention not found, got: {}",
        dep_errors[0]
    );
}

// Scenario 28: create with cross-session dep → rejected in dep_errors, proposal still created.
#[tokio::test]
async fn test_create_with_cross_session_dep_rejected() {
    let state = AppState::new_sqlite_test();
    let (session1, _) = setup_session_with_gate(&state, "verified", false).await;
    let (session2, _) = setup_session_with_gate(&state, "verified", false).await;

    // Create proposal in session2
    let other_proposal_id = create_test_proposal(&state, &session2.id).await;

    // Try to create in session1 with dep from session2
    let options = CreateProposalOptions {
        title: "Cross-session dep".to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![other_proposal_id.as_str().to_string()],
        expected_proposal_count: None,
    };
    let (_, dep_errors, _) = create_proposal_impl(&state, session1.id.clone(), options)
        .await
        .expect("proposal itself should be created despite cross-session dep error");

    assert_eq!(dep_errors.len(), 1, "Expected one dep error");
    assert!(
        dep_errors[0].contains("not in same session"),
        "Error should mention session mismatch, got: {}",
        dep_errors[0]
    );
}

// Scenario 29: update add_depends_on self-dep → rejected.
#[tokio::test]
async fn test_update_add_depends_on_self_dep_rejected() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;
    let proposal_id = create_test_proposal(&state, &session.id).await;

    let options = UpdateProposalOptions {
        add_depends_on: vec![proposal_id.as_str().to_string()],
        source: UpdateSource::Api,
        ..Default::default()
    };
    let (_, dep_errors) = update_proposal_impl(&state, &proposal_id, options)
        .await
        .expect("update itself should succeed despite self-dep error");

    assert_eq!(dep_errors.len(), 1, "Expected one dep error");
    assert!(
        dep_errors[0].contains("self-dependency"),
        "Error should mention self-dependency, got: {}",
        dep_errors[0]
    );
}

// Scenario 30: add_depends_on cycle detection — A→B already exists, B→A rejected.
#[tokio::test]
async fn test_update_add_depends_on_cycle_rejected() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;

    let a_id = create_test_proposal(&state, &session.id).await;
    let b_id = create_test_proposal(&state, &session.id).await;

    // A depends on B (A→B)
    let (_, errs) = update_proposal_impl(
        &state,
        &a_id,
        UpdateProposalOptions {
            add_depends_on: vec![b_id.as_str().to_string()],
            source: UpdateSource::Api,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(
        errs.is_empty(),
        "First dep (A→B) should succeed: {:?}",
        errs
    );

    // Now try B depends on A (B→A) — would create cycle A→B→A
    let (_, dep_errors) = update_proposal_impl(
        &state,
        &b_id,
        UpdateProposalOptions {
            add_depends_on: vec![a_id.as_str().to_string()],
            source: UpdateSource::Api,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(dep_errors.len(), 1, "Expected one dep error");
    assert!(
        dep_errors[0].contains("would create cycle"),
        "Error should mention cycle, got: {}",
        dep_errors[0]
    );
}

// Scenario 31: add_blocks cycle detection (reverse direction) — A→B exists, add_blocks=[B] on A
// would insert B→A, creating a cycle. Rejected.
#[tokio::test]
async fn test_update_add_blocks_cycle_rejected() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;

    let a_id = create_test_proposal(&state, &session.id).await;
    let b_id = create_test_proposal(&state, &session.id).await;

    // A depends on B (A→B)
    let (_, errs) = update_proposal_impl(
        &state,
        &a_id,
        UpdateProposalOptions {
            add_depends_on: vec![b_id.as_str().to_string()],
            source: UpdateSource::Api,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(
        errs.is_empty(),
        "First dep (A→B) should succeed: {:?}",
        errs
    );

    // Update A with add_blocks=[B] → would insert B depends_on A (B→A)
    // Cycle check: would_create_cycle(B, A) → true since A→B exists → rejected
    let (_, dep_errors) = update_proposal_impl(
        &state,
        &a_id,
        UpdateProposalOptions {
            add_blocks: vec![b_id.as_str().to_string()],
            source: UpdateSource::Api,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(dep_errors.len(), 1, "Expected one dep error");
    assert!(
        dep_errors[0].contains("would create cycle"),
        "Error should mention cycle, got: {}",
        dep_errors[0]
    );
}

// Scenario 32: add_blocks self-dep → rejected.
#[tokio::test]
async fn test_update_add_blocks_self_dep_rejected() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;
    let proposal_id = create_test_proposal(&state, &session.id).await;

    let options = UpdateProposalOptions {
        add_blocks: vec![proposal_id.as_str().to_string()],
        source: UpdateSource::Api,
        ..Default::default()
    };
    let (_, dep_errors) = update_proposal_impl(&state, &proposal_id, options)
        .await
        .expect("update itself should succeed despite self-dep error");

    assert_eq!(dep_errors.len(), 1, "Expected one dep error");
    assert!(
        dep_errors[0].contains("self-dependency"),
        "Error should mention self-dependency, got: {}",
        dep_errors[0]
    );
}

// Scenario 33: partial failure — valid dep + nonexistent dep → one inserted, one error.
#[tokio::test]
async fn test_update_add_depends_on_partial_failure() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;

    let a_id = create_test_proposal(&state, &session.id).await;
    let b_id = create_test_proposal(&state, &session.id).await;

    // B add_depends_on: [A (valid), nonexistent (invalid)]
    let (_, dep_errors) = update_proposal_impl(
        &state,
        &b_id,
        UpdateProposalOptions {
            add_depends_on: vec![a_id.as_str().to_string(), "nonexistent-id".to_string()],
            source: UpdateSource::Api,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Exactly one error (for nonexistent), valid dep was inserted
    assert_eq!(
        dep_errors.len(),
        1,
        "Expected one dep error, got: {:?}",
        dep_errors
    );
    assert!(
        dep_errors[0].contains("not found"),
        "Error should mention not found, got: {}",
        dep_errors[0]
    );

    // Valid dep was inserted: B should have 1 dependency (A)
    let dep_count = state
        .proposal_dependency_repo
        .count_dependencies(&b_id)
        .await
        .expect("count_dependencies should succeed");
    assert_eq!(
        dep_count, 1,
        "B→A dep should have been inserted successfully (1 dep)"
    );
}

// ============================================================================
// Stale Plan Guard Tests — Version Gate for Proposal Creation
// ============================================================================

// Proof obligation 1: a NULL overview receipt remains a backwards-compatible passthrough.
// A v2 bundle still requires the paired blueprint receipt.
#[tokio::test]
async fn test_stale_plan_guard_null_passthrough() {
    let state = AppState::new_sqlite_test();
    let project_id = ProjectId::new();

    let artifact = Artifact::new_inline("Plan", ArtifactType::Specification, "# Plan v1", "test");
    let artifact_id = artifact.id.clone();
    state.artifact_repo.create(artifact).await.unwrap();

    let blueprint = Artifact::new_inline(
        "Plan Blueprint",
        ArtifactType::Specification,
        "# Implementation blueprint",
        "test",
    );
    let blueprint_id = blueprint.id.clone();
    state.artifact_repo.create(blueprint).await.unwrap();

    // Builder default: plan_version_last_read = None (no .plan_version_last_read() call)
    let mut session = IdeationSession::builder()
        .project_id(project_id)
        .title("Legacy Session")
        .plan_artifact_id(artifact_id)
        .blueprint_version_last_read(1)
        .cross_project_checked(true)
        .build();
    session.plan_blueprint_artifact_id = Some(blueprint_id);
    let session_id = session.id.clone();
    state.ideation_session_repo.create(session).await.unwrap();
    let blueprint_read_session_id = session_id.as_str().to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE ideation_sessions SET blueprint_version_last_read = 1 WHERE id = ?1",
                rusqlite::params![blueprint_read_session_id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();

    let options = CreateProposalOptions {
        title: "Legacy Proposal".to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![],
        expected_proposal_count: None,
    };
    let result = create_proposal_impl(&state, session_id, options).await;
    assert!(
        result.is_ok(),
        "NULL plan_version_last_read must not block proposal creation, got: {:?}",
        result.err()
    );
}

// Proof obligation 2: plan_version_last_read == artifact.version → OK (fresh read).
// Simulates agent calling get_session_plan (sets plan_version_last_read = 1 via SQL).
#[tokio::test]
async fn test_stale_plan_guard_fresh_version_ok() {
    let state = AppState::new_sqlite_test();
    let project_id = ProjectId::new();

    let artifact = Artifact::new_inline("Plan", ArtifactType::Specification, "# Plan v1", "test");
    let artifact_id = artifact.id.clone();
    state.artifact_repo.create(artifact).await.unwrap();

    let blueprint = Artifact::new_inline(
        "Plan Blueprint",
        ArtifactType::Specification,
        "# Implementation blueprint",
        "test",
    );
    let blueprint_id = blueprint.id.clone();
    state.artifact_repo.create(blueprint).await.unwrap();

    let mut session = IdeationSession::builder()
        .project_id(project_id)
        .title("Fresh Read Session")
        .plan_artifact_id(artifact_id)
        .blueprint_version_last_read(1)
        .cross_project_checked(true)
        .build();
    session.plan_blueprint_artifact_id = Some(blueprint_id);
    let session_id = session.id.clone();
    state.ideation_session_repo.create(session).await.unwrap();

    // Simulate get_session_plan acknowledgment for both v2 plan-bundle artifacts.
    mark_plan_bundle_read(&state, &session_id).await;

    let options = CreateProposalOptions {
        title: "Fresh Proposal".to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![],
        expected_proposal_count: None,
    };
    let result = create_proposal_impl(&state, session_id, options).await;
    assert!(
        result.is_ok(),
        "plan_version_last_read == artifact.version must succeed, got: {:?}",
        result.err()
    );
}

// Proof obligations 3 & 4: plan_version_last_read < artifact.version → 400 error
// with actionable message naming get_session_plan.
#[tokio::test]
async fn test_stale_plan_guard_stale_version_blocked_with_actionable_error() {
    let state = AppState::new_sqlite_test();
    let project_id = ProjectId::new();

    let artifact = Artifact::new_inline("Plan", ArtifactType::Specification, "# Plan v1", "test");
    let artifact_id = artifact.id.clone();
    state.artifact_repo.create(artifact).await.unwrap();

    let blueprint = Artifact::new_inline(
        "Plan Blueprint",
        ArtifactType::Specification,
        "# Implementation blueprint",
        "test",
    );
    let blueprint_id = blueprint.id.clone();
    state.artifact_repo.create(blueprint).await.unwrap();

    let mut session = IdeationSession::builder()
        .project_id(project_id)
        .title("Stale Read Session")
        .plan_artifact_id(artifact_id.clone())
        .blueprint_version_last_read(1)
        .cross_project_checked(true)
        .build();
    session.plan_blueprint_artifact_id = Some(blueprint_id);
    let session_id = session.id.clone();
    state.ideation_session_repo.create(session).await.unwrap();

    // Agent reads the full v2 bundle at v1.
    mark_plan_bundle_read(&state, &session_id).await;

    // Simulate child bumping plan to v2 (plan_version_last_read is now stale)
    let aid = artifact_id.as_str().to_string();
    state
        .db
        .run(move |conn| {
            conn.execute(
                "UPDATE artifacts SET version = 2 WHERE id = ?1",
                rusqlite::params![aid],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        })
        .await
        .unwrap();

    let options = CreateProposalOptions {
        title: "Stale Proposal".to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![],
        expected_proposal_count: None,
    };
    let result = create_proposal_impl(&state, session_id, options).await;
    assert!(
        result.is_err(),
        "Stale plan_version_last_read must block proposal creation"
    );
    match result.unwrap_err() {
        AppError::Validation(msg) => {
            assert!(
                msg.contains("get_session_plan"),
                "Error must name 'get_session_plan' as the fix, got: {}",
                msg
            );
            assert!(
                msg.contains("v2"),
                "Error must include current artifact version (v2), got: {}",
                msg
            );
            assert!(
                msg.contains("v1"),
                "Error must include last-read version (v1), got: {}",
                msg
            );
        }
        other => panic!("Expected AppError::Validation, got: {:?}", other),
    }
}

// Scenario 25: Concurrent create during status transition — TOCTOU prevention.
// If status changes to Reviewing while create is queued, the transaction reads the
// current status atomically — either succeeds (Verified before change) or fails (Reviewing after).
// No partial state is possible.
#[tokio::test]
async fn test_concurrent_create_during_status_transition_no_partial_state() {
    use std::sync::Arc;

    let state = Arc::new(AppState::new_sqlite_test());
    let (session, _) = setup_session_with_gate(&state, "verified", true).await;
    let session_id = session.id.clone();

    let options = CreateProposalOptions {
        title: "Race Condition Test".to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![],
        expected_proposal_count: None,
    };

    // Concurrently: attempt create AND change status to Reviewing
    let sid = session_id.as_str().to_string();
    let (create_result, transition_result) = tokio::join!(
        create_proposal_impl(&state, session_id.clone(), options),
        state.db.run(move |conn| {
            conn.execute(
                "UPDATE ideation_sessions SET verification_status = 'reviewing' WHERE id = ?1",
                rusqlite::params![sid],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
            Ok(())
        }),
    );

    // Transition must always succeed
    transition_result.expect("Status transition must succeed");

    // Create either succeeded (ran before transition) or failed (ran after transition)
    // — never a partial/corrupt state
    match create_result {
        Ok((proposal, _dep_errors, _auto_accept_triggered)) => {
            // Proposal was created; verify it's in DB (no partial write)
            let in_db = state
                .task_proposal_repo
                .get_by_id(&proposal.id)
                .await
                .unwrap();
            assert!(in_db.is_some(), "Created proposal must be in DB");
        }
        Err(AppError::Validation(_)) => {
            // Gate blocked the create (status was already Reviewing when transaction ran)
            // No partial state — verify proposal count is 0
            let proposals = state
                .task_proposal_repo
                .get_by_session(&session_id)
                .await
                .unwrap();
            assert_eq!(proposals.len(), 0, "No partial proposals should exist");
        }
        Err(other) => panic!("Unexpected error type: {:?}", other),
    }
}

// ============================================================================
// expected_proposal_count Gating & Auto-Accept Tests — Scenarios 23-29
// ============================================================================

/// Shared helper: build CreateProposalOptions with an expected count and a title.
fn make_proposal_options(title: &str, expected_count: Option<u32>) -> CreateProposalOptions {
    CreateProposalOptions {
        title: title.to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: Some(r#"["src-tauri/src"]"#.to_string()),
        estimated_complexity: None,
        target_project: None,
        depends_on: vec![],
        expected_proposal_count: expected_count,
    }
}

// Scenario 23: First proposal locks expected_proposal_count on the session.
#[tokio::test]
async fn test_expected_count_set_on_first_proposal() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;

    create_proposal_impl(
        &state,
        session.id.clone(),
        make_proposal_options("First", Some(3)),
    )
    .await
    .expect("first proposal should succeed");

    let updated = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .unwrap()
        .expect("session must exist");

    assert_eq!(
        updated.expected_proposal_count,
        Some(3),
        "expected_proposal_count must be locked to 3 after first proposal"
    );
}

// Scenario 24: Subsequent proposal with different expected count → Validation error with mismatch message.
#[tokio::test]
async fn test_expected_count_mismatch_rejected() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;

    // First proposal: locks expected=3
    create_proposal_impl(
        &state,
        session.id.clone(),
        make_proposal_options("First", Some(3)),
    )
    .await
    .expect("first proposal should succeed");

    // Second proposal: claims expected=5 — must be rejected
    let result = create_proposal_impl(
        &state,
        session.id.clone(),
        make_proposal_options("Second", Some(5)),
    )
    .await;

    assert!(
        result.is_err(),
        "Mismatched expected_proposal_count must be rejected"
    );
    match result.unwrap_err() {
        AppError::Validation(msg) => {
            assert!(
                msg.contains("mismatch"),
                "Error must contain 'mismatch', got: {msg}"
            );
            assert!(
                msg.contains('3'),
                "Error must mention stored count 3, got: {msg}"
            );
        }
        other => panic!("Expected AppError::Validation, got: {other:?}"),
    }
}

// Scenario 25: ready_to_finalize=true when active count reaches expected count.
// Verifies the signal is returned; session remains Active (agent must call finalize_proposals).
#[tokio::test]
async fn test_ready_to_finalize_on_count_match() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;

    // Proposals 1 and 2: no signal
    for i in 1..=2 {
        let (_, _, ready) = create_proposal_impl(
            &state,
            session.id.clone(),
            make_proposal_options(&format!("Proposal {i}"), Some(3)),
        )
        .await
        .expect("proposal should succeed");
        assert!(
            !ready,
            "ready_to_finalize must be false for proposal {i} of 3"
        );
    }

    // Proposal 3: count == expected → ready_to_finalize=true
    let (_, _, ready) = create_proposal_impl(
        &state,
        session.id.clone(),
        make_proposal_options("Proposal 3", Some(3)),
    )
    .await
    .expect("third proposal should succeed");

    assert!(
        ready,
        "ready_to_finalize must be true when active count == expected"
    );

    // Session must remain Active — agent drives finalize_proposals explicitly
    let updated = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .unwrap()
        .expect("session must exist");

    assert_eq!(
        updated.status,
        IdeationSessionStatus::Active,
        "Session must remain Active after ready_to_finalize signal"
    );
    assert!(
        updated.auto_accept_status.is_none(),
        "auto_accept_status must not be set — fire-and-forget removed"
    );
}

// Scenario 26: Partial proposals (crash safety) — session stays Active with null auto_accept_status.
#[tokio::test]
async fn test_partial_proposals_no_auto_accept() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;

    // Create only 2 of 3 expected proposals
    for i in 1..=2 {
        let (_, _, triggered) = create_proposal_impl(
            &state,
            session.id.clone(),
            make_proposal_options(&format!("Proposal {i}"), Some(3)),
        )
        .await
        .expect("proposal should succeed");
        assert!(!triggered, "Must not trigger after only {i} of 3 proposals");
    }

    let updated = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .unwrap()
        .expect("session must exist");

    assert_eq!(
        updated.status,
        IdeationSessionStatus::Active,
        "Session must remain Active with only 2 of 3 proposals"
    );
    assert!(
        updated.auto_accept_status.is_none(),
        "auto_accept_status must be null when count < expected"
    );
}

// Scenario 27: Verification gate blocks finalize_proposals synchronously.
// Proposals gate (require_verification_for_proposals) is off so proposals can be created.
// Accept gate (require_verification_for_accept) is on and session is unverified → finalize fails.
#[tokio::test]
async fn test_finalize_blocked_by_verification_gate() {
    let state = AppState::new_sqlite_test();
    // Proposal creation gate disabled (gate_enabled=false) + verification_status='unverified'
    let (session, _) = setup_session_with_gate(&state, "unverified", false).await;

    // Enable require_verification_for_accept on the in-memory settings repo
    let mut settings = state
        .ideation_settings_repo
        .get_settings()
        .await
        .expect("get settings should succeed");
    settings.require_verification_for_accept = true;
    state
        .ideation_settings_repo
        .update_settings(&settings)
        .await
        .expect("update settings should succeed");

    // Create 3 proposals with expected=3 — last one returns ready_to_finalize=true
    for i in 1..=3 {
        create_proposal_impl(
            &state,
            session.id.clone(),
            make_proposal_options(&format!("Proposal {i}"), Some(3)),
        )
        .await
        .expect("proposal creation should succeed (proposal gate is off)");
    }

    // Explicitly call finalize_proposals — must fail with validation error
    let execution_state = std::sync::Arc::new(ExecutionState::new());
    let result =
        finalize_proposals_impl(&state, &execution_state, session.id.as_str(), false).await;

    assert!(
        result.is_err(),
        "finalize_proposals must fail when verification gate blocks acceptance"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, AppError::Validation(_)),
        "Error must be Validation, got: {err:?}"
    );

    // Session must remain Active (no state change on failure)
    let updated = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .unwrap()
        .expect("session must exist");

    assert_eq!(
        updated.status,
        IdeationSessionStatus::Active,
        "Session must remain Active when finalize fails"
    );
}

#[tokio::test]
async fn test_finalize_confirmation_does_not_auto_verify_before_user_accepts() {
    use ralphx_lib::application::plan_verification_service::{
        get_plan_verification_status, PlanVerificationStatusKind,
    };

    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "unverified", false).await;

    let mut settings = state
        .ideation_settings_repo
        .get_settings()
        .await
        .expect("get settings should succeed");
    settings.require_accept_for_finalize = true;
    settings.require_verification_for_accept = true;
    settings.auto_verify_plans = true;
    state
        .ideation_settings_repo
        .update_settings(&settings)
        .await
        .expect("update settings should succeed");

    create_proposal_impl(
        &state,
        session.id.clone(),
        make_proposal_options("Proposal", Some(1)),
    )
    .await
    .expect("proposal creation should succeed");

    let execution_state = std::sync::Arc::new(ExecutionState::new());
    let response = finalize_proposals_impl(&state, &execution_state, session.id.as_str(), false)
        .await
        .expect("finalize should pause for user acceptance before verification admission");
    assert_eq!(response.status, "pending_acceptance");

    let status = get_plan_verification_status(&state, &session.id)
        .await
        .expect("verification status should load");
    assert_eq!(status.status, PlanVerificationStatusKind::Unverified);
    let persisted = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .expect("session read should succeed")
        .expect("session should exist");
    assert!(persisted.pending_initial_prompt.is_none());
}

#[tokio::test]
async fn test_finalize_rejects_feature_without_affected_paths() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;

    let mut options = make_proposal_options("Feature Without Scope", Some(1));
    options.affected_paths = None;

    create_proposal_impl(&state, session.id.clone(), options)
        .await
        .expect("proposal creation should still succeed");

    let execution_state = std::sync::Arc::new(ExecutionState::new());
    let result =
        finalize_proposals_impl(&state, &execution_state, session.id.as_str(), false).await;

    assert!(
        result.is_err(),
        "finalize must reject feature proposals without scope"
    );
    match result.unwrap_err() {
        AppError::Validation(msg) => {
            assert!(
                msg.contains("affected_paths"),
                "Error must mention affected_paths, got: {msg}"
            );
            assert!(
                msg.contains("Feature Without Scope"),
                "Error must include proposal title, got: {msg}"
            );
        }
        other => panic!("Expected validation error, got {other:?}"),
    }
}

#[tokio::test]
async fn test_finalize_allows_research_without_affected_paths() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;

    let mut options = make_proposal_options("Research Spike", Some(1));
    options.category = ProposalCategory::Research;
    options.affected_paths = None;

    create_proposal_impl(&state, session.id.clone(), options)
        .await
        .expect("proposal creation should succeed");

    let execution_state = std::sync::Arc::new(ExecutionState::new());
    let response = finalize_proposals_impl(&state, &execution_state, session.id.as_str(), false)
        .await
        .expect("research proposal should finalize without affected_paths");

    assert_eq!(response.tasks_created, 1);
}

#[tokio::test]
async fn test_finalize_allows_design_without_affected_paths() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;

    let mut options = make_proposal_options("Design Spike", Some(1));
    options.category = ProposalCategory::Design;
    options.affected_paths = None;

    create_proposal_impl(&state, session.id.clone(), options)
        .await
        .expect("proposal creation should succeed");

    let execution_state = std::sync::Arc::new(ExecutionState::new());
    let response = finalize_proposals_impl(&state, &execution_state, session.id.as_str(), false)
        .await
        .expect("design proposal should finalize without affected_paths");

    assert_eq!(response.tasks_created, 1);
}

#[tokio::test]
async fn test_finalize_ignores_foreign_feature_without_affected_paths() {
    let state = AppState::new_sqlite_test();
    let project_id = ProjectId::new();
    let project = ralphx_lib::domain::entities::Project::new(
        "Foreign Scope Source".to_string(),
        "/tmp/source-project".to_string(),
    );
    let mut project = project;
    project.id = project_id.clone();
    state.project_repo.create(project).await.unwrap();

    let artifact = Artifact::new_inline(
        "Cross-project plan",
        ArtifactType::Specification,
        "# Source plan",
        "test",
    );
    let artifact_id = artifact.id.clone();
    state.artifact_repo.create(artifact).await.unwrap();

    let blueprint = Artifact::new_inline(
        "Cross-project blueprint",
        ArtifactType::Specification,
        "# Implementation blueprint",
        "test",
    );
    let blueprint_id = blueprint.id.clone();
    state.artifact_repo.create(blueprint).await.unwrap();

    let mut session = IdeationSession::builder()
        .project_id(project_id)
        .title("Cross-project source session")
        .plan_artifact_id(artifact_id)
        .blueprint_version_last_read(1)
        .cross_project_checked(true)
        .expected_proposal_count(1)
        .build();
    session.plan_blueprint_artifact_id = Some(blueprint_id);
    let session_id = session.id.clone();
    state.ideation_session_repo.create(session).await.unwrap();
    mark_plan_bundle_read(&state, &session_id).await;

    let options = CreateProposalOptions {
        title: "Foreign feature without local scope".to_string(),
        description: None,
        category: ProposalCategory::Feature,
        suggested_priority: Priority::Medium,
        steps: None,
        acceptance_criteria: None,
        affected_paths: None,
        estimated_complexity: None,
        target_project: Some("/tmp/other-project".to_string()),
        depends_on: vec![],
        expected_proposal_count: Some(1),
    };

    create_proposal_impl(&state, session_id.clone(), options)
        .await
        .expect("proposal creation should succeed");

    let execution_state = std::sync::Arc::new(ExecutionState::new());
    let response = finalize_proposals_impl(&state, &execution_state, session_id.as_str(), false)
        .await
        .expect("foreign implementation proposal should not block source-session finalize");

    assert_eq!(response.session_status, "accepted");
    assert_eq!(response.tasks_created, 0);
    assert_eq!(response.skipped_foreign_count, 1);
}

// Scenario 28: No gating when expected_proposal_count is omitted (backward compatibility).
#[tokio::test]
async fn test_no_gating_when_count_omitted() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;

    let (_, _, triggered) = create_proposal_impl(
        &state,
        session.id.clone(),
        make_proposal_options("Legacy Proposal", None),
    )
    .await
    .expect("proposal without expected count should succeed");

    assert!(
        !triggered,
        "No trigger when expected_proposal_count is omitted"
    );

    let updated = state
        .ideation_session_repo
        .get_by_id(&session.id)
        .await
        .unwrap()
        .expect("session must exist");

    assert!(
        updated.expected_proposal_count.is_none(),
        "expected_proposal_count must remain null when not provided"
    );
    assert!(
        updated.auto_accept_status.is_none(),
        "auto_accept_status must remain null when no gating"
    );
}

// Scenario 29: Archived proposals are excluded from the active count.
// Archiving a proposal reduces the active count, delaying the auto-accept trigger.
#[tokio::test]
async fn test_archived_proposal_not_counted() {
    let state = AppState::new_sqlite_test();
    let (session, _) = setup_session_with_gate(&state, "verified", false).await;

    // Create 2 proposals (active=2, expected=3 → no trigger)
    let mut ids = vec![];
    for i in 1..=2 {
        let (p, _, triggered) = create_proposal_impl(
            &state,
            session.id.clone(),
            make_proposal_options(&format!("Proposal {i}"), Some(3)),
        )
        .await
        .expect("proposal should succeed");
        assert!(!triggered, "Must not trigger after {i} of 3 proposals");
        ids.push(p.id);
    }

    // Archive proposal 1 → active count drops to 1
    archive_proposal_impl(&state, ids[0].clone())
        .await
        .expect("archive should succeed");

    // Proposal 3: active count is 2 (1 archived) → NOT equal to expected=3, no trigger
    let (_, _, triggered) = create_proposal_impl(
        &state,
        session.id.clone(),
        make_proposal_options("Proposal 3", Some(3)),
    )
    .await
    .expect("proposal 3 should succeed");

    assert!(
        !triggered,
        "Must NOT trigger: active count is 2 (one archived), not 3"
    );

    // Proposal 4: active count is now 3 (=expected) → triggers
    let (_, _, triggered) = create_proposal_impl(
        &state,
        session.id.clone(),
        make_proposal_options("Proposal 4", Some(3)),
    )
    .await
    .expect("proposal 4 should succeed");

    assert!(
        triggered,
        "Must trigger: 3 active proposals == expected count of 3"
    );
}

// ============================================================================
// compute_validation_hint unit tests
// ============================================================================

use chrono::{Duration, Utc};
use ralphx_lib::domain::entities::ValidationCacheMetadata;

fn make_validation_cache(
    commit_sha: &str,
    tests_ran: bool,
    tests_passed: bool,
) -> ValidationCacheMetadata {
    ValidationCacheMetadata {
        version: 1,
        commit_sha: commit_sha.to_string(),
        tests_ran,
        tests_passed,
        test_summary: None,
        captured_at: Utc::now(),
        captured_by: "execution_complete".to_string(),
    }
}

fn git(repo: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git should run");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn setup_validation_context_git_repo() -> (tempfile::TempDir, String) {
    let repo = tempfile::tempdir_in(std::env::current_dir().expect("cwd"))
        .expect("temp git repo should be created");
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("README should be written");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let head_sha = git(repo.path(), &["rev-parse", "HEAD"]);
    (repo, head_sha)
}

async fn seeded_validation_context_state(
    purpose: ValidationPurpose,
) -> (AppState, tempfile::TempDir, TaskId) {
    seeded_validation_context_state_with_command_status(purpose, ValidationCommandStatus::Passed)
        .await
}

async fn seeded_validation_context_state_with_command_status(
    purpose: ValidationPurpose,
    command_status: ValidationCommandStatus,
) -> (AppState, tempfile::TempDir, TaskId) {
    let state = AppState::new_test();
    let (repo, head_sha) = setup_validation_context_git_repo();
    let project = Project::new(
        "Validation Context".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    let project = state
        .project_repo
        .create(project)
        .await
        .expect("project should be created");
    let mut task = Task::new(project.id.clone(), "Validation context task".to_string());
    task.internal_status = InternalStatus::Executing;
    task.task_branch = Some("main".to_string());
    task.worktree_path = Some(repo.path().to_string_lossy().to_string());
    let task_id = task.id.clone();
    state
        .task_repo
        .create(task)
        .await
        .expect("task should be created");
    state
        .task_repo
        .persist_status_change(
            &task_id,
            InternalStatus::Ready,
            InternalStatus::Executing,
            "agent-started",
        )
        .await
        .expect("execution episode should be recorded");
    let episode_entered_at = state
        .task_repo
        .get_status_last_entered_at(&task_id, InternalStatus::Executing)
        .await
        .expect("status history lookup should succeed")
        .expect("execution episode should exist");
    let run = ValidationRun {
        id: format!("run-{}", purpose.as_str()),
        task_id: task_id.clone(),
        project_id: project.id,
        purpose,
        context_type: ValidationContextType::Execution,
        requested_by_agent: Some("ralphx-execution-worker".to_string()),
        status: ValidationRunStatus::Passed,
        mode: ValidationRunMode::Force,
        policy_enabled: true,
        head_sha: Some(head_sha.clone()),
        start_content_fingerprint: None,
        validated_content_fingerprint: None,
        promoted_commit_sha: None,
        base_ref: Some("main".to_string()),
        analysis_fingerprint: None,
        status_episode_entered_at: Some(episode_entered_at),
        started_at: episode_entered_at,
        completed_at: Some(episode_entered_at + Duration::seconds(1)),
    };
    state
        .validation_run_repo
        .create_run(&run)
        .await
        .expect("validation run should persist");
    state
        .validation_run_repo
        .add_command_result(&ValidationCommandResult {
            id: format!("command-{}", purpose.as_str()),
            validation_run_id: run.id,
            task_id: task_id.clone(),
            project_id: run.project_id,
            command_source: ValidationCommandSource::AgentSelected,
            command_ref: None,
            command: "cargo test validation".to_string(),
            cwd: repo.path().to_string_lossy().to_string(),
            label: Some("Validation tests".to_string()),
            category: ValidationCommandCategory::Test,
            reason: None,
            related_files: Vec::new(),
            cache_key: format!("cache-{}", purpose.as_str()),
            cache_decision: if command_status == ValidationCommandStatus::Cached {
                ValidationCacheDecision::Cached
            } else {
                ValidationCacheDecision::Forced
            },
            status: command_status,
            exit_code: Some(0),
            duration_ms: Some(1),
            stdout_snippet: None,
            stderr_snippet: None,
            stdout_log_path: None,
            stderr_log_path: None,
            launcher_kind: Some("production_shell_resolver".to_string()),
            resolved_shell_path: Some("/bin/sh".to_string()),
            head_sha: Some(head_sha),
            analysis_fingerprint: None,
            status_episode_entered_at: Some(episode_entered_at),
            created_at: episode_entered_at + Duration::seconds(1),
        })
        .await
        .expect("validation command should persist");

    (state, repo, task_id)
}

#[tokio::test]
async fn task_context_ignores_baseline_first_class_validation_for_skip_tests_hint() {
    let (state, _repo, task_id) =
        seeded_validation_context_state(ValidationPurpose::Baseline).await;

    let context = get_task_context_impl(&state, &task_id)
        .await
        .expect("task context should load");

    assert!(
        context.validation_cache.is_none(),
        "baseline validation must remain informational instead of reusable cache proof"
    );
    assert!(
        !context
            .context_hints
            .iter()
            .any(|hint| hint.contains("VALIDATION SUMMARY: skip_tests")),
        "baseline validation must not produce skip_tests context hints"
    );
}

#[tokio::test]
async fn task_context_keeps_current_final_validation_skip_tests_hint() {
    let (state, _repo, task_id) = seeded_validation_context_state(ValidationPurpose::Final).await;

    let context = get_task_context_impl(&state, &task_id)
        .await
        .expect("task context should load");
    let cache = context
        .validation_cache
        .expect("current final validation should produce reusable cache data");

    assert_eq!(cache.validation_hint, "skip_tests");
    assert!(
        context
            .context_hints
            .iter()
            .any(|hint| hint.contains("VALIDATION SUMMARY: skip_tests")),
        "current final validation should still produce skip_tests context hints"
    );
}

#[tokio::test]
async fn task_context_reuses_current_final_validation_with_cached_test_command() {
    let (state, _repo, task_id) = seeded_validation_context_state_with_command_status(
        ValidationPurpose::Final,
        ValidationCommandStatus::Cached,
    )
    .await;

    let context = get_task_context_impl(&state, &task_id)
        .await
        .expect("task context should load");
    let cache = context
        .validation_cache
        .expect("cached current final validation should produce reusable cache data");

    assert_eq!(cache.validation_hint, "skip_tests");
    assert!(cache.tests_passed);
    assert!(
        cache
            .test_summary
            .as_deref()
            .is_some_and(|summary| summary.contains("passed or reused")),
        "cached command summary should mention reused validation evidence"
    );
}

#[tokio::test]
async fn task_context_skips_newer_baseline_and_reuses_current_final_validation_hint() {
    let (state, repo, task_id) = seeded_validation_context_state(ValidationPurpose::Final).await;
    let task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .expect("task lookup should succeed")
        .expect("task should exist");
    let episode_entered_at = state
        .task_repo
        .get_status_last_entered_at(&task_id, InternalStatus::Executing)
        .await
        .expect("status history lookup should succeed")
        .expect("execution episode should exist");
    let head_sha = git(repo.path(), &["rev-parse", "HEAD"]);
    let baseline_started_at = episode_entered_at + Duration::seconds(2);
    let baseline_run = ValidationRun {
        id: "run-newer-baseline".to_string(),
        task_id: task_id.clone(),
        project_id: task.project_id.clone(),
        purpose: ValidationPurpose::Baseline,
        context_type: ValidationContextType::Execution,
        requested_by_agent: Some("ralphx-execution-worker".to_string()),
        status: ValidationRunStatus::Passed,
        mode: ValidationRunMode::Force,
        policy_enabled: true,
        head_sha: Some(head_sha.clone()),
        start_content_fingerprint: None,
        validated_content_fingerprint: None,
        promoted_commit_sha: None,
        base_ref: Some("main".to_string()),
        analysis_fingerprint: None,
        status_episode_entered_at: Some(episode_entered_at),
        started_at: baseline_started_at,
        completed_at: Some(baseline_started_at + Duration::seconds(1)),
    };
    state
        .validation_run_repo
        .create_run(&baseline_run)
        .await
        .expect("newer baseline run should persist");
    state
        .validation_run_repo
        .add_command_result(&ValidationCommandResult {
            id: "command-newer-baseline".to_string(),
            validation_run_id: baseline_run.id,
            task_id: task_id.clone(),
            project_id: task.project_id,
            command_source: ValidationCommandSource::AgentSelected,
            command_ref: None,
            command: "cargo test validation".to_string(),
            cwd: repo.path().to_string_lossy().to_string(),
            label: Some("Baseline tests".to_string()),
            category: ValidationCommandCategory::Test,
            reason: None,
            related_files: Vec::new(),
            cache_key: "cache-newer-baseline".to_string(),
            cache_decision: ValidationCacheDecision::Forced,
            status: ValidationCommandStatus::Passed,
            exit_code: Some(0),
            duration_ms: Some(1),
            stdout_snippet: None,
            stderr_snippet: None,
            stdout_log_path: None,
            stderr_log_path: None,
            launcher_kind: Some("production_shell_resolver".to_string()),
            resolved_shell_path: Some("/bin/sh".to_string()),
            head_sha: Some(head_sha),
            analysis_fingerprint: None,
            status_episode_entered_at: Some(episode_entered_at),
            created_at: baseline_started_at + Duration::seconds(1),
        })
        .await
        .expect("baseline command should persist");

    let context = get_task_context_impl(&state, &task_id)
        .await
        .expect("task context should load");
    let cache = context
        .validation_cache
        .expect("current final validation should be reused after skipping newer baseline");

    assert_eq!(cache.validation_hint, "skip_tests");
    assert!(
        context
            .context_hints
            .iter()
            .any(|hint| hint.contains("VALIDATION SUMMARY: skip_tests")),
        "newer baseline should not mask current final validation evidence"
    );
}

#[test]
fn compute_validation_hint_sha_match_tests_passed_returns_skip_tests() {
    let cache = make_validation_cache("abc12345def67890", true, true);
    let (hint, msg) = compute_validation_hint(&cache, "abc12345def67890", Some(cache.captured_at));
    assert_eq!(hint, "skip_tests");
    assert!(
        msg.contains("Tests passed"),
        "hint_message should mention 'Tests passed', got: {}",
        msg
    );
    assert!(
        msg.contains("abc12345"),
        "hint_message should contain truncated SHA, got: {}",
        msg
    );
}

#[test]
fn compute_validation_hint_sha_match_tests_ran_false_returns_skip_test_validation() {
    let cache = make_validation_cache("abc12345def67890", false, false);
    let (hint, msg) = compute_validation_hint(&cache, "abc12345def67890", Some(cache.captured_at));
    assert_eq!(hint, "skip_test_validation");
    assert!(
        msg.contains("No tests were run"),
        "hint_message should mention 'No tests were run', got: {}",
        msg
    );
    assert!(
        msg.contains("abc12345"),
        "hint_message should contain truncated SHA, got: {}",
        msg
    );
}

#[test]
fn compute_validation_hint_sha_mismatch_returns_run_tests() {
    let cache = make_validation_cache("abc12345def67890", true, true);
    let (hint, msg) = compute_validation_hint(&cache, "differentsha12345", Some(cache.captured_at));
    assert_eq!(hint, "run_tests");
    assert!(
        msg.contains("SHA changed") || msg.contains("Cache stale"),
        "hint_message should mention stale cache, got: {}",
        msg
    );
}

#[test]
fn compute_validation_hint_tests_failed_same_sha_returns_run_tests() {
    let cache = make_validation_cache("abc12345def67890", true, false);
    let (hint, msg) = compute_validation_hint(&cache, "abc12345def67890", Some(cache.captured_at));
    assert_eq!(hint, "run_tests");
    assert!(
        msg.contains("Tests failed"),
        "hint_message should mention 'Tests failed', got: {}",
        msg
    );
    assert!(
        msg.contains("abc12345"),
        "hint_message should contain truncated SHA, got: {}",
        msg
    );
}

#[test]
fn compute_validation_hint_prior_episode_cache_returns_run_tests() {
    let cache = make_validation_cache("abc12345def67890", true, true);
    let next_episode = cache.captured_at + Duration::seconds(1);

    let (hint, msg) = compute_validation_hint(&cache, "abc12345def67890", Some(next_episode));

    assert_eq!(hint, "run_tests");
    assert!(
        msg.contains("current execution episode"),
        "hint_message should mention stale episode, got: {}",
        msg
    );
}

#[test]
fn compute_validation_hint_sha_mismatch_overrides_failed_tests() {
    // Even if tests passed, SHA mismatch always → run_tests
    let cache = make_validation_cache("aaaa1111bbbb2222", true, true);
    let (hint, _) = compute_validation_hint(&cache, "cccc3333dddd4444", Some(cache.captured_at));
    assert_eq!(hint, "run_tests");
}

#[test]
fn compute_validation_hint_sha_mismatch_with_short_sha() {
    // Edge case: SHA shorter than 8 chars should not panic
    let cache = make_validation_cache("abc", true, true);
    let (hint, msg) = compute_validation_hint(&cache, "def", Some(cache.captured_at));
    assert_eq!(hint, "run_tests");
    assert!(
        msg.contains("Cache stale") || msg.contains("SHA changed"),
        "got: {}",
        msg
    );
}

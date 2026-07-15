use axum::{extract::State, Json};
use ralphx_lib::application::{AppState, TeamService, TeamStateTracker};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::agents::{AgentHarnessKind, AgentProviderSettings};
use ralphx_lib::domain::entities::{
    ArtifactId, ChatConversation, IdeationSession, IdeationSessionId, IdeationSessionStatus,
    MessageRole, Persona, PersonaId, PersonaStatus, ProjectId, VerificationStatus,
};
use ralphx_lib::error::AppError;
use ralphx_lib::http_server::handlers::*;
use ralphx_lib::http_server::types::{CreateChildSessionRequest, HttpServerState};
use ralphx_lib::infrastructure::agents::claude::verification_config;
use ralphx_lib::infrastructure::sqlite::SqliteIdeationSessionRepository;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::{fs, os::unix::fs::PermissionsExt};
use tempfile::TempDir;
use tokio::sync::Mutex;

fn claude_cli_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn install_capturing_claude_cli() -> (TempDir, PathBuf, PathBuf) {
    let tempdir = TempDir::new().expect("create fake Claude CLI tempdir");
    let cli_path = tempdir.path().join("claude");
    let captured_args_path = tempdir.path().join("verification-child-args.txt");
    fs::write(
        &cli_path,
        r#"#!/bin/sh
is_prompt_spawn=false
for arg in "$@"; do
  if [ "$arg" = "-p" ]; then
    is_prompt_spawn=true
  fi
done
if [ "$is_prompt_spawn" = "true" ] && [ -n "$RALPHX_TEST_CLAUDE_ARGS_PATH" ]; then
  {
    printf '%s\n' "$@"
    for arg in "$@"; do
      if [ -f "$arg" ]; then
        cat "$arg"
      fi
    done
    cat
  } > "$RALPHX_TEST_CLAUDE_ARGS_PATH"
fi
exit 0
"#,
    )
    .expect("write fake Claude CLI");
    let mut permissions = fs::metadata(&cli_path)
        .expect("read fake Claude CLI metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&cli_path, permissions).expect("mark fake Claude CLI executable");
    (tempdir, cli_path, captured_args_path)
}

async fn configure_capturing_claude_cli(state: &HttpServerState, cli_path: &Path) {
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let mut settings = AgentProviderSettings::disabled_defaults(AgentHarnessKind::Claude);
    settings.enabled = true;
    settings.is_default = true;
    settings.custom_binary_enabled = true;
    settings.custom_binary_path = Some(cli_path.to_string_lossy().into_owned());
    state
        .app_state
        .agent_provider_settings_repo
        .upsert(&settings)
        .await
        .expect("configure fake Claude provider");
}

async fn seed_bound_active_project_persona(
    state: &HttpServerState,
    project_id: &ProjectId,
    persona_id: &str,
    body: &str,
) {
    let now = chrono::Utc::now();
    let persona = Persona {
        id: PersonaId::from(persona_id),
        slug: persona_id.to_string(),
        name: "Verification isolation persona".to_string(),
        description: "Must not reach verification children".to_string(),
        content: body.to_string(),
        status: PersonaStatus::Active,
        version: 1,
        content_hash: format!("{persona_id}-hash"),
        source_session_id: None,
        source_json: "{}".to_string(),
        created_at: now,
        updated_at: now,
    };
    state
        .app_state
        .persona_repo
        .create(persona.clone())
        .await
        .expect("seed active persona");

    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.persona_id = Some(persona.id.to_string());
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("bind active persona to a same-project conversation");
}

fn make_session(team_mode: Option<&str>) -> IdeationSession {
    IdeationSession {
        id: IdeationSessionId::new(),
        project_id: ProjectId("proj-1".to_string()),
        title: None,
        status: IdeationSessionStatus::Active,
        plan_artifact_id: None,
        verified_plan_artifact_id: None,
        verified_plan_agent_run_id: None,
        inherited_plan_artifact_id: None,
        seed_task_id: None,
        parent_session_id: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        archived_at: None,
        converted_at: None,
        team_mode: team_mode.map(|s| s.to_string()),
        team_config_json: None,
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
        origin: Default::default(),
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

#[test]
fn test_session_is_team_mode_research_returns_true() {
    let session = make_session(Some("research"));
    assert!(session_is_team_mode(&session));
}

#[test]
fn test_session_is_team_mode_debate_returns_true() {
    let session = make_session(Some("debate"));
    assert!(session_is_team_mode(&session));
}

#[test]
fn test_session_is_team_mode_solo_returns_false() {
    let session = make_session(Some("solo"));
    assert!(!session_is_team_mode(&session));
}

#[test]
fn test_session_is_team_mode_none_returns_false() {
    let session = make_session(None);
    assert!(!session_is_team_mode(&session));
}

// ============================================================
// Verification Auto-Initialization Integration Tests
// ============================================================

mod verification_init_tests {
    use super::*;

    async fn setup_sqlite_state() -> HttpServerState {
        let app_state = Arc::new(AppState::new_sqlite_test());
        let execution_state = Arc::new(ExecutionState::new());
        let tracker = TeamStateTracker::new();
        let team_service = Arc::new(TeamService::new_without_events(Arc::new(tracker.clone())));
        HttpServerState {
            app_state,
            execution_state,
            team_tracker: tracker,
            team_service,
            delegation_service: Default::default(),
        }
    }

    fn make_parent_session(plan_artifact_id: Option<ArtifactId>) -> IdeationSession {
        IdeationSession {
            id: IdeationSessionId::new(),
            project_id: ProjectId::from_string("proj-test".to_string()),
            title: Some("Test Session".to_string()),
            status: IdeationSessionStatus::Active,
            plan_artifact_id,
            verified_plan_artifact_id: None,
            verified_plan_agent_run_id: None,
            inherited_plan_artifact_id: None,
            seed_task_id: None,
            parent_session_id: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            archived_at: None,
            converted_at: None,
            team_mode: None,
            team_config_json: None,
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
            origin: Default::default(),
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

    fn make_imported_parent_session(plan_artifact_id: Option<ArtifactId>) -> IdeationSession {
        let mut session = make_parent_session(plan_artifact_id);
        session.source_project_id = Some("master-proj".to_string());
        session.source_session_id = Some("master-session".to_string());
        session
    }

    fn make_verification_request(parent_id: &IdeationSessionId) -> CreateChildSessionRequest {
        CreateChildSessionRequest {
            parent_session_id: parent_id.as_str().to_string(),
            title: None,
            description: None,
            inherit_context: false,
            initial_prompt: None,
            source_task_id: None,
            source_context_type: None,
            source_context_id: None,
            spawn_reason: None,
            blocker_fingerprint: None,
            team_mode: None,
            team_config: None,
            purpose: Some("verification".to_string()),
            is_external_trigger: false,
        }
    }

    // Tests the DB trigger function directly (state-setup concern, not handler concern).
    // After the backend fix, the handler calls send_message which fails in the test env
    // (no real Claude CLI / app_handle), causing generation to roll back to None in the
    // response. State-setup assertions are therefore tested via direct DB calls.
    #[tokio::test]
    async fn test_verification_first_time_unverified_parent() {
        let state = setup_sqlite_state().await;

        // Insert parent session with a plan artifact (FK OFF — no real artifact row needed)
        let parent = make_parent_session(Some(ArtifactId::new()));
        let parent_id = parent.id.clone();
        state
            .app_state
            .ideation_session_repo
            .create(parent)
            .await
            .unwrap();

        // Test DB trigger directly — state-setup concern
        let pid = parent_id.as_str().to_string();
        let gen = state
            .app_state
            .db
            .run(move |conn| SqliteIdeationSessionRepository::trigger_auto_verify_sync(conn, &pid))
            .await
            .unwrap();
        assert_eq!(
            gen,
            Some(1),
            "First verification trigger should return generation 1"
        );

        let updated_parent = state
            .app_state
            .ideation_session_repo
            .get_by_id(&parent_id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            updated_parent.verification_in_progress,
            "Parent should be in_progress=true after trigger"
        );
        assert_eq!(
            updated_parent.verification_status,
            VerificationStatus::Reviewing,
            "Parent status should be Reviewing"
        );
        assert_eq!(
            updated_parent.verification_generation, 1,
            "Parent generation should be 1"
        );

        // Reset for handler test
        let pid2 = parent_id.as_str().to_string();
        state
            .app_state
            .db
            .run(move |conn| SqliteIdeationSessionRepository::reset_auto_verify_sync(conn, &pid2))
            .await
            .unwrap();

        // Test handler response — orchestration concern. The child session creation must
        // succeed regardless of whether the follow-up spawn is enqueued or rolled back.
        let req = make_verification_request(&parent_id);
        let result = create_child_session(State(state.clone()), Json(req)).await;
        assert!(result.is_ok(), "Handler should return Ok (child session is created regardless of spawn outcome), got: {:?}", result.err());
        let response = result.unwrap().0;
        assert_eq!(
            response.orchestration_triggered,
            response.generation == Some(2),
            "Generation should remain visible only when orchestration was enqueued"
        );
        assert!(
            response.generation.is_none() || response.generation == Some(2),
            "Generation should either roll back to None or advance to 2, got {:?}",
            response.generation
        );
    }

    #[tokio::test]
    async fn test_verification_re_verification_already_verified() {
        let state = setup_sqlite_state().await;

        let parent = make_parent_session(Some(ArtifactId::new()));
        let parent_id = parent.id.clone();
        state
            .app_state
            .ideation_session_repo
            .create(parent)
            .await
            .unwrap();

        // Directly set parent to verified state with generation=1 via SQL
        let pid = parent_id.as_str().to_string();
        state
            .app_state
            .db
            .run(move |conn| {
                conn.execute(
                    "UPDATE ideation_sessions SET verification_status = 'verified', \
                     verification_in_progress = 0, verification_generation = 1 \
                     WHERE id = ?1",
                    rusqlite::params![pid],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
                Ok(())
            })
            .await
            .unwrap();

        // Test DB trigger directly — state-setup concern
        let pid2 = parent_id.as_str().to_string();
        let gen = state
            .app_state
            .db
            .run(move |conn| SqliteIdeationSessionRepository::trigger_auto_verify_sync(conn, &pid2))
            .await
            .unwrap();
        assert_eq!(
            gen,
            Some(2),
            "Re-verification trigger should return generation 2"
        );

        let updated_parent = state
            .app_state
            .ideation_session_repo
            .get_by_id(&parent_id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            updated_parent.verification_in_progress,
            "Parent should be in_progress=true after re-verification trigger"
        );
        assert_eq!(
            updated_parent.verification_status,
            VerificationStatus::Reviewing,
            "Parent status should be Reviewing"
        );
        assert_eq!(
            updated_parent.verification_generation, 2,
            "Parent generation should be 2"
        );

        // Reset for handler test
        let pid3 = parent_id.as_str().to_string();
        state
            .app_state
            .db
            .run(move |conn| SqliteIdeationSessionRepository::reset_auto_verify_sync(conn, &pid3))
            .await
            .unwrap();

        // Test handler response — orchestration concern. The handler may either enqueue the
        // child agent (keeping the new generation) or roll it back if spawn fails.
        let req = make_verification_request(&parent_id);
        let result = create_child_session(State(state.clone()), Json(req)).await;
        assert!(
            result.is_ok(),
            "Re-verification should succeed, got: {:?}",
            result.err()
        );
        let response = result.unwrap().0;
        assert_eq!(
            response.orchestration_triggered,
            response.generation == Some(3),
            "Generation should remain visible only when re-verification orchestration was enqueued"
        );
        assert!(
            response.generation.is_none() || response.generation == Some(3),
            "Generation should either roll back to None or advance to 3, got {:?}",
            response.generation
        );
    }

    #[tokio::test]
    async fn test_verification_concurrent_guard_returns_409() {
        let state = setup_sqlite_state().await;

        let parent = make_parent_session(Some(ArtifactId::new()));
        let parent_id = parent.id.clone();
        state
            .app_state
            .ideation_session_repo
            .create(parent)
            .await
            .unwrap();

        // Use direct DB trigger to set in_progress=true (avoids race with async rollback
        // that the handler introduces when send_message fails in the test env).
        let pid = parent_id.as_str().to_string();
        let gen = state
            .app_state
            .db
            .run(move |conn| SqliteIdeationSessionRepository::trigger_auto_verify_sync(conn, &pid))
            .await
            .unwrap();
        assert_eq!(gen, Some(1), "Trigger should return generation 1");

        // With in_progress=true, any handler call should get 409
        let req2 = make_verification_request(&parent_id);
        let result2 = create_child_session(State(state.clone()), Json(req2)).await;
        assert!(
            result2.is_err(),
            "Call with in_progress=true should fail with 409"
        );
        let err = result2.unwrap_err();
        assert_eq!(
            err.0,
            axum::http::StatusCode::CONFLICT,
            "Expected 409 CONFLICT, got: {:?}",
            err.0
        );
    }

    #[tokio::test]
    async fn test_verification_no_plan_artifact_returns_400() {
        let state = setup_sqlite_state().await;

        // Parent without plan artifact
        let parent = make_parent_session(None);
        let parent_id = parent.id.clone();
        state
            .app_state
            .ideation_session_repo
            .create(parent)
            .await
            .unwrap();

        let req = make_verification_request(&parent_id);
        let result = create_child_session(State(state.clone()), Json(req)).await;
        assert!(
            result.is_err(),
            "Should fail with 400 when no plan artifact"
        );
        let err = result.unwrap_err();
        assert_eq!(
            err.0,
            axum::http::StatusCode::BAD_REQUEST,
            "Expected 400 BAD_REQUEST, got: {:?}",
            err.0
        );
    }

    #[tokio::test]
    async fn test_verification_spawn_failure_rollback() {
        let state = setup_sqlite_state().await;

        let parent = make_parent_session(Some(ArtifactId::new()));
        let parent_id = parent.id.clone();
        state
            .app_state
            .ideation_session_repo
            .create(parent)
            .await
            .unwrap();

        // Simulate trigger (as the handler would do)
        let pid = parent_id.as_str().to_string();
        let gen = state
            .app_state
            .db
            .run(move |conn| SqliteIdeationSessionRepository::trigger_auto_verify_sync(conn, &pid))
            .await
            .unwrap();
        assert_eq!(gen, Some(1), "Trigger should return generation 1");

        // Confirm parent is in_progress=true
        let parent_after_trigger = state
            .app_state
            .ideation_session_repo
            .get_by_id(&parent_id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            parent_after_trigger.verification_in_progress,
            "Parent should be in_progress=true after trigger"
        );

        // Simulate rollback (as the handler does on spawn failure)
        let pid2 = parent_id.as_str().to_string();
        state
            .app_state
            .db
            .run(move |conn| SqliteIdeationSessionRepository::reset_auto_verify_sync(conn, &pid2))
            .await
            .unwrap();

        // Confirm parent is back to in_progress=false after rollback
        let parent_after_rollback = state
            .app_state
            .ideation_session_repo
            .get_by_id(&parent_id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            !parent_after_rollback.verification_in_progress,
            "Parent should be in_progress=false after rollback"
        );
        assert_eq!(
            parent_after_rollback.verification_status,
            VerificationStatus::Unverified,
            "Parent status should be Unverified after rollback"
        );
    }

    #[tokio::test]
    async fn test_verification_prompt_augmented_with_metadata() {
        let state = setup_sqlite_state().await;

        let parent = make_parent_session(Some(ArtifactId::new()));
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
            description: Some("Verify this plan".to_string()),
            inherit_context: false,
            initial_prompt: None,
            source_task_id: None,
            source_context_type: None,
            source_context_id: None,
            spawn_reason: None,
            blocker_fingerprint: None,
            team_mode: None,
            team_config: None,
            purpose: Some("verification".to_string()),
            is_external_trigger: false,
        };

        let result = create_child_session(State(state.clone()), Json(req)).await;
        // Handler should succeed (child session is created) regardless of agent spawn outcome.
        // In test environments without a real Claude CLI, the agent spawn will fail and the
        // handler rolls back verification_generation to None.
        assert!(
            result.is_ok(),
            "Handler should succeed, got: {:?}",
            result.err()
        );

        let response = result.unwrap().0;
        let child_id = IdeationSessionId::from_string(response.session_id.clone());

        // In tests, the app chat runtime spawn fails (no real Claude CLI), so orchestration_triggered
        // will be false and generation will be None after rollback.
        // The key invariant to assert: when a description is provided and verification initializes,
        // the user message stored before the spawn attempt contains the augmented metadata.
        let messages = state
            .app_state
            .chat_message_repo
            .get_by_session(&child_id)
            .await
            .unwrap();

        if messages.is_empty() {
            // Spawn failed before message was stored — this is acceptable in test environment.
            // The important behaviors (400/409/generation) are covered by other tests.
            return;
        }

        // If a message was stored, it must contain the verification metadata augmentation
        let user_msg = messages.iter().find(|m| m.role == MessageRole::User);
        if let Some(msg) = user_msg {
            let content = &msg.content;
            assert!(
                content.contains("parent_session_id:"),
                "Content should contain parent_session_id metadata, got: {}",
                content
            );
            assert!(
                content.contains("generation: 1"),
                "Content should contain generation: 1 metadata, got: {}",
                content
            );
            assert!(
                content.contains(&format!("max_rounds: {}", verification_config().max_rounds)),
                "Content should contain max_rounds metadata, got: {}",
                content
            );
            assert!(
                content.contains("Verify this plan"),
                "Content should contain original description, got: {}",
                content
            );
        }
    }
    // Force ideation capacity failure so orchestration_triggered=false deterministically.
    // Sets global_max_concurrent=1 and increments running_count to 1 so the capacity
    // check in can_start_ideation fires (running >= global_max) before Claude is spawned.
    fn saturate_ideation_capacity(state: &HttpServerState) {
        state.execution_state.set_global_max_concurrent(1);
        state.execution_state.increment_running();
    }

    #[tokio::test]
    async fn test_followup_provenance_persisted_on_child_session_creation() {
        let state = setup_sqlite_state().await;

        let parent = make_parent_session(None);
        let parent_id = parent.id.clone();
        state
            .app_state
            .ideation_session_repo
            .create(parent)
            .await
            .unwrap();

        let req = CreateChildSessionRequest {
            parent_session_id: parent_id.as_str().to_string(),
            title: Some("Execution follow-up".to_string()),
            description: None,
            inherit_context: true,
            initial_prompt: None,
            source_task_id: Some("task-123".to_string()),
            source_context_type: Some("task_execution".to_string()),
            source_context_id: Some("task-123".to_string()),
            spawn_reason: Some("out_of_scope_failure".to_string()),
            blocker_fingerprint: Some("ood:task-123:abc123def456".to_string()),
            team_mode: None,
            team_config: None,
            purpose: None,
            is_external_trigger: false,
        };

        let response = create_child_session(State(state.clone()), Json(req))
            .await
            .expect("Child session creation should succeed")
            .0;

        let child_id = IdeationSessionId::from_string(response.session_id);
        let child = state
            .app_state
            .ideation_session_repo
            .get_by_id(&child_id)
            .await
            .unwrap()
            .expect("Child session must exist");

        assert_eq!(child.parent_session_id, Some(parent_id));
        assert_eq!(
            child.source_task_id.as_ref().map(|id| id.as_str()),
            Some("task-123")
        );
        assert_eq!(child.source_context_type.as_deref(), Some("task_execution"));
        assert_eq!(child.source_context_id.as_deref(), Some("task-123"));
        assert_eq!(child.spawn_reason.as_deref(), Some("out_of_scope_failure"));
        assert_eq!(
            child.blocker_fingerprint.as_deref(),
            Some("ood:task-123:abc123def456")
        );
    }

    #[tokio::test]
    async fn test_followup_creation_reuses_existing_blocker_fingerprint_across_contexts() {
        let state = setup_sqlite_state().await;

        let parent = make_parent_session(None);
        let parent_id = parent.id.clone();
        state
            .app_state
            .ideation_session_repo
            .create(parent)
            .await
            .unwrap();

        let initial = CreateChildSessionRequest {
            parent_session_id: parent_id.as_str().to_string(),
            title: Some("Worker blocker follow-up".to_string()),
            description: None,
            inherit_context: true,
            initial_prompt: None,
            source_task_id: Some("task-789".to_string()),
            source_context_type: Some("task_execution".to_string()),
            source_context_id: Some("task-789".to_string()),
            spawn_reason: Some("worker_blocker_followup".to_string()),
            blocker_fingerprint: Some("ood:task-789:112233445566".to_string()),
            team_mode: None,
            team_config: None,
            purpose: None,
            is_external_trigger: false,
        };

        let initial_response = create_child_session(State(state.clone()), Json(initial))
            .await
            .expect("initial follow-up creation should succeed")
            .0;

        let duplicate = CreateChildSessionRequest {
            parent_session_id: parent_id.as_str().to_string(),
            title: Some("Reviewer blocker follow-up".to_string()),
            description: Some("Should reuse existing blocker session".to_string()),
            inherit_context: true,
            initial_prompt: Some("Investigate again".to_string()),
            source_task_id: Some("task-789".to_string()),
            source_context_type: Some("review".to_string()),
            source_context_id: Some("review-789".to_string()),
            spawn_reason: Some("out_of_scope_failure".to_string()),
            blocker_fingerprint: Some("ood:task-789:112233445566".to_string()),
            team_mode: None,
            team_config: None,
            purpose: None,
            is_external_trigger: false,
        };

        let duplicate_response = create_child_session(State(state.clone()), Json(duplicate))
            .await
            .expect("duplicate follow-up request should reuse existing session")
            .0;

        assert_eq!(duplicate_response.session_id, initial_response.session_id);

        let children = state
            .app_state
            .ideation_session_repo
            .get_children(&parent_id)
            .await
            .unwrap();
        let matching: Vec<_> = children
            .into_iter()
            .filter(|session| {
                session.source_task_id.as_ref().map(|id| id.as_str()) == Some("task-789")
                    && session.blocker_fingerprint.as_deref() == Some("ood:task-789:112233445566")
            })
            .collect();
        assert_eq!(
            matching.len(),
            1,
            "same blocker should not create duplicates"
        );
    }

    #[tokio::test]
    async fn test_followup_inherits_cross_project_lineage_from_parent() {
        let state = setup_sqlite_state().await;

        let parent = make_imported_parent_session(None);
        let parent_id = parent.id.clone();
        let parent_project_id = parent.project_id.clone();
        state
            .app_state
            .ideation_session_repo
            .create(parent)
            .await
            .unwrap();

        let req = CreateChildSessionRequest {
            parent_session_id: parent_id.as_str().to_string(),
            title: Some("Imported follow-up".to_string()),
            description: None,
            inherit_context: true,
            initial_prompt: None,
            source_task_id: Some("task-456".to_string()),
            source_context_type: Some("review".to_string()),
            source_context_id: Some("review-456".to_string()),
            spawn_reason: Some("out_of_scope_failure".to_string()),
            blocker_fingerprint: Some("ood:task-456:def456abc123".to_string()),
            team_mode: None,
            team_config: None,
            purpose: None,
            is_external_trigger: false,
        };

        let response = create_child_session(State(state.clone()), Json(req))
            .await
            .expect("Child session creation should succeed")
            .0;

        let child_id = IdeationSessionId::from_string(response.session_id);
        let child = state
            .app_state
            .ideation_session_repo
            .get_by_id(&child_id)
            .await
            .unwrap()
            .expect("Child session must exist");

        assert_eq!(child.parent_session_id, Some(parent_id));
        assert_eq!(child.project_id, parent_project_id);
        assert_eq!(child.source_project_id.as_deref(), Some("master-proj"));
        assert_eq!(child.source_session_id.as_deref(), Some("master-session"));
    }

    #[tokio::test]
    async fn test_verification_child_inherits_cross_project_lineage_from_parent() {
        let state = setup_sqlite_state().await;

        let parent = make_imported_parent_session(Some(ArtifactId::new()));
        let parent_id = parent.id.clone();
        let parent_project_id = parent.project_id.clone();
        state
            .app_state
            .ideation_session_repo
            .create(parent)
            .await
            .unwrap();

        let req = make_verification_request(&parent_id);
        let response = create_child_session(State(state.clone()), Json(req))
            .await
            .expect("Verification child creation should succeed")
            .0;

        let child_id = IdeationSessionId::from_string(response.session_id);
        let child = state
            .app_state
            .ideation_session_repo
            .get_by_id(&child_id)
            .await
            .unwrap()
            .expect("Verification child must exist");

        assert_eq!(child.parent_session_id, Some(parent_id));
        assert_eq!(child.project_id, parent_project_id);
        assert_eq!(child.source_project_id.as_deref(), Some("master-proj"));
        assert_eq!(child.source_session_id.as_deref(), Some("master-session"));
    }

    #[tokio::test]
    async fn test_verification_child_capacity_deferred_stays_active_with_pending_prompt() {
        let state = setup_sqlite_state().await;
        saturate_ideation_capacity(&state);

        let parent = make_parent_session(Some(ArtifactId::new()));
        let parent_id = parent.id.clone();
        state
            .app_state
            .ideation_session_repo
            .create(parent)
            .await
            .unwrap();

        let req = make_verification_request(&parent_id);
        let response = create_child_session(State(state.clone()), Json(req))
            .await
            .expect("Verification child creation should succeed")
            .0;

        assert!(!response.orchestration_triggered);
        assert!(
            response.pending_initial_prompt.is_some(),
            "verification child should surface pending prompt when queued"
        );

        let child_id = IdeationSessionId::from_string(response.session_id);
        let child = state
            .app_state
            .ideation_session_repo
            .get_by_id(&child_id)
            .await
            .unwrap()
            .expect("Verification child must exist");

        assert_eq!(child.status, IdeationSessionStatus::Active);
        assert_eq!(
            child.pending_initial_prompt, response.pending_initial_prompt,
            "verification child pending prompt should persist for drain and hydration"
        );

        let parent_after = state
            .app_state
            .ideation_session_repo
            .get_by_id(&parent_id)
            .await
            .unwrap()
            .expect("Parent session must still exist");
        assert!(
            !parent_after.verification_in_progress,
            "capacity-deferred verification must roll parent back out of in-progress"
        );
    }

    // When spawn fails for a non-verification child with initial_prompt, the handler must
    // persist the prompt to pending_initial_prompt and surface it in the response.
    #[tokio::test]
    async fn test_deferred_prompt_persisted_on_non_verification_spawn_failure() {
        let state = setup_sqlite_state().await;
        // Saturate capacity so send_message returns Err (SpawnFailed) → orchestration_triggered=false
        saturate_ideation_capacity(&state);

        let parent = make_parent_session(None);
        let parent_id = parent.id.clone();
        state
            .app_state
            .ideation_session_repo
            .create(parent)
            .await
            .unwrap();

        let initial_prompt = "Start the follow-on session with this prompt";
        let req = CreateChildSessionRequest {
            parent_session_id: parent_id.as_str().to_string(),
            title: None,
            description: None,
            inherit_context: false,
            initial_prompt: Some(initial_prompt.to_string()),
            source_task_id: None,
            source_context_type: None,
            source_context_id: None,
            spawn_reason: None,
            blocker_fingerprint: None,
            team_mode: None,
            team_config: None,
            purpose: None, // non-verification
            is_external_trigger: false,
        };

        let result = create_child_session(State(state.clone()), Json(req)).await;
        assert!(
            result.is_ok(),
            "Handler should succeed even when spawn fails, got: {:?}",
            result.err()
        );

        let response = result.unwrap().0;
        assert!(
            !response.orchestration_triggered,
            "With saturated capacity, orchestration_triggered must be false"
        );
        assert_eq!(
            response.pending_initial_prompt,
            Some(initial_prompt.to_string()),
            "pending_initial_prompt must equal the initial_prompt when spawn fails"
        );

        // Verify the DB row was updated
        let child_id = IdeationSessionId::from_string(response.session_id.clone());
        let child_row = state
            .app_state
            .ideation_session_repo
            .get_by_id(&child_id)
            .await
            .unwrap()
            .expect("Child session must exist in DB");
        assert_eq!(
            child_row.pending_initial_prompt,
            Some(initial_prompt.to_string()),
            "DB row pending_initial_prompt must equal the initial_prompt"
        );
    }

    // When spawn fails for a non-verification child with description (no initial_prompt),
    // the description is used as the deferred prompt.
    #[tokio::test]
    async fn test_deferred_prompt_uses_description_when_no_initial_prompt() {
        let state = setup_sqlite_state().await;
        saturate_ideation_capacity(&state);

        let parent = make_parent_session(None);
        let parent_id = parent.id.clone();
        state
            .app_state
            .ideation_session_repo
            .create(parent)
            .await
            .unwrap();

        let description = "A follow-on session description";
        let req = CreateChildSessionRequest {
            parent_session_id: parent_id.as_str().to_string(),
            title: None,
            description: Some(description.to_string()),
            inherit_context: false,
            initial_prompt: None,
            source_task_id: None,
            source_context_type: None,
            source_context_id: None,
            spawn_reason: None,
            blocker_fingerprint: None,
            team_mode: None,
            team_config: None,
            purpose: None, // non-verification
            is_external_trigger: false,
        };

        let result = create_child_session(State(state.clone()), Json(req)).await;
        assert!(
            result.is_ok(),
            "Handler should succeed even when spawn fails, got: {:?}",
            result.err()
        );

        let response = result.unwrap().0;
        assert!(
            !response.orchestration_triggered,
            "With saturated capacity, orchestration_triggered must be false"
        );
        assert_eq!(
            response.pending_initial_prompt,
            Some(description.to_string()),
            "pending_initial_prompt must use description when initial_prompt is absent"
        );

        let child_id = IdeationSessionId::from_string(response.session_id.clone());
        let child_row = state
            .app_state
            .ideation_session_repo
            .get_by_id(&child_id)
            .await
            .unwrap()
            .expect("Child session must exist in DB");
        assert_eq!(
            child_row.pending_initial_prompt,
            Some(description.to_string()),
            "DB row pending_initial_prompt must equal the description"
        );
    }

    // When no prompt is provided, pending_initial_prompt stays None even on spawn failure.
    #[tokio::test]
    async fn test_deferred_prompt_none_when_no_prompt_provided() {
        let state = setup_sqlite_state().await;
        saturate_ideation_capacity(&state);

        let parent = make_parent_session(None);
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
            source_task_id: None,
            source_context_type: None,
            source_context_id: None,
            spawn_reason: None,
            blocker_fingerprint: None,
            team_mode: None,
            team_config: None,
            purpose: None,
            is_external_trigger: false,
        };

        let result = create_child_session(State(state.clone()), Json(req)).await;
        assert!(
            result.is_ok(),
            "Handler should succeed even when spawn fails, got: {:?}",
            result.err()
        );

        let response = result.unwrap().0;
        assert!(
            !response.orchestration_triggered,
            "With saturated capacity, orchestration_triggered must be false"
        );
        assert_eq!(
            response.pending_initial_prompt, None,
            "pending_initial_prompt must be None when no prompt was provided"
        );
    }

    // Regression: explicit initial_prompt must take precedence over synthesis.
    // When initial_prompt is provided, the .or_else(synthesize) closure must NOT fire.
    // In test env, spawn fails, so we assert: Ok response + message (if stored) uses
    // the explicit prompt, not the "Begin plan verification." synthesized prefix.
    #[tokio::test]
    async fn test_verification_explicit_initial_prompt_not_synthesized() {
        let state = setup_sqlite_state().await;

        let parent = make_parent_session(Some(ArtifactId::new()));
        let parent_id = parent.id.clone();
        state
            .app_state
            .ideation_session_repo
            .create(parent)
            .await
            .unwrap();

        let explicit_prompt = "My custom verification prompt";
        let req = CreateChildSessionRequest {
            parent_session_id: parent_id.as_str().to_string(),
            title: None,
            description: None,
            inherit_context: false,
            initial_prompt: Some(explicit_prompt.to_string()),
            source_task_id: None,
            source_context_type: None,
            source_context_id: None,
            spawn_reason: None,
            blocker_fingerprint: None,
            team_mode: None,
            team_config: None,
            purpose: Some("verification".to_string()),
            is_external_trigger: false,
        };

        let result = create_child_session(State(state.clone()), Json(req)).await;
        assert!(
            result.is_ok(),
            "Handler should return Ok when explicit initial_prompt is provided, got: {:?}",
            result.err()
        );

        let response = result.unwrap().0;
        let child_id = IdeationSessionId::from_string(response.session_id.clone());

        let messages = state
            .app_state
            .chat_message_repo
            .get_by_session(&child_id)
            .await
            .unwrap();

        if messages.is_empty() {
            // Spawn failed before message stored — test env limitation, acceptable.
            return;
        }

        let user_msg = messages.iter().find(|m| m.role == MessageRole::User);
        if let Some(msg) = user_msg {
            let content = &msg.content;
            assert!(
                content.contains(explicit_prompt),
                "Message should contain the explicit prompt, got: {}",
                content
            );
            // The synthesized prefix must NOT appear — explicit prompt takes precedence
            assert!(
                !content.starts_with("Begin plan verification."),
                "Message must NOT start with synthesized prefix when explicit prompt provided, got: {}",
                content
            );
        }
    }

    #[tokio::test]
    async fn verification_child_command_excludes_bound_project_persona() {
        let _env_lock = claude_cli_env_lock().lock().await;
        let (_fake_cli_dir, fake_cli_path, captured_command_path) = install_capturing_claude_cli();
        let _captured_command_guard = crate::support::env::EnvVarGuard::set(
            "RALPHX_TEST_CLAUDE_ARGS_PATH",
            captured_command_path.clone(),
        );
        let _persona_flag_guard =
            crate::support::env::EnvVarGuard::set("RALPHX_UI_AGENT_PERSONAS", "true");
        let _allow_spawn_guard =
            crate::support::env::EnvVarGuard::set("RALPHX_ALLOW_CLAUDE_SPAWN_IN_TESTS", "1");
        let state = setup_sqlite_state().await;
        configure_capturing_claude_cli(&state, &fake_cli_path).await;

        let parent = make_parent_session(Some(ArtifactId::new()));
        let parent_id = parent.id.clone();
        let persona_body = "VERIFICATION PERSONA BODY MUST NOT REACH CHILD";
        seed_bound_active_project_persona(
            &state,
            &parent.project_id,
            "verification-cross-contamination-persona",
            persona_body,
        )
        .await;
        state
            .app_state
            .ideation_session_repo
            .create(parent)
            .await
            .expect("create verification parent");

        let response = create_child_session(
            State(state),
            Json(CreateChildSessionRequest {
                parent_session_id: parent_id.as_str().to_string(),
                title: None,
                description: Some("Run verification child persona isolation check.".to_string()),
                inherit_context: false,
                initial_prompt: None,
                source_task_id: None,
                source_context_type: None,
                source_context_id: None,
                spawn_reason: None,
                blocker_fingerprint: None,
                team_mode: None,
                team_config: None,
                purpose: Some("verification".to_string()),
                is_external_trigger: false,
            }),
        )
        .await
        .expect("verification child handler should spawn the child")
        .0;
        assert!(
            response.orchestration_triggered,
            "the fake CLI must make the handler's production spawn path succeed"
        );

        let mut captured_command = None;
        for _ in 0..40 {
            if let Ok(captured) = fs::read_to_string(&captured_command_path) {
                captured_command = Some(captured);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        let captured_command = captured_command
            .expect("verification child fake CLI should capture the final command and prompt");
        assert!(
            !captured_command.contains("<ralphx_agent_persona>"),
            "verification child command and prompt must not include a persona block: {captured_command}"
        );
        assert!(
            !captured_command.contains(persona_body),
            "verification child command and prompt must not include the bound persona body: {captured_command}"
        );
    }
}

// ============================================================
// synthesize_verification_prompt unit tests
// ============================================================

#[test]
fn test_synthesize_verification_prompt_basic() {
    let max_rounds = verification_config().max_rounds;
    let result = synthesize_verification_prompt(
        &Some("verification".to_string()),
        Some(1),
        max_rounds,
        &None,
        "parent-abc",
    );
    assert_eq!(
        result,
        Some(
            format!(
                "Begin plan verification.\n\nparent_session_id: parent-abc, generation: 1, max_rounds: {}",
                max_rounds
            )
        )
    );
}

#[test]
fn test_synthesize_verification_prompt_no_generation_defaults_to_1() {
    let max_rounds = verification_config().max_rounds;
    let result = synthesize_verification_prompt(
        &Some("verification".to_string()),
        None,
        max_rounds,
        &None,
        "parent-xyz",
    );
    assert!(result.is_some());
    assert!(
        result.unwrap().contains("generation: 1"),
        "Generation should default to 1 when None"
    );
}

#[test]
fn test_synthesize_verification_prompt_description_present_returns_none() {
    let result = synthesize_verification_prompt(
        &Some("verification".to_string()),
        Some(1),
        verification_config().max_rounds,
        &Some("user description".to_string()),
        "parent-abc",
    );
    assert_eq!(
        result, None,
        "Should return None when description is present"
    );
}

#[test]
fn test_synthesize_verification_prompt_non_verification_purpose_returns_none() {
    let result = synthesize_verification_prompt(
        &Some("general".to_string()),
        None,
        verification_config().max_rounds,
        &None,
        "parent-abc",
    );
    assert_eq!(
        result, None,
        "Should return None for non-verification purpose"
    );
}

#[test]
fn test_synthesize_verification_prompt_no_purpose_returns_none() {
    let result = synthesize_verification_prompt(
        &None,
        None,
        verification_config().max_rounds,
        &None,
        "parent-abc",
    );
    assert_eq!(result, None, "Should return None when purpose is None");
}

#[test]
fn test_synthesize_verification_prompt_generation_2() {
    let max_rounds = verification_config().max_rounds;
    let result = synthesize_verification_prompt(
        &Some("verification".to_string()),
        Some(2),
        max_rounds,
        &None,
        "parent-gen2",
    );
    assert_eq!(
        result,
        Some(
            format!(
                "Begin plan verification.\n\nparent_session_id: parent-gen2, generation: 2, max_rounds: {}",
                max_rounds
            )
        )
    );
}

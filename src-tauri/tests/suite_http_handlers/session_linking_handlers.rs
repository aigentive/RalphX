use axum::{extract::State, http::StatusCode, Json};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    ArtifactId, IdeationSession, IdeationSessionId, IdeationSessionStatus, ProjectId,
};
use ralphx_lib::http_server::handlers::*;
use ralphx_lib::http_server::types::{CreateChildSessionRequest, HttpServerState};
use std::sync::Arc;

// Verification Auto-Initialization Integration Tests
// ============================================================

mod verification_init_tests {
    use super::*;

    async fn setup_sqlite_state() -> HttpServerState {
        let app_state = Arc::new(AppState::new_sqlite_test());
        let execution_state = Arc::new(ExecutionState::new());
        HttpServerState {
            app_state,
            execution_state,
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
            verified_plan_blueprint_artifact_id: None,
            plan_blueprint_artifact_id: None,
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
    #[tokio::test]
    async fn test_verification_child_creation_is_rejected() {
        let state = setup_sqlite_state().await;
        let parent = make_parent_session(Some(ArtifactId::new()));
        let parent_id = parent.id.clone();
        state
            .app_state
            .ideation_session_repo
            .create(parent)
            .await
            .expect("create parent");

        let result = create_child_session(
            State(state),
            Json(CreateChildSessionRequest {
                parent_session_id: parent_id.as_str().to_string(),
                title: None,
                description: Some("legacy verifier child".to_string()),
                inherit_context: false,
                initial_prompt: None,
                source_task_id: None,
                source_context_type: None,
                source_context_id: None,
                spawn_reason: None,
                blocker_fingerprint: None,
                purpose: Some("verification".to_string()),
                is_external_trigger: false,
            }),
        )
        .await;

        let (status, Json(body)) = result.expect_err("verification children must be retired");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body["error"],
            "Verification runs in the active Plan conversation and cannot be created as a child session"
        );
    }
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
}

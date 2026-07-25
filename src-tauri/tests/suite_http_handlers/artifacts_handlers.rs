// Integration tests for child session plan artifact independence.
//
// Tests validate the complete end-to-end behavior of the child/parent plan separation:
// - Child sessions with inherited plans create independent artifacts (not chained to parent)
// - update_plan_artifact rejects inherited-only artifacts with a clear, actionable message
// - get_session_plan returns own vs inherited plan with the correct is_inherited flag
// - Parent plan is unaffected by child plan operations
// - 422 responses include the full error message body (not just a status code)
// - Sessions with no plan at all return None from get_session_plan

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use ralphx_lib::application::plan_verification_service::{
    get_plan_verification_status, PlanVerificationStatusKind,
};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    AgentRun, AgentRunActionKind, Artifact, ArtifactContent, ArtifactId, ArtifactMetadata,
    ArtifactType, AutomationRunId, ChatConversation, IdeationSession, IdeationSessionBuilder,
    IdeationSessionFlow, IdeationSessionId, IdeationSessionStatus, NotificationCategory, Project,
    ProjectId, SessionOrigin, SessionPurpose, Task, VerificationRunSnapshot, VerificationStatus,
};
use ralphx_lib::domain::repositories::IdeationSessionRepository;
use ralphx_lib::domain::services::running_agent_registry::RunningAgentKey;
use ralphx_lib::domain::services::{MemoryRunningAgentRegistry, RunningAgentRegistry};
use ralphx_lib::error::AppError;
use ralphx_lib::http_server::handlers::*;
use ralphx_lib::http_server::types::HttpServerState;
use ralphx_lib::infrastructure::agents::claude::verification_config;
use ralphx_lib::infrastructure::memory::{
    MemoryAgentProviderSettingsRepository, MemoryIdeationSessionRepository,
};
use ralphx_lib::infrastructure::sqlite::{
    SqliteAgentRunRepository, SqliteChatConversationRepository,
    SqliteIdeationSessionRepository as SessionRepo,
};
use std::sync::Arc;

// ============================================================
// Test Infrastructure
// ============================================================

async fn setup_test_state() -> HttpServerState {
    let app_state = Arc::new(AppState::new_sqlite_test());
    let execution_state = Arc::new(ExecutionState::new());
    HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
    }
}

async fn setup_model_native_verification_test_state() -> HttpServerState {
    let mut app_state = AppState::new_sqlite_test();
    app_state.agent_run_repo = Arc::new(SqliteAgentRunRepository::from_shared(Arc::clone(
        app_state.db.inner(),
    )));
    app_state.chat_conversation_repo = Arc::new(SqliteChatConversationRepository::from_shared(
        Arc::clone(app_state.db.inner()),
    ));
    let app_state = Arc::new(app_state);
    let execution_state = Arc::new(ExecutionState::new());
    HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
    }
}

async fn setup_disabled_provider_test_state() -> HttpServerState {
    let mut app_state = AppState::new_sqlite_test();
    app_state.agent_provider_settings_repo = Arc::new(MemoryAgentProviderSettingsRepository::new());
    let app_state = Arc::new(app_state);
    let execution_state = Arc::new(ExecutionState::new());
    HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
    }
}

async fn quiesce_auto_verification(state: &HttpServerState, session_id: &IdeationSessionId) {
    let sid = session_id.as_str().to_string();
    state
        .app_state
        .db
        .run(move |conn| SessionRepo::reset_auto_verify_sync(conn, &sid))
        .await
        .unwrap();

    let children = state
        .app_state
        .ideation_session_repo
        .get_verification_children(session_id)
        .await
        .unwrap();
    for child in children {
        let key = RunningAgentKey::new("ideation", child.id.as_str());
        if let Some(info) = state.app_state.running_agent_registry.get(&key).await {
            state
                .app_state
                .running_agent_registry
                .unregister(&key, &info.agent_run_id)
                .await;
        }
        state
            .app_state
            .ideation_session_repo
            .update_status(&child.id, IdeationSessionStatus::Archived)
            .await
            .unwrap();
    }
}

async fn create_plan_artifact_quiesced(
    state: &HttpServerState,
    session_id: &IdeationSessionId,
    title: &str,
    content: &str,
) -> ArtifactResponse {
    let response = create_plan_artifact(
        State(state.clone()),
        Json(CreatePlanArtifactRequest {
            session_id: session_id.as_str().to_string(),
            title: title.to_string(),
            content: content.to_string(),
            blueprint_title: None,
            blueprint_content: Some("# Implementation Blueprint".to_string()),
        }),
    )
    .await
    .expect("Plan creation should succeed")
    .0;
    quiesce_auto_verification(state, session_id).await;
    response
}

fn make_active_session() -> IdeationSession {
    IdeationSession {
        id: IdeationSessionId::new(),
        project_id: ProjectId::from_string("proj-test".to_string()),
        title: Some("Test Session".to_string()),
        status: IdeationSessionStatus::Active,
        plan_artifact_id: None,
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

async fn create_planning_session(state: &HttpServerState) -> IdeationSession {
    let mut session = make_active_session();
    session.session_flow = IdeationSessionFlow::Planning;
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap()
}

async fn plan_notifications(
    state: &HttpServerState,
) -> Vec<ralphx_lib::domain::entities::Notification> {
    state
        .app_state
        .notification_repo
        .list(None, None, 50)
        .await
        .unwrap()
        .notifications
        .into_iter()
        .filter(|notification| notification.category == NotificationCategory::PlanApproval)
        .collect()
}

#[tokio::test]
async fn create_plan_artifact_records_each_non_automation_planning_artifact() {
    let state = setup_test_state().await;
    let session = create_planning_session(&state).await;
    let conversation = ChatConversation::new_ideation(session.id.clone());
    state
        .app_state
        .chat_conversation_repo
        .create(conversation.clone())
        .await
        .unwrap();

    let first =
        create_plan_artifact_quiesced(&state, &session.id, "Draft one", "first draft").await;
    let second =
        create_plan_artifact_quiesced(&state, &session.id, "Draft two", "second draft").await;

    let rows = plan_notifications(&state).await;
    assert_eq!(rows.len(), 2);
    let first_dedupe_key = format!(
        "plan:{}:{}",
        session.id,
        first.plan_target_id.as_deref().expect("v2 plan target")
    );
    assert!(rows.iter().any(|row| {
        row.dedupe_key.as_deref() == Some(first_dedupe_key.as_str()) && row.read_at.is_some()
    }));
    let second_dedupe_key = format!(
        "plan:{}:{}",
        session.id,
        second.plan_target_id.as_deref().expect("v2 plan target")
    );
    assert!(rows.iter().any(|row| {
        row.dedupe_key.as_deref() == Some(second_dedupe_key.as_str()) && row.read_at.is_none()
    }));
    let conversation_id = conversation.id.as_str();
    assert!(rows
        .iter()
        .all(|row| row.target.conversation_id.as_deref() == Some(conversation_id.as_str())));
}

#[tokio::test]
async fn create_plan_artifact_skips_automation_owned_and_implementation_sessions() {
    let state = setup_test_state().await;

    let automation_session = create_planning_session(&state).await;
    let mut automation_conversation = ChatConversation::new_ideation(automation_session.id.clone());
    automation_conversation.automation_run_id = Some(AutomationRunId::new());
    state
        .app_state
        .chat_conversation_repo
        .create(automation_conversation)
        .await
        .unwrap();
    create_plan_artifact_quiesced(&state, &automation_session.id, "Automation plan", "draft").await;

    let implementation_session = create_planning_session(&state).await;
    let mut implementation_task = Task::new(
        implementation_session.project_id.clone(),
        "Implementation task".to_string(),
    );
    implementation_task.ideation_session_id = Some(implementation_session.id.clone());
    state
        .app_state
        .task_repo
        .create(implementation_task)
        .await
        .unwrap();
    create_plan_artifact_quiesced(
        &state,
        &implementation_session.id,
        "Implementation-linked plan",
        "draft",
    )
    .await;

    assert!(plan_notifications(&state).await.is_empty());
}

/// Create a parent session and its plan artifact.
/// Returns (parent_session_id, parent_artifact_id_str).
async fn create_parent_with_plan(state: &HttpServerState) -> (IdeationSessionId, String) {
    let parent = make_active_session();
    let parent_id = parent.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(parent)
        .await
        .unwrap();

    let result =
        create_plan_artifact_quiesced(state, &parent_id, "Parent Plan", "Parent plan content")
            .await;

    let artifact_id = result.id.clone();
    (parent_id, artifact_id)
}

/// Create a child session with `inherited_plan_artifact_id` set, `plan_artifact_id = None`.
/// This simulates what `create_child_session` does with `inherit_context: true`.
async fn create_child_inheriting(
    state: &HttpServerState,
    parent_id: &IdeationSessionId,
    inherited_artifact_id: &str,
) -> IdeationSessionId {
    let mut child = make_active_session();
    let child_id = child.id.clone();
    child.parent_session_id = Some(parent_id.clone());
    child.plan_artifact_id = None;
    child.inherited_plan_artifact_id =
        Some(ArtifactId::from_string(inherited_artifact_id.to_string()));
    state
        .app_state
        .ideation_session_repo
        .create(child)
        .await
        .unwrap();
    child_id
}

// ============================================================
// Test 1 + 6: Child creates own plan — independent, not chained to parent's artifact
// ============================================================

/// Scenario: child has inherited_plan_artifact_id but no own plan.
/// create_plan_artifact must create a NEW artifact (different ID, no version chain to parent).
#[tokio::test]
async fn test_child_creates_independent_plan_not_chained_to_parent() {
    let state = setup_test_state().await;

    let (parent_id, parent_artifact_id) = create_parent_with_plan(&state).await;
    let child_id = create_child_inheriting(&state, &parent_id, &parent_artifact_id).await;

    // Act: child calls create_plan_artifact
    let result = create_plan_artifact(
        State(state.clone()),
        Json(CreatePlanArtifactRequest {
            session_id: child_id.as_str().to_string(),
            title: "Child Plan".to_string(),
            content: "Child's own plan content".to_string(),
            blueprint_title: None,
            blueprint_content: Some("# Implementation Blueprint".to_string()),
        }),
    )
    .await
    .expect("Child should be able to create its own plan artifact");

    let child_artifact_id = result.0.id.clone();

    // Child's artifact is different from parent's (independent)
    assert_ne!(
        child_artifact_id, parent_artifact_id,
        "Child plan artifact ID must differ from parent's"
    );

    // Child's plan_artifact_id is now set to the new artifact
    let child = state
        .app_state
        .ideation_session_repo
        .get_by_id(&child_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        child.plan_artifact_id.as_ref().map(|id| id.as_str()),
        Some(child_artifact_id.as_str()),
        "Child's plan_artifact_id should point to the new independent artifact"
    );

    // Child's artifact is NOT version-chained to parent's plan (only 1 history entry)
    let child_artifact_entity = ArtifactId::from_string(child_artifact_id.clone());
    let history = state
        .app_state
        .artifact_repo
        .get_version_history(&child_artifact_entity)
        .await
        .unwrap();
    assert_eq!(
        history.len(),
        1,
        "Child artifact should have no version chain to parent's plan (got {} history entries)",
        history.len()
    );

    // Parent's plan_artifact_id is unchanged
    let parent = state
        .app_state
        .ideation_session_repo
        .get_by_id(&parent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        parent.plan_artifact_id.as_ref().map(|id| id.as_str()),
        Some(parent_artifact_id.as_str()),
        "Parent's plan_artifact_id must not change when child creates its own plan"
    );
}

// ============================================================
// Test 2: Child updates own plan — succeeds, version is incremented
// ============================================================

/// Scenario: child already has its own plan (plan_artifact_id set).
/// update_plan_artifact should succeed and increment the version.
#[tokio::test]
async fn test_child_can_update_own_plan() {
    let state = setup_test_state().await;

    let (parent_id, parent_artifact_id) = create_parent_with_plan(&state).await;
    let child_id = create_child_inheriting(&state, &parent_id, &parent_artifact_id).await;

    // Child creates its own plan first
    let create_result =
        create_plan_artifact_quiesced(&state, &child_id, "Child Plan v1", "v1 content").await;
    let child_artifact_id = create_result.id.clone();

    // Child updates its own plan
    let update_result = update_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(UpdatePlanArtifactRequest {
            artifact_id: child_artifact_id.clone(),
            content: "v2 content".to_string(),
            caller_session_id: None,
        }),
    )
    .await;

    assert!(
        update_result.is_ok(),
        "Child should be able to update its own plan artifact"
    );

    let updated = update_result.unwrap().0;
    assert_eq!(updated.version, 2, "Version should be incremented to 2");
    assert_eq!(
        updated.content, "v2 content",
        "Updated content should be reflected"
    );

    // Child's plan_artifact_id now points to the updated artifact
    let child = state
        .app_state
        .ideation_session_repo
        .get_by_id(&child_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        child.plan_artifact_id.as_ref().map(|id| id.as_str()),
        Some(updated.id.as_str()),
        "Child's plan_artifact_id should be updated to the new version"
    );
}

#[tokio::test]
async fn verification_action_can_complete_the_plan_version_it_revises() {
    let state = setup_model_native_verification_test_state().await;
    let (session_id, original_artifact_id) = create_parent_with_plan(&state).await;
    let conversation = ChatConversation::new_ideation(session_id.clone());
    let conversation_id = conversation.id;
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let original_target = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap()
        .plan_artifact_bundle()
        .expect("created plan should be a complete bundle")
        .action_target_id();

    let mut run = AgentRun::new(conversation_id);
    run.action_kind = Some(AgentRunActionKind::VerifyPlan);
    run.action_context_id = Some(session_id.as_str().to_string());
    run.action_target_id = Some(original_target);
    let run = state.app_state.agent_run_repo.create(run).await.unwrap();

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "x-ralphx-agent-run-id",
        axum::http::HeaderValue::from_str(&run.id.as_str()).unwrap(),
    );
    headers.insert(
        "x-ralphx-conversation-id",
        axum::http::HeaderValue::from_str(&conversation_id.as_str()).unwrap(),
    );

    let revised = update_plan_artifact(
        State(state.clone()),
        headers.clone(),
        Json(UpdatePlanArtifactRequest {
            artifact_id: original_artifact_id,
            content: "reviewed and revised plan".to_string(),
            caller_session_id: None,
        }),
    )
    .await
    .expect("the verification action should be able to revise its plan")
    .0;

    let run_after_revision = state
        .app_state
        .agent_run_repo
        .get_by_id(&run.id)
        .await
        .unwrap()
        .unwrap();
    let revised_target = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap()
        .plan_artifact_bundle()
        .expect("revised plan should remain a complete bundle")
        .action_target_id();
    assert_eq!(
        run_after_revision.action_target_id.as_deref(),
        Some(revised_target.as_str()),
        "the live action authority must follow the version it created"
    );

    let completed = complete_plan_verification_http(State(state.clone()), headers.clone())
        .await
        .expect("the live action should complete proof for its revised artifact")
        .0;
    assert_eq!(completed.plan_artifact_id, revised.id);
    let _ = complete_plan_verification_http(State(state.clone()), headers)
        .await
        .expect("retrying an already-recorded completion should be idempotent");

    let verified = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        verified
            .verified_plan_artifact_id
            .as_ref()
            .map(ArtifactId::as_str),
        Some(revised.id.as_str())
    );
    let events = state
        .app_state
        .external_events_repo
        .get_events_after_cursor(&[verified.project_id.as_str().to_string()], 0, 20)
        .await
        .expect("external verification events should load");
    assert_eq!(
        events
            .iter()
            .filter(|event| event.event_type == "ideation:verified")
            .count(),
        1,
        "authoritative completion must emit one ideation:verified event"
    );
}

#[tokio::test]
async fn replacing_v2_bundle_advances_versions_relation_and_verifier_state_atomically() {
    let state = setup_model_native_verification_test_state().await;
    let session = make_active_session();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();
    let first = create_plan_artifact_quiesced(&state, &session_id, "Plan v1", "Overview v1").await;
    let first_blueprint_id = first
        .blueprint_artifact
        .as_ref()
        .expect("v2 bundle should include Blueprint")
        .id
        .clone();
    let old_target = first
        .plan_target_id
        .clone()
        .expect("v2 bundle should expose target");
    let conversation = ChatConversation::new_ideation(session_id.clone());
    let conversation_id = conversation.id;
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let mut run = AgentRun::new(conversation_id);
    run.action_kind = Some(AgentRunActionKind::VerifyPlan);
    run.action_context_id = Some(session_id.to_string());
    run.action_target_id = Some(old_target.clone());
    let run = state.app_state.agent_run_repo.create(run).await.unwrap();
    let marker_session_id = session_id.to_string();
    let marker_artifact_id = first.id.clone();
    let marker_target = old_target.clone();
    state
        .app_state
        .db
        .run(move |conn| {
            conn.execute_batch(
                "CREATE TABLE deferred_retarget_audit (
                    artifact_id TEXT NOT NULL,
                    plan_target_id TEXT NOT NULL
                 );
                 CREATE TRIGGER capture_deferred_retarget
                 AFTER UPDATE OF artifact_id, plan_target_id
                 ON deferred_plan_approval_notifications
                 BEGIN
                    INSERT INTO deferred_retarget_audit (artifact_id, plan_target_id)
                    VALUES (NEW.artifact_id, NEW.plan_target_id);
                 END;",
            )?;
            conn.execute(
                "INSERT INTO deferred_plan_approval_notifications
                    (session_id, artifact_id, plan_target_id, created_at)
                 VALUES (?1, ?2, ?3, datetime('now'))",
                rusqlite::params![marker_session_id, marker_artifact_id, marker_target],
            )?;
            Ok(())
        })
        .await
        .unwrap();

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "x-ralphx-agent-run-id",
        axum::http::HeaderValue::from_str(&run.id.as_str()).unwrap(),
    );
    headers.insert(
        "x-ralphx-conversation-id",
        axum::http::HeaderValue::from_str(&conversation_id.as_str()).unwrap(),
    );
    let replacement = create_plan_artifact_with_headers(
        State(state.clone()),
        headers,
        Json(CreatePlanArtifactRequest {
            session_id: session_id.to_string(),
            title: "Plan v2".to_string(),
            content: "Overview v2".to_string(),
            blueprint_title: None,
            blueprint_content: Some("Blueprint v2".to_string()),
        }),
    )
    .await
    .expect("bundle replacement should succeed")
    .0;
    let replacement_blueprint = replacement
        .blueprint_artifact
        .as_ref()
        .expect("replacement should include Blueprint");
    let new_target = replacement
        .plan_target_id
        .clone()
        .expect("replacement should expose target");

    assert_eq!(replacement.version, 2);
    assert_eq!(replacement_blueprint.version, 2);
    let run_after = state
        .app_state
        .agent_run_repo
        .get_by_id(&run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        run_after.action_target_id.as_deref(),
        Some(new_target.as_str())
    );
    let old_overview_id = first.id.clone();
    let old_blueprint_id = first_blueprint_id;
    let new_overview_id = replacement.id.clone();
    let new_blueprint_id = replacement_blueprint.id.clone();
    let (marker, old_relation_count, new_relation_count) = state
        .app_state
        .db
        .run(move |conn| {
            let marker = conn.query_row(
                "SELECT artifact_id, plan_target_id
                 FROM deferred_retarget_audit
                 ORDER BY rowid DESC
                 LIMIT 1",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )?;
            let old_relation_count = conn.query_row(
                "SELECT COUNT(*) FROM artifact_relations
                 WHERE relation_type = 'related_to'
                   AND ((from_artifact_id = ?1 AND to_artifact_id = ?2)
                     OR (from_artifact_id = ?2 AND to_artifact_id = ?1))",
                rusqlite::params![old_overview_id, old_blueprint_id],
                |row| row.get::<_, i64>(0),
            )?;
            let new_relation_count = conn.query_row(
                "SELECT COUNT(*) FROM artifact_relations
                 WHERE relation_type = 'related_to'
                   AND ((from_artifact_id = ?1 AND to_artifact_id = ?2)
                     OR (from_artifact_id = ?2 AND to_artifact_id = ?1))",
                rusqlite::params![new_overview_id, new_blueprint_id],
                |row| row.get::<_, i64>(0),
            )?;
            Ok((marker, old_relation_count, new_relation_count))
        })
        .await
        .unwrap();
    assert_eq!(marker, (replacement.id, new_target));
    assert_eq!(old_relation_count, 0);
    assert_eq!(new_relation_count, 1);
}

#[tokio::test]
async fn promoting_v1_bundle_retargets_active_verifier_and_deferred_marker() {
    let state = setup_model_native_verification_test_state().await;
    let overview = state
        .app_state
        .artifact_repo
        .create(Artifact::new_inline(
            "Legacy overview",
            ArtifactType::Specification,
            "Legacy content",
            "test",
        ))
        .await
        .unwrap();
    let mut session = make_active_session();
    session.plan_artifact_id = Some(overview.id.clone());
    session.plan_contract_version = 1;
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();
    let conversation = ChatConversation::new_ideation(session_id.clone());
    let conversation_id = conversation.id;
    state
        .app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();
    let mut run = AgentRun::new(conversation_id);
    run.action_kind = Some(AgentRunActionKind::VerifyPlan);
    run.action_context_id = Some(session_id.to_string());
    run.action_target_id = Some(overview.id.to_string());
    let run = state.app_state.agent_run_repo.create(run).await.unwrap();
    let marker_session_id = session_id.to_string();
    let marker_artifact_id = overview.id.to_string();
    state
        .app_state
        .db
        .run(move |conn| {
            conn.execute_batch(
                "CREATE TABLE deferred_retarget_audit (
                    artifact_id TEXT NOT NULL,
                    plan_target_id TEXT NOT NULL
                 );
                 CREATE TRIGGER capture_deferred_retarget
                 AFTER UPDATE OF artifact_id, plan_target_id
                 ON deferred_plan_approval_notifications
                 BEGIN
                    INSERT INTO deferred_retarget_audit (artifact_id, plan_target_id)
                    VALUES (NEW.artifact_id, NEW.plan_target_id);
                 END;",
            )?;
            conn.execute(
                "INSERT INTO deferred_plan_approval_notifications
                    (session_id, artifact_id, plan_target_id, created_at)
                 VALUES (?1, ?2, ?2, datetime('now'))",
                rusqlite::params![marker_session_id, marker_artifact_id],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "x-ralphx-agent-run-id",
        axum::http::HeaderValue::from_str(&run.id.as_str()).unwrap(),
    );
    headers.insert(
        "x-ralphx-conversation-id",
        axum::http::HeaderValue::from_str(&conversation_id.as_str()).unwrap(),
    );

    let promoted = create_plan_artifact_with_headers(
        State(state.clone()),
        headers,
        Json(CreatePlanArtifactRequest {
            session_id: session_id.to_string(),
            title: "Promoted overview".to_string(),
            content: "Promoted content".to_string(),
            blueprint_title: None,
            blueprint_content: Some("New Blueprint".to_string()),
        }),
    )
    .await
    .expect("v1 promotion should succeed")
    .0;
    let new_target = promoted
        .plan_target_id
        .clone()
        .expect("promoted bundle should expose target");

    assert_eq!(promoted.version, 2);
    let run_after = state
        .app_state
        .agent_run_repo
        .get_by_id(&run.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        run_after.action_target_id.as_deref(),
        Some(new_target.as_str())
    );
    let marker = state
        .app_state
        .db
        .run(move |conn| {
            conn.query_row(
                "SELECT artifact_id, plan_target_id
                 FROM deferred_retarget_audit
                 ORDER BY rowid DESC
                 LIMIT 1",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();
    assert_eq!(marker, (promoted.id, new_target));
}

#[tokio::test]
async fn updating_a_plan_does_not_interrupt_drafting_with_automatic_verification() {
    let state = setup_test_state().await;
    state.execution_state.pause();
    let mut project = Project::new("Auto verification project".to_string(), "/tmp".to_string());
    project.id = ProjectId::from_string("proj-test".to_string());
    state.app_state.project_repo.create(project).await.unwrap();
    let (session_id, original_artifact_id) = create_parent_with_plan(&state).await;
    let mut settings = state
        .app_state
        .ideation_settings_repo
        .get_settings()
        .await
        .unwrap();
    settings.auto_verify_plans = true;
    state
        .app_state
        .ideation_settings_repo
        .update_settings(&settings)
        .await
        .unwrap();

    let revised = update_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(UpdatePlanArtifactRequest {
            artifact_id: original_artifact_id,
            content: "new version requiring automatic verification".to_string(),
            caller_session_id: None,
        }),
    )
    .await
    .expect("plan update should remain a drafting-only operation")
    .0;

    let status = get_plan_verification_status(&state.app_state, &session_id)
        .await
        .unwrap();
    assert_eq!(
        status.plan_artifact_id.as_deref(),
        Some(revised.id.as_str())
    );
    assert_eq!(
        status.status,
        PlanVerificationStatusKind::Unverified,
        "automatic verification must wait for an acceptance attempt"
    );
    let session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        session.pending_initial_prompt.is_none(),
        "draft revisions must not enqueue a verification prompt"
    );
}

#[tokio::test]
async fn creating_a_plan_does_not_launch_automatic_verification_during_drafting() {
    let state = setup_disabled_provider_test_state().await;
    let session = make_active_session();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();
    let mut settings = state
        .app_state
        .ideation_settings_repo
        .get_settings()
        .await
        .unwrap();
    settings.auto_verify_plans = true;
    state
        .app_state
        .ideation_settings_repo
        .update_settings(&settings)
        .await
        .unwrap();

    let created = create_plan_artifact(
        State(state.clone()),
        Json(CreatePlanArtifactRequest {
            session_id: session_id.as_str().to_string(),
            title: "Committed plan".to_string(),
            content: "Plan content".to_string(),
            blueprint_title: None,
            blueprint_content: Some("# Implementation Blueprint".to_string()),
        }),
    )
    .await
    .expect("plan creation should not depend on verification launch availability")
    .0;

    let persisted = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        persisted.plan_artifact_id.as_ref().map(ArtifactId::as_str),
        Some(created.id.as_str())
    );
    assert_eq!(
        get_plan_verification_status(&state.app_state, &session_id)
            .await
            .unwrap()
            .status,
        PlanVerificationStatusKind::Unverified
    );
    assert!(
        persisted.pending_initial_prompt.is_none(),
        "plan creation must not enqueue verification before acceptance"
    );
}

// ============================================================
// Test 3 + 8: update_plan_artifact on inherited-only artifact → 422 with clear message
// ============================================================

/// Scenario: an artifact is referenced ONLY as inherited_plan_artifact_id
/// (no session owns it via plan_artifact_id).
/// update_plan_artifact must return 422 with an actionable error message.
#[tokio::test]
async fn test_update_inherited_only_plan_returns_422_with_clear_message() {
    let state = setup_test_state().await;

    // Create an artifact directly in the repo — no session owns it via plan_artifact_id.
    // This simulates the case where the artifact belongs to no owning session
    // but is referenced as inherited by a child session.
    let orphan_artifact = Artifact::new_inline(
        "Inherited Plan",
        ArtifactType::Specification,
        "Inherited plan content",
        "orchestrator",
    );
    let orphan_id = orphan_artifact.id.clone();
    state
        .app_state
        .artifact_repo
        .create(orphan_artifact)
        .await
        .unwrap();

    // Child session references the artifact as inherited (not owned)
    let mut child = make_active_session();
    child.plan_artifact_id = None;
    child.inherited_plan_artifact_id = Some(orphan_id.clone());
    state
        .app_state
        .ideation_session_repo
        .create(child)
        .await
        .unwrap();

    // Act: try to update the inherited-only artifact
    let result = update_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(UpdatePlanArtifactRequest {
            artifact_id: orphan_id.as_str().to_string(),
            content: "Attempted override".to_string(),
            caller_session_id: None,
        }),
    )
    .await;

    // Assert: rejected with 422
    assert!(
        result.is_err(),
        "update_plan_artifact should fail for an inherited-only artifact"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "Should return 422 Unprocessable Entity, got {:?}",
        err.status
    );

    // Error message must be present and actionable
    let msg = err
        .message
        .expect("422 response must include an error message body");
    assert!(
        msg.contains("Cannot update inherited plan"),
        "Error message must explain the problem: got '{}'",
        msg
    );
    assert!(
        msg.contains("create_plan_artifact"),
        "Error message must direct to create_plan_artifact: got '{}'",
        msg
    );
}

// ============================================================
// Test 4: get_session_plan returns own plan (is_inherited: false)
// ============================================================

/// Scenario: child has its own plan (plan_artifact_id set).
/// get_session_plan must return the child's own plan with is_inherited = false.
#[tokio::test]
async fn test_get_session_plan_returns_own_plan_as_not_inherited() {
    let state = setup_test_state().await;

    let (parent_id, parent_artifact_id) = create_parent_with_plan(&state).await;
    let child_id = create_child_inheriting(&state, &parent_id, &parent_artifact_id).await;

    // Child creates its own plan
    let create_result = create_plan_artifact(
        State(state.clone()),
        Json(CreatePlanArtifactRequest {
            session_id: child_id.as_str().to_string(),
            title: "Child Own Plan".to_string(),
            content: "Child's own content".to_string(),
            blueprint_title: None,
            blueprint_content: Some("# Implementation Blueprint".to_string()),
        }),
    )
    .await
    .unwrap();
    let child_artifact_id = create_result.0.id.clone();

    // Act: get_session_plan for child
    let plan = get_session_plan(State(state.clone()), Path(child_id.as_str().to_string()))
        .await
        .expect("get_session_plan should succeed")
        .0
        .expect("Child should have a plan visible");

    // Assert: returns child's own plan with is_inherited = false
    assert_eq!(
        plan.id, child_artifact_id,
        "Should return child's own plan, not the inherited one"
    );
    assert_eq!(
        plan.is_inherited,
        Some(false),
        "Own plan should have is_inherited = false"
    );
    assert_eq!(plan.artifact_type, "specification");
    assert_eq!(plan.content_type, "inline");
    assert_eq!(plan.content, "Child's own content");
    assert!(
        plan.derived_from.is_empty(),
        "HTTP artifact responses should serialize an empty derived_from array, not null"
    );
}

// ============================================================
// Test 5: get_session_plan returns inherited plan (is_inherited: true) when no own plan
// ============================================================

/// Scenario: child has inherited_plan_artifact_id but no own plan.
/// get_session_plan must return the inherited plan with is_inherited = true.
#[tokio::test]
async fn test_get_session_plan_returns_inherited_plan_when_no_own_plan() {
    let state = setup_test_state().await;

    let (parent_id, parent_artifact_id) = create_parent_with_plan(&state).await;
    let child_id = create_child_inheriting(&state, &parent_id, &parent_artifact_id).await;

    // Act: get_session_plan for child (no own plan created yet)
    let plan = get_session_plan(State(state.clone()), Path(child_id.as_str().to_string()))
        .await
        .expect("get_session_plan should succeed")
        .0
        .expect("Child should see the inherited plan");

    // Assert: returns parent's plan with is_inherited = true
    assert_eq!(
        plan.id, parent_artifact_id,
        "Should return the inherited parent plan"
    );
    assert_eq!(
        plan.is_inherited,
        Some(true),
        "Inherited plan should have is_inherited = true"
    );
}

// ============================================================
// Test 7: Parent plan unaffected by all child plan operations
// ============================================================

/// Scenario: child creates its own plan and updates it.
/// Parent's plan_artifact_id and artifact content must remain unchanged throughout.
#[tokio::test]
async fn test_parent_plan_unaffected_by_child_plan_operations() {
    let state = setup_test_state().await;

    let (parent_id, parent_artifact_id) = create_parent_with_plan(&state).await;
    let child_id = create_child_inheriting(&state, &parent_id, &parent_artifact_id).await;

    // Child creates its own plan
    let child_create =
        create_plan_artifact_quiesced(&state, &child_id, "Child Plan", "Child content v1").await;
    let child_artifact_id = child_create.id.clone();

    // Child updates its own plan
    let _ = update_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(UpdatePlanArtifactRequest {
            artifact_id: child_artifact_id.clone(),
            content: "Child content v2".to_string(),
            caller_session_id: None,
        }),
    )
    .await
    .expect("Child update should succeed");

    // Assert: parent's session still points to original artifact
    let parent = state
        .app_state
        .ideation_session_repo
        .get_by_id(&parent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        parent.plan_artifact_id.as_ref().map(|id| id.as_str()),
        Some(parent_artifact_id.as_str()),
        "Parent's plan_artifact_id must be unchanged after child operations"
    );

    // Assert: parent's artifact is still version 1 with original content
    let parent_artifact = state
        .app_state
        .artifact_repo
        .get_by_id(&ArtifactId::from_string(parent_artifact_id.clone()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        parent_artifact.metadata.version, 1,
        "Parent artifact version must still be 1 after child's update"
    );

    // Assert: get_session_plan for parent still returns the original plan as not inherited
    let parent_plan = get_session_plan(State(state.clone()), Path(parent_id.as_str().to_string()))
        .await
        .unwrap()
        .0
        .unwrap();
    assert_eq!(
        parent_plan.id, parent_artifact_id,
        "Parent's get_session_plan should return the original plan"
    );
    assert_eq!(
        parent_plan.is_inherited,
        Some(false),
        "Parent's own plan should not be marked as inherited"
    );
}

// ============================================================
// Test 9: Session with no plan returns None from get_session_plan
// ============================================================

/// Scenario: session has neither plan_artifact_id nor inherited_plan_artifact_id
/// (simulates pre-migration sessions or sessions that never created a plan).
/// get_session_plan must return None — not an error.
#[tokio::test]
async fn test_get_session_plan_returns_none_when_no_plan() {
    let state = setup_test_state().await;

    let session = make_active_session();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = get_session_plan(State(state.clone()), Path(session_id.as_str().to_string()))
        .await
        .expect("get_session_plan should not error for a session with no plan");

    assert!(
        result.0.is_none(),
        "Session with no plan should return None (got Some)"
    );
}

// ============================================================
// Test 10: get_session_plan populates project_working_directory when project exists
// ============================================================

/// Scenario: session belongs to a project with a working_directory set.
/// get_session_plan must populate project_working_directory in the response.
#[tokio::test]
async fn test_get_session_plan_includes_project_working_directory() {
    let state = setup_test_state().await;

    // Create a real project with working_directory set
    let expected_dir = "/test/projects/my-project".to_string();
    let project = Project::new("My Test Project".to_string(), expected_dir.clone());
    let project_id = project.id.clone();
    state
        .app_state
        .project_repo
        .create(project)
        .await
        .expect("Failed to create project");

    // Create an ideation session linked to that project
    let mut session = make_active_session();
    session.project_id = project_id.clone();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Create a plan so get_session_plan returns Some(...)
    let _plan = create_plan_artifact(
        State(state.clone()),
        Json(CreatePlanArtifactRequest {
            session_id: session_id.as_str().to_string(),
            title: "Test Plan".to_string(),
            content: "Plan content".to_string(),
            blueprint_title: None,
            blueprint_content: Some("# Implementation Blueprint".to_string()),
        }),
    )
    .await
    .expect("create_plan_artifact should succeed");

    // Act: get_session_plan
    let result = get_session_plan(State(state.clone()), Path(session_id.as_str().to_string()))
        .await
        .expect("get_session_plan should succeed")
        .0
        .expect("Session with plan should return Some");

    // Assert: project_working_directory is populated
    assert_eq!(
        result.project_working_directory,
        Some(expected_dir),
        "project_working_directory must be populated from the project record"
    );
}

// ============================================================
// Extra: Second create_plan_artifact call on child chains to OWN plan, not parent's
// ============================================================

/// Scenario: child already has its own plan; agent calls create_plan_artifact again.
/// The second call must use the child's OWN artifact as the base (not the parent's),
/// and the child's plan_artifact_id must be updated to the new artifact.
#[tokio::test]
async fn test_child_second_plan_updates_child_not_parent() {
    let state = setup_test_state().await;

    let (parent_id, parent_artifact_id) = create_parent_with_plan(&state).await;
    let child_id = create_child_inheriting(&state, &parent_id, &parent_artifact_id).await;

    // Child creates first plan
    let first = create_plan_artifact(
        State(state.clone()),
        Json(CreatePlanArtifactRequest {
            session_id: child_id.as_str().to_string(),
            title: "Child Plan v1".to_string(),
            content: "v1".to_string(),
            blueprint_title: None,
            blueprint_content: Some("# Implementation Blueprint".to_string()),
        }),
    )
    .await
    .unwrap();
    let first_artifact_id = first.0.id.clone();

    // Child creates second plan (a new revision of their own plan)
    let second = create_plan_artifact(
        State(state.clone()),
        Json(CreatePlanArtifactRequest {
            session_id: child_id.as_str().to_string(),
            title: "Child Plan v2".to_string(),
            content: "v2".to_string(),
            blueprint_title: None,
            blueprint_content: Some("# Implementation Blueprint".to_string()),
        }),
    )
    .await
    .unwrap();
    let second_artifact_id = second.0.id.clone();

    // Second artifact is distinct from both parent's and first child artifact
    assert_ne!(
        second_artifact_id, parent_artifact_id,
        "Second child plan must not reuse parent's artifact"
    );
    assert_ne!(
        second_artifact_id, first_artifact_id,
        "Second child plan must be a new artifact"
    );

    // Child's plan_artifact_id now points to the second artifact
    let child = state
        .app_state
        .ideation_session_repo
        .get_by_id(&child_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        child.plan_artifact_id.as_ref().map(|id| id.as_str()),
        Some(second_artifact_id.as_str()),
        "Child's plan_artifact_id must point to the latest artifact"
    );

    // Parent's plan_artifact_id is completely unaffected
    let parent = state
        .app_state
        .ideation_session_repo
        .get_by_id(&parent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        parent.plan_artifact_id.as_ref().map(|id| id.as_str()),
        Some(parent_artifact_id.as_str()),
        "Parent's plan_artifact_id must be unchanged after child creates multiple plans"
    );
}

// ============================================================
// Verification reset tests
// ============================================================

/// Scenario: session has verification_status=Verified and verification_in_progress=false.
/// update_plan_artifact must reset verification_status to Unverified.
#[tokio::test]
async fn test_update_plan_artifact_resets_verification_when_not_in_progress() {
    let state = setup_test_state().await;

    // Create session with verified status, NOT in progress
    let mut session = make_active_session();
    session.verification_status = VerificationStatus::Verified;
    session.verification_in_progress = false;
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Create initial plan artifact for this session
    let create_result =
        create_plan_artifact_quiesced(&state, &session_id, "Plan v1", "Initial content").await;
    let artifact_id = create_result.id.clone();

    // Update the plan artifact
    let _ = update_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(UpdatePlanArtifactRequest {
            artifact_id: artifact_id.clone(),
            content: "Updated content".to_string(),
            caller_session_id: None,
        }),
    )
    .await
    .expect("Plan update should succeed");

    // Assert: verification_status is reset to Unverified
    let updated_session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_session.verification_status,
        VerificationStatus::Unverified,
        "verification_status must be reset to Unverified after plan update (was Verified)"
    );
    assert!(
        !updated_session.verification_in_progress,
        "verification_in_progress must remain false after reset"
    );
}

/// Scenario: session has verification_status=Verified and verification_in_progress=true
/// (verification loop is actively running, producing auto-corrections).
/// update_plan_artifact must NOT reset verification to prevent the loop-reset paradox (C2).
#[tokio::test]
async fn test_update_plan_artifact_skips_reset_when_verification_in_progress() {
    let state = setup_test_state().await;

    // Create session, then persist verification_in_progress=true via update_verification_state.
    // SQLite's create() does not persist verification fields — they default to 0/'unverified'.
    let session = make_active_session();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();
    // Persist in_progress=true so the guard in reset_verification activates
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id, VerificationStatus::Verified, true)
        .await
        .unwrap();

    // Create initial plan artifact for this session
    let create_result = create_plan_artifact(
        State(state.clone()),
        Json(CreatePlanArtifactRequest {
            session_id: session_id.as_str().to_string(),
            title: "Plan v1".to_string(),
            content: "Initial content".to_string(),
            blueprint_title: None,
            blueprint_content: Some("# Implementation Blueprint".to_string()),
        }),
    )
    .await
    .expect("Plan creation should succeed");
    let artifact_id = create_result.0.id.clone();

    // Use reset_verification to verify it returns false when in_progress=true.
    let reset_result = state
        .app_state
        .ideation_session_repo
        .reset_verification(&session_id)
        .await
        .unwrap();
    assert!(
        !reset_result,
        "reset_verification should return false since verification_in_progress=true in the repo"
    );

    // Create a second session with in_progress=true to further verify the guard
    let session2 = make_active_session();
    let session2_id = session2.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session2)
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session2_id, VerificationStatus::NeedsRevision, true)
        .await
        .unwrap();

    // Reset should return false because in_progress=true
    let reset_skipped = state
        .app_state
        .ideation_session_repo
        .reset_verification(&session2_id)
        .await
        .unwrap();
    assert!(
        !reset_skipped,
        "reset_verification must return false when verification_in_progress=true (loop active)"
    );

    // Verification status must be unchanged
    let unchanged = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session2_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        unchanged.verification_status,
        VerificationStatus::NeedsRevision,
        "verification_status must not change when in_progress=true"
    );
    assert!(
        unchanged.verification_in_progress,
        "verification_in_progress must remain true"
    );

    // Now update plan artifact for session with artifact — it calls reset_verification internally.
    // Since the session for the artifact has in_progress=false, the update succeeds normally.
    let update_result = update_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(UpdatePlanArtifactRequest {
            artifact_id,
            content: "Auto-corrected content from verification loop".to_string(),
            caller_session_id: None,
        }),
    )
    .await;
    assert!(
        update_result.is_ok(),
        "update_plan_artifact should succeed even when a different session has in_progress=true"
    );
}

// ============================================================
// Batch proposal linking tests (step 7)
// ============================================================

/// link_proposals_to_plan with 25 proposals — all should be linked in one transaction.
#[tokio::test]
async fn test_link_proposals_to_plan_batch_25() {
    use ralphx_lib::domain::entities::{
        Complexity, Priority, ProposalCategory, ProposalStatus, TaskProposal, TaskProposalId,
    };

    let state = setup_test_state().await;
    let (parent_id, artifact_id) = create_parent_with_plan(&state).await;
    let parent = state
        .app_state
        .ideation_session_repo
        .get_by_id(&parent_id)
        .await
        .unwrap()
        .unwrap();
    let blueprint_artifact_id = parent
        .plan_blueprint_artifact_id
        .as_ref()
        .expect("v2 plan should include a Blueprint")
        .as_str()
        .to_string();

    // Create 25 proposals via the repo (SQLite-backed in new_sqlite_test state)
    let mut proposal_ids = Vec::new();
    for i in 0..25usize {
        let proposal = TaskProposal {
            id: TaskProposalId::new(),
            session_id: IdeationSessionId::new(),
            title: format!("Proposal {i}"),
            description: None,
            category: ProposalCategory::Feature,
            steps: None,
            acceptance_criteria: None,
            suggested_priority: Priority::Medium,
            priority_score: 50,
            priority_reason: None,
            priority_factors: None,
            estimated_complexity: Complexity::Moderate,
            user_priority: None,
            user_modified: false,
            status: ProposalStatus::Pending,
            selected: false,
            created_task_id: None,
            plan_artifact_id: None,
            plan_version_at_creation: None,
            blueprint_artifact_id: None,
            blueprint_version_at_creation: None,
            sort_order: i as i32,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            archived_at: None,
            target_project: None,
            migrated_from_session_id: None,
            migrated_from_proposal_id: None,
            affected_paths: None,
        };
        let saved = state
            .app_state
            .task_proposal_repo
            .create(proposal)
            .await
            .unwrap();
        proposal_ids.push(saved.id.as_str().to_string());
    }

    // Link all 25 in one call
    let result = link_proposals_to_plan(
        State(state.clone()),
        Json(LinkProposalsToPlanRequest {
            artifact_id: artifact_id.clone(),
            proposal_ids: proposal_ids.clone(),
        }),
    )
    .await;
    assert!(
        result.is_ok(),
        "link_proposals_to_plan should succeed for 25 proposals"
    );

    // Verify all 25 now have plan_artifact_id set
    let linked = state
        .app_state
        .task_proposal_repo
        .get_by_plan_artifact_id(&ArtifactId::from_string(artifact_id.clone()))
        .await
        .unwrap();
    assert_eq!(
        linked.len(),
        25,
        "All 25 proposals should be linked to the artifact"
    );
    for p in &linked {
        assert_eq!(
            p.plan_artifact_id.as_ref().map(|id| id.as_str()),
            Some(artifact_id.as_str()),
            "Proposal {} should point to the artifact",
            p.id
        );
        assert_eq!(
            p.plan_version_at_creation,
            Some(1),
            "plan_version_at_creation should be set to artifact version 1"
        );
        assert_eq!(
            p.blueprint_artifact_id.as_ref().map(|id| id.as_str()),
            Some(blueprint_artifact_id.as_str()),
            "Proposal {} should point to the plan's exact Blueprint",
            p.id
        );
        assert_eq!(
            p.blueprint_version_at_creation,
            Some(1),
            "blueprint_version_at_creation should be set to Blueprint version 1"
        );
    }
}

/// link_proposals_to_plan with zero proposals — should succeed without errors.
#[tokio::test]
async fn test_link_proposals_zero_proposals_succeeds() {
    let state = setup_test_state().await;
    let (_, artifact_id) = create_parent_with_plan(&state).await;

    let result = link_proposals_to_plan(
        State(state.clone()),
        Json(LinkProposalsToPlanRequest {
            artifact_id: artifact_id.clone(),
            proposal_ids: vec![],
        }),
    )
    .await;
    assert!(
        result.is_ok(),
        "link_proposals_to_plan with zero proposals should succeed"
    );
}

/// create_plan_artifact returns 404 when the session does not exist.
#[tokio::test]
async fn test_create_plan_artifact_session_not_found() {
    let state = setup_test_state().await;

    let result = create_plan_artifact(
        State(state.clone()),
        Json(CreatePlanArtifactRequest {
            session_id: "nonexistent-session-id".to_string(),
            title: "Plan".to_string(),
            content: "content".to_string(),
            blueprint_title: None,
            blueprint_content: Some("# Implementation Blueprint".to_string()),
        }),
    )
    .await;

    assert!(result.is_err(), "Should fail for nonexistent session");
    let err = result.unwrap_err();
    assert_eq!(
        err.status,
        StatusCode::NOT_FOUND,
        "Should return 404 for nonexistent session"
    );
}

/// update_plan_artifact resolves a stale (old-version) artifact ID to the latest.
/// Passing v1's ID after a second update (v2 exists) should succeed and produce v3.
#[tokio::test]
async fn test_update_plan_artifact_stale_id_resolved_to_latest() {
    let state = setup_test_state().await;
    let (_, v1_id) = create_parent_with_plan(&state).await;

    // Update → v2
    let v2 = update_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(UpdatePlanArtifactRequest {
            artifact_id: v1_id.clone(),
            content: "v2 content".to_string(),
            caller_session_id: None,
        }),
    )
    .await
    .expect("v1→v2 update should succeed");
    let v2_id = v2.0.id.clone();
    assert_eq!(v2.0.version, 2);

    // Update again using the STALE v1 ID — should still succeed, resolving to v2 first
    let v3 = update_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(UpdatePlanArtifactRequest {
            artifact_id: v1_id.clone(), // stale
            content: "v3 content".to_string(),
            caller_session_id: None,
        }),
    )
    .await
    .expect("Stale v1 ID should be resolved to v2, then produce v3");
    assert_eq!(v3.0.version, 3, "Should produce version 3");
    assert_eq!(
        v3.0.previous_artifact_id.as_deref(),
        Some(v2_id.as_str()),
        "v3's previous should be v2 (stale ID resolved to v2 before creating v3)"
    );
}

// ============================================================
// Auto-verify trigger tests (Phase 2)
// ============================================================

/// trigger_auto_verify_sync sets in_progress=1 and increments generation atomically.
/// A second call on the same session returns None (already in_progress=1 — skip).
#[tokio::test]
async fn test_trigger_auto_verify_sync_atomicity_and_skip() {
    use ralphx_lib::infrastructure::sqlite::SqliteIdeationSessionRepository as SessionRepo;

    let state = setup_test_state().await;
    let session = make_active_session();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // First trigger: should succeed and return generation=1
    let sid = session_id.as_str().to_string();
    let gen = state
        .app_state
        .db
        .run(move |conn| SessionRepo::trigger_auto_verify_sync(conn, &sid))
        .await
        .unwrap();
    assert_eq!(gen, Some(1), "First trigger should return generation=1");

    // Verify session state: in_progress=true, generation=1, status=Reviewing
    let after_trigger = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        after_trigger.verification_in_progress,
        "in_progress must be true after trigger"
    );
    assert_eq!(
        after_trigger.verification_generation, 1,
        "generation must be 1"
    );
    assert_eq!(
        after_trigger.verification_status,
        VerificationStatus::Reviewing,
        "status must be Reviewing"
    );
    assert_eq!(
        after_trigger.verification_current_round, None,
        "current_round summary must be cleared on a fresh verification start"
    );
    assert_eq!(
        after_trigger.verification_max_rounds, None,
        "session summary max_rounds stays unset until the native snapshot projects it"
    );
    assert_eq!(
        after_trigger.verification_gap_count, 0,
        "gap_count summary must be cleared on a fresh verification start"
    );
    assert_eq!(
        after_trigger.verification_gap_score, None,
        "gap_score summary must be cleared on a fresh verification start"
    );
    assert_eq!(
        after_trigger.verification_convergence_reason, None,
        "convergence_reason summary must be cleared on a fresh verification start"
    );

    let snapshot = state
        .app_state
        .ideation_session_repo
        .get_verification_run_snapshot(&session_id, 1)
        .await
        .unwrap()
        .expect("fresh verification start must seed a native run snapshot");
    assert_eq!(snapshot.generation, 1);
    assert_eq!(snapshot.status, VerificationStatus::Reviewing);
    assert!(snapshot.in_progress, "fresh snapshot must be in progress");
    assert_eq!(snapshot.current_round, 0);
    assert_eq!(
        snapshot.max_rounds,
        verification_config().max_rounds,
        "fresh snapshot must carry the configured round budget"
    );
    assert!(
        snapshot.current_gaps.is_empty(),
        "fresh snapshot must start gap-free"
    );
    assert!(
        snapshot.rounds.is_empty(),
        "fresh snapshot must start with no round history"
    );

    // Second trigger on same session: in_progress=1, so must be skipped
    let sid2 = session_id.as_str().to_string();
    let gen2 = state
        .app_state
        .db
        .run(move |conn| SessionRepo::trigger_auto_verify_sync(conn, &sid2))
        .await
        .unwrap();
    assert_eq!(
        gen2, None,
        "Second trigger must return None (already in_progress)"
    );

    // Generation must NOT have been incremented again
    let after_skip = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        after_skip.verification_generation, 1,
        "generation must remain 1 after skip"
    );
}

/// reset_auto_verify_sync unconditionally resets in_progress=0 and status=unverified.
/// This is the spawn-failure recovery path.
#[tokio::test]
async fn test_reset_auto_verify_sync_clears_in_progress() {
    use ralphx_lib::infrastructure::sqlite::SqliteIdeationSessionRepository as SessionRepo;

    let state = setup_test_state().await;
    let session = make_active_session();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Put session into triggered state
    let sid = session_id.as_str().to_string();
    state
        .app_state
        .db
        .run(move |conn| SessionRepo::trigger_auto_verify_sync(conn, &sid))
        .await
        .unwrap();

    // Verify it's in triggered state
    let triggered = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert!(triggered.verification_in_progress);

    // Reset (simulates spawn failure recovery)
    let sid2 = session_id.as_str().to_string();
    state
        .app_state
        .db
        .run(move |conn| SessionRepo::reset_auto_verify_sync(conn, &sid2))
        .await
        .unwrap();

    // Verify state is reset
    let after_reset = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        !after_reset.verification_in_progress,
        "in_progress must be false after reset"
    );
    assert_eq!(
        after_reset.verification_status,
        VerificationStatus::Unverified,
        "status must be Unverified after reset"
    );
    let (effective_status, effective_in_progress) = state
        .app_state
        .ideation_session_repo
        .get_verification_status(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        effective_status,
        VerificationStatus::Unverified,
        "authoritative verification status must also reset to Unverified"
    );
    assert!(
        !effective_in_progress,
        "authoritative verification status must also clear in_progress"
    );
}

/// create_plan_artifact skips trigger when verification_in_progress=1 (mutex held).
/// Uses the trigger_auto_verify_sync guard: if in_progress=1 when create_plan_artifact runs,
/// the trigger is skipped — generation is NOT incremented again.
#[tokio::test]
async fn test_create_plan_artifact_skips_trigger_when_already_in_progress() {
    let state = setup_test_state().await;
    let session = make_active_session();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // Manually set in_progress=1 with generation=0 (simulates prior verifier holding the lock)
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();

    let _ = create_plan_artifact(
        State(state.clone()),
        Json(CreatePlanArtifactRequest {
            session_id: session_id.as_str().to_string(),
            title: "Plan".to_string(),
            content: "Plan content".to_string(),
            blueprint_title: None,
            blueprint_content: Some("# Implementation Blueprint".to_string()),
        }),
    )
    .await
    .expect("create_plan_artifact should succeed even when in_progress=1");

    let after = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();

    // Trigger must have been skipped — generation stays 0
    assert_eq!(
        after.verification_generation, 0,
        "generation must not be incremented when trigger is skipped (in_progress=1)"
    );
    // in_progress stays 1 (the existing verifier still holds the lock)
    assert!(
        after.verification_in_progress,
        "in_progress must remain true (existing verifier holds lock)"
    );
}

/// update_plan_artifact does NOT have auto-verify trigger logic.
/// Verifies structurally: only create_plan_artifact contains the trigger.
/// When verification is running (in_progress=1), update_plan_artifact must NOT reset or
/// re-trigger verification — generation stays the same and in_progress stays true.
#[tokio::test]
async fn test_update_plan_artifact_does_not_trigger_auto_verify() {
    let state = setup_test_state().await;

    // Create session + plan
    let session = make_active_session();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();
    let create_result =
        create_plan_artifact_quiesced(&state, &session_id, "Plan v1", "initial").await;
    let artifact_id = create_result.id.clone();

    // Read session to get the current plan_artifact_id (create may have changed it)
    // and capture generation (may be >0 if auto_verify=true in YAML)
    let after_create = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    let gen_after_create = after_create.verification_generation;
    let latest_artifact_id = after_create
        .plan_artifact_id
        .as_ref()
        .unwrap()
        .as_str()
        .to_string();

    // Manually put session into triggered state (simulating auto-verify running)
    // Use update_verification_state to ensure in_progress=1 regardless of auto_verify setting
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&session_id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();

    // Capture generation before update (same as gen_after_create — update_verification_state
    // does not change generation)
    let before_update = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    let gen_before_update = before_update.verification_generation;
    assert!(
        before_update.verification_in_progress,
        "in_progress must be true before update"
    );

    // update_plan_artifact must NOT re-trigger (no trigger logic in update handler)
    // reset_verification_sync has in_progress=0 guard — no-op while running
    let _ = update_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(UpdatePlanArtifactRequest {
            artifact_id: latest_artifact_id,
            content: "auto-corrected plan content".to_string(),
            caller_session_id: None,
        }),
    )
    .await
    .expect("update_plan_artifact should succeed while in_progress=1");

    // Verify generation was NOT incremented by update_plan_artifact
    let after_update = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        after_update.verification_generation, gen_before_update,
        "generation must not be incremented by update_plan_artifact (before={}, after={})",
        gen_before_update, after_update.verification_generation
    );
    // in_progress must remain true (reset_verification_sync is no-op when in_progress=1)
    assert!(
        after_update.verification_in_progress,
        "in_progress must remain true after update_plan_artifact while verification is running"
    );
    let _ = (gen_after_create, artifact_id); // suppress unused warnings
}

/// update_plan_artifact: when proposals are linked to old artifact, they are batch-updated
/// to the new artifact ID after update_plan_artifact runs.
#[tokio::test]
async fn test_update_plan_artifact_batch_updates_linked_proposals() {
    use ralphx_lib::domain::entities::{
        Complexity, Priority, ProposalCategory, ProposalStatus, TaskProposal, TaskProposalId,
    };

    let state = setup_test_state().await;
    let (_, artifact_id) = create_parent_with_plan(&state).await;

    // Create and link 5 proposals to the initial artifact
    let mut proposal_ids = Vec::new();
    for i in 0..5usize {
        let proposal = TaskProposal {
            id: TaskProposalId::new(),
            session_id: IdeationSessionId::new(),
            title: format!("Linked Proposal {i}"),
            description: None,
            category: ProposalCategory::Feature,
            steps: None,
            acceptance_criteria: None,
            suggested_priority: Priority::Medium,
            priority_score: 50,
            priority_reason: None,
            priority_factors: None,
            estimated_complexity: Complexity::Moderate,
            user_priority: None,
            user_modified: false,
            status: ProposalStatus::Pending,
            selected: false,
            created_task_id: None,
            plan_artifact_id: None,
            plan_version_at_creation: None,
            blueprint_artifact_id: None,
            blueprint_version_at_creation: None,
            sort_order: i as i32,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            archived_at: None,
            target_project: None,
            migrated_from_session_id: None,
            migrated_from_proposal_id: None,
            affected_paths: None,
        };
        let saved = state
            .app_state
            .task_proposal_repo
            .create(proposal)
            .await
            .unwrap();
        proposal_ids.push(saved.id.as_str().to_string());
    }

    let _ = link_proposals_to_plan(
        State(state.clone()),
        Json(LinkProposalsToPlanRequest {
            artifact_id: artifact_id.clone(),
            proposal_ids,
        }),
    )
    .await
    .expect("Initial link should succeed");

    // Update the plan artifact — proposals must be re-linked to the new artifact
    let updated = update_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(UpdatePlanArtifactRequest {
            artifact_id: artifact_id.clone(),
            content: "new plan content".to_string(),
            caller_session_id: None,
        }),
    )
    .await
    .expect("update_plan_artifact should succeed");
    let new_artifact_id = updated.0.id.clone();

    // Old artifact should have 0 proposals linked
    let old_linked = state
        .app_state
        .task_proposal_repo
        .get_by_plan_artifact_id(&ArtifactId::from_string(artifact_id.clone()))
        .await
        .unwrap();
    assert_eq!(
        old_linked.len(),
        0,
        "Old artifact should have no proposals after update"
    );

    // New artifact should have all 5 proposals linked
    let new_linked = state
        .app_state
        .task_proposal_repo
        .get_by_plan_artifact_id(&ArtifactId::from_string(new_artifact_id.clone()))
        .await
        .unwrap();
    assert_eq!(
        new_linked.len(),
        5,
        "All 5 proposals should be re-linked to the new artifact"
    );
}

// ============================================================================
// apply_edits Unit Tests
// ============================================================================

#[test]
fn test_single_edit() {
    let content = "hello world";
    let edits = vec![PlanEdit {
        old_text: "hello".to_string(),
        new_text: "goodbye".to_string(),
    }];
    let result = apply_edits(content, &edits).unwrap();
    assert_eq!(result, "goodbye world");
}

#[test]
fn test_multiple_sequential_edits() {
    let content = "one two three four";
    let edits = vec![
        PlanEdit {
            old_text: "one".to_string(),
            new_text: "a".to_string(),
        },
        PlanEdit {
            old_text: "two".to_string(),
            new_text: "b".to_string(),
        },
        PlanEdit {
            old_text: "three".to_string(),
            new_text: "c".to_string(),
        },
    ];
    let result = apply_edits(content, &edits).unwrap();
    assert_eq!(result, "a b c four");
}

#[test]
fn test_anchor_not_found() {
    let content = "hello world";
    let edits = vec![PlanEdit {
        old_text: "goodbye".to_string(),
        new_text: "foo".to_string(),
    }];
    let err = apply_edits(content, &edits).unwrap_err();
    match err {
        EditError::AnchorNotFound {
            edit_index,
            old_text_preview,
        } => {
            assert_eq!(edit_index, 0);
            assert_eq!(old_text_preview, "goodbye");
        }
        _ => panic!("Expected AnchorNotFound"),
    }
}

#[test]
fn test_ambiguous_anchor() {
    let content = "hello hello world";
    let edits = vec![PlanEdit {
        old_text: "hello".to_string(),
        new_text: "hi".to_string(),
    }];
    let err = apply_edits(content, &edits).unwrap_err();
    match err {
        EditError::AmbiguousAnchor {
            edit_index,
            old_text_preview,
        } => {
            assert_eq!(edit_index, 0);
            assert_eq!(old_text_preview, "hello");
        }
        _ => panic!("Expected AmbiguousAnchor"),
    }
}

#[test]
fn test_deletion() {
    let content = "keep this delete this keep that";
    let edits = vec![PlanEdit {
        old_text: "delete this".to_string(),
        new_text: "".to_string(),
    }];
    let result = apply_edits(content, &edits).unwrap();
    assert_eq!(result, "keep this  keep that");
}

#[test]
fn test_insertion_via_expansion() {
    let content = "## Heading\n\nContent";
    let edits = vec![PlanEdit {
        old_text: "## Heading".to_string(),
        new_text: "## Heading\n\nNew paragraph".to_string(),
    }];
    let result = apply_edits(content, &edits).unwrap();
    assert_eq!(result, "## Heading\n\nNew paragraph\n\nContent");
}

#[test]
fn test_overlapping_sequential_edits() {
    // Edit 0 changes the target of edit 1 — edit 1 searches the mutated result
    let content = "foo bar baz";
    let edits = vec![
        PlanEdit {
            old_text: "foo".to_string(),
            new_text: "hello world".to_string(),
        },
        PlanEdit {
            old_text: "bar".to_string(),
            new_text: "goodbye".to_string(),
        },
    ];
    let result = apply_edits(content, &edits).unwrap();
    assert_eq!(result, "hello world goodbye baz");
}

#[test]
fn test_phantom_match_by_design() {
    // Edit 0 introduces text that matches edit 1's old_text
    // Edit 1 operates on the introduced text (sequential semantics)
    let content = "one two";
    let edits = vec![
        PlanEdit {
            old_text: "one".to_string(),
            new_text: "phantom".to_string(),
        },
        PlanEdit {
            old_text: "phantom".to_string(),
            new_text: "replaced".to_string(),
        },
    ];
    let result = apply_edits(content, &edits).unwrap();
    assert_eq!(result, "replaced two");
}

#[test]
fn test_phantom_ambiguity() {
    // Edit 0's new_text creates a SECOND occurrence of edit 1's old_text
    // This should return AmbiguousAnchor
    let content = "duplicate here";
    let edits = vec![
        PlanEdit {
            old_text: "here".to_string(),
            new_text: "duplicate here again".to_string(),
        },
        PlanEdit {
            old_text: "duplicate".to_string(),
            new_text: "replaced".to_string(),
        },
    ];
    let err = apply_edits(content, &edits).unwrap_err();
    match err {
        EditError::AmbiguousAnchor { edit_index, .. } => {
            assert_eq!(edit_index, 1);
        }
        _ => panic!("Expected AmbiguousAnchor for edit 1"),
    }
}

#[test]
fn test_large_content() {
    let mut content = "#".repeat(10_000);
    content.push_str("\nTARGET\n");
    let edits = vec![PlanEdit {
        old_text: "\nTARGET\n".to_string(),
        new_text: "\nREPLACED\n".to_string(),
    }];
    let result = apply_edits(&content, &edits).unwrap();
    assert!(result.contains("\nREPLACED\n"));
    assert!(!result.contains("\nTARGET\n"));
    assert_eq!(result.len(), 10_000 + 10); // 10K + "\nREPLACED\n"
}

#[test]
fn test_unicode_content() {
    let content = "Hello 🌍 世界";
    let edits = vec![
        PlanEdit {
            old_text: "🌍".to_string(),
            new_text: "🌎".to_string(),
        },
        PlanEdit {
            old_text: "世界".to_string(),
            new_text: "World".to_string(),
        },
    ];
    let result = apply_edits(content, &edits).unwrap();
    assert_eq!(result, "Hello 🌎 World");
}

// ============================================================================
// edit_plan_artifact Integration Tests
// ============================================================================

/// Test 1: Happy path — create plan, edit it. New version created, version incremented.
#[tokio::test]
async fn test_edit_plan_artifact_happy_path() {
    let state = setup_test_state().await;
    let (_, artifact_id) = create_parent_with_plan(&state).await;

    let result = edit_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(EditPlanArtifactRequest {
            artifact_id: artifact_id.clone(),
            edits: vec![PlanEdit {
                old_text: "Parent plan content".to_string(),
                new_text: "Updated plan content".to_string(),
            }],
            caller_session_id: None,
        }),
    )
    .await;

    assert!(result.is_ok(), "edit_plan_artifact should succeed");
    let response = result.unwrap().0;
    assert_eq!(response.version, 2, "Version should be incremented to 2");
    assert_eq!(response.content, "Updated plan content");
    assert_eq!(
        response.previous_artifact_id,
        Some(artifact_id),
        "previous_artifact_id should be the pre-edit artifact ID"
    );
}

/// Test 2: Stale ID resolution — edit with original ID after update resolves to latest.
#[tokio::test]
async fn test_edit_plan_artifact_resolves_stale_id() {
    let state = setup_test_state().await;
    let (_, original_artifact_id) = create_parent_with_plan(&state).await;

    // First update to create a stale original ID
    let update_result = update_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(UpdatePlanArtifactRequest {
            artifact_id: original_artifact_id.clone(),
            content: "v2 content with unique anchor phrase here".to_string(),
            caller_session_id: None,
        }),
    )
    .await
    .expect("update_plan_artifact should succeed");
    let v2_id = update_result.0.id.clone();
    assert_ne!(v2_id, original_artifact_id);

    // Edit using the ORIGINAL (stale) artifact ID — should auto-resolve to v2
    let result = edit_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(EditPlanArtifactRequest {
            artifact_id: original_artifact_id.clone(), // stale ID
            edits: vec![PlanEdit {
                old_text: "unique anchor phrase here".to_string(),
                new_text: "replaced anchor phrase".to_string(),
            }],
            caller_session_id: None,
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "edit_plan_artifact with stale ID should auto-resolve and succeed"
    );
    let response = result.unwrap().0;
    assert_eq!(response.version, 3, "Should be at version 3 (v1 → v2 → v3)");
    assert_eq!(
        response.previous_artifact_id,
        Some(v2_id),
        "previous_artifact_id should be the resolved (latest) artifact, not the original stale ID"
    );
}

/// Test 3: Inherited plan guard — artifact referenced only as inherited cannot be edited.
#[tokio::test]
async fn test_edit_plan_artifact_rejects_inherited_plan() {
    let state = setup_test_state().await;

    // Create orphan artifact (no session owns it via plan_artifact_id)
    let orphan = Artifact::new_inline(
        "Inherited Plan",
        ArtifactType::Specification,
        "Inherited plan content that is unique enough for anchoring",
        "orchestrator",
    );
    let orphan_id = orphan.id.as_str().to_string();
    state.app_state.artifact_repo.create(orphan).await.unwrap();

    // Child session references it as inherited only (plan_artifact_id = None)
    let mut child = make_active_session();
    child.plan_artifact_id = None;
    child.inherited_plan_artifact_id = Some(ArtifactId::from_string(orphan_id.clone()));
    state
        .app_state
        .ideation_session_repo
        .create(child)
        .await
        .unwrap();

    let result = edit_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(EditPlanArtifactRequest {
            artifact_id: orphan_id.clone(),
            edits: vec![PlanEdit {
                old_text: "Inherited plan content that is unique enough for anchoring".to_string(),
                new_text: "Should be rejected".to_string(),
            }],
            caller_session_id: None,
        }),
    )
    .await;

    assert!(
        result.is_err(),
        "edit_plan_artifact on inherited-only artifact should fail"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "Should return 422, got {:?}",
        err.status
    );
    let msg = err.message.expect("422 should include message body");
    assert!(
        msg.contains("Cannot edit inherited plan"),
        "Error must mention inherited plan: got '{msg}'"
    );
    assert!(
        msg.contains("create_plan_artifact"),
        "Error must direct to create_plan_artifact: got '{msg}'"
    );
}

/// Test 4: Session archived guard — plan owned by an archived session cannot be edited.
#[tokio::test]
async fn test_edit_plan_artifact_rejects_archived_session() {
    let state = setup_test_state().await;

    // Create inline artifact directly (bypassing handler so we can set session.status = Archived)
    let artifact = Artifact::new_inline(
        "Archived Session Plan",
        ArtifactType::Specification,
        "Content that belongs to an archived session uniquely",
        "orchestrator",
    );
    let artifact_id = artifact.id.as_str().to_string();
    state
        .app_state
        .artifact_repo
        .create(artifact)
        .await
        .unwrap();

    // Session owns the artifact but is Archived
    let mut session = make_active_session();
    session.status = IdeationSessionStatus::Archived;
    session.plan_artifact_id = Some(ArtifactId::from_string(artifact_id.clone()));
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = edit_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(EditPlanArtifactRequest {
            artifact_id,
            edits: vec![PlanEdit {
                old_text: "Content that belongs to an archived session uniquely".to_string(),
                new_text: "Should not be applied".to_string(),
            }],
            caller_session_id: None,
        }),
    )
    .await;

    assert!(
        result.is_err(),
        "edit_plan_artifact on archived session should fail"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "Should return 422, got {:?}",
        err.status
    );
}

/// Test 5: File-backed artifact guard — file-backed artifacts cannot be edited.
#[tokio::test]
async fn test_edit_plan_artifact_rejects_file_backed_artifact() {
    let state = setup_test_state().await;

    // Create a file-backed artifact directly via repo
    let artifact = Artifact {
        id: ArtifactId::new(),
        artifact_type: ArtifactType::Specification,
        name: "File-backed Plan".to_string(),
        content: ArtifactContent::File {
            path: "/tmp/plan.md".to_string(),
        },
        metadata: ArtifactMetadata::new("orchestrator").with_version(1),
        derived_from: vec![],
        bucket_id: None,
        archived_at: None,
    };
    let artifact_id = artifact.id.as_str().to_string();
    state
        .app_state
        .artifact_repo
        .create(artifact)
        .await
        .unwrap();

    // Active session owns the file-backed artifact
    let mut session = make_active_session();
    session.plan_artifact_id = Some(ArtifactId::from_string(artifact_id.clone()));
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = edit_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(EditPlanArtifactRequest {
            artifact_id,
            edits: vec![PlanEdit {
                old_text: "any text".to_string(),
                new_text: "replacement".to_string(),
            }],
            caller_session_id: None,
        }),
    )
    .await;

    assert!(
        result.is_err(),
        "edit_plan_artifact on file-backed artifact should fail"
    );
    let err = result.unwrap_err();
    assert_eq!(
        err.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "Should return 422, got {:?}",
        err.status
    );
    let msg = err.message.expect("422 should include message body");
    assert!(
        msg.contains("file-backed artifact"),
        "Error must mention file-backed artifact: got '{msg}'"
    );
    assert!(
        msg.contains("update_plan_artifact"),
        "Error must suggest update_plan_artifact: got '{msg}'"
    );
}

/// Test 6: Verification reset — editing plan resets verification when not in_progress.
#[tokio::test]
async fn test_edit_plan_artifact_resets_verification() {
    let state = setup_test_state().await;
    let (parent_session_id, artifact_id) = create_parent_with_plan(&state).await;

    // Set verification status to Verified (not in_progress)
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&parent_session_id, VerificationStatus::Verified, false)
        .await
        .unwrap();

    let before = state
        .app_state
        .ideation_session_repo
        .get_by_id(&parent_session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before.verification_status, VerificationStatus::Verified);
    assert!(!before.verification_in_progress);
    let gen_before = before.verification_generation;

    // Edit the plan
    let _ = edit_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(EditPlanArtifactRequest {
            artifact_id,
            edits: vec![PlanEdit {
                old_text: "Parent plan content".to_string(),
                new_text: "Edited content".to_string(),
            }],
            caller_session_id: None,
        }),
    )
    .await
    .expect("edit_plan_artifact should succeed");

    // Verification must be reset to Unverified
    let after = state
        .app_state
        .ideation_session_repo
        .get_by_id(&parent_session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        after.verification_status,
        VerificationStatus::Unverified,
        "Verification should be reset to Unverified after editing plan"
    );
    assert!(
        !after.verification_in_progress,
        "in_progress should remain false"
    );
    assert_eq!(
        after.verification_generation,
        gen_before + 1,
        "plan edits that invalidate finished verification must increment generation"
    );
}

/// Test 7: Verification preserved during loop — edit while in_progress=1 does NOT reset.
#[tokio::test]
async fn test_edit_plan_artifact_preserves_verification_during_loop() {
    let state = setup_test_state().await;
    let (parent_session_id, artifact_id) = create_parent_with_plan(&state).await;

    // Simulate active verification loop (in_progress=1)
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&parent_session_id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();

    let before = state
        .app_state
        .ideation_session_repo
        .get_by_id(&parent_session_id)
        .await
        .unwrap()
        .unwrap();
    let gen_before = before.verification_generation;
    assert!(before.verification_in_progress);

    // Edit plan while verification loop is running
    let _ = edit_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(EditPlanArtifactRequest {
            artifact_id,
            edits: vec![PlanEdit {
                old_text: "Parent plan content".to_string(),
                new_text: "Auto-corrected content".to_string(),
            }],
            caller_session_id: None,
        }),
    )
    .await
    .expect("edit_plan_artifact should succeed even while verification is running");

    // Verification state must be UNCHANGED (CAS guard prevents reset)
    let after = state
        .app_state
        .ideation_session_repo
        .get_by_id(&parent_session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        after.verification_generation, gen_before,
        "generation must not change while in_progress=true (before={gen_before}, after={})",
        after.verification_generation
    );
    assert!(
        after.verification_in_progress,
        "in_progress must remain true"
    );
    assert_eq!(
        after.verification_status,
        VerificationStatus::Reviewing,
        "status must remain Reviewing"
    );
}

/// Test 8: Zero edits guard — empty edits array rejected before transaction.
#[tokio::test]
async fn test_edit_plan_artifact_rejects_empty_edits() {
    let state = setup_test_state().await;
    let (_, artifact_id) = create_parent_with_plan(&state).await;

    let result = edit_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(EditPlanArtifactRequest {
            artifact_id,
            edits: vec![],

            caller_session_id: None,
        }),
    )
    .await;

    assert!(result.is_err(), "Empty edits should be rejected");
    let err = result.unwrap_err();
    assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
    let msg = err.message.expect("422 should include message body");
    assert!(
        msg.contains("edits array must not be empty"),
        "Error should mention empty edits: got '{msg}'"
    );
}

/// Test 9: Empty old_text guard — edit with old_text="" rejected before transaction.
#[tokio::test]
async fn test_edit_plan_artifact_rejects_empty_old_text() {
    let state = setup_test_state().await;
    let (_, artifact_id) = create_parent_with_plan(&state).await;

    let result = edit_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(EditPlanArtifactRequest {
            artifact_id,
            edits: vec![PlanEdit {
                old_text: "".to_string(),
                new_text: "some replacement".to_string(),
            }],
            caller_session_id: None,
        }),
    )
    .await;

    assert!(result.is_err(), "Empty old_text should be rejected");
    let err = result.unwrap_err();
    assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
    let msg = err.message.expect("422 should include message body");
    assert!(
        msg.contains("old_text must not be empty"),
        "Error should mention empty old_text: got '{msg}'"
    );
}

/// Test 10: Input size limit — old_text > 100KB rejected before transaction.
#[tokio::test]
async fn test_edit_plan_artifact_rejects_oversized_input() {
    let state = setup_test_state().await;
    let (_, artifact_id) = create_parent_with_plan(&state).await;

    let oversized_old_text = "x".repeat(100_001); // 1 byte over 100KB limit
    let result = edit_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(EditPlanArtifactRequest {
            artifact_id,
            edits: vec![PlanEdit {
                old_text: oversized_old_text,
                new_text: "replacement".to_string(),
            }],
            caller_session_id: None,
        }),
    )
    .await;

    assert!(result.is_err(), "Oversized old_text should be rejected");
    let err = result.unwrap_err();
    assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
    let msg = err.message.expect("422 should include message body");
    assert!(
        msg.contains("100KB"),
        "Error should mention 100KB limit: got '{msg}'"
    );
}

/// Test 11: Output size limit — edits growing content > 500KB rejected atomically (no partial version).
///
/// Strategy: initial content = 499KB of text + unique anchor.
/// Edit replaces anchor with 5KB → total ≈ 504KB → exceeds 500KB post-apply guard.
/// Both old_text and new_text stay well under the 100KB per-field limit.
#[tokio::test]
async fn test_edit_plan_artifact_rejects_oversized_output() {
    let state = setup_test_state().await;

    let parent = make_active_session();
    let parent_id = parent.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(parent)
        .await
        .unwrap();

    // 499KB of filler + unique anchor — total still under 500KB
    let large_content = format!("{}{}", "A".repeat(499_000), "UNIQUE_ANCHOR_FOR_SIZE_TEST");
    let create_result =
        create_plan_artifact_quiesced(&state, &parent_id, "Plan", &large_content).await;
    let artifact_id = create_result.id.clone();

    // Replace anchor with 5KB — total becomes ≈ 504KB, exceeds 500KB post-apply guard.
    // new_text is well under the 100KB per-field limit.
    let result = edit_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(EditPlanArtifactRequest {
            artifact_id: artifact_id.clone(),
            edits: vec![PlanEdit {
                old_text: "UNIQUE_ANCHOR_FOR_SIZE_TEST".to_string(),
                new_text: "Y".repeat(5_000),
            }],
            caller_session_id: None,
        }),
    )
    .await;

    assert!(result.is_err(), "Oversized result should be rejected");
    let err = result.unwrap_err();
    assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
    let msg = err.message.expect("422 should include message body");
    assert!(
        msg.contains("500KB"),
        "Error should mention 500KB limit: got '{msg}'"
    );

    // Verify no spurious new version was created (atomic rollback)
    let session_after = state
        .app_state
        .ideation_session_repo
        .get_by_id(&parent_id)
        .await
        .unwrap()
        .unwrap();
    let plan_id = session_after
        .plan_artifact_id
        .expect("Plan must still exist");
    let artifact = state
        .app_state
        .artifact_repo
        .get_by_id(&plan_id)
        .await
        .unwrap()
        .expect("Original artifact must still exist");
    assert_eq!(
        artifact.metadata.version, 1,
        "Version must remain 1 — no partial version should be created on error"
    );
}

/// Test 12: Response fields correct for event emission — previous_artifact_id and session_id populated.
#[tokio::test]
async fn test_edit_plan_artifact_response_has_correct_event_fields() {
    let state = setup_test_state().await;
    let (_, artifact_id) = create_parent_with_plan(&state).await;

    let result = edit_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(EditPlanArtifactRequest {
            artifact_id: artifact_id.clone(),
            edits: vec![PlanEdit {
                old_text: "Parent plan content".to_string(),
                new_text: "Revised plan content".to_string(),
            }],
            caller_session_id: None,
        }),
    )
    .await
    .expect("edit_plan_artifact should succeed");

    let response = result.0;
    // previous_artifact_id must be set for frontend cache invalidation
    assert_eq!(
        response.previous_artifact_id,
        Some(artifact_id.clone()),
        "previous_artifact_id must be the pre-edit artifact ID"
    );
    // New artifact ID must differ from old
    assert_ne!(
        response.id, artifact_id,
        "New artifact ID must differ from old"
    );
    // session_id must be populated (used in plan_artifact:updated event payload)
    assert!(
        response.session_id.is_some(),
        "session_id must be populated in response"
    );
}

/// Test 13: Proposals batch-updated — linked proposals point to new artifact version after edit.
#[tokio::test]
async fn test_edit_plan_artifact_batch_updates_linked_proposals() {
    use ralphx_lib::domain::entities::{
        Complexity, Priority, ProposalCategory, ProposalStatus, TaskProposal, TaskProposalId,
    };

    let state = setup_test_state().await;
    let (_, artifact_id) = create_parent_with_plan(&state).await;

    // Create 3 proposals and link them to the initial artifact
    let mut proposal_ids = Vec::new();
    for i in 0..3usize {
        let proposal = TaskProposal {
            id: TaskProposalId::new(),
            session_id: IdeationSessionId::new(),
            title: format!("Edit Proposal {i}"),
            description: None,
            category: ProposalCategory::Feature,
            steps: None,
            acceptance_criteria: None,
            suggested_priority: Priority::Medium,
            priority_score: 50,
            priority_reason: None,
            priority_factors: None,
            estimated_complexity: Complexity::Moderate,
            user_priority: None,
            user_modified: false,
            status: ProposalStatus::Pending,
            selected: false,
            created_task_id: None,
            plan_artifact_id: None,
            plan_version_at_creation: None,
            blueprint_artifact_id: None,
            blueprint_version_at_creation: None,
            sort_order: i as i32,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            archived_at: None,
            target_project: None,
            migrated_from_session_id: None,
            migrated_from_proposal_id: None,
            affected_paths: None,
        };
        let saved = state
            .app_state
            .task_proposal_repo
            .create(proposal)
            .await
            .unwrap();
        proposal_ids.push(saved.id.as_str().to_string());
    }

    let _ = link_proposals_to_plan(
        State(state.clone()),
        Json(LinkProposalsToPlanRequest {
            artifact_id: artifact_id.clone(),
            proposal_ids: proposal_ids.clone(),
        }),
    )
    .await
    .expect("Initial link should succeed");

    // Edit the plan — proposals must follow to the new version
    let edited = edit_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(EditPlanArtifactRequest {
            artifact_id: artifact_id.clone(),
            edits: vec![PlanEdit {
                old_text: "Parent plan content".to_string(),
                new_text: "Edited plan content".to_string(),
            }],
            caller_session_id: None,
        }),
    )
    .await
    .expect("edit_plan_artifact should succeed");
    let new_artifact_id = edited.0.id.clone();
    assert_ne!(new_artifact_id, artifact_id);

    // Old artifact should have 0 proposals
    let old_linked = state
        .app_state
        .task_proposal_repo
        .get_by_plan_artifact_id(&ArtifactId::from_string(artifact_id.clone()))
        .await
        .unwrap();
    assert_eq!(
        old_linked.len(),
        0,
        "Old artifact should have no proposals after edit"
    );

    // New artifact should have all 3 proposals
    let new_linked = state
        .app_state
        .task_proposal_repo
        .get_by_plan_artifact_id(&ArtifactId::from_string(new_artifact_id.clone()))
        .await
        .unwrap();
    assert_eq!(
        new_linked.len(),
        3,
        "All 3 proposals should be re-linked to the new artifact"
    );
}

// ============================================================
// Verification Freeze Tests (Phase 6)
// ============================================================
//
// 6A: lock blocks external writes during generating; caller_session_id bypasses
// 6B: SKIPPED — ralphx-plan-verifier agents are autonomous (no stdin pipes) and do NOT
//     register in InteractiveProcessRegistry. is_generating = is_running.
//     waiting_for_input cannot be distinguished from idle for verification agents.
// 6C: no verification children → Ok(())
// 6D: HTTP handler returns 409 on update_plan_artifact during freeze
// 6D': HTTP handler returns 409 on edit_plan_artifact during freeze
//
// Helper: build a verification child session for freeze tests.

fn make_verification_child(parent_id: &IdeationSessionId) -> IdeationSession {
    let mut child = make_active_session();
    child.parent_session_id = Some(parent_id.clone());
    child.session_purpose = SessionPurpose::Verification;
    child
}

/// 6A: check_verification_freeze blocks external writes when child is running,
/// and bypasses correctly when caller_session_id matches the child.
#[tokio::test]
async fn test_6a_freeze_blocks_external_writes_and_bypasses_for_caller() {
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let running_registry = Arc::new(MemoryRunningAgentRegistry::new());

    // Create parent and verification child
    let parent = make_active_session();
    let parent_id = parent.id.clone();
    session_repo.create(parent.clone()).await.unwrap();

    let child = make_verification_child(&parent_id);
    let child_id = child.id.clone();
    session_repo.create(child).await.unwrap();

    // Not running yet — should Ok
    let result = check_verification_freeze(
        std::slice::from_ref(&parent),
        None,
        running_registry.as_ref(),
        session_repo.as_ref(),
    )
    .await;
    assert!(
        result.is_ok(),
        "Should be Ok when verification child is not running"
    );

    // Mark parent as having verification in progress in the repo — freeze truth is now
    // authoritative and no longer comes from the caller-provided session struct.
    session_repo
        .update_verification_state(&parent_id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();
    let parent_verifying = session_repo.get_by_id(&parent_id).await.unwrap().unwrap();

    // Register child as generating
    running_registry
        .set_running(RunningAgentKey::new("ideation", child_id.as_str()))
        .await;

    // Without caller_session_id → 409 Conflict
    let result = check_verification_freeze(
        std::slice::from_ref(&parent_verifying),
        None,
        running_registry.as_ref(),
        session_repo.as_ref(),
    )
    .await;
    assert!(
        matches!(result, Err(AppError::Conflict(_))),
        "Should return Conflict when verification child is running and no bypass"
    );

    // With caller_session_id = child.id → bypass → Ok
    let result = check_verification_freeze(
        std::slice::from_ref(&parent_verifying),
        Some(child_id.as_str()),
        running_registry.as_ref(),
        session_repo.as_ref(),
    )
    .await;
    assert!(
        result.is_ok(),
        "Should bypass freeze when caller IS the verification child"
    );

    // Unregister (agent exited) → Ok for all callers
    running_registry
        .as_ref()
        .unregister(
            &RunningAgentKey::new("ideation", child_id.as_str()),
            "test-agent-run",
        )
        .await;
    let result = check_verification_freeze(
        std::slice::from_ref(&parent_verifying),
        None,
        running_registry.as_ref(),
        session_repo.as_ref(),
    )
    .await;
    assert!(
        result.is_ok(),
        "Should be Ok after verification child exits"
    );
}

/// 6A': freeze is released when verification_in_progress transitions to false,
/// even if the child session process is still registered as running.
#[tokio::test]
async fn test_6a_prime_freeze_released_when_verification_complete() {
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let running_registry = Arc::new(MemoryRunningAgentRegistry::new());

    // Create parent and verification child
    let parent = make_active_session();
    let parent_id = parent.id.clone();
    session_repo.create(parent.clone()).await.unwrap();

    let child = make_verification_child(&parent_id);
    let child_id = child.id.clone();
    session_repo.create(child).await.unwrap();

    // Set verification_in_progress=true and register child as running (freeze active)
    session_repo
        .update_verification_state(&parent_id, VerificationStatus::Reviewing, true)
        .await
        .unwrap();
    running_registry
        .set_running(RunningAgentKey::new("ideation", child_id.as_str()))
        .await;

    // Fetch the updated parent so check_verification_freeze sees in_progress=true
    let parent_verifying = session_repo.get_by_id(&parent_id).await.unwrap().unwrap();

    // Phase 1: freeze active → Conflict
    let result = check_verification_freeze(
        &[parent_verifying],
        None,
        running_registry.as_ref(),
        session_repo.as_ref(),
    )
    .await;
    assert!(
        matches!(result, Err(AppError::Conflict(_))),
        "Should return Conflict when verification_in_progress=true and child is running"
    );

    // Set verification_in_progress=false (verification round completed)
    session_repo
        .update_verification_state(&parent_id, VerificationStatus::Verified, false)
        .await
        .unwrap();

    // Fetch updated parent (in_progress=false now)
    let parent_completed = session_repo.get_by_id(&parent_id).await.unwrap().unwrap();

    // Phase 2: freeze released — Ok even though child is still in running registry
    let result = check_verification_freeze(
        &[parent_completed],
        None,
        running_registry.as_ref(),
        session_repo.as_ref(),
    )
    .await;
    assert!(
        result.is_ok(),
        "Should return Ok after verification_in_progress set to false"
    );
}

/// 6A-default: parent never started verification (in_progress=false by default).
/// A running verification child should NOT trigger the freeze.
#[tokio::test]
async fn test_6a_default_false_with_children() {
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let running_registry = Arc::new(MemoryRunningAgentRegistry::new());

    // Create parent with default verification_in_progress=false
    let parent = make_active_session();
    let parent_id = parent.id.clone();
    assert!(
        !parent.verification_in_progress,
        "make_active_session() should default verification_in_progress to false"
    );
    session_repo.create(parent.clone()).await.unwrap();

    // Create verification child and register it as running
    let child = make_verification_child(&parent_id);
    let child_id = child.id.clone();
    session_repo.create(child).await.unwrap();
    running_registry
        .set_running(RunningAgentKey::new("ideation", child_id.as_str()))
        .await;

    // Early-out guard: in_progress=false → freeze check is skipped → Ok
    let result = check_verification_freeze(
        &[parent],
        None,
        running_registry.as_ref(),
        session_repo.as_ref(),
    )
    .await;
    assert!(
        result.is_ok(),
        "Should return Ok when verification_in_progress=false, even with a running child"
    );
}

/// 6C: no verification children → Ok(())
#[tokio::test]
async fn test_6c_no_verification_children_returns_ok() {
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let running_registry = Arc::new(MemoryRunningAgentRegistry::new());

    // Parent with no verification children
    let parent = make_active_session();
    session_repo.create(parent.clone()).await.unwrap();

    let result = check_verification_freeze(
        &[parent],
        None,
        running_registry.as_ref(),
        session_repo.as_ref(),
    )
    .await;
    assert!(
        result.is_ok(),
        "Should return Ok when there are no verification children"
    );
}

/// Helper: build a test HttpServerState with a pre-seeded running registry.
async fn setup_freeze_state(registry: Arc<MemoryRunningAgentRegistry>) -> HttpServerState {
    let app_state = Arc::new(AppState::new_sqlite_test_with_registry(registry));
    let execution_state = Arc::new(ExecutionState::new());
    HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
    }
}

fn caller_session_header(session_id: &IdeationSessionId) -> axum::http::HeaderMap {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "x-ralphx-caller-session-id",
        axum::http::HeaderValue::from_str(session_id.as_str()).unwrap(),
    );
    headers
}

async fn save_current_verification_snapshot(
    state: &HttpServerState,
    parent_id: &IdeationSessionId,
    status: VerificationStatus,
    in_progress: bool,
    current_round: u32,
    max_rounds: u32,
) {
    let generation = state
        .app_state
        .ideation_session_repo
        .get_by_id(parent_id)
        .await
        .unwrap()
        .unwrap()
        .verification_generation;
    state
        .app_state
        .ideation_session_repo
        .save_verification_run_snapshot(
            parent_id,
            &VerificationRunSnapshot {
                generation,
                status,
                in_progress,
                current_round,
                max_rounds,
                best_round_index: None,
                convergence_reason: None,
                current_gaps: vec![],
                rounds: vec![],
            },
        )
        .await
        .unwrap();
}

/// 6D: update_plan_artifact returns 409 during freeze; 200 with caller_session_id bypass.
#[tokio::test]
async fn test_6d_update_plan_artifact_returns_409_during_freeze() {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let state = setup_freeze_state(Arc::clone(&registry)).await;

    // Create parent with a plan artifact
    let (parent_id, artifact_id) = create_parent_with_plan(&state).await;

    // Create verification child
    let mut child = make_active_session();
    child.parent_session_id = Some(parent_id.clone());
    child.session_purpose = SessionPurpose::Verification;
    let child_id = child.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(child)
        .await
        .unwrap();

    // Mark parent as having verification in progress (as happens in production when
    // verification starts). The early-out guard skips the freeze check when false.
    save_current_verification_snapshot(
        &state,
        &parent_id,
        VerificationStatus::Reviewing,
        true,
        1,
        5,
    )
    .await;

    // Register child as running (freeze active)
    registry
        .set_running(RunningAgentKey::new("ideation", child_id.as_str()))
        .await;

    // update_plan_artifact WITHOUT caller_session_id → 409
    let result = update_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(UpdatePlanArtifactRequest {
            artifact_id: artifact_id.clone(),
            content: "attempted overwrite during freeze".to_string(),
            caller_session_id: None,
        }),
    )
    .await;
    assert!(result.is_err(), "Should fail during freeze");
    let err = result.unwrap_err();
    assert_eq!(
        err.status,
        StatusCode::CONFLICT,
        "Expected 409 Conflict, got {}",
        err.status
    );

    // update_plan_artifact WITH caller_session_id = child.id → 200
    let result = update_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(UpdatePlanArtifactRequest {
            artifact_id: artifact_id.clone(),
            content: "verifier update — allowed".to_string(),
            caller_session_id: Some(child_id.as_str().to_string()),
        }),
    )
    .await;
    assert!(
        result.is_ok(),
        "Verification agent should be allowed to update its own plan: {:?}",
        result.err()
    );

    // Phase 3: set in_progress=false (verification complete) → freeze released
    save_current_verification_snapshot(
        &state,
        &parent_id,
        VerificationStatus::Verified,
        false,
        0,
        0,
    )
    .await;

    // update_plan_artifact WITHOUT caller_session_id → 200 (freeze released)
    let result = update_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(UpdatePlanArtifactRequest {
            artifact_id: artifact_id.clone(),
            content: "update after freeze released".to_string(),
            caller_session_id: None,
        }),
    )
    .await;
    assert!(
        result.is_ok(),
        "Should succeed after verification_in_progress set to false: {:?}",
        result.err()
    );
}

/// 6D': edit_plan_artifact returns 409 during freeze; 200 with caller_session_id bypass.
#[tokio::test]
async fn test_6d_prime_edit_plan_artifact_returns_409_during_freeze() {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let state = setup_freeze_state(Arc::clone(&registry)).await;

    // Create parent with a plan artifact
    let (parent_id, artifact_id) = create_parent_with_plan(&state).await;

    // Create verification child
    let mut child = make_active_session();
    child.parent_session_id = Some(parent_id.clone());
    child.session_purpose = SessionPurpose::Verification;
    let child_id = child.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(child)
        .await
        .unwrap();

    // Mark parent as having verification in progress (as happens in production when
    // verification starts). The early-out guard skips the freeze check when false.
    save_current_verification_snapshot(
        &state,
        &parent_id,
        VerificationStatus::Reviewing,
        true,
        1,
        5,
    )
    .await;

    // Register child as running (freeze active)
    registry
        .set_running(RunningAgentKey::new("ideation", child_id.as_str()))
        .await;

    // edit_plan_artifact WITHOUT caller_session_id → 409
    let result = edit_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(EditPlanArtifactRequest {
            artifact_id: artifact_id.clone(),
            edits: vec![PlanEdit {
                old_text: "Parent plan content".to_string(),
                new_text: "overwrite attempt".to_string(),
            }],
            caller_session_id: None,
        }),
    )
    .await;
    assert!(result.is_err(), "Should fail during freeze");
    let err = result.unwrap_err();
    assert_eq!(
        err.status,
        StatusCode::CONFLICT,
        "Expected 409 Conflict, got {}",
        err.status
    );

    // edit_plan_artifact WITH caller_session_id = child.id → 200
    let result = edit_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(EditPlanArtifactRequest {
            artifact_id: artifact_id.clone(),
            edits: vec![PlanEdit {
                old_text: "Parent plan content".to_string(),
                new_text: "verifier edit — allowed".to_string(),
            }],
            caller_session_id: Some(child_id.as_str().to_string()),
        }),
    )
    .await;
    assert!(
        result.is_ok(),
        "Verification agent should be allowed to edit its own plan: {:?}",
        result.err()
    );

    // Phase 3: set in_progress=false (verification complete) → freeze released
    save_current_verification_snapshot(
        &state,
        &parent_id,
        VerificationStatus::Verified,
        false,
        0,
        0,
    )
    .await;

    // edit_plan_artifact WITHOUT caller_session_id → 200 (freeze released)
    let result = edit_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(EditPlanArtifactRequest {
            artifact_id: artifact_id.clone(),
            edits: vec![PlanEdit {
                old_text: "verifier edit — allowed".to_string(),
                new_text: "edit after freeze released".to_string(),
            }],
            caller_session_id: None,
        }),
    )
    .await;
    assert!(
        result.is_ok(),
        "Should succeed after verification_in_progress set to false: {:?}",
        result.err()
    );
}

/// Freeze must still apply when the parent summary is stale but the active-generation snapshot
/// says verification is still running.
#[tokio::test]
async fn test_6d_update_plan_artifact_uses_snapshot_truth_when_summary_is_stale() {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let state = setup_freeze_state(Arc::clone(&registry)).await;

    let (parent_id, artifact_id) = create_parent_with_plan(&state).await;

    let mut child = make_active_session();
    child.parent_session_id = Some(parent_id.clone());
    child.session_purpose = SessionPurpose::Verification;
    let child_id = child.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(child)
        .await
        .unwrap();

    save_current_verification_snapshot(
        &state,
        &parent_id,
        VerificationStatus::Reviewing,
        true,
        2,
        5,
    )
    .await;

    state
        .app_state
        .ideation_session_repo
        .update_verification_state(&parent_id, VerificationStatus::Unverified, false)
        .await
        .unwrap();

    registry
        .set_running(RunningAgentKey::new("ideation", child_id.as_str()))
        .await;

    let result = update_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(UpdatePlanArtifactRequest {
            artifact_id: artifact_id.clone(),
            content: "attempted overwrite during stale-summary freeze".to_string(),
            caller_session_id: None,
        }),
    )
    .await;

    assert!(
        result.is_err(),
        "snapshot-backed in-progress verification must still freeze plan edits"
    );
    let err = result.unwrap_err();
    assert_eq!(err.status, StatusCode::CONFLICT);
}

/// 6D'': update_plan_artifact also honors the transport-owned caller-session header.
#[tokio::test]
async fn test_6d_double_prime_update_plan_artifact_header_bypasses_freeze() {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let state = setup_freeze_state(Arc::clone(&registry)).await;

    let (parent_id, artifact_id) = create_parent_with_plan(&state).await;

    let mut child = make_active_session();
    child.parent_session_id = Some(parent_id.clone());
    child.session_purpose = SessionPurpose::Verification;
    let child_id = child.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(child)
        .await
        .unwrap();

    save_current_verification_snapshot(
        &state,
        &parent_id,
        VerificationStatus::Reviewing,
        true,
        1,
        5,
    )
    .await;

    registry
        .set_running(RunningAgentKey::new("ideation", child_id.as_str()))
        .await;

    let result = update_plan_artifact(
        State(state.clone()),
        caller_session_header(&child_id),
        Json(UpdatePlanArtifactRequest {
            artifact_id: artifact_id.clone(),
            content: "header-based verifier update".to_string(),
            caller_session_id: None,
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "transport-owned caller header should bypass the freeze without body caller_session_id: {:?}",
        result.err()
    );
}

/// 6D''': edit_plan_artifact also honors the transport-owned caller-session header.
#[tokio::test]
async fn test_6d_triple_prime_edit_plan_artifact_header_bypasses_freeze() {
    let registry = Arc::new(MemoryRunningAgentRegistry::new());
    let state = setup_freeze_state(Arc::clone(&registry)).await;

    let (parent_id, artifact_id) = create_parent_with_plan(&state).await;

    let mut child = make_active_session();
    child.parent_session_id = Some(parent_id.clone());
    child.session_purpose = SessionPurpose::Verification;
    let child_id = child.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(child)
        .await
        .unwrap();

    save_current_verification_snapshot(
        &state,
        &parent_id,
        VerificationStatus::Reviewing,
        true,
        1,
        5,
    )
    .await;

    registry
        .set_running(RunningAgentKey::new("ideation", child_id.as_str()))
        .await;

    let result = edit_plan_artifact(
        State(state.clone()),
        caller_session_header(&child_id),
        Json(EditPlanArtifactRequest {
            artifact_id: artifact_id.clone(),
            edits: vec![PlanEdit {
                old_text: "Parent plan content".to_string(),
                new_text: "header-based verifier edit".to_string(),
            }],
            caller_session_id: None,
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "transport-owned caller header should bypass the freeze without body caller_session_id: {:?}",
        result.err()
    );
}

// ============================================================
// Origin-based verification policy
// ============================================================

#[tokio::test]
async fn test_external_origin_plan_creation_waits_for_acceptance_before_auto_verification() {
    let state = setup_test_state().await;

    // Create an External-origin session
    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .origin(SessionOrigin::External)
        .build();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    // External origin must not bypass the acceptance-bound trigger policy.
    let result = create_plan_artifact(
        State(state.clone()),
        Json(CreatePlanArtifactRequest {
            session_id: session_id.as_str().to_string(),
            title: "External Plan".to_string(),
            content: "Plan content from external agent".to_string(),
            blueprint_title: None,
            blueprint_content: Some("# Implementation Blueprint".to_string()),
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "create_plan_artifact should succeed for external session: {:?}",
        result.err()
    );

    let updated = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .expect("session must still exist");

    assert_eq!(updated.verification_generation, 0);
    assert_eq!(updated.verification_status, VerificationStatus::Unverified);
    assert!(!updated.verification_in_progress);
    assert_eq!(updated.verification_confirmation_status, None);
    assert!(updated.pending_initial_prompt.is_none());
}

#[tokio::test]
async fn test_planning_flow_session_does_not_auto_verify_or_prompt_on_plan_create() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .session_flow(IdeationSessionFlow::Planning)
        .build();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let result = create_plan_artifact(
        State(state.clone()),
        Json(CreatePlanArtifactRequest {
            session_id: session_id.as_str().to_string(),
            title: "Agent Plan".to_string(),
            content: "Plan-mode content".to_string(),
            blueprint_title: None,
            blueprint_content: Some("# Implementation Blueprint".to_string()),
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "create_plan_artifact should succeed for planning-flow session: {:?}",
        result.err()
    );

    let updated = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .expect("session must still exist");
    assert_eq!(updated.verification_generation, 0);
    assert_eq!(updated.verification_status, VerificationStatus::Unverified);
    assert!(!updated.verification_in_progress);
    assert_eq!(updated.verification_confirmation_status, None);

    let children = state
        .app_state
        .ideation_session_repo
        .get_verification_children(&session_id)
        .await
        .unwrap();
    assert!(
        children.is_empty(),
        "planning-flow sessions should not spawn verification children on plan creation"
    );
}

#[tokio::test]
async fn test_planning_flow_plan_starts_draft_then_approves_current_artifact() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .session_flow(IdeationSessionFlow::Planning)
        .build();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let created = create_plan_artifact(
        State(state.clone()),
        Json(CreatePlanArtifactRequest {
            session_id: session_id.as_str().to_string(),
            title: "Agent Plan".to_string(),
            content: "Plan-mode content".to_string(),
            blueprint_title: None,
            blueprint_content: Some("# Implementation Blueprint".to_string()),
        }),
    )
    .await
    .expect("create_plan_artifact should succeed")
    .0;
    assert_eq!(created.plan_approval_status.as_deref(), Some("draft"));

    let approved = approve_plan_artifact(
        State(state.clone()),
        Json(ApprovePlanArtifactRequest {
            session_id: session_id.as_str().to_string(),
            artifact_id: Some(created.id.clone()),
            blueprint_artifact_id: created
                .blueprint_artifact
                .as_ref()
                .map(|artifact| artifact.id.clone()),
            blueprint_artifact_version: created
                .blueprint_artifact
                .as_ref()
                .map(|artifact| artifact.version),
        }),
    )
    .await
    .expect("approve_plan_artifact should succeed for planning sessions")
    .0;

    assert_eq!(approved.id, created.id);
    assert_eq!(approved.version, created.version);
    assert_eq!(approved.plan_approval_status.as_deref(), Some("approved"));
    assert_eq!(
        approved.plan_approved_artifact_id.as_deref(),
        Some(created.id.as_str())
    );
    assert_eq!(approved.plan_approved_version, Some(created.version));
    assert!(
        approved.plan_approved_at.is_some(),
        "approved plans should expose the approval timestamp"
    );

    let current = get_session_plan(State(state.clone()), Path(session_id.as_str().to_string()))
        .await
        .expect("get_session_plan should succeed")
        .0
        .expect("planning session should have a plan");
    assert_eq!(current.plan_approval_status.as_deref(), Some("approved"));
    assert_eq!(
        current.plan_approved_artifact_id.as_deref(),
        Some(created.id.as_str())
    );
    assert_eq!(current.plan_approved_version, Some(created.version));
}

#[tokio::test]
async fn test_submit_plan_complexity_assessment_persists_for_current_approved_plan() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .session_flow(IdeationSessionFlow::Planning)
        .build();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let created = create_plan_artifact(
        State(state.clone()),
        Json(CreatePlanArtifactRequest {
            session_id: session_id.as_str().to_string(),
            title: "Agent Plan".to_string(),
            content: "Plan-mode content".to_string(),
            blueprint_title: None,
            blueprint_content: Some("# Implementation Blueprint".to_string()),
        }),
    )
    .await
    .expect("create_plan_artifact should succeed")
    .0;

    let _ = approve_plan_artifact(
        State(state.clone()),
        Json(ApprovePlanArtifactRequest {
            session_id: session_id.as_str().to_string(),
            artifact_id: Some(created.id.clone()),
            blueprint_artifact_id: created
                .blueprint_artifact
                .as_ref()
                .map(|artifact| artifact.id.clone()),
            blueprint_artifact_version: created
                .blueprint_artifact
                .as_ref()
                .map(|artifact| artifact.version),
        }),
    )
    .await
    .expect("approve_plan_artifact should succeed");

    let submitted = submit_plan_complexity_assessment(
        State(state.clone()),
        Json(SubmitPlanComplexityAssessmentRequest {
            session_id: session_id.as_str().to_string(),
            artifact_id: created.id.clone(),
            artifact_version: created.version,
            blueprint_artifact_id: created
                .blueprint_artifact
                .as_ref()
                .map(|artifact| artifact.id.clone()),
            blueprint_artifact_version: created
                .blueprint_artifact
                .as_ref()
                .map(|artifact| artifact.version),
            level: "complex".to_string(),
            score: 84,
            recommended_action: "create_proposals".to_string(),
            confidence: 0.91,
            reason_summary: "Multiple dependent work items need tracked checkpoints.".to_string(),
            signals: Some(serde_json::json!({
                "dependent_work_items": 4,
                "cross_layer_scope": true
            })),
        }),
    )
    .await
    .expect("submit_plan_complexity_assessment should succeed")
    .0;

    assert!(submitted.success);
    assert_eq!(submitted.assessment.session_id, session_id.as_str());
    assert_eq!(submitted.assessment.artifact_id, created.id);
    assert_eq!(submitted.assessment.artifact_version, created.version);
    assert_eq!(submitted.assessment.level, "complex");
    assert_eq!(submitted.assessment.score, 84);
    assert_eq!(submitted.assessment.recommended_action, "create_proposals");
    assert_eq!(
        submitted.assessment.assessed_by,
        "ralphx-utility-plan-complexity"
    );

    let fetched =
        get_plan_complexity_assessment(State(state.clone()), Path(session_id.as_str().to_string()))
            .await
            .expect("get_plan_complexity_assessment should succeed")
            .0
            .expect("assessment should be persisted for the current plan");
    assert_eq!(fetched.id, submitted.assessment.id);
    assert_eq!(
        fetched.signals["dependent_work_items"],
        serde_json::json!(4)
    );
}

#[tokio::test]
async fn test_submit_plan_complexity_assessment_rejects_draft_plan() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .session_flow(IdeationSessionFlow::Planning)
        .build();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let created = create_plan_artifact(
        State(state.clone()),
        Json(CreatePlanArtifactRequest {
            session_id: session_id.as_str().to_string(),
            title: "Agent Plan".to_string(),
            content: "Plan-mode content".to_string(),
            blueprint_title: None,
            blueprint_content: Some("# Implementation Blueprint".to_string()),
        }),
    )
    .await
    .expect("create_plan_artifact should succeed")
    .0;

    let result = submit_plan_complexity_assessment(
        State(state.clone()),
        Json(SubmitPlanComplexityAssessmentRequest {
            session_id: session_id.as_str().to_string(),
            artifact_id: created.id.clone(),
            artifact_version: created.version,
            blueprint_artifact_id: created
                .blueprint_artifact
                .as_ref()
                .map(|artifact| artifact.id.clone()),
            blueprint_artifact_version: created
                .blueprint_artifact
                .as_ref()
                .map(|artifact| artifact.version),
            level: "simple".to_string(),
            score: 20,
            recommended_action: "implement_directly".to_string(),
            confidence: 0.8,
            reason_summary: "Linear small plan.".to_string(),
            signals: None,
        }),
    )
    .await;

    assert!(
        result.is_err(),
        "draft plans should not accept complexity assessments"
    );
    let err = result.unwrap_err();
    assert_eq!(err.status, StatusCode::CONFLICT);
    let msg = err.message.expect("409 should include message body");
    assert!(
        msg.contains("current approved plan version"),
        "error should explain approval requirement: got '{msg}'"
    );
}

#[tokio::test]
async fn test_approve_plan_artifact_rejects_non_planning_session() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .session_flow(IdeationSessionFlow::Ideation)
        .build();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let created =
        create_plan_artifact_quiesced(&state, &session_id, "Ideation Plan", "Ideation content")
            .await;

    let result = approve_plan_artifact(
        State(state.clone()),
        Json(ApprovePlanArtifactRequest {
            session_id: session_id.as_str().to_string(),
            artifact_id: Some(created.id.clone()),
            blueprint_artifact_id: None,
            blueprint_artifact_version: None,
        }),
    )
    .await;

    assert!(
        result.is_err(),
        "approve_plan_artifact should reject full ideation sessions"
    );
    let err = result.unwrap_err();
    assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
    let msg = err.message.expect("422 should include message body");
    assert!(
        msg.contains("Plan approval is only available for planning sessions"),
        "error should explain the planning-session guard: got '{msg}'"
    );
}

#[tokio::test]
async fn test_approve_plan_artifact_rejects_stale_artifact_id() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .session_flow(IdeationSessionFlow::Planning)
        .build();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let v1 = create_plan_artifact(
        State(state.clone()),
        Json(CreatePlanArtifactRequest {
            session_id: session_id.as_str().to_string(),
            title: "Agent Plan".to_string(),
            content: "Plan v1".to_string(),
            blueprint_title: None,
            blueprint_content: Some("# Implementation Blueprint".to_string()),
        }),
    )
    .await
    .expect("create_plan_artifact should succeed")
    .0;

    let v2 = update_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(UpdatePlanArtifactRequest {
            artifact_id: v1.id.clone(),
            content: "Plan v2".to_string(),
            caller_session_id: None,
        }),
    )
    .await
    .expect("update_plan_artifact should succeed")
    .0;
    assert_ne!(v1.id, v2.id);

    let result = approve_plan_artifact(
        State(state.clone()),
        Json(ApprovePlanArtifactRequest {
            session_id: session_id.as_str().to_string(),
            artifact_id: Some(v1.id),
            blueprint_artifact_id: None,
            blueprint_artifact_version: None,
        }),
    )
    .await;

    assert!(result.is_err(), "approving a stale artifact id should fail");
    let err = result.unwrap_err();
    assert_eq!(err.status, StatusCode::CONFLICT);
    let msg = err.message.expect("409 should include message body");
    assert!(
        msg.contains("Plan changed before approval"),
        "error should explain stale approval: got '{msg}'"
    );
}

#[tokio::test]
async fn test_plan_update_after_approval_returns_current_plan_to_draft() {
    let state = setup_test_state().await;

    let session = IdeationSessionBuilder::new()
        .project_id(ProjectId::new())
        .session_flow(IdeationSessionFlow::Planning)
        .build();
    let session_id = session.id.clone();
    state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();

    let created = create_plan_artifact(
        State(state.clone()),
        Json(CreatePlanArtifactRequest {
            session_id: session_id.as_str().to_string(),
            title: "Agent Plan".to_string(),
            content: "Plan v1".to_string(),
            blueprint_title: None,
            blueprint_content: Some("# Implementation Blueprint".to_string()),
        }),
    )
    .await
    .expect("create_plan_artifact should succeed")
    .0;

    let approved = approve_plan_artifact(
        State(state.clone()),
        Json(ApprovePlanArtifactRequest {
            session_id: session_id.as_str().to_string(),
            artifact_id: Some(created.id.clone()),
            blueprint_artifact_id: created
                .blueprint_artifact
                .as_ref()
                .map(|artifact| artifact.id.clone()),
            blueprint_artifact_version: created
                .blueprint_artifact
                .as_ref()
                .map(|artifact| artifact.version),
        }),
    )
    .await
    .expect("approve_plan_artifact should succeed")
    .0;
    assert_eq!(approved.plan_approval_status.as_deref(), Some("approved"));

    let updated = update_plan_artifact(
        State(state.clone()),
        axum::http::HeaderMap::new(),
        Json(UpdatePlanArtifactRequest {
            artifact_id: created.id.clone(),
            content: "Plan v2".to_string(),
            caller_session_id: None,
        }),
    )
    .await
    .expect("update_plan_artifact should succeed")
    .0;

    assert_ne!(updated.id, created.id);
    assert_eq!(updated.plan_approval_status.as_deref(), Some("draft"));
    assert_eq!(updated.plan_approved_artifact_id, None);
    assert_eq!(updated.plan_approved_version, None);
    assert_eq!(updated.plan_approved_at, None);

    let current = get_session_plan(State(state.clone()), Path(session_id.as_str().to_string()))
        .await
        .expect("get_session_plan should succeed")
        .0
        .expect("planning session should have a plan");
    assert_eq!(current.id, updated.id);
    assert_eq!(current.plan_approval_status.as_deref(), Some("draft"));
}

// Integration tests — ideation event emission paths (all 7 event types)
//
// Verifies that each ideation event:
//   1. Inserts a row into external_events with the correct event_type string
//   2. Calls webhook_publisher.publish() with the correct EventType variant
//
// Events covered:
//   IdeationSessionCreated  — via start_ideation_http (4 paths; tested via primary HTTP path)
//   IdeationPlanCreated     — via create_plan_artifact handler
//   IdeationProposalsReady  — via finalize_proposals handler
//   IdeationSessionAccepted — via finalize_proposals handler (when session converts)
//   IdeationVerified        — via post_verification_status handler (status=verified)
//   IdeationAutoProposeSent — via auto_propose_with_retry (success path)
//   IdeationAutoProposeFailed — via auto_propose_with_retry (all retries exhausted)
//
// Uses AppState::new_test() / new_sqlite_test() (no real DB files, no network).

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::{
    extract::{Path, State},
    Json,
};
use ralphx_domain::entities::EventType;
use ralphx_lib::application::{AppState, TeamService, TeamStateTracker};
use ralphx_lib::application::chat_service::MockChatService;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    IdeationSessionBuilder, IdeationSessionId, Priority,
    Project, ProjectId, ProposalCategory, TaskProposal,
};
use ralphx_lib::domain::repositories::ExternalEventsRepository;
use ralphx_lib::domain::state_machine::services::WebhookPublisher as WebhookPublisherTrait;
use ralphx_lib::http_server::handlers::{
    auto_propose_with_retry, create_plan_artifact, finalize_proposals, start_ideation_http,
    post_verification_status, StartIdeationRequest,
};
use ralphx_lib::http_server::project_scope::ProjectScope;
use ralphx_lib::http_server::types::{
    CreatePlanArtifactRequest, FinalizeProposalsRequest, HttpServerState, UpdateVerificationRequest,
};
use ralphx_lib::infrastructure::memory::MemoryExternalEventsRepository;

// ============================================================================
// RecordingWebhookPublisher — captures publish() calls for assertion
// ============================================================================

#[derive(Default)]
pub struct RecordingWebhookPublisher {
    pub calls: Mutex<Vec<(EventType, String)>>,
}

impl RecordingWebhookPublisher {
    pub fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
        }
    }

    pub fn recorded_event_types(&self) -> Vec<EventType> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|(et, _)| *et)
            .collect()
    }

    pub fn was_called_with(&self, event_type: &EventType) -> bool {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .any(|(et, _)| et == event_type)
    }
}

#[async_trait]
impl WebhookPublisherTrait for RecordingWebhookPublisher {
    async fn publish(
        &self,
        event_type: EventType,
        project_id: &str,
        _payload: serde_json::Value,
    ) {
        self.calls
            .lock()
            .unwrap()
            .push((event_type, project_id.to_string()));
    }
}

// ============================================================================
// Test helpers
// ============================================================================

/// Create an HttpServerState with injected RecordingWebhookPublisher and
/// a separate MemoryExternalEventsRepository that the caller can inspect.
fn make_http_state(
    base_state: AppState,
    recording: Arc<RecordingWebhookPublisher>,
    mem_events: Arc<MemoryExternalEventsRepository>,
) -> HttpServerState {
    let mut app_state = base_state;
    app_state.webhook_publisher =
        Some(Arc::clone(&recording) as Arc<dyn WebhookPublisherTrait>);
    app_state.external_events_repo =
        Arc::clone(&mem_events) as Arc<dyn ExternalEventsRepository>;
    let app_state = Arc::new(app_state);
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

fn unrestricted_scope() -> ProjectScope {
    ProjectScope(None)
}

fn make_project(id: &str) -> Project {
    use ralphx_lib::domain::entities::project::GitMode;
    Project {
        id: ProjectId::from_string(id.to_string()),
        name: "Test Project".to_string(),
        working_directory: "/tmp/test".to_string(),
        git_mode: GitMode::Worktree,
        base_branch: None,
        worktree_parent_directory: None,
        use_feature_branches: false,
        merge_validation_mode: Default::default(),
        merge_strategy: Default::default(),
        detected_analysis: None,
        custom_analysis: None,
        analyzed_at: None,
        github_pr_enabled: false,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        archived_at: None,
    }
}

fn make_proposal(session_id: IdeationSessionId, title: &str) -> TaskProposal {
    let mut proposal =
        TaskProposal::new(session_id, title, ProposalCategory::Feature, Priority::Medium);
    proposal.affected_paths = Some(r#"["src/ideation/test_scope.rs"]"#.to_string());
    proposal
}

// ============================================================================
// Helper: read all external_events for a project
// ============================================================================

async fn get_events(
    repo: &MemoryExternalEventsRepository,
    project_id: &str,
) -> Vec<String> {
    repo.get_events_after_cursor(&[project_id.to_string()], 0, 100)
        .await
        .unwrap()
        .into_iter()
        .map(|e| e.event_type)
        .collect()
}

// ============================================================================
// Test 1: IdeationSessionCreated
// ============================================================================

#[tokio::test]
async fn test_session_created_emits_event() {
    let recording = Arc::new(RecordingWebhookPublisher::new());
    let mem_events = Arc::new(MemoryExternalEventsRepository::new());
    let state = make_http_state(
        AppState::new_sqlite_test(),
        Arc::clone(&recording),
        Arc::clone(&mem_events),
    );

    let project_id = "proj-session-created";
    state
        .app_state
        .project_repo
        .create(make_project(project_id))
        .await
        .unwrap();

    let result = start_ideation_http(
        State(state),
        unrestricted_scope(),
        axum::http::HeaderMap::new(),
        Json(StartIdeationRequest {
            project_id: project_id.to_string(),
            title: Some("Test Session".to_string()),
            prompt: None,
            initial_prompt: None,
            idempotency_key: None,
        }),
    )
    .await;

    assert!(result.is_ok(), "start_ideation_http failed: {:?}", result.err());

    // Verify external_events table
    let event_types = get_events(&mem_events, project_id).await;
    assert!(
        event_types.contains(&"ideation:session_created".to_string()),
        "Expected ideation:session_created in external_events, got: {:?}",
        event_types
    );

    // Verify webhook publish
    assert!(
        recording.was_called_with(&EventType::IdeationSessionCreated),
        "Expected IdeationSessionCreated webhook publish, got: {:?}",
        recording.recorded_event_types()
    );
}

// ============================================================================
// Test 2: IdeationPlanCreated
// ============================================================================

#[tokio::test]
async fn test_plan_created_emits_event() {
    let recording = Arc::new(RecordingWebhookPublisher::new());
    let mem_events = Arc::new(MemoryExternalEventsRepository::new());
    let state = make_http_state(
        AppState::new_sqlite_test(),
        Arc::clone(&recording),
        Arc::clone(&mem_events),
    );

    let project_id = "proj-plan-created";
    let session_id = IdeationSessionId::new();

    // Create project + session directly via repo
    state
        .app_state
        .project_repo
        .create(make_project(project_id))
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .create(
            IdeationSessionBuilder::new()
                .id(session_id.clone())
                .project_id(ProjectId::from_string(project_id.to_string()))
                .build(),
        )
        .await
        .unwrap();

    let result = create_plan_artifact(
        State(state),
        Json(CreatePlanArtifactRequest {
            session_id: session_id.as_str().to_string(),
            title: "Test Plan".to_string(),
            content: "# Test plan content".to_string(),
        }),
    )
    .await;

    assert!(result.is_ok(), "create_plan_artifact failed: {:?}", result.err());

    // Verify external_events table
    let event_types = get_events(&mem_events, project_id).await;
    assert!(
        event_types.contains(&"ideation:plan_created".to_string()),
        "Expected ideation:plan_created in external_events, got: {:?}",
        event_types
    );

    // Verify webhook publish
    assert!(
        recording.was_called_with(&EventType::IdeationPlanCreated),
        "Expected IdeationPlanCreated webhook publish, got: {:?}",
        recording.recorded_event_types()
    );
}

// ============================================================================
// Test 3: IdeationProposalsReady + IdeationSessionAccepted (both from finalize_proposals)
// ============================================================================

#[tokio::test]
async fn test_proposals_ready_and_session_accepted_emit_events() {
    let recording = Arc::new(RecordingWebhookPublisher::new());
    let mem_events = Arc::new(MemoryExternalEventsRepository::new());
    let state = make_http_state(
        AppState::new_sqlite_test(),
        Arc::clone(&recording),
        Arc::clone(&mem_events),
    );

    let project_id = "proj-proposals-ready";
    let session_id = IdeationSessionId::new();

    // Create project + session + proposals
    state
        .app_state
        .project_repo
        .create(make_project(project_id))
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .create(
            IdeationSessionBuilder::new()
                .id(session_id.clone())
                .project_id(ProjectId::from_string(project_id.to_string()))
                .dependencies_acknowledged(true)
                .build(),
        )
        .await
        .unwrap();

    let p1 = make_proposal(session_id.clone(), "Task A");
    let p2 = make_proposal(session_id.clone(), "Task B");
    state
        .app_state
        .task_proposal_repo
        .create(p1)
        .await
        .unwrap();
    state
        .app_state
        .task_proposal_repo
        .create(p2)
        .await
        .unwrap();

    let result = finalize_proposals(
        State(state),
        axum::http::HeaderMap::new(),
        Json(FinalizeProposalsRequest {
            session_id: session_id.as_str().to_string(),
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "finalize_proposals failed: {:?}",
        result.err()
    );

    let resp = result.unwrap().0;

    // Verify IdeationProposalsReady always fires
    let event_types = get_events(&mem_events, project_id).await;
    assert!(
        event_types.contains(&"ideation:proposals_ready".to_string()),
        "Expected ideation:proposals_ready in external_events, got: {:?}",
        event_types
    );
    assert!(
        recording.was_called_with(&EventType::IdeationProposalsReady),
        "Expected IdeationProposalsReady webhook publish, got: {:?}",
        recording.recorded_event_types()
    );

    // Verify IdeationSessionAccepted fires when session converts
    if resp.session_status == "accepted" {
        assert!(
            event_types.contains(&"ideation:session_accepted".to_string()),
            "Expected ideation:session_accepted in external_events when session is accepted, got: {:?}",
            event_types
        );
        assert!(
            recording.was_called_with(&EventType::IdeationSessionAccepted),
            "Expected IdeationSessionAccepted webhook publish, got: {:?}",
            recording.recorded_event_types()
        );
    }
}

// ============================================================================
// Test 4: IdeationSessionAccepted (explicit — ensures session converts)
// ============================================================================

#[tokio::test]
async fn test_session_accepted_emits_event_when_proposals_applied() {
    let recording = Arc::new(RecordingWebhookPublisher::new());
    let mem_events = Arc::new(MemoryExternalEventsRepository::new());
    let state = make_http_state(
        AppState::new_sqlite_test(),
        Arc::clone(&recording),
        Arc::clone(&mem_events),
    );

    let project_id = "proj-session-accepted";
    let session_id = IdeationSessionId::new();

    state
        .app_state
        .project_repo
        .create(make_project(project_id))
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .create(
            IdeationSessionBuilder::new()
                .id(session_id.clone())
                .project_id(ProjectId::from_string(project_id.to_string()))
                .build(),
        )
        .await
        .unwrap();

    // Seed one proposal — applying it should convert the session to Accepted
    let proposal = make_proposal(session_id.clone(), "Only Task");
    state
        .app_state
        .task_proposal_repo
        .create(proposal)
        .await
        .unwrap();

    let result = finalize_proposals(
        State(state),
        axum::http::HeaderMap::new(),
        Json(FinalizeProposalsRequest {
            session_id: session_id.as_str().to_string(),
        }),
    )
    .await;

    assert!(result.is_ok(), "finalize_proposals failed: {:?}", result.err());
    let resp = result.unwrap().0;

    // When all proposals are applied the session transitions to Accepted
    assert_eq!(
        resp.session_status, "accepted",
        "Expected session_status = accepted after applying all proposals"
    );

    let event_types = get_events(&mem_events, project_id).await;
    assert!(
        event_types.contains(&"ideation:session_accepted".to_string()),
        "Expected ideation:session_accepted in external_events, got: {:?}",
        event_types
    );
    assert!(
        recording.was_called_with(&EventType::IdeationSessionAccepted),
        "Expected IdeationSessionAccepted webhook publish, got: {:?}",
        recording.recorded_event_types()
    );
}

// ============================================================================
// Test 5: IdeationVerified
// ============================================================================

#[tokio::test]
async fn test_verified_emits_event() {
    let recording = Arc::new(RecordingWebhookPublisher::new());
    let mem_events = Arc::new(MemoryExternalEventsRepository::new());
    let state = make_http_state(
        AppState::new_sqlite_test(),
        Arc::clone(&recording),
        Arc::clone(&mem_events),
    );

    let project_id = "proj-verified";
    let session_id = IdeationSessionId::new();

    state
        .app_state
        .project_repo
        .create(make_project(project_id))
        .await
        .unwrap();
    state
        .app_state
        .ideation_session_repo
        .create(
            IdeationSessionBuilder::new()
                .id(session_id.clone())
                .project_id(ProjectId::from_string(project_id.to_string()))
                .build(),
        )
        .await
        .unwrap();

    // Transition Unverified → Reviewing first (required before → Verified)
    let review_result = post_verification_status(
        State(state.clone()),
        Path(session_id.as_str().to_string()),
        Json(UpdateVerificationRequest {
            status: "reviewing".to_string(),
            in_progress: true,
            round: Some(1),
            gaps: None,
            convergence_reason: None,
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await;
    assert!(
        review_result.is_ok(),
        "post_verification_status(reviewing) failed: {:?}",
        review_result.err()
    );

    // Now transition Reviewing → Verified
    let result = post_verification_status(
        State(state),
        Path(session_id.as_str().to_string()),
        Json(UpdateVerificationRequest {
            status: "verified".to_string(),
            in_progress: false,
            round: None,
            gaps: None,
            convergence_reason: Some("zero_blocking".to_string()),
            max_rounds: None,
            parse_failed: None,
            generation: None,
        }),
    )
    .await;

    assert!(
        result.is_ok(),
        "post_verification_status(verified) failed: {:?}",
        result.err()
    );

    // Verify external_events table
    let event_types = get_events(&mem_events, project_id).await;
    assert!(
        event_types.contains(&"ideation:verified".to_string()),
        "Expected ideation:verified in external_events, got: {:?}",
        event_types
    );

    // Verify webhook publish
    assert!(
        recording.was_called_with(&EventType::IdeationVerified),
        "Expected IdeationVerified webhook publish, got: {:?}",
        recording.recorded_event_types()
    );
}

// ============================================================================
// Test 6: IdeationAutoProposeSent
// ============================================================================

#[tokio::test]
async fn test_auto_propose_sent_emits_event() {
    let recording = Arc::new(RecordingWebhookPublisher::new());
    let mem_events = Arc::new(MemoryExternalEventsRepository::new());

    // MockChatService defaults to available=true — send_message returns Ok
    let chat_service = MockChatService::new();
    chat_service.queue_text_response("proposals applied").await;

    let session_id = "session-auto-propose-sent";
    let project_id = "proj-auto-propose-sent";

    auto_propose_with_retry(
        session_id,
        project_id,
        &chat_service,
        Arc::clone(&mem_events) as Arc<dyn ExternalEventsRepository>,
        Some(Arc::clone(&recording) as Arc<dyn WebhookPublisherTrait>),
        &[], // no retries needed — first attempt succeeds
    )
    .await;

    let event_types = get_events(&mem_events, project_id).await;
    assert!(
        event_types.contains(&"ideation:auto_propose_sent".to_string()),
        "Expected ideation:auto_propose_sent in external_events, got: {:?}",
        event_types
    );
    assert!(
        recording.was_called_with(&EventType::IdeationAutoProposeSent),
        "Expected IdeationAutoProposeSent webhook publish, got: {:?}",
        recording.recorded_event_types()
    );
}

// ============================================================================
// Test 7: IdeationAutoProposeFailed
// ============================================================================

#[tokio::test]
async fn test_auto_propose_failed_emits_event() {
    let recording = Arc::new(RecordingWebhookPublisher::new());
    let mem_events = Arc::new(MemoryExternalEventsRepository::new());

    // Set chat service to unavailable — all attempts fail
    let chat_service = MockChatService::new();
    chat_service.set_available(false).await;

    let session_id = "session-auto-propose-failed";
    let project_id = "proj-auto-propose-failed";

    auto_propose_with_retry(
        session_id,
        project_id,
        &chat_service,
        Arc::clone(&mem_events) as Arc<dyn ExternalEventsRepository>,
        Some(Arc::clone(&recording) as Arc<dyn WebhookPublisherTrait>),
        &[0, 0], // two retries with 0ms delay — all 3 attempts will fail
    )
    .await;

    let event_types = get_events(&mem_events, project_id).await;
    assert!(
        event_types.contains(&"ideation:auto_propose_failed".to_string()),
        "Expected ideation:auto_propose_failed in external_events, got: {:?}",
        event_types
    );
    assert!(
        recording.was_called_with(&EventType::IdeationAutoProposeFailed),
        "Expected IdeationAutoProposeFailed webhook publish, got: {:?}",
        recording.recorded_event_types()
    );
}

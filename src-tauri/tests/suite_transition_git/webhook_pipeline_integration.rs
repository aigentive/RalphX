// E2E integration tests — RalphX webhook + pipeline flow
//
// Test coverage:
//   1. Full ideation-to-tasks pipeline (session → plan → propose → accept → tasks)
//   2. Webhook HTTP lifecycle (register → list → unregister, idempotent re-registration)
//   3. Webhook delivery + HMAC signature (end-to-end via ConcreteWebhookPublisher)
//   4. Webhook failure tracking → deactivation after 10 consecutive failures
//
// Uses the SQLite-backed apply-test app state for the ideation/apply pipeline and
// `ConcreteWebhookPublisher` with `MockWebhookHttpClient` — no network calls.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::support::real_git_repo::setup_real_git_repo;
use ralphx_domain::entities::EventType;
use ralphx_lib::application::AppState;
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{
    IdeationSession, IdeationSessionId, Priority, Project, ProjectId, ProposalCategory,
    TaskProposal, VerificationStatus,
};
use ralphx_lib::domain::repositories::{WebhookRegistration, WebhookRegistrationRepository};
use ralphx_lib::domain::state_machine::services::WebhookPublisher as WebhookPublisherTrait;
use ralphx_lib::http_server::handlers::*;
use ralphx_lib::http_server::project_scope::ProjectScope;
use ralphx_lib::http_server::types::HttpServerState;
use ralphx_lib::infrastructure::memory::MemoryWebhookRegistrationRepository;
use ralphx_lib::infrastructure::{ConcreteWebhookPublisher, MockWebhookHttpClient};

type HmacSha256 = Hmac<Sha256>;

// ============================================================================
// Shared setup helpers (TestContext pattern)
// ============================================================================

async fn setup_test_state() -> HttpServerState {
    let app_state = Arc::new(AppState::new_sqlite_for_apply_test());
    let execution_state = Arc::new(ExecutionState::new());
    HttpServerState {
        app_state,
        execution_state,
        delegation_service: Default::default(),
        external_mcp_supervisor: None,
    }
}

fn make_project(id: &str, name: &str) -> Project {
    use ralphx_lib::domain::entities::project::GitMode;
    Project {
        id: ProjectId::from_string(id.to_string()),
        name: name.to_string(),
        working_directory: format!("/tmp/{}", id),
        git_mode: GitMode::Worktree,
        base_branch: None,
        worktree_parent_directory: None,
        use_feature_branches: true,
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
    let mut proposal = TaskProposal::new(
        session_id,
        title,
        ProposalCategory::Feature,
        Priority::Medium,
    );
    proposal.affected_paths = Some(r#"["src/webhook/test_scope.rs"]"#.to_string());
    proposal
}

async fn acknowledge_dependencies(state: &HttpServerState, session_id: &str) {
    let _ = analyze_session_dependencies(State(state.clone()), Path(session_id.to_string()))
        .await
        .expect("Failed to analyze and acknowledge dependencies");
}

async fn mark_session_verified(state: &HttpServerState, session_id: &IdeationSessionId) {
    state
        .app_state
        .ideation_session_repo
        .update_verification_state(session_id, VerificationStatus::Verified, false)
        .await
        .expect("Failed to mark session verified");
}

/// Recompute HMAC-SHA256(secret, data) → lowercase hex (same algorithm as WebhookPublisher).
fn compute_expected_hmac(secret: &str, data: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key");
    mac.update(data);
    let result = mac.finalize().into_bytes();
    result
        .iter()
        .fold(String::with_capacity(result.len() * 2), |mut acc, b| {
            use std::fmt::Write;
            let _ = write!(acc, "{:02x}", b);
            acc
        })
}

/// Build a webhook registration for tests.
fn make_registration(id: &str, url: &str, project_id: &str, secret: &str) -> WebhookRegistration {
    WebhookRegistration {
        id: id.to_string(),
        api_key_id: "test-key-1".to_string(),
        url: url.to_string(),
        event_types: None, // None = match all events
        project_ids: format!(r#"["{}"]"#, project_id),
        secret: secret.to_string(),
        active: true,
        failure_count: 0,
        last_failure_at: None,
        created_at: "2024-01-01T00:00:00Z".to_string(),
        updated_at: "2024-01-01T00:00:00Z".to_string(),
    }
}

/// Build a headers map with the API key ID header set.
fn headers_with_key(api_key_id: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-ralphx-key-id",
        api_key_id.parse().expect("valid header value"),
    );
    headers
}

fn unrestricted_scope() -> ProjectScope {
    ProjectScope(None)
}

// ============================================================================
// Test 1: Full ideation-to-tasks pipeline flow
//
// Flow: create project → start ideation session (HTTP) → seed proposals →
//       apply proposals (HTTP) → verify tasks created (HTTP batch status)
//
// This tests the external agent's end-to-end scheduling workflow.
// ============================================================================

#[tokio::test]
async fn test_pipeline_session_to_tasks_e2e() {
    let state = setup_test_state().await;

    // --- Step 1: Create project ---
    let repo = setup_real_git_repo();
    let mut project = make_project("proj-pipe-e2e", "Pipeline E2E Project");
    project.working_directory = repo.path_string();
    project.base_branch = Some("main".to_string());
    state.app_state.project_repo.create(project).await.unwrap();

    // --- Step 2: Start ideation session via HTTP handler ---
    let start_req = StartIdeationRequest {
        project_id: "proj-pipe-e2e".to_string(),
        title: Some("E2E Pipeline Session".to_string()),
        prompt: None,
        initial_prompt: None,
        idempotency_key: None,
    };
    let start_result = start_ideation_http(
        State(state.clone()),
        unrestricted_scope(),
        axum::http::HeaderMap::new(),
        Json(start_req),
    )
    .await;

    assert!(
        start_result.is_ok(),
        "start_ideation_http must succeed: {:?}",
        start_result.err()
    );
    let session_resp = start_result.unwrap().0;
    assert!(
        !session_resp.session_id.is_empty(),
        "session_id must be non-empty"
    );

    let session_id = IdeationSessionId::from_string(session_resp.session_id.clone());

    // --- Step 3: Seed proposals directly via repo (simulates orchestrator output) ---
    let p1 = make_proposal(session_id.clone(), "Implement authentication module");
    let p2 = make_proposal(session_id.clone(), "Add rate limiting middleware");

    let created_p1 = state.app_state.task_proposal_repo.create(p1).await.unwrap();
    let created_p2 = state.app_state.task_proposal_repo.create(p2).await.unwrap();

    // --- Step 4: Simulate dependency review before finalize/apply ---
    acknowledge_dependencies(&state, session_id.as_str()).await;
    let acknowledged_session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .expect("Failed to reload acknowledged session")
        .expect("Acknowledged session must exist");
    assert!(
        acknowledged_session.dependencies_acknowledged,
        "Dependency review must acknowledge the session before apply"
    );
    mark_session_verified(&state, &session_id).await;

    // --- Step 5: Apply proposals (accept + schedule) via HTTP handler ---
    let apply_req = ExternalApplyProposalsRequest {
        session_id: session_resp.session_id.clone(),
        proposal_ids: vec![
            created_p1.id.as_str().to_string(),
            created_p2.id.as_str().to_string(),
        ],
        target_column: "auto".to_string(),
        base_branch_override: None,
    };
    let apply_result =
        external_apply_proposals(State(state.clone()), unrestricted_scope(), Json(apply_req)).await;

    assert!(
        apply_result.is_ok(),
        "external_apply_proposals must succeed: {:?}",
        apply_result
            .err()
            .map(|e| (e.status, e.message.unwrap_or_default()))
    );
    let apply_resp = apply_result.unwrap().0;
    assert_eq!(
        apply_resp.created_task_ids.len(),
        2,
        "Two tasks must be created"
    );
    assert!(
        apply_resp.session_converted,
        "Session must be marked converted"
    );
    assert!(
        apply_resp.execution_plan_id.is_some(),
        "Execution plan must be created"
    );

    let task_ids = apply_resp.created_task_ids.clone();

    // --- Step 6: Verify tasks via batch status HTTP handler ---
    let batch_req = BatchTaskStatusRequest {
        task_ids: task_ids.clone(),
    };
    let batch_result =
        batch_task_status_http(State(state.clone()), unrestricted_scope(), Json(batch_req)).await;

    assert!(batch_result.is_ok(), "batch_task_status_http must succeed");
    let batch_resp = batch_result.unwrap().0;

    assert_eq!(
        batch_resp.tasks.len(),
        2,
        "All 2 tasks must be returned in batch status"
    );
    assert!(batch_resp.errors.is_empty(), "No errors expected");
    assert_eq!(batch_resp.returned_count, 2);

    let titles: Vec<&str> = batch_resp.tasks.iter().map(|t| t.title.as_str()).collect();
    assert!(
        titles.contains(&"Implement authentication module"),
        "Task 'Implement authentication module' must be present"
    );
    assert!(
        titles.contains(&"Add rate limiting middleware"),
        "Task 'Add rate limiting middleware' must be present"
    );

    // All tasks belong to the correct project
    for task in &batch_resp.tasks {
        assert_eq!(task.project_id, "proj-pipe-e2e");
    }
}

// ============================================================================
// Test 2: Webhook HTTP handler lifecycle
//
// Flow: register webhook (HTTP) → list webhooks (HTTP) → unregister (HTTP) →
//       list webhooks again (HTTP, expect 0) → re-register same URL (idempotent)
// ============================================================================

#[tokio::test]
async fn test_webhook_http_registration_lifecycle() {
    let state = setup_test_state().await;

    let api_key_id = "api-key-webhook-test";
    let webhook_url = "http://127.0.0.1:18789/hooks/ralphx";
    let project_id = "proj-webhook-http";

    // Create project so scope validation works
    state
        .app_state
        .project_repo
        .create(make_project(project_id, "Webhook HTTP Test"))
        .await
        .unwrap();

    // --- Step 1: Register webhook ---
    let reg_req = RegisterWebhookRequest {
        url: webhook_url.to_string(),
        event_types: Some(vec![
            "task:status_changed".to_string(),
            "review:ready".to_string(),
        ]),
        project_ids: vec![project_id.to_string()],
    };
    let reg_result = register_webhook_http(
        State(state.clone()),
        unrestricted_scope(),
        headers_with_key(api_key_id),
        Json(reg_req),
    )
    .await;

    assert!(
        reg_result.is_ok(),
        "register_webhook_http must succeed: {:?}",
        reg_result.err()
    );
    let reg_resp = reg_result.unwrap().0;
    assert!(!reg_resp.id.is_empty(), "Webhook ID must be non-empty");
    assert_eq!(reg_resp.url, webhook_url);
    assert!(reg_resp.active, "Newly registered webhook must be active");
    assert_eq!(
        reg_resp.secret.len(),
        64,
        "Secret must be 64-char hex string (HMAC-SHA256 key)"
    );
    assert!(
        reg_resp.secret.chars().all(|c| c.is_ascii_hexdigit()),
        "Secret must be hex characters"
    );
    let expected_event_types: Vec<String> = vec![
        "task:status_changed".to_string(),
        "review:ready".to_string(),
    ];
    assert_eq!(reg_resp.event_types, Some(expected_event_types));

    let webhook_id = reg_resp.id.clone();

    // --- Step 2: List webhooks → expect 1 ---
    let list_result = list_webhooks_http(State(state.clone()), headers_with_key(api_key_id)).await;

    assert!(list_result.is_ok(), "list_webhooks_http must succeed");
    let list_resp = list_result.unwrap().0;
    assert_eq!(list_resp.webhooks.len(), 1, "Exactly 1 webhook expected");
    assert_eq!(list_resp.webhooks[0].url, webhook_url);
    assert!(list_resp.webhooks[0].active);
    assert_eq!(list_resp.webhooks[0].failure_count, 0);

    // --- Step 3: Unregister webhook ---
    let unreg_result = unregister_webhook_http(
        State(state.clone()),
        headers_with_key(api_key_id),
        Path(webhook_id.clone()),
    )
    .await;

    assert!(unreg_result.is_ok(), "unregister_webhook_http must succeed");
    let unreg_resp = unreg_result.unwrap().0;
    assert!(unreg_resp.success);
    assert_eq!(unreg_resp.id, webhook_id);

    // --- Step 4: List webhooks → expect 0 ---
    let list_result2 = list_webhooks_http(State(state.clone()), headers_with_key(api_key_id)).await;

    assert!(list_result2.is_ok());
    let list_resp2 = list_result2.unwrap().0;
    assert_eq!(
        list_resp2.webhooks.len(),
        0,
        "No webhooks expected after unregister"
    );

    // --- Step 5: Re-register same URL → idempotent (same ID returned) ---
    let rereg_req = RegisterWebhookRequest {
        url: webhook_url.to_string(),
        event_types: None,
        project_ids: vec![project_id.to_string()],
    };
    let rereg_result = register_webhook_http(
        State(state.clone()),
        unrestricted_scope(),
        headers_with_key(api_key_id),
        Json(rereg_req),
    )
    .await;

    assert!(rereg_result.is_ok(), "Re-registration must succeed");
    let rereg_resp = rereg_result.unwrap().0;
    assert_eq!(
        rereg_resp.id, webhook_id,
        "Re-registration must return the same webhook ID (idempotent)"
    );
    assert!(rereg_resp.active, "Re-registered webhook must be active");
}

// ============================================================================
// Test 3: Webhook delivery + HMAC signature verification
//
// Flow: seed webhook registration → call publisher.publish() →
//       wait for async delivery → assert HTTP call received with valid HMAC
//
// Verifies the E2E delivery envelope structure and signature correctness.
// ============================================================================

#[tokio::test]
async fn test_webhook_delivery_and_hmac_signature() {
    let repo = Arc::new(MemoryWebhookRegistrationRepository::new());
    let mock_client = Arc::new(MockWebhookHttpClient::new(200));

    let publisher = ConcreteWebhookPublisher::new(
        Arc::clone(&repo) as Arc<dyn WebhookRegistrationRepository>,
        Arc::clone(&mock_client) as Arc<dyn ralphx_lib::infrastructure::WebhookHttpClient>,
    );

    let secret = "e2e-test-secret-known-value";
    let reg = make_registration(
        "wh-e2e-sig",
        "http://127.0.0.1:18789/hooks/ralphx",
        "proj-sig-test",
        secret,
    );
    repo.upsert(reg).await.unwrap();

    let event_payload = serde_json::json!({
        "task_id": "task-abc-123",
        "from_status": "Blocked",
        "to_status": "Backlog",
    });

    publisher
        .publish(
            EventType::TaskStatusChanged,
            "proj-sig-test",
            event_payload.clone(),
        )
        .await;

    // Wait for the spawned tokio task to complete delivery
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert_eq!(
        mock_client.call_count(),
        1,
        "Exactly 1 HTTP call expected for successful delivery"
    );

    let calls = mock_client.calls.lock().unwrap();
    let call = &calls[0];

    assert_eq!(call.url, "http://127.0.0.1:18789/hooks/ralphx");

    // --- Verify Content-Type header ---
    let content_type = call
        .headers
        .get("Content-Type")
        .expect("Content-Type header required");
    assert_eq!(content_type, "application/json");

    // --- Verify X-Webhook-Event header ---
    let event_header = call
        .headers
        .get("X-Webhook-Event")
        .expect("X-Webhook-Event header required");
    assert_eq!(event_header, "task:status_changed");

    // --- Verify X-Webhook-Signature header format ---
    let sig_header = call
        .headers
        .get("X-Webhook-Signature")
        .expect("X-Webhook-Signature header required");

    assert!(
        sig_header.starts_with("sha256="),
        "X-Webhook-Signature must be prefixed with 'sha256=', got: {sig_header}"
    );

    let hex_part = &sig_header["sha256=".len()..];
    assert_eq!(hex_part.len(), 64, "HMAC-SHA256 hex must be 64 chars");
    assert!(
        hex_part
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "Signature hex must be lowercase"
    );

    // --- Verify HMAC signature matches expected computation ---
    let expected_sig = compute_expected_hmac(secret, &call.body);
    assert_eq!(
        hex_part, expected_sig,
        "HMAC signature must match expected HMAC-SHA256(secret, body)"
    );

    // --- Verify envelope body structure ---
    let envelope: serde_json::Value =
        serde_json::from_slice(&call.body).expect("Delivery body must be valid JSON");

    assert_eq!(envelope["event_type"], "task:status_changed");
    assert_eq!(envelope["project_id"], "proj-sig-test");
    assert_eq!(envelope["webhook_id"], "wh-e2e-sig");
    assert!(
        envelope["timestamp"].is_string(),
        "Envelope must include an RFC3339 timestamp"
    );
    assert_eq!(
        envelope["payload"]["task_id"], "task-abc-123",
        "Payload must be nested inside envelope"
    );
}

// ============================================================================
// Test 4: Webhook failure tracking → deactivation after 10 consecutive failures
//
// Flow: register webhook → 10x publish (each gets 404 non-retryable) →
//       verify webhook is inactive → 11th publish results in 0 HTTP calls
//
// Verifies the deactivation threshold and post-deactivation cache behavior.
// ============================================================================

#[tokio::test]
async fn test_webhook_deactivation_after_10_consecutive_failures() {
    let repo = Arc::new(MemoryWebhookRegistrationRepository::new());
    // 404 is non-retryable — each publish() increments failure_count by 1 immediately
    let mock_client = Arc::new(MockWebhookHttpClient::new(404));

    let publisher = ConcreteWebhookPublisher::new(
        Arc::clone(&repo) as Arc<dyn WebhookRegistrationRepository>,
        Arc::clone(&mock_client) as Arc<dyn ralphx_lib::infrastructure::WebhookHttpClient>,
    );

    let reg = make_registration(
        "wh-deact",
        "http://127.0.0.1:18789/hooks/ralphx",
        "proj-deact",
        "deact-secret",
    );
    repo.upsert(reg).await.unwrap();

    // Trigger 10 consecutive non-retryable 404 failures
    for i in 0..10 {
        publisher
            .publish(
                EventType::TaskStatusChanged,
                "proj-deact",
                serde_json::json!({"attempt": i}),
            )
            .await;
        // Allow the spawned task to complete before the next publish()
        // Each publish() evicts the cache on failure, so the next one re-queries the DB
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // --- Verify webhook is now deactivated in the repo ---
    let stored = repo.get_by_id("wh-deact").await.unwrap().unwrap();
    assert!(
        !stored.active,
        "Webhook must be deactivated after 10 consecutive failures"
    );
    assert_eq!(stored.failure_count, 10, "failure_count must be exactly 10");

    // Total HTTP calls so far: 10 (one per publish, no retries on 404)
    assert_eq!(
        mock_client.call_count(),
        10,
        "Exactly 10 HTTP calls expected"
    );

    // --- Verify 11th publish produces 0 additional HTTP calls ---
    // Webhook is inactive → list_active_for_project returns empty → no delivery
    publisher
        .publish(
            EventType::TaskStatusChanged,
            "proj-deact",
            serde_json::json!({"attempt": 10}),
        )
        .await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(
        mock_client.call_count(),
        10,
        "No additional HTTP calls after webhook deactivation (inactive webhooks are skipped)"
    );
}

// ============================================================================
// Test 5: Re-registration refreshes project_ids scope (PDM-280 fix)
//
// Flow: register webhook with proj-a → re-register same URL with [proj-a, proj-b]
//       → verify list_active_for_project(proj-b) returns the webhook
//       → verify id and secret preserved across re-registration
// ============================================================================

#[tokio::test]
async fn test_reregistration_refreshes_project_ids() {
    let state = setup_test_state().await;

    let api_key_id = "api-key-reregister-test";
    let webhook_url = "http://127.0.0.1:18789/hooks/reregister";
    let proj_a = "proj-reregister-a";
    let proj_b = "proj-reregister-b";

    // Create both projects
    state
        .app_state
        .project_repo
        .create(make_project(proj_a, "Reregister Project A"))
        .await
        .unwrap();
    state
        .app_state
        .project_repo
        .create(make_project(proj_b, "Reregister Project B"))
        .await
        .unwrap();

    // --- Step 1: Register webhook scoped to proj-a only ---
    let reg_req = RegisterWebhookRequest {
        url: webhook_url.to_string(),
        event_types: None,
        project_ids: vec![proj_a.to_string()],
    };
    let reg_result = register_webhook_http(
        State(state.clone()),
        unrestricted_scope(),
        headers_with_key(api_key_id),
        Json(reg_req),
    )
    .await;
    assert!(reg_result.is_ok(), "Initial registration must succeed");
    let reg_resp = reg_result.unwrap().0;
    let original_id = reg_resp.id.clone();
    let original_secret = reg_resp.secret.clone();

    // proj-b must NOT be in scope yet
    let before = state
        .app_state
        .webhook_registration_repo
        .list_active_for_project(proj_b)
        .await
        .unwrap();
    assert_eq!(
        before.len(),
        0,
        "proj-b must not be in scope before re-registration"
    );

    // --- Step 2: Re-register same URL with expanded scope [proj-a, proj-b] ---
    let rereg_req = RegisterWebhookRequest {
        url: webhook_url.to_string(),
        event_types: None,
        project_ids: vec![proj_a.to_string(), proj_b.to_string()],
    };
    let rereg_result = register_webhook_http(
        State(state.clone()),
        unrestricted_scope(),
        headers_with_key(api_key_id),
        Json(rereg_req),
    )
    .await;
    assert!(rereg_result.is_ok(), "Re-registration must succeed");
    let rereg_resp = rereg_result.unwrap().0;

    // Same id and secret preserved
    assert_eq!(
        rereg_resp.id, original_id,
        "Webhook id must be preserved across re-registration"
    );
    assert_eq!(
        rereg_resp.secret, original_secret,
        "Webhook secret must be preserved across re-registration"
    );
    assert!(rereg_resp.active, "Re-registered webhook must be active");

    // proj-b must now appear
    let after = state
        .app_state
        .webhook_registration_repo
        .list_active_for_project(proj_b)
        .await
        .unwrap();
    assert_eq!(
        after.len(),
        1,
        "proj-b must appear in list_active_for_project after re-registration"
    );
    assert_eq!(after[0].id, original_id);

    // proj-a still appears
    let for_a = state
        .app_state
        .webhook_registration_repo
        .list_active_for_project(proj_a)
        .await
        .unwrap();
    assert_eq!(
        for_a.len(),
        1,
        "proj-a must still appear after re-registration"
    );
}

// ============================================================================
// Test 6: Full pipeline + webhook registration in one flow
//
// Flow: create project → session → proposals → apply → register webhook →
//       list webhooks → unregister webhook → list webhooks (empty)
//
// This is the combined E2E scenario an autonomous agent would execute.
// ============================================================================

#[tokio::test]
async fn test_full_pipeline_with_webhook_registration() {
    let state = setup_test_state().await;

    let project_id = "proj-full-e2e";
    let api_key_id = "api-key-full-e2e";

    // --- Create project ---
    state
        .app_state
        .project_repo
        .create(make_project(project_id, "Full E2E Pipeline"))
        .await
        .unwrap();

    // --- Create ideation session ---
    let pid = ProjectId::from_string(project_id.to_string());
    let session = IdeationSession::new(pid.clone());
    let created_session = state
        .app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();
    let session_id = created_session.id.clone();

    // --- Seed proposals ---
    let proposal_titles = ["Auth service", "Webhook handler", "Event dispatcher"];
    let mut proposal_ids = Vec::new();

    for title in &proposal_titles {
        let p = make_proposal(session_id.clone(), title);
        let created = state.app_state.task_proposal_repo.create(p).await.unwrap();
        proposal_ids.push(created.id.as_str().to_string());
    }

    // Multi-proposal apply requires dependency review acknowledgment first.
    acknowledge_dependencies(&state, session_id.as_str()).await;
    mark_session_verified(&state, &session_id).await;

    // --- Apply proposals → creates tasks ---
    let apply_req = ExternalApplyProposalsRequest {
        session_id: session_id.as_str().to_string(),
        proposal_ids: proposal_ids.clone(),
        target_column: "auto".to_string(),
        base_branch_override: None,
    };
    let apply_result =
        external_apply_proposals(State(state.clone()), unrestricted_scope(), Json(apply_req)).await;

    assert!(apply_result.is_ok(), "Apply proposals must succeed");
    let apply_resp = apply_result.unwrap().0;
    assert_eq!(
        apply_resp.created_task_ids.len(),
        3,
        "3 tasks must be created from 3 proposals"
    );

    // --- Verify tasks via batch status ---
    let batch_result = batch_task_status_http(
        State(state.clone()),
        unrestricted_scope(),
        Json(BatchTaskStatusRequest {
            task_ids: apply_resp.created_task_ids.clone(),
        }),
    )
    .await;
    assert!(batch_result.is_ok());
    let batch_resp = batch_result.unwrap().0;
    assert_eq!(batch_resp.tasks.len(), 3);

    // --- Register webhook for this project ---
    let reg_result = register_webhook_http(
        State(state.clone()),
        unrestricted_scope(),
        headers_with_key(api_key_id),
        Json(RegisterWebhookRequest {
            url: "http://127.0.0.1:18789/hooks/ralphx".to_string(),
            event_types: None, // subscribe to all events
            project_ids: vec![project_id.to_string()],
        }),
    )
    .await;

    assert!(reg_result.is_ok(), "Webhook registration must succeed");
    let reg_resp = reg_result.unwrap().0;
    let webhook_id = reg_resp.id.clone();

    // Secret is a 64-char hex string
    assert_eq!(reg_resp.secret.len(), 64);
    // No event filter → all events
    assert!(
        reg_resp.event_types.is_none(),
        "No event_types filter means subscribed to all events"
    );

    // --- List webhooks → 1 active ---
    let list_result = list_webhooks_http(State(state.clone()), headers_with_key(api_key_id)).await;
    let list_resp = list_result.unwrap().0;
    assert_eq!(list_resp.webhooks.len(), 1);
    assert_eq!(list_resp.webhooks[0].id, webhook_id);
    assert!(list_resp.webhooks[0].active);
    assert_eq!(list_resp.webhooks[0].failure_count, 0);

    // --- Unregister webhook ---
    let unreg_result = unregister_webhook_http(
        State(state.clone()),
        headers_with_key(api_key_id),
        Path(webhook_id),
    )
    .await;
    assert!(unreg_result.is_ok());
    assert!(unreg_result.unwrap().0.success);

    // --- List webhooks → 0 active ---
    let list_result2 = list_webhooks_http(State(state.clone()), headers_with_key(api_key_id)).await;
    let list_resp2 = list_result2.unwrap().0;
    assert_eq!(
        list_resp2.webhooks.len(),
        0,
        "No active webhooks after unregistration"
    );

    // --- Verify session was converted ---
    let final_session = state
        .app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        final_session.status,
        ralphx_lib::domain::entities::IdeationSessionStatus::Accepted,
        "Session must be marked Accepted after all proposals are applied"
    );
}

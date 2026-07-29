// Integration tests for ideation webhook payload enrichment.
//
// Verifies that `ideation:plan_created`, `ideation:verified`, and
// `ideation:session_accepted` external events are enriched with the
// presentation fields added by WebhookPresentationContext::inject_into.
//
// Tests:
//   1. plan_created: enrichment fields present when session has title
//   2. plan_created (None-omit): session_title key absent when session has no title
//   3. verified: enrichment fields present when session has title
//   4. session_accepted: enrichment fields present when session has title
//   5. backward compat: all original fields still present for each event type

use std::sync::Arc;

use async_trait::async_trait;
use ralphx_domain::entities::EventType;
use ralphx_lib::domain::repositories::ExternalEventsRepository;
use ralphx_lib::domain::services::payload_enrichment::{
    emit_external_webhook_event, PresentationKind, WebhookPresentationContext,
};
use ralphx_lib::domain::state_machine::services::WebhookPublisher as WebhookPublisherTrait;
use ralphx_lib::infrastructure::memory::MemoryExternalEventsRepository;

// ============================================================================
// NoOpWebhookPublisher — used to satisfy emit_external_webhook_event signature
// ============================================================================

struct NoOpWebhookPublisher;

#[async_trait]
impl WebhookPublisherTrait for NoOpWebhookPublisher {
    async fn publish(&self, _event_type: EventType, _project_id: &str, _payload: serde_json::Value) {
        // intentionally empty — enrichment tests verify stored events, not webhook delivery
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Emit an enriched event and return the stored payload as parsed JSON.
///
/// Mirrors the production pattern used in HTTP handlers:
///   1. Build base payload
///   2. Create WebhookPresentationContext
///   3. inject_into(&mut payload)
///   4. emit_external_webhook_event(...)
///
/// Then reads the stored event back from the repo and parses JSON.
async fn emit_and_get_payload(
    event_type_str: &str,
    project_id: &str,
    mut base_payload: serde_json::Value,
    ctx: WebhookPresentationContext,
    events_repo: &Arc<MemoryExternalEventsRepository>,
    publisher: &Arc<dyn WebhookPublisherTrait>,
) -> serde_json::Value {
    ctx.inject_into(&mut base_payload);

    emit_external_webhook_event(
        event_type_str,
        project_id,
        base_payload,
        &(Arc::clone(events_repo) as Arc<dyn ExternalEventsRepository>),
        publisher,
    )
    .await
    .expect("emit_external_webhook_event must not fail");

    let events = events_repo
        .get_events_after_cursor(std::slice::from_ref(&project_id.to_string()), 0, 1000)
        .await
        .expect("get_events_after_cursor");

    let ev = events
        .iter()
        .find(|e| e.event_type == event_type_str)
        .unwrap_or_else(|| panic!("{event_type_str} event must be stored"));

    serde_json::from_str(&ev.payload)
        .unwrap_or_else(|e| panic!("{event_type_str} payload must be valid JSON: {e}"))
}

// ============================================================================
// Test 1 — ideation:plan_created enrichment: all presentation fields present
// ============================================================================

#[tokio::test]
async fn test_plan_created_payload_includes_enrichment_fields() {
    let events_repo = Arc::new(MemoryExternalEventsRepository::new());
    let publisher: Arc<dyn WebhookPublisherTrait> = Arc::new(NoOpWebhookPublisher);
    let project_id = "proj-pc-enrichment-test";

    let base_payload = serde_json::json!({
        "session_id": "session-pc-test",
        "project_id": project_id,
        "artifact_id": "artifact-pc-test",
        "plan_title": "My Plan",
        "timestamp": "2026-01-01T00:00:00Z",
    });

    let ctx = WebhookPresentationContext {
        project_name: Some("My Project".to_string()),
        session_title: Some("My Ideation Session".to_string()),
        task_title: None,
        presentation_kind: Some(PresentationKind::PlanCreated),
    };

    let payload = emit_and_get_payload(
        "ideation:plan_created",
        project_id,
        base_payload,
        ctx,
        &events_repo,
        &publisher,
    )
    .await;

    // Enrichment fields
    assert_eq!(
        payload["project_name"].as_str().unwrap(),
        "My Project",
        "project_name must be present"
    );
    assert_eq!(
        payload["session_title"].as_str().unwrap(),
        "My Ideation Session",
        "session_title must be present"
    );
    assert_eq!(
        payload["presentation_kind"].as_str().unwrap(),
        "plan_created",
        "presentation_kind must be plan_created"
    );
    let hc = payload["human_context"].as_str().unwrap();
    assert!(!hc.is_empty(), "human_context must not be empty");
    assert!(hc.contains("My Project"), "human_context must include project_name");
    assert!(hc.contains("My Ideation Session"), "human_context must include session_title");

    // Backward compat: original fields still present
    assert_eq!(payload["session_id"].as_str().unwrap(), "session-pc-test");
    assert_eq!(payload["project_id"].as_str().unwrap(), project_id);
    assert_eq!(payload["artifact_id"].as_str().unwrap(), "artifact-pc-test");
    assert_eq!(payload["plan_title"].as_str().unwrap(), "My Plan");
    assert!(payload.get("timestamp").is_some(), "timestamp must be present");
}

// ============================================================================
// Test 2 — None-omit: when session has no title, session_title key is absent
// ============================================================================

#[tokio::test]
async fn test_plan_created_without_session_title_omits_key() {
    let events_repo = Arc::new(MemoryExternalEventsRepository::new());
    let publisher: Arc<dyn WebhookPublisherTrait> = Arc::new(NoOpWebhookPublisher);
    let project_id = "proj-pc-none-omit-test";

    let base_payload = serde_json::json!({
        "session_id": "session-pc-none",
        "project_id": project_id,
        "artifact_id": "artifact-pc-none",
        "plan_title": "Plan Without Session Title",
        "timestamp": "2026-01-01T00:00:00Z",
    });

    // session_title is None — must NOT appear in the stored payload
    let ctx = WebhookPresentationContext {
        project_name: Some("My Project".to_string()),
        session_title: None,
        task_title: None,
        presentation_kind: Some(PresentationKind::PlanCreated),
    };

    let payload = emit_and_get_payload(
        "ideation:plan_created",
        project_id,
        base_payload,
        ctx,
        &events_repo,
        &publisher,
    )
    .await;

    // session_title key must be ABSENT (not null, not empty string)
    assert!(
        payload.get("session_title").is_none(),
        "session_title key must be absent when session has no title, got: {:?}",
        payload.get("session_title")
    );

    // project_name and human_context must still be present (no session_title, just project)
    assert_eq!(
        payload["project_name"].as_str().unwrap(),
        "My Project",
        "project_name must still be present"
    );
    // human_context with only project_name = "[My Project]"
    let hc = payload["human_context"].as_str().unwrap();
    assert!(hc.contains("My Project"), "human_context must include project_name");

    // presentation_kind must still be present
    assert_eq!(
        payload["presentation_kind"].as_str().unwrap(),
        "plan_created"
    );
}

// ============================================================================
// Test 3 — ideation:verified enrichment: all presentation fields present
// ============================================================================

#[tokio::test]
async fn test_verified_payload_includes_enrichment_fields() {
    let events_repo = Arc::new(MemoryExternalEventsRepository::new());
    let publisher: Arc<dyn WebhookPublisherTrait> = Arc::new(NoOpWebhookPublisher);
    let project_id = "proj-verified-enrichment-test";

    let base_payload = serde_json::json!({
        "session_id": "session-verified-test",
        "project_id": project_id,
        "convergence_reason": "All gaps resolved",
        "timestamp": "2026-01-01T00:00:00Z",
    });

    let ctx = WebhookPresentationContext {
        project_name: Some("Verified Project".to_string()),
        session_title: Some("Verification Session".to_string()),
        task_title: None,
        presentation_kind: Some(PresentationKind::Verified),
    };

    let payload = emit_and_get_payload(
        "ideation:verified",
        project_id,
        base_payload,
        ctx,
        &events_repo,
        &publisher,
    )
    .await;

    // Enrichment fields
    assert_eq!(
        payload["project_name"].as_str().unwrap(),
        "Verified Project"
    );
    assert_eq!(
        payload["session_title"].as_str().unwrap(),
        "Verification Session"
    );
    assert_eq!(
        payload["presentation_kind"].as_str().unwrap(),
        "verified",
        "presentation_kind must be verified"
    );
    let hc = payload["human_context"].as_str().unwrap();
    assert!(hc.contains("Verified Project"), "human_context must include project_name");
    assert!(hc.contains("Verification Session"), "human_context must include session_title");

    // Backward compat: original fields still present
    assert_eq!(payload["session_id"].as_str().unwrap(), "session-verified-test");
    assert_eq!(payload["project_id"].as_str().unwrap(), project_id);
    assert_eq!(payload["convergence_reason"].as_str().unwrap(), "All gaps resolved");
    assert!(payload.get("timestamp").is_some(), "timestamp must be present");
}

// ============================================================================
// Test 4 — ideation:session_accepted enrichment: all presentation fields present
// ============================================================================

#[tokio::test]
async fn test_session_accepted_payload_includes_enrichment_fields() {
    let events_repo = Arc::new(MemoryExternalEventsRepository::new());
    let publisher: Arc<dyn WebhookPublisherTrait> = Arc::new(NoOpWebhookPublisher);
    let project_id = "proj-accepted-enrichment-test";

    let base_payload = serde_json::json!({
        "session_id": "session-accepted-test",
        "project_id": project_id,
        "timestamp": "2026-01-01T00:00:00Z",
    });

    let ctx = WebhookPresentationContext {
        project_name: Some("Accepted Project".to_string()),
        session_title: Some("Planning Session".to_string()),
        task_title: None,
        presentation_kind: Some(PresentationKind::SessionAccepted),
    };

    let payload = emit_and_get_payload(
        "ideation:session_accepted",
        project_id,
        base_payload,
        ctx,
        &events_repo,
        &publisher,
    )
    .await;

    // Enrichment fields
    assert_eq!(
        payload["project_name"].as_str().unwrap(),
        "Accepted Project"
    );
    assert_eq!(
        payload["session_title"].as_str().unwrap(),
        "Planning Session"
    );
    assert_eq!(
        payload["presentation_kind"].as_str().unwrap(),
        "session_accepted",
        "presentation_kind must be session_accepted"
    );
    let hc = payload["human_context"].as_str().unwrap();
    assert!(hc.contains("Accepted Project"), "human_context must include project_name");
    assert!(hc.contains("Planning Session"), "human_context must include session_title");

    // Backward compat: original fields still present
    assert_eq!(payload["session_id"].as_str().unwrap(), "session-accepted-test");
    assert_eq!(payload["project_id"].as_str().unwrap(), project_id);
    assert!(payload.get("timestamp").is_some(), "timestamp must be present");
}

// ============================================================================
// Test 5 — None-omit for ideation:verified: session_title key absent when None
// ============================================================================

#[tokio::test]
async fn test_verified_without_session_title_omits_key() {
    let events_repo = Arc::new(MemoryExternalEventsRepository::new());
    let publisher: Arc<dyn WebhookPublisherTrait> = Arc::new(NoOpWebhookPublisher);
    let project_id = "proj-verified-none-omit-test";

    let base_payload = serde_json::json!({
        "session_id": "session-verified-none",
        "project_id": project_id,
        "convergence_reason": "Converged",
        "timestamp": "2026-01-01T00:00:00Z",
    });

    let ctx = WebhookPresentationContext {
        project_name: Some("Some Project".to_string()),
        session_title: None,
        task_title: None,
        presentation_kind: Some(PresentationKind::Verified),
    };

    let payload = emit_and_get_payload(
        "ideation:verified",
        project_id,
        base_payload,
        ctx,
        &events_repo,
        &publisher,
    )
    .await;

    // session_title key must be ABSENT (not null) when it's None
    assert!(
        payload.get("session_title").is_none(),
        "session_title key must be absent when session has no title"
    );

    // All other fields still present
    assert!(payload.get("project_name").is_some());
    assert!(payload.get("presentation_kind").is_some());
    assert!(payload.get("human_context").is_some());
}

// ============================================================================
// Test 6 — None-omit for ideation:session_accepted: session_title key absent
// ============================================================================

#[tokio::test]
async fn test_session_accepted_without_session_title_omits_key() {
    let events_repo = Arc::new(MemoryExternalEventsRepository::new());
    let publisher: Arc<dyn WebhookPublisherTrait> = Arc::new(NoOpWebhookPublisher);
    let project_id = "proj-accepted-none-omit-test";

    let base_payload = serde_json::json!({
        "session_id": "session-accepted-none",
        "project_id": project_id,
        "timestamp": "2026-01-01T00:00:00Z",
    });

    let ctx = WebhookPresentationContext {
        project_name: Some("Some Project".to_string()),
        session_title: None,
        task_title: None,
        presentation_kind: Some(PresentationKind::SessionAccepted),
    };

    let payload = emit_and_get_payload(
        "ideation:session_accepted",
        project_id,
        base_payload,
        ctx,
        &events_repo,
        &publisher,
    )
    .await;

    // session_title key must be ABSENT when it's None
    assert!(
        payload.get("session_title").is_none(),
        "session_title key must be absent when session has no title"
    );

    // project_name, presentation_kind, human_context still present
    assert!(payload.get("project_name").is_some());
    assert!(payload.get("presentation_kind").is_some());
    assert!(payload.get("human_context").is_some());
}

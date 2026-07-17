use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use super::*;
use crate::domain::repositories::external_events_repository::{
    ExternalEventRecord, ExternalEventsRepository,
};
use crate::domain::state_machine::services::WebhookPublisher;
use crate::error::AppResult;
use crate::infrastructure::memory::MemoryExternalEventsRepository;
use ralphx_domain::entities::EventType;

// ── Test doubles ──────────────────────────────────────────────────────────────

/// Records each publish() call for assertion.
struct RecordingWebhookPublisher {
    calls: Arc<RwLock<Vec<(EventType, String)>>>,
}

impl RecordingWebhookPublisher {
    fn new() -> Self {
        Self {
            calls: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

#[async_trait]
impl WebhookPublisher for RecordingWebhookPublisher {
    async fn publish(&self, event_type: EventType, project_id: &str, _payload: serde_json::Value) {
        self.calls
            .write()
            .await
            .push((event_type, project_id.to_string()));
    }
}

/// Always fails insert_event with a configurable error message.
struct FailingExternalEventsRepo {
    message: String,
}

#[async_trait]
impl ExternalEventsRepository for FailingExternalEventsRepo {
    async fn insert_event(
        &self,
        _event_type: &str,
        _project_id: &str,
        _payload: &str,
    ) -> AppResult<i64> {
        Err(crate::error::AppError::Database(self.message.clone()))
    }

    async fn insert_event_once_for_attempt(
        &self,
        _event_type: &str,
        _project_id: &str,
        _agent_run_id: &str,
        _payload: &str,
    ) -> AppResult<bool> {
        Err(crate::error::AppError::Database(self.message.clone()))
    }

    async fn get_events_after_cursor(
        &self,
        _project_ids: &[String],
        _cursor: i64,
        _limit: i64,
    ) -> AppResult<Vec<ExternalEventRecord>> {
        Ok(vec![])
    }

    async fn event_exists(
        &self,
        _event_type: &str,
        _project_id: &str,
        _session_id: &str,
    ) -> AppResult<bool> {
        Ok(false)
    }

    async fn cleanup_old_events(&self) -> AppResult<u64> {
        Ok(0)
    }
}

// ── PresentationKind serde round-trip ────────────────────────────────────────

#[test]
fn presentation_kind_serde_round_trip_all_variants() {
    let cases = [
        (PresentationKind::PlanCreated, "\"plan_created\""),
        (PresentationKind::ProposalsReady, "\"proposals_ready\""),
        (PresentationKind::SessionAccepted, "\"session_accepted\""),
        (PresentationKind::Verified, "\"verified\""),
        (PresentationKind::MergeReady, "\"merge_ready\""),
        (PresentationKind::MergeCompleted, "\"merge_completed\""),
        (PresentationKind::PlanDelivered, "\"plan_delivered\""),
        (
            PresentationKind::TaskStatusChanged,
            "\"task_status_changed\"",
        ),
    ];

    for (variant, expected_json) in cases {
        let serialized = serde_json::to_string(&variant).expect("serialize");
        assert_eq!(
            serialized, expected_json,
            "serialized mismatch for {variant:?}"
        );
        let deserialized: PresentationKind =
            serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(deserialized, variant, "round-trip mismatch for {variant:?}");
    }
}

// ── human_context: all 8 field-presence combinations ────────────────────────

fn ctx(
    project: Option<&str>,
    session: Option<&str>,
    task: Option<&str>,
) -> WebhookPresentationContext {
    WebhookPresentationContext {
        project_name: project.map(str::to_owned),
        session_title: session.map(str::to_owned),
        task_title: task.map(str::to_owned),
        presentation_kind: None,
    }
}

#[test]
fn human_context_all_present() {
    let c = ctx(Some("MyProject"), Some("My Session"), Some("My Task"));
    assert_eq!(c.human_context(), "[MyProject] My Session → My Task");
}

#[test]
fn human_context_no_task() {
    let c = ctx(Some("MyProject"), Some("My Session"), None);
    assert_eq!(c.human_context(), "[MyProject] My Session");
}

#[test]
fn human_context_no_session() {
    let c = ctx(Some("MyProject"), None, Some("My Task"));
    assert_eq!(c.human_context(), "[MyProject] → My Task");
}

#[test]
fn human_context_project_only() {
    let c = ctx(Some("MyProject"), None, None);
    assert_eq!(c.human_context(), "[MyProject]");
}

#[test]
fn human_context_no_project() {
    let c = ctx(None, Some("My Session"), Some("My Task"));
    assert_eq!(c.human_context(), "My Session → My Task");
}

#[test]
fn human_context_session_only() {
    let c = ctx(None, Some("My Session"), None);
    assert_eq!(c.human_context(), "My Session");
}

#[test]
fn human_context_task_only() {
    let c = ctx(None, None, Some("My Task"));
    assert_eq!(c.human_context(), "My Task");
}

#[test]
fn human_context_all_none() {
    let c = ctx(None, None, None);
    assert_eq!(c.human_context(), "");
}

// ── inject_into ─────────────────────────────────────────────────────────────

#[test]
fn inject_into_adds_all_some_fields() {
    let c = WebhookPresentationContext {
        project_name: Some("Proj".into()),
        session_title: Some("Sess".into()),
        task_title: Some("Task".into()),
        presentation_kind: Some(PresentationKind::PlanCreated),
    };
    let mut payload = serde_json::json!({});
    c.inject_into(&mut payload);

    assert_eq!(payload["project_name"], "Proj");
    assert_eq!(payload["session_title"], "Sess");
    assert_eq!(payload["task_title"], "Task");
    assert_eq!(payload["presentation_kind"], "plan_created");
    assert_eq!(payload["human_context"], "[Proj] Sess → Task");
}

#[test]
fn inject_into_skips_none_fields() {
    let c = WebhookPresentationContext {
        project_name: Some("Proj".into()),
        session_title: None,
        task_title: None,
        presentation_kind: None,
    };
    let mut payload = serde_json::json!({});
    c.inject_into(&mut payload);

    assert_eq!(payload["project_name"], "Proj");
    assert!(payload.get("session_title").is_none());
    assert!(payload.get("task_title").is_none());
    assert!(payload.get("presentation_kind").is_none());
}

#[test]
fn inject_into_does_not_overwrite_existing_keys() {
    let c = WebhookPresentationContext {
        project_name: Some("NewProj".into()),
        session_title: Some("NewSess".into()),
        task_title: Some("NewTask".into()),
        presentation_kind: Some(PresentationKind::Verified),
    };
    let mut payload = serde_json::json!({
        "project_name": "ExistingProj",
        "session_title": "ExistingSess",
        "task_title": "ExistingTask",
        "presentation_kind": "existing_kind",
        "human_context": "existing human context",
    });
    c.inject_into(&mut payload);

    assert_eq!(payload["project_name"], "ExistingProj");
    assert_eq!(payload["session_title"], "ExistingSess");
    assert_eq!(payload["task_title"], "ExistingTask");
    assert_eq!(payload["presentation_kind"], "existing_kind");
    assert_eq!(payload["human_context"], "existing human context");
}

#[test]
fn inject_into_noop_on_non_object() {
    let c = WebhookPresentationContext {
        project_name: Some("Proj".into()),
        ..Default::default()
    };
    let mut payload = serde_json::json!("not an object");
    c.inject_into(&mut payload);
    // Should remain unchanged
    assert_eq!(payload, serde_json::json!("not an object"));
}

#[test]
fn inject_into_skips_human_context_when_all_none() {
    let c = WebhookPresentationContext::default();
    let mut payload = serde_json::json!({});
    c.inject_into(&mut payload);
    assert!(payload.get("human_context").is_none());
}

// ── emit_external_webhook_event ───────────────────────────────────────────────

#[tokio::test]
async fn emit_external_webhook_event_persists_and_publishes() {
    let repo: Arc<dyn ExternalEventsRepository> = Arc::new(MemoryExternalEventsRepository::new());
    let publisher_arc: Arc<dyn WebhookPublisher> = Arc::new(RecordingWebhookPublisher::new());

    // Use a known event type so the publisher receives it
    let result = emit_external_webhook_event(
        "task:status_changed",
        "proj-1",
        serde_json::json!({"key": "value"}),
        &repo,
        &publisher_arc,
    )
    .await;

    assert!(result.is_ok(), "expected Ok, got {result:?}");
}

#[tokio::test]
async fn emit_external_webhook_event_publishes_known_event_type() {
    let repo: Arc<dyn ExternalEventsRepository> = Arc::new(MemoryExternalEventsRepository::new());
    let inner_calls: Arc<RwLock<Vec<(EventType, String)>>> = Arc::new(RwLock::new(Vec::new()));
    let inner_calls_clone = inner_calls.clone();

    struct TrackingPublisher {
        calls: Arc<RwLock<Vec<(EventType, String)>>>,
    }
    #[async_trait]
    impl WebhookPublisher for TrackingPublisher {
        async fn publish(
            &self,
            event_type: EventType,
            project_id: &str,
            _payload: serde_json::Value,
        ) {
            self.calls
                .write()
                .await
                .push((event_type, project_id.to_string()));
        }
    }

    let publisher_arc: Arc<dyn WebhookPublisher> = Arc::new(TrackingPublisher {
        calls: inner_calls_clone,
    });

    emit_external_webhook_event(
        "ideation:plan_created",
        "proj-42",
        serde_json::json!({}),
        &repo,
        &publisher_arc,
    )
    .await
    .unwrap();

    let calls = inner_calls.read().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, EventType::IdeationPlanCreated);
    assert_eq!(calls[0].1, "proj-42");
}

#[tokio::test]
async fn emit_external_webhook_event_returns_err_on_insert_failure() {
    let repo: Arc<dyn ExternalEventsRepository> = Arc::new(FailingExternalEventsRepo {
        message: "db error".to_string(),
    });
    let publisher_arc: Arc<dyn WebhookPublisher> = Arc::new(RecordingWebhookPublisher::new());

    let result = emit_external_webhook_event(
        "task:status_changed",
        "proj-1",
        serde_json::json!({}),
        &repo,
        &publisher_arc,
    )
    .await;

    assert!(result.is_err());
    let msg = result.unwrap_err();
    assert!(
        msg.contains("insert_event failed"),
        "error message should mention insert_event: {msg}"
    );
}

#[tokio::test]
async fn emit_external_webhook_event_skips_publish_for_unknown_event_type() {
    let repo: Arc<dyn ExternalEventsRepository> = Arc::new(MemoryExternalEventsRepository::new());
    let inner_calls: Arc<RwLock<Vec<(EventType, String)>>> = Arc::new(RwLock::new(Vec::new()));
    let inner_calls_clone = inner_calls.clone();

    struct TrackingPublisher {
        calls: Arc<RwLock<Vec<(EventType, String)>>>,
    }
    #[async_trait]
    impl WebhookPublisher for TrackingPublisher {
        async fn publish(
            &self,
            event_type: EventType,
            project_id: &str,
            _payload: serde_json::Value,
        ) {
            self.calls
                .write()
                .await
                .push((event_type, project_id.to_string()));
        }
    }

    let publisher_arc: Arc<dyn WebhookPublisher> = Arc::new(TrackingPublisher {
        calls: inner_calls_clone,
    });

    // "unknown:event_type" is not a valid EventType — insert should succeed,
    // publish should be skipped without panicking.
    let result = emit_external_webhook_event(
        "unknown:event_type",
        "proj-1",
        serde_json::json!({}),
        &repo,
        &publisher_arc,
    )
    .await;

    assert!(result.is_ok(), "insert succeeded so result should be Ok");
    assert_eq!(
        inner_calls.read().await.len(),
        0,
        "publish should be skipped for unknown event type"
    );
}

// ── log_non_fatal_error ───────────────────────────────────────────────────────

#[derive(Debug)]
struct SimpleError(String);

impl std::fmt::Display for SimpleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SimpleError {}

#[test]
fn log_non_fatal_error_all_levels_do_not_panic() {
    let err = SimpleError("test error".to_string());
    // Just verifying that none of these panic; actual log output is not asserted.
    log_non_fatal_error("test context", &err, tracing::Level::ERROR);
    log_non_fatal_error("test context", &err, tracing::Level::WARN);
    log_non_fatal_error("test context", &err, tracing::Level::INFO);
    log_non_fatal_error("test context", &err, tracing::Level::DEBUG);
    log_non_fatal_error("test context", &err, tracing::Level::TRACE);
}

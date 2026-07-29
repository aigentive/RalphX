#[cfg(test)]
mod tests {
    use std::collections::{HashMap, VecDeque};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use tokio::sync::RwLock;

    use crate::domain::repositories::external_events_repository::ExternalEventsRepository;
    use crate::domain::repositories::{WebhookRegistration, WebhookRegistrationRepository};
    use crate::domain::services::payload_enrichment::{PresentationKind, WebhookPresentationContext};
    use crate::domain::state_machine::services::WebhookPublisher as WebhookPublisherTrait;
    use crate::infrastructure::memory::{
        MemoryExternalEventsRepository, MemoryWebhookRegistrationRepository,
    };
    use crate::infrastructure::webhook_http_client::{
        MockWebhookHttpClient, WebhookDeliveryError, WebhookHttpClient,
    };
    use crate::infrastructure::webhook_publisher::{compute_hmac_signature, WebhookPublisher};
    use ralphx_domain::entities::EventType;

    // ============================================================================
    // RecordingWebhookPublisher — captures publish() calls for assertions
    // ============================================================================

    struct RecordingWebhookPublisher {
        calls: Arc<RwLock<Vec<(EventType, String, serde_json::Value)>>>,
    }

    impl RecordingWebhookPublisher {
        fn new() -> Self {
            Self {
                calls: Arc::new(RwLock::new(Vec::new())),
            }
        }

        async fn call_count(&self) -> usize {
            self.calls.read().await.len()
        }

        async fn count_for(&self, event_type: &str) -> usize {
            let calls = self.calls.read().await;
            calls
                .iter()
                .filter(|(et, _, _)| et.to_string() == event_type)
                .count()
        }

        /// Returns the captured payload for the first call matching `event_type`.
        async fn get_payload_for(&self, event_type: &str) -> Option<serde_json::Value> {
            let calls = self.calls.read().await;
            calls
                .iter()
                .find(|(et, _, _)| et.to_string() == event_type)
                .map(|(_, _, payload)| payload.clone())
        }
    }

    #[async_trait]
    impl WebhookPublisherTrait for RecordingWebhookPublisher {
        async fn publish(
            &self,
            event_type: EventType,
            project_id: &str,
            payload: serde_json::Value,
        ) {
            self.calls
                .write()
                .await
                .push((event_type, project_id.to_string(), payload));
        }
    }

    // ============================================================================
    // Dual-channel helper — mirrors the production emit pattern:
    //   1. insert_event into ExternalEventsRepository
    //   2. publish via WebhookPublisher
    // ============================================================================

    async fn emit_both_channels(
        event_type: EventType,
        project_id: &str,
        task_id: &str,
        events_repo: &MemoryExternalEventsRepository,
        publisher: &RecordingWebhookPublisher,
    ) {
        let payload = serde_json::json!({
            "task_id": task_id,
            "project_id": project_id,
            "timestamp": "2026-01-01T00:00:00Z",
        });
        events_repo
            .insert_event(&event_type.to_string(), project_id, &payload.to_string())
            .await
            .expect("insert_event must not fail");
        publisher
            .publish(event_type, project_id, payload)
            .await;
    }

    // ============================================================================
    // SequencedMockHttpClient — returns a pre-defined sequence of status codes
    // ============================================================================

    struct SequencedMockHttpClient {
        responses: Mutex<VecDeque<u16>>,
        calls: Mutex<Vec<String>>,
    }

    impl SequencedMockHttpClient {
        fn new(responses: Vec<u16>) -> Self {
            Self {
                responses: Mutex::new(responses.into_iter().collect()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl WebhookHttpClient for SequencedMockHttpClient {
        async fn post(
            &self,
            url: &str,
            _body: Vec<u8>,
            _headers: HashMap<String, String>,
        ) -> Result<u16, WebhookDeliveryError> {
            self.calls.lock().unwrap().push(url.to_string());
            let status = self
                .responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(200);
            Ok(status)
        }
    }

    // ============================================================================
    // Test helpers
    // ============================================================================

    fn make_registration(id: &str, url: &str, project_id: &str) -> WebhookRegistration {
        WebhookRegistration {
            id: id.to_string(),
            api_key_id: "key-1".to_string(),
            url: url.to_string(),
            event_types: None, // matches all events
            project_ids: format!(r#"["{}"]"#, project_id),
            secret: "test-secret-key".to_string(),
            active: true,
            failure_count: 0,
            last_failure_at: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }


    async fn seed_repo(
        repo: &MemoryWebhookRegistrationRepository,
        reg: WebhookRegistration,
    ) {
        repo.upsert(reg).await.unwrap();
    }

    // ============================================================================
    // Test 1: Delivery success — publish() with a matching webhook, 200 → 1 call
    // ============================================================================

    #[tokio::test]
    async fn test_delivery_success_records_one_call() {
        let repo = Arc::new(MemoryWebhookRegistrationRepository::new());
        seed_repo(
            &repo,
            make_registration("wh-1", "http://example.com/hook", "proj-1"),
        )
        .await;

        let mock_client = Arc::new(MockWebhookHttpClient::new(200));
        let publisher =
            WebhookPublisher::new(Arc::clone(&repo) as _, Arc::clone(&mock_client) as _);

        publisher
            .publish(
                EventType::TaskCreated,
                "proj-1",
                serde_json::json!({"task_id": "task-1"}),
            )
            .await;

        // Let the spawned task complete
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(mock_client.call_count(), 1, "Expected exactly 1 HTTP call on success");

        let calls = mock_client.calls.lock().unwrap();
        assert_eq!(calls[0].url, "http://example.com/hook");
    }

    // ============================================================================
    // Test 2: Retry on 5xx — [503, 503, 200] → 3 calls total
    // ============================================================================

    #[tokio::test]
    async fn test_retry_on_5xx_makes_three_calls() {
        let repo = Arc::new(MemoryWebhookRegistrationRepository::new());
        seed_repo(
            &repo,
            make_registration("wh-2", "http://example.com/hook-retry", "proj-2"),
        )
        .await;

        // Respond with two 503s then success
        let seq_client = Arc::new(SequencedMockHttpClient::new(vec![503, 503, 200]));
        let publisher =
            WebhookPublisher::new(Arc::clone(&repo) as _, Arc::clone(&seq_client) as _);

        publisher
            .publish(
                EventType::TaskCreated,
                "proj-2",
                serde_json::json!({"task_id": "task-2"}),
            )
            .await;

        // Wait long enough for 0s + 1s + 2s backoff delays plus margin
        tokio::time::sleep(Duration::from_secs(4)).await;

        assert_eq!(
            seq_client.call_count(),
            3,
            "Expected 3 calls: two 503s retried, then 200 success"
        );
    }

    // ============================================================================
    // Test 3: No retry on 4xx — 404 response → only 1 call
    // ============================================================================

    #[tokio::test]
    async fn test_no_retry_on_4xx() {
        let repo = Arc::new(MemoryWebhookRegistrationRepository::new());
        seed_repo(
            &repo,
            make_registration("wh-3", "http://example.com/hook-404", "proj-3"),
        )
        .await;

        let mock_client = Arc::new(MockWebhookHttpClient::new(404));
        let publisher =
            WebhookPublisher::new(Arc::clone(&repo) as _, Arc::clone(&mock_client) as _);

        publisher
            .publish(
                EventType::TaskCreated,
                "proj-3",
                serde_json::json!({"task_id": "task-3"}),
            )
            .await;

        // Short wait — no retries expected
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(
            mock_client.call_count(),
            1,
            "4xx responses must NOT be retried"
        );
    }

    // ============================================================================
    // Test 4: Failure tracking after exhausted retries — failure_count incremented
    // ============================================================================

    #[tokio::test]
    async fn test_failure_count_incremented_after_exhausted_retries() {
        let repo = Arc::new(MemoryWebhookRegistrationRepository::new());
        let reg = make_registration("wh-4", "http://example.com/hook-fail", "proj-4");
        seed_repo(&repo, reg).await;

        // All 3 attempts return 503 — retries exhausted
        let seq_client = Arc::new(SequencedMockHttpClient::new(vec![503, 503, 503]));
        let publisher =
            WebhookPublisher::new(Arc::clone(&repo) as _, Arc::clone(&seq_client) as _);

        publisher
            .publish(
                EventType::TaskCreated,
                "proj-4",
                serde_json::json!({"task_id": "task-4"}),
            )
            .await;

        // Wait for all three attempts (0s + 1s + 2s) plus margin
        tokio::time::sleep(Duration::from_secs(4)).await;

        assert_eq!(
            seq_client.call_count(),
            3,
            "All 3 retry attempts must be made"
        );

        let stored = repo.get_by_id("wh-4").await.unwrap().unwrap();
        assert_eq!(
            stored.failure_count, 1,
            "failure_count must be incremented after exhausted retries"
        );
    }

    // ============================================================================
    // Test 5: Cache invalidation — after invalidate_project(), next publish fetches fresh
    // ============================================================================

    #[tokio::test]
    async fn test_cache_invalidation_forces_fresh_repo_read() {
        let repo = Arc::new(MemoryWebhookRegistrationRepository::new());

        // Start with one webhook
        seed_repo(
            &repo,
            make_registration("wh-5a", "http://example.com/hook-a", "proj-5"),
        )
        .await;

        let mock_client = Arc::new(MockWebhookHttpClient::new(200));
        let publisher =
            WebhookPublisher::new(Arc::clone(&repo) as _, Arc::clone(&mock_client) as _);

        // First publish — populates cache from repo
        publisher
            .publish(
                EventType::TaskCreated,
                "proj-5",
                serde_json::json!({"v": 1}),
            )
            .await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(mock_client.call_count(), 1, "First publish must call 1 webhook");

        // Add a second webhook to the repo while cache still holds old data
        seed_repo(
            &repo,
            make_registration("wh-5b", "http://example.com/hook-b", "proj-5"),
        )
        .await;

        // Without invalidation: cache still serves old list (1 webhook)
        publisher
            .publish(
                EventType::TaskCreated,
                "proj-5",
                serde_json::json!({"v": 2}),
            )
            .await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            mock_client.call_count(),
            2,
            "Without invalidation, cached list (1 item) still used"
        );

        // Invalidate and publish again — should pick up both webhooks
        publisher.invalidate_project("proj-5");
        publisher
            .publish(
                EventType::TaskCreated,
                "proj-5",
                serde_json::json!({"v": 3}),
            )
            .await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            mock_client.call_count(),
            4,
            "After invalidation, fresh repo read returns 2 webhooks — total calls = 4"
        );
    }

    // ============================================================================
    // Test 6: HMAC signature correctness
    // ============================================================================

    #[test]
    fn test_hmac_signature_format_and_correctness() {
        let secret = "my-webhook-secret";
        let payload = b"hello world";

        let signature = compute_hmac_signature(secret, payload).expect("HMAC should not fail");

        // Must be exactly 64 lowercase hex chars (SHA256 output = 32 bytes = 64 hex chars)
        assert_eq!(signature.len(), 64, "HMAC-SHA256 hex must be 64 chars");
        assert!(
            signature.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "Signature must be lowercase hex"
        );

        // Must be deterministic
        let signature2 = compute_hmac_signature(secret, payload).expect("HMAC should not fail");
        assert_eq!(signature, signature2, "HMAC signature must be deterministic");

        // Different secrets must produce different signatures
        let other_sig =
            compute_hmac_signature("other-secret", payload).expect("HMAC should not fail");
        assert_ne!(
            signature, other_sig,
            "Different secrets must produce different HMAC signatures"
        );
    }

    // ============================================================================
    // Test 7: Execution lifecycle — task:execution_started + task:execution_completed
    //   Both events must appear exactly once in both channels.
    // ============================================================================

    #[tokio::test]
    async fn test_execution_lifecycle_webhooks() {
        let events_repo = MemoryExternalEventsRepository::new();
        let publisher = RecordingWebhookPublisher::new();
        let project_id = "proj-exec";
        let task_id = "task-exec-1";

        // Fire task:execution_started
        emit_both_channels(
            EventType::TaskExecutionStarted,
            project_id,
            task_id,
            &events_repo,
            &publisher,
        )
        .await;

        // Fire task:execution_completed
        emit_both_channels(
            EventType::TaskExecutionCompleted,
            project_id,
            task_id,
            &events_repo,
            &publisher,
        )
        .await;

        // Channel 1: external_events repo
        let rows = events_repo
            .get_events_after_cursor(&[project_id.to_string()], 0, 100)
            .await
            .unwrap();
        assert_eq!(
            rows.iter()
                .filter(|e| e.event_type == "task:execution_started")
                .count(),
            1,
            "task:execution_started must appear exactly once in external_events"
        );
        assert_eq!(
            rows.iter()
                .filter(|e| e.event_type == "task:execution_completed")
                .count(),
            1,
            "task:execution_completed must appear exactly once in external_events"
        );

        // Channel 2: webhook publisher
        assert_eq!(
            publisher.count_for("task:execution_started").await,
            1,
            "task:execution_started must be published exactly once"
        );
        assert_eq!(
            publisher.count_for("task:execution_completed").await,
            1,
            "task:execution_completed must be published exactly once"
        );

        // Dedup: total call counts match exactly
        assert_eq!(rows.len(), 2, "Exactly 2 rows total in external_events");
        assert_eq!(
            publisher.call_count().await,
            2,
            "Exactly 2 webhook publishes total"
        );
    }

    // ============================================================================
    // Test 8: Review lifecycle — review:ready, review:approved,
    //   review:changes_requested, review:escalated
    //   All 4 events must appear exactly once in both channels.
    // ============================================================================

    #[tokio::test]
    async fn test_review_lifecycle_webhooks() {
        let events_repo = MemoryExternalEventsRepository::new();
        let publisher = RecordingWebhookPublisher::new();
        let project_id = "proj-review";
        let task_id = "task-review-1";

        for event_type in [
            EventType::ReviewReady,
            EventType::ReviewApproved,
            EventType::ReviewChangesRequested,
            EventType::ReviewEscalated,
        ] {
            emit_both_channels(event_type, project_id, task_id, &events_repo, &publisher).await;
        }

        let rows = events_repo
            .get_events_after_cursor(&[project_id.to_string()], 0, 100)
            .await
            .unwrap();

        for (event_str, label) in [
            ("review:ready", "review:ready"),
            ("review:approved", "review:approved"),
            ("review:changes_requested", "review:changes_requested"),
            ("review:escalated", "review:escalated"),
        ] {
            assert_eq!(
                rows.iter().filter(|e| e.event_type == event_str).count(),
                1,
                "{label} must appear exactly once in external_events"
            );
            assert_eq!(
                publisher.count_for(event_str).await,
                1,
                "{label} must be published exactly once"
            );
        }

        // Dedup: exactly 4 events total, no duplicates
        assert_eq!(rows.len(), 4, "Exactly 4 rows total in external_events");
        assert_eq!(
            publisher.call_count().await,
            4,
            "Exactly 4 webhook publishes total"
        );
    }

    // ============================================================================
    // Test 9: Merge lifecycle — merge:completed + merge:conflict
    //   Both events must appear exactly once in both channels.
    //   Tests both agent-path and programmatic-path event shapes.
    // ============================================================================

    #[tokio::test]
    async fn test_merge_lifecycle_webhooks() {
        let events_repo = MemoryExternalEventsRepository::new();
        let publisher = RecordingWebhookPublisher::new();
        let project_id = "proj-merge";
        let task_id = "task-merge-1";

        // merge:completed (agent path — git.rs complete_merge)
        emit_both_channels(
            EventType::MergeCompleted,
            project_id,
            task_id,
            &events_repo,
            &publisher,
        )
        .await;

        // merge:conflict (agent path — git.rs report_conflict)
        emit_both_channels(
            EventType::MergeConflict,
            project_id,
            task_id,
            &events_repo,
            &publisher,
        )
        .await;

        let rows = events_repo
            .get_events_after_cursor(&[project_id.to_string()], 0, 100)
            .await
            .unwrap();

        assert_eq!(
            rows.iter()
                .filter(|e| e.event_type == "merge:completed")
                .count(),
            1,
            "merge:completed must appear exactly once in external_events"
        );
        assert_eq!(
            rows.iter()
                .filter(|e| e.event_type == "merge:conflict")
                .count(),
            1,
            "merge:conflict must appear exactly once in external_events"
        );

        assert_eq!(
            publisher.count_for("merge:completed").await,
            1,
            "merge:completed must be published exactly once"
        );
        assert_eq!(
            publisher.count_for("merge:conflict").await,
            1,
            "merge:conflict must be published exactly once"
        );

        // Dedup: total counts
        assert_eq!(rows.len(), 2, "Exactly 2 rows total in external_events");
        assert_eq!(
            publisher.call_count().await,
            2,
            "Exactly 2 webhook publishes total"
        );
    }

    #[tokio::test]
    async fn test_x_webhook_signature_header_present() {
        let repo = Arc::new(MemoryWebhookRegistrationRepository::new());
        let mut reg = make_registration("wh-sig", "http://example.com/hook-sig", "proj-sig");
        reg.secret = "known-secret".to_string();
        seed_repo(&repo, reg).await;

        let mock_client = Arc::new(MockWebhookHttpClient::new(200));
        let publisher =
            WebhookPublisher::new(Arc::clone(&repo) as _, Arc::clone(&mock_client) as _);

        publisher
            .publish(
                EventType::TaskCreated,
                "proj-sig",
                serde_json::json!({"task_id": "task-sig"}),
            )
            .await;

        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(mock_client.call_count(), 1);

        let calls = mock_client.calls.lock().unwrap();
        let sig_header = calls[0]
            .headers
            .get("X-Webhook-Signature")
            .expect("X-Webhook-Signature header must be present");

        // Must start with "sha256="
        assert!(
            sig_header.starts_with("sha256="),
            "X-Webhook-Signature must be prefixed with 'sha256=', got: {sig_header}"
        );

        // The hex part after "sha256=" must be 64 lowercase hex chars
        let hex_part = &sig_header["sha256=".len()..];
        assert_eq!(hex_part.len(), 64, "Hex part of signature must be 64 chars");
        assert!(
            hex_part
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "Hex part must be lowercase hex"
        );
    }

    // ============================================================================
    // Test 11: merge:completed payload captures human_context and presentation_kind
    // ============================================================================

    #[tokio::test]
    async fn test_merge_completed_payload_captures_presentation_fields() {
        let publisher = RecordingWebhookPublisher::new();

        // Build a merge:completed payload that mirrors what complete_merge_internal emits
        let ctx = WebhookPresentationContext {
            project_name: Some("Test Project".to_string()),
            session_title: Some("Test Session".to_string()),
            task_title: Some("Test Task".to_string()),
            presentation_kind: Some(PresentationKind::MergeCompleted),
        };
        let mut payload = serde_json::json!({
            "task_id": "task-wp-test",
            "project_id": "proj-wp-test",
            "session_id": "session-wp-test",
            "category": "regular",
            "source_branch": "ralphx/task/test",
            "target_branch": "main",
            "commit_sha": "abc123def456",
            "timestamp": "2026-01-01T00:00:00Z",
        });
        ctx.inject_into(&mut payload);

        publisher
            .publish(EventType::MergeCompleted, "proj-wp-test", payload.clone())
            .await;

        // Payload must be captured by the recording publisher
        let captured = publisher
            .get_payload_for("merge:completed")
            .await
            .expect("merge:completed payload must be captured");

        // Enrichment fields
        assert_eq!(
            captured["human_context"].as_str().unwrap(),
            "[Test Project] Test Session \u{2192} Test Task",
            "human_context must use [project] session → task format"
        );
        assert_eq!(
            captured["presentation_kind"].as_str().unwrap(),
            "merge_completed",
            "presentation_kind must be merge_completed"
        );
        assert_eq!(
            captured["project_name"].as_str().unwrap(),
            "Test Project"
        );
        assert_eq!(
            captured["session_title"].as_str().unwrap(),
            "Test Session"
        );
        assert_eq!(
            captured["task_title"].as_str().unwrap(),
            "Test Task"
        );

        // Backward compat: original fields still present
        assert_eq!(captured["task_id"].as_str().unwrap(), "task-wp-test");
        assert_eq!(captured["project_id"].as_str().unwrap(), "proj-wp-test");
        assert_eq!(captured["session_id"].as_str().unwrap(), "session-wp-test");
        assert_eq!(captured["commit_sha"].as_str().unwrap(), "abc123def456");
        assert_eq!(captured["target_branch"].as_str().unwrap(), "main");
        assert!(captured.get("timestamp").is_some(), "timestamp must be present");
    }
}

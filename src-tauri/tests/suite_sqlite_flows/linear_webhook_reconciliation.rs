use std::sync::Arc;

use chrono::{Duration, Utc};
use hmac::{Hmac, Mac};
use ralphx_lib::application::linear_webhook_reconciliation_service::{
    ExternalIssueLink, LinearDelivery, LinearDeliveryRecord, LinearWebhookAction,
    LinearWebhookHeaders, LinearWebhookReconciliationService, LinearWebhookRequest,
    LinearWebhookStore, MemoryLinearWebhookStore,
};
use ralphx_lib::domain::entities::{
    ConflictResolution, ExternalStatusMapping, ExternalSyncConfig, InternalStatus, ProjectId,
    SyncDirection, SyncProvider, SyncSettings, TaskId, WorkflowColumn, WorkflowSchema,
};
use ralphx_lib::domain::repositories::WorkflowRepository;
use ralphx_lib::infrastructure::memory::MemoryWorkflowRepository;
use ralphx_lib::infrastructure::sqlite::{DbConnection, SqliteLinearWebhookStore};
use ralphx_lib::testing::SqliteTestDb;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

const SIGNING_SECRET: &str = "linear-webhook-secret";
const DELIVERY_ID: &str = "234d1a4e-b617-4388-90fe-adc3633d6b72";
const WEBHOOK_ID: &str = "000042e3-d123-4980-b49f-8e140eef9329";
const ISSUE_ID: &str = "539068e2-ae88-4d09-bd75-22eb4a59612f";
const TASK_ID: &str = "task-linked-to-linear";
const PROJECT_ID: &str = "project-linear";

fn signature_for(raw_body: &[u8], secret: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(raw_body);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn request_for(raw_body: String, signature: Option<String>) -> LinearWebhookRequest {
    request_for_event(raw_body, signature, "Issue")
}

fn request_for_event(
    raw_body: String,
    signature: Option<String>,
    event: &str,
) -> LinearWebhookRequest {
    LinearWebhookRequest {
        headers: LinearWebhookHeaders {
            signature,
            delivery: Some(DELIVERY_ID.to_string()),
            event: Some(event.to_string()),
        },
        raw_body: raw_body.into_bytes(),
    }
}

fn issue_payload(timestamp_ms: i64, state_name: &str) -> String {
    serde_json::json!({
        "action": "update",
        "type": "Issue",
        "createdAt": "2026-06-16T18:00:00.000Z",
        "organizationId": "linear-org",
        "webhookTimestamp": timestamp_ms,
        "webhookId": WEBHOOK_ID,
        "url": "https://linear.app/acme/issue/LIN-123/example",
        "data": {
            "id": ISSUE_ID,
            "identifier": "LIN-123",
            "title": "Example issue",
            "url": "https://linear.app/acme/issue/LIN-123/example",
            "state": {
                "name": state_name
            }
        }
    })
    .to_string()
}

fn issue_payload_without_state(timestamp_ms: i64) -> String {
    serde_json::json!({
        "action": "update",
        "type": "Issue",
        "createdAt": "2026-06-16T18:00:00.000Z",
        "organizationId": "linear-org",
        "webhookTimestamp": timestamp_ms,
        "webhookId": WEBHOOK_ID,
        "data": {
            "id": ISSUE_ID,
            "identifier": "LIN-123",
            "title": "Example issue",
            "url": "https://linear.app/acme/issue/LIN-123/example"
        }
    })
    .to_string()
}

async fn mapped_workflow_repo() -> Arc<dyn WorkflowRepository> {
    let repo = Arc::new(MemoryWorkflowRepository::new());
    let mut workflow = WorkflowSchema::new(
        "Linear Workflow",
        vec![
            WorkflowColumn::new("ready", "Ready", InternalStatus::Ready),
            WorkflowColumn::new("in_progress", "In Progress", InternalStatus::Executing),
        ],
    )
    .as_default();
    workflow.external_sync = Some(ExternalSyncConfig {
        provider: SyncProvider::Linear,
        mapping: [(
            "In Progress".to_string(),
            ExternalStatusMapping {
                external_status: "In Progress".to_string(),
                internal_status: InternalStatus::Executing,
                column_id: "in_progress".to_string(),
            },
        )]
        .into_iter()
        .collect(),
        sync: SyncSettings {
            direction: SyncDirection::Bidirectional,
            webhook: Some(true),
        },
        conflict_resolution: ConflictResolution::ExternalWins,
    });
    repo.create(workflow).await.unwrap();
    repo
}

async fn unmapped_workflow_repo() -> Arc<dyn WorkflowRepository> {
    let repo = Arc::new(MemoryWorkflowRepository::new());
    repo.create(
        WorkflowSchema::new(
            "No Linear Mapping",
            vec![WorkflowColumn::new("ready", "Ready", InternalStatus::Ready)],
        )
        .as_default(),
    )
    .await
    .unwrap();
    repo
}

async fn service_with(store: Arc<MemoryLinearWebhookStore>) -> LinearWebhookReconciliationService {
    LinearWebhookReconciliationService::new(
        SIGNING_SECRET.to_string(),
        store,
        mapped_workflow_repo().await,
    )
}

fn linked_issue() -> ExternalIssueLink {
    ExternalIssueLink {
        provider: SyncProvider::Linear,
        project_id: ProjectId::from_string(PROJECT_ID.to_string()),
        task_id: Some(TaskId::from_string(TASK_ID.to_string())),
        external_id: ISSUE_ID.to_string(),
        external_key: Some("LIN-123".to_string()),
        external_url: Some("https://linear.app/acme/issue/LIN-123/example".to_string()),
        last_external_status: None,
    }
}

#[tokio::test]
async fn missing_signature_is_rejected_before_body_parse_or_delivery_recording() {
    let store = Arc::new(MemoryLinearWebhookStore::new());
    let service = service_with(Arc::clone(&store)).await;

    let result = service
        .handle(request_for("not-json".to_string(), None), Utc::now())
        .await;

    assert!(result.unwrap_err().is_missing_signature());
    assert_eq!(store.delivery_count().await, 0);
}

#[tokio::test]
async fn invalid_signature_is_rejected_before_body_parse_or_delivery_recording() {
    let store = Arc::new(MemoryLinearWebhookStore::new());
    let service = service_with(Arc::clone(&store)).await;
    let raw_body = issue_payload(Utc::now().timestamp_millis(), "In Progress");
    let signature = signature_for(raw_body.as_bytes(), "wrong-secret");

    let result = service
        .handle(request_for(raw_body, Some(signature)), Utc::now())
        .await;

    assert!(result.unwrap_err().is_invalid_signature());
    assert_eq!(store.delivery_count().await, 0);
}

#[tokio::test]
async fn malformed_raw_body_is_rejected_after_signature_but_before_delivery_recording() {
    let store = Arc::new(MemoryLinearWebhookStore::new());
    let service = service_with(Arc::clone(&store)).await;
    let raw_body = "{malformed-json".to_string();
    let signature = signature_for(raw_body.as_bytes(), SIGNING_SECRET);

    let result = service
        .handle(request_for(raw_body, Some(signature)), Utc::now())
        .await;

    assert!(result.unwrap_err().is_malformed_body());
    assert_eq!(store.delivery_count().await, 0);
}

#[tokio::test]
async fn stale_webhook_timestamp_is_rejected_before_delivery_recording() {
    let store = Arc::new(MemoryLinearWebhookStore::new());
    let service = service_with(Arc::clone(&store)).await;
    let now = Utc::now();
    let raw_body = issue_payload(
        (now - Duration::seconds(90)).timestamp_millis(),
        "In Progress",
    );
    let signature = signature_for(raw_body.as_bytes(), SIGNING_SECRET);

    let result = service
        .handle(request_for(raw_body, Some(signature)), now)
        .await;

    assert!(result.unwrap_err().is_stale_timestamp());
    assert_eq!(store.delivery_count().await, 0);
}

#[tokio::test]
async fn duplicate_linear_delivery_is_idempotent_and_does_not_transition_twice() {
    let store = Arc::new(MemoryLinearWebhookStore::new());
    store.upsert_issue_link(linked_issue()).await.unwrap();
    let service = service_with(Arc::clone(&store)).await;
    let raw_body = issue_payload(Utc::now().timestamp_millis(), "In Progress");
    let signature = signature_for(raw_body.as_bytes(), SIGNING_SECRET);

    let first = service
        .handle(
            request_for(raw_body.clone(), Some(signature.clone())),
            Utc::now(),
        )
        .await
        .unwrap();
    let second = service
        .handle(request_for(raw_body, Some(signature)), Utc::now())
        .await
        .unwrap();

    assert_eq!(
        first.action,
        LinearWebhookAction::TransitionedTask {
            task_id: TaskId::from_string(TASK_ID.to_string()),
            target_status: InternalStatus::Executing,
        }
    );
    assert!(second.duplicate);
    assert_eq!(store.delivery_count().await, 1);
}

#[tokio::test]
async fn issue_webhook_records_unlinked_issue_without_transitioning() {
    let store = Arc::new(MemoryLinearWebhookStore::new());
    let service = service_with(Arc::clone(&store)).await;
    let raw_body = issue_payload(Utc::now().timestamp_millis(), "In Progress");
    let signature = signature_for(raw_body.as_bytes(), SIGNING_SECRET);

    let outcome = service
        .handle(request_for(raw_body, Some(signature)), Utc::now())
        .await
        .unwrap();

    assert_eq!(outcome.action, LinearWebhookAction::RecordedIssue);
    assert_eq!(store.delivery_count().await, 1);
    assert_eq!(store.activity_count().await, 1);
}

#[tokio::test]
async fn issue_webhook_without_state_updates_existing_link_but_reports_no_mapped_status() {
    let store = Arc::new(MemoryLinearWebhookStore::new());
    store.upsert_issue_link(linked_issue()).await.unwrap();
    let service = service_with(Arc::clone(&store)).await;
    let raw_body = issue_payload_without_state(Utc::now().timestamp_millis());
    let signature = signature_for(raw_body.as_bytes(), SIGNING_SECRET);

    let outcome = service
        .handle(request_for(raw_body, Some(signature)), Utc::now())
        .await
        .unwrap();

    assert_eq!(outcome.action, LinearWebhookAction::NoMappedStatus);
    assert_eq!(store.activity_count().await, 1);
}

#[tokio::test]
async fn issue_webhook_with_unmapped_workflow_reports_no_mapped_status() {
    let store = Arc::new(MemoryLinearWebhookStore::new());
    store.upsert_issue_link(linked_issue()).await.unwrap();
    let service = LinearWebhookReconciliationService::new(
        SIGNING_SECRET.to_string(),
        store,
        unmapped_workflow_repo().await,
    );
    let raw_body = issue_payload(Utc::now().timestamp_millis(), "In Progress");
    let signature = signature_for(raw_body.as_bytes(), SIGNING_SECRET);

    let outcome = service
        .handle(request_for(raw_body, Some(signature)), Utc::now())
        .await
        .unwrap();

    assert_eq!(outcome.action, LinearWebhookAction::NoMappedStatus);
}

#[tokio::test]
async fn issue_webhook_with_branchless_link_reports_no_linked_task_after_status_mapping() {
    let store = Arc::new(MemoryLinearWebhookStore::new());
    let mut link = linked_issue();
    link.task_id = None;
    store.upsert_issue_link(link).await.unwrap();
    let service = service_with(Arc::clone(&store)).await;
    let raw_body = issue_payload(Utc::now().timestamp_millis(), "In Progress");
    let signature = signature_for(raw_body.as_bytes(), SIGNING_SECRET);

    let outcome = service
        .handle(request_for(raw_body, Some(signature)), Utc::now())
        .await
        .unwrap();

    assert_eq!(outcome.action, LinearWebhookAction::NoLinkedTask);
}

#[tokio::test]
async fn sqlite_store_uses_generic_external_issue_link_schema_for_issue_transitions() {
    let db = SqliteTestDb::new("linear-webhook-generic-link-schema");
    let store = Arc::new(SqliteLinearWebhookStore::new(DbConnection::from_shared(
        db.shared_conn(),
    )));
    store.upsert_issue_link(linked_issue()).await.unwrap();
    let webhook_store: Arc<dyn LinearWebhookStore> = store.clone();
    let service = LinearWebhookReconciliationService::new(
        SIGNING_SECRET.to_string(),
        webhook_store,
        mapped_workflow_repo().await,
    );
    let raw_body = issue_payload(Utc::now().timestamp_millis(), "In Progress");
    let signature = signature_for(raw_body.as_bytes(), SIGNING_SECRET);

    let outcome = service
        .handle(request_for(raw_body, Some(signature)), Utc::now())
        .await
        .unwrap();

    assert_eq!(
        outcome.action,
        LinearWebhookAction::TransitionedTask {
            task_id: TaskId::from_string(TASK_ID.to_string()),
            target_status: InternalStatus::Executing,
        }
    );
    let link = store
        .get_issue_link(ISSUE_ID)
        .await
        .unwrap()
        .expect("updated Linear issue link should be readable");
    assert_eq!(link.task_id, Some(TaskId::from_string(TASK_ID.to_string())));
    assert_eq!(link.last_external_status.as_deref(), Some("In Progress"));
    let row = db.with_connection(|conn| {
        conn.query_row(
            "SELECT local_object_kind, local_object_id, local_project_id, external_key, local_state
             FROM external_issue_links
             WHERE provider = 'linear' AND external_kind = 'issue' AND external_id = ?1",
            rusqlite::params![ISSUE_ID],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .unwrap()
    });
    assert_eq!(row.0, "task");
    assert_eq!(row.1, TASK_ID);
    assert_eq!(row.2.as_deref(), Some(PROJECT_ID));
    assert_eq!(row.3.as_deref(), Some("LIN-123"));
    assert_eq!(row.4.as_deref(), Some("In Progress"));
}

#[tokio::test]
async fn sqlite_store_persists_config_deliveries_and_issue_activity_idempotently() {
    let db = SqliteTestDb::new("linear-webhook-store-config-delivery");
    let store = SqliteLinearWebhookStore::new(DbConnection::from_shared(db.shared_conn()));

    assert_eq!(store.get_config().await.unwrap(), (false, None));
    store
        .set_signing_secret_ref(Some("linear-secret-ref".to_string()), true)
        .await
        .unwrap();
    assert_eq!(
        store.get_signing_secret_ref().await.unwrap().as_deref(),
        Some("linear-secret-ref")
    );
    assert_eq!(
        store.get_config().await.unwrap(),
        (true, Some("linear-secret-ref".to_string()))
    );

    let delivery = LinearDelivery {
        delivery_id: DELIVERY_ID.to_string(),
        webhook_id: Some(WEBHOOK_ID.to_string()),
        event_type: "Issue".to_string(),
        received_at: Utc::now(),
    };
    assert_eq!(
        store.record_delivery(delivery.clone()).await.unwrap(),
        LinearDeliveryRecord::Recorded
    );
    assert_eq!(
        store.record_delivery(delivery).await.unwrap(),
        LinearDeliveryRecord::Duplicate
    );

    store
        .record_issue_activity(DELIVERY_ID, ISSUE_ID, "Comment")
        .await
        .unwrap();
    store
        .record_issue_activity(DELIVERY_ID, ISSUE_ID, "Comment")
        .await
        .unwrap();
    let event_count = db.with_connection(|conn| {
        conn.query_row(
            "SELECT COUNT(*) FROM external_issue_sync_events
             WHERE provider = 'linear' AND external_id = ?1 AND delivery_id = ?2 AND event_type = 'Comment'",
            rusqlite::params![ISSUE_ID, DELIVERY_ID],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
    });
    assert_eq!(event_count, 1);
}

#[tokio::test]
async fn sqlite_store_rejects_branchless_issue_link_persistence() {
    let db = SqliteTestDb::new("linear-webhook-store-rejects-branchless-link");
    let store = SqliteLinearWebhookStore::new(DbConnection::from_shared(db.shared_conn()));

    let result = store
        .upsert_issue_link(ExternalIssueLink {
            provider: SyncProvider::Linear,
            project_id: ProjectId::from_string(PROJECT_ID.to_string()),
            task_id: None,
            external_id: ISSUE_ID.to_string(),
            external_key: Some("LIN-123".to_string()),
            external_url: None,
            last_external_status: None,
        })
        .await;

    assert!(result
        .unwrap_err()
        .to_string()
        .contains("must be attached to a task"));
}

#[tokio::test]
async fn comment_and_attachment_events_record_issue_activity() {
    let store = Arc::new(MemoryLinearWebhookStore::new());
    let service = service_with(Arc::clone(&store)).await;
    let now = Utc::now();
    let comment_body = serde_json::json!({
        "action": "create",
        "type": "Comment",
        "webhookTimestamp": now.timestamp_millis(),
        "webhookId": WEBHOOK_ID,
        "data": {
            "id": "comment-1",
            "issueId": ISSUE_ID
        }
    })
    .to_string();
    let comment_signature = signature_for(comment_body.as_bytes(), SIGNING_SECRET);
    let comment = service
        .handle(
            request_for_event(comment_body, Some(comment_signature), "Comment"),
            now,
        )
        .await
        .unwrap();

    let attachment_body = serde_json::json!({
        "action": "create",
        "type": "Attachment",
        "webhookTimestamp": now.timestamp_millis(),
        "webhookId": "attachment-webhook-id",
        "data": {
            "id": "attachment-1",
            "issue": {
                "id": ISSUE_ID
            }
        }
    })
    .to_string();
    let attachment_signature = signature_for(attachment_body.as_bytes(), SIGNING_SECRET);
    let attachment = service
        .handle(
            LinearWebhookRequest {
                headers: LinearWebhookHeaders {
                    signature: Some(attachment_signature),
                    delivery: Some("attachment-delivery-id".to_string()),
                    event: Some("Attachment".to_string()),
                },
                raw_body: attachment_body.into_bytes(),
            },
            now,
        )
        .await
        .unwrap();

    assert_eq!(comment.action, LinearWebhookAction::RecordedIssueActivity);
    assert_eq!(
        attachment.action,
        LinearWebhookAction::RecordedIssueActivity
    );
    assert_eq!(store.delivery_count().await, 2);
    assert_eq!(store.activity_count().await, 2);
}

#[tokio::test]
async fn unsupported_verified_event_is_recorded_without_local_mutation() {
    let store = Arc::new(MemoryLinearWebhookStore::new());
    let service = service_with(Arc::clone(&store)).await;
    let raw_body = serde_json::json!({
        "action": "update",
        "type": "Project",
        "webhookTimestamp": Utc::now().timestamp_millis(),
        "webhookId": WEBHOOK_ID,
        "data": {
            "id": "project-from-linear"
        }
    })
    .to_string();
    let signature = signature_for(raw_body.as_bytes(), SIGNING_SECRET);

    let outcome = service
        .handle(request_for(raw_body, Some(signature)), Utc::now())
        .await
        .unwrap();

    assert_eq!(outcome.action, LinearWebhookAction::UnsupportedEvent);
    assert_eq!(store.delivery_count().await, 1);
}

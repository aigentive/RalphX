use std::sync::Arc;

use chrono::{Duration, Utc};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use super::{
    ExternalIssueLink, LinearWebhookAction, LinearWebhookHeaders,
    LinearWebhookReconciliationService, LinearWebhookRequest, LinearWebhookStore,
    MemoryLinearWebhookStore,
};
use crate::domain::entities::{
    ConflictResolution, ExternalStatusMapping, ExternalSyncConfig, InternalStatus, ProjectId,
    SyncDirection, SyncProvider, SyncSettings, TaskId, WorkflowColumn, WorkflowSchema,
};
use crate::domain::repositories::WorkflowRepository;
use crate::infrastructure::memory::MemoryWorkflowRepository;

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
    request_for_event(raw_body, signature, "Issue", Some(DELIVERY_ID.to_string()))
}

fn request_for_event(
    raw_body: String,
    signature: Option<String>,
    event: &str,
    delivery: Option<String>,
) -> LinearWebhookRequest {
    LinearWebhookRequest {
        headers: LinearWebhookHeaders {
            signature,
            delivery,
            event: Some(event.to_string()),
        },
        raw_body: raw_body.into_bytes(),
    }
}

fn issue_payload(timestamp_ms: i64, state_name: Option<&str>) -> String {
    let mut data = serde_json::json!({
        "id": ISSUE_ID,
        "identifier": "LIN-123",
        "title": "Example issue",
        "url": "https://linear.app/acme/issue/LIN-123/example"
    });
    if let Some(state_name) = state_name {
        data["state"] = serde_json::json!({ "name": state_name });
    }
    serde_json::json!({
        "action": "update",
        "type": "Issue",
        "createdAt": "2026-06-16T18:00:00.000Z",
        "organizationId": "linear-org",
        "webhookTimestamp": timestamp_ms,
        "webhookId": WEBHOOK_ID,
        "data": data
    })
    .to_string()
}

fn activity_payload(timestamp_ms: i64, event_type: &str, data: serde_json::Value) -> String {
    serde_json::json!({
        "action": "create",
        "type": event_type,
        "webhookTimestamp": timestamp_ms,
        "webhookId": WEBHOOK_ID,
        "data": data
    })
    .to_string()
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

#[tokio::test]
async fn rejects_invalid_inputs_before_recording_delivery() {
    let store = Arc::new(MemoryLinearWebhookStore::new());
    let service = service_with(Arc::clone(&store)).await;

    let missing_signature = service
        .handle(request_for("not-json".to_string(), None), Utc::now())
        .await
        .unwrap_err();
    assert!(missing_signature.is_missing_signature());

    let raw_body = issue_payload(Utc::now().timestamp_millis(), Some("In Progress"));
    let invalid_signature = service
        .handle(
            request_for(
                raw_body.clone(),
                Some(signature_for(raw_body.as_bytes(), "wrong-secret")),
            ),
            Utc::now(),
        )
        .await
        .unwrap_err();
    assert!(invalid_signature.is_invalid_signature());

    let malformed_body = "{malformed-json".to_string();
    let malformed = service
        .handle(
            request_for(
                malformed_body.clone(),
                Some(signature_for(malformed_body.as_bytes(), SIGNING_SECRET)),
            ),
            Utc::now(),
        )
        .await
        .unwrap_err();
    assert!(malformed.is_malformed_body());

    let now = Utc::now();
    let stale_body = issue_payload(
        (now - Duration::seconds(90)).timestamp_millis(),
        Some("In Progress"),
    );
    let stale = service
        .handle(
            request_for(
                stale_body.clone(),
                Some(signature_for(stale_body.as_bytes(), SIGNING_SECRET)),
            ),
            now,
        )
        .await
        .unwrap_err();
    assert!(stale.is_stale_timestamp());
    assert_eq!(store.delivery_count().await, 0);
}

#[tokio::test]
async fn issue_events_cover_transition_and_non_transition_outcomes() {
    let store = Arc::new(MemoryLinearWebhookStore::new());
    let service = service_with(Arc::clone(&store)).await;
    let raw_body = issue_payload(Utc::now().timestamp_millis(), Some("In Progress"));
    let outcome = service
        .handle(
            request_for(
                raw_body.clone(),
                Some(signature_for(raw_body.as_bytes(), SIGNING_SECRET)),
            ),
            Utc::now(),
        )
        .await
        .unwrap();
    assert_eq!(outcome.action, LinearWebhookAction::RecordedIssue);

    store.upsert_issue_link(linked_issue()).await.unwrap();
    let transition_body = issue_payload(Utc::now().timestamp_millis(), Some("In Progress"));
    let transitioned = service
        .handle(
            request_for_event(
                transition_body.clone(),
                Some(signature_for(transition_body.as_bytes(), SIGNING_SECRET)),
                "Issue",
                Some("delivery-transition".to_string()),
            ),
            Utc::now(),
        )
        .await
        .unwrap();
    assert_eq!(
        transitioned.action,
        LinearWebhookAction::TransitionedTask {
            task_id: TaskId::from_string(TASK_ID.to_string()),
            target_status: InternalStatus::Executing,
        }
    );

    let duplicate = service
        .handle(
            request_for_event(
                transition_body.clone(),
                Some(signature_for(transition_body.as_bytes(), SIGNING_SECRET)),
                "Issue",
                Some("delivery-transition".to_string()),
            ),
            Utc::now(),
        )
        .await
        .unwrap();
    assert!(duplicate.duplicate);
    assert_eq!(duplicate.action, LinearWebhookAction::DuplicateDelivery);

    let no_state_body = issue_payload(Utc::now().timestamp_millis(), None);
    let no_status = service
        .handle(
            request_for_event(
                no_state_body.clone(),
                Some(signature_for(no_state_body.as_bytes(), SIGNING_SECRET)),
                "Issue",
                Some("delivery-no-status".to_string()),
            ),
            Utc::now(),
        )
        .await
        .unwrap();
    assert_eq!(no_status.action, LinearWebhookAction::NoMappedStatus);
}

#[tokio::test]
async fn issue_events_cover_unmapped_workflow_and_branchless_link() {
    let store = Arc::new(MemoryLinearWebhookStore::new());
    store.upsert_issue_link(linked_issue()).await.unwrap();
    let service = LinearWebhookReconciliationService::new(
        SIGNING_SECRET.to_string(),
        Arc::clone(&store) as Arc<dyn LinearWebhookStore>,
        unmapped_workflow_repo().await,
    );
    let raw_body = issue_payload(Utc::now().timestamp_millis(), Some("In Progress"));
    let outcome = service
        .handle(
            request_for(
                raw_body.clone(),
                Some(signature_for(raw_body.as_bytes(), SIGNING_SECRET)),
            ),
            Utc::now(),
        )
        .await
        .unwrap();
    assert_eq!(outcome.action, LinearWebhookAction::NoMappedStatus);

    let branchless_store = Arc::new(MemoryLinearWebhookStore::new());
    let mut link = linked_issue();
    link.task_id = None;
    branchless_store.upsert_issue_link(link).await.unwrap();
    let branchless_service = service_with(Arc::clone(&branchless_store)).await;
    let branchless_body = issue_payload(Utc::now().timestamp_millis(), Some("In Progress"));
    let branchless = branchless_service
        .handle(
            request_for(
                branchless_body.clone(),
                Some(signature_for(branchless_body.as_bytes(), SIGNING_SECRET)),
            ),
            Utc::now(),
        )
        .await
        .unwrap();
    assert_eq!(branchless.action, LinearWebhookAction::NoLinkedTask);
}

#[tokio::test]
async fn activity_and_unsupported_events_record_expected_actions() {
    let store = Arc::new(MemoryLinearWebhookStore::new());
    let service = service_with(Arc::clone(&store)).await;
    let now = Utc::now();

    let comment_body = activity_payload(
        now.timestamp_millis(),
        "Comment",
        serde_json::json!({ "id": "comment-1", "issueId": ISSUE_ID }),
    );
    let comment = service
        .handle(
            request_for_event(
                comment_body.clone(),
                Some(signature_for(comment_body.as_bytes(), SIGNING_SECRET)),
                "Comment",
                Some("comment-delivery".to_string()),
            ),
            now,
        )
        .await
        .unwrap();
    assert_eq!(comment.action, LinearWebhookAction::RecordedIssueActivity);

    let attachment_body = activity_payload(
        now.timestamp_millis(),
        "Attachment",
        serde_json::json!({ "id": "attachment-1", "issue": { "id": ISSUE_ID } }),
    );
    let attachment = service
        .handle(
            request_for_event(
                attachment_body.clone(),
                Some(signature_for(attachment_body.as_bytes(), SIGNING_SECRET)),
                "Attachment",
                Some("attachment-delivery".to_string()),
            ),
            now,
        )
        .await
        .unwrap();
    assert_eq!(
        attachment.action,
        LinearWebhookAction::RecordedIssueActivity
    );

    let unsupported_body = activity_payload(
        now.timestamp_millis(),
        "Project",
        serde_json::json!({ "id": "project-from-linear" }),
    );
    let unsupported = service
        .handle(
            request_for_event(
                unsupported_body.clone(),
                Some(signature_for(unsupported_body.as_bytes(), SIGNING_SECRET)),
                "Project",
                Some("project-delivery".to_string()),
            ),
            now,
        )
        .await
        .unwrap();
    assert_eq!(unsupported.action, LinearWebhookAction::UnsupportedEvent);
    assert_eq!(store.activity_count().await, 2);
}

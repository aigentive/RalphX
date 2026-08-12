use chrono::Utc;

use crate::domain::integrations::linear_webhook::{
    ExternalIssueLink, LinearDelivery, LinearDeliveryRecord, LinearWebhookStore,
};
use crate::domain::entities::{ProjectId, SyncProvider, TaskId};
use crate::infrastructure::sqlite::{DbConnection, SqliteLinearWebhookStore};
use crate::testing::SqliteTestDb;

const DELIVERY_ID: &str = "234d1a4e-b617-4388-90fe-adc3633d6b72";
const WEBHOOK_ID: &str = "000042e3-d123-4980-b49f-8e140eef9329";
const ISSUE_ID: &str = "539068e2-ae88-4d09-bd75-22eb4a59612f";
const TASK_ID: &str = "task-linked-to-linear";
const PROJECT_ID: &str = "project-linear";

fn linked_issue() -> ExternalIssueLink {
    ExternalIssueLink {
        provider: SyncProvider::Linear,
        project_id: ProjectId::from_string(PROJECT_ID.to_string()),
        task_id: Some(TaskId::from_string(TASK_ID.to_string())),
        external_id: ISSUE_ID.to_string(),
        external_key: Some("LIN-123".to_string()),
        external_url: Some("https://linear.app/acme/issue/LIN-123/example".to_string()),
        last_external_status: Some("In Progress".to_string()),
    }
}

#[tokio::test]
async fn sqlite_store_covers_config_delivery_and_activity_persistence() {
    let db = SqliteTestDb::new("lib-linear-webhook-store-config-delivery");
    let store = SqliteLinearWebhookStore::new(DbConnection::from_shared(db.shared_conn()));

    assert_eq!(store.get_config().await.unwrap(), (false, None));
    store
        .set_signing_secret_ref(Some("linear-secret-ref".to_string()), true)
        .await
        .unwrap();
    assert_eq!(
        store.get_config().await.unwrap(),
        (true, Some("linear-secret-ref".to_string()))
    );
    assert_eq!(
        store.get_signing_secret_ref().await.unwrap().as_deref(),
        Some("linear-secret-ref")
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
async fn sqlite_store_covers_issue_link_upsert_read_and_validation() {
    let db = SqliteTestDb::new("lib-linear-webhook-store-links");
    let store = SqliteLinearWebhookStore::new(DbConnection::from_shared(db.shared_conn()));

    assert!(store.get_issue_link(ISSUE_ID).await.unwrap().is_none());

    store.upsert_issue_link(linked_issue()).await.unwrap();
    let link = store
        .get_issue_link(ISSUE_ID)
        .await
        .unwrap()
        .expect("updated Linear issue link should be readable");
    assert_eq!(link.provider, SyncProvider::Linear);
    assert_eq!(link.project_id, ProjectId::from_string(PROJECT_ID.to_string()));
    assert_eq!(link.task_id, Some(TaskId::from_string(TASK_ID.to_string())));
    assert_eq!(link.external_key.as_deref(), Some("LIN-123"));
    assert_eq!(link.last_external_status.as_deref(), Some("In Progress"));

    let mut branchless = linked_issue();
    branchless.task_id = None;
    let error = store.upsert_issue_link(branchless).await.unwrap_err();
    assert!(error
        .to_string()
        .contains("must be attached to a task before persistence"));
}

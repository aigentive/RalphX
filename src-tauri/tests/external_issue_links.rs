use std::sync::Arc;

use ralphx_lib::application::external_issue_link_service::ExternalIssueLinkService;
use ralphx_lib::domain::integrations::{
    ExternalIssueLinkRepository, ExternalIssueLinkUpsert, ExternalIssueLocalObject,
    ExternalIssueSyncRecordUpsert, ExternalIssueSyncStatus,
};
use ralphx_lib::infrastructure::memory::MemoryExternalIssueLinkRepository;
use ralphx_lib::infrastructure::sqlite::SqliteExternalIssueLinkRepository;
use ralphx_lib::testing::SqliteTestDb;

fn sample_linear_link() -> ExternalIssueLinkUpsert {
    ExternalIssueLinkUpsert {
        provider: "linear".to_string(),
        external_kind: "issue".to_string(),
        external_id: "lin_123".to_string(),
        external_key: Some("LIN-42".to_string()),
        external_url: Some("https://linear.app/acme/issue/LIN-42".to_string()),
        local_object: ExternalIssueLocalObject::task("task-1"),
        local_project_id: Some("project-1".to_string()),
        local_sha: Some("abc123".to_string()),
        local_state: Some("merged".to_string()),
        idempotency_key: "linear:issue:lin_123:task:task-1".to_string(),
        metadata_json: Some(r#"{"source":"test"}"#.to_string()),
    }
}

#[tokio::test]
async fn memory_repo_upserts_links_and_finds_provider_identity() {
    let repo = MemoryExternalIssueLinkRepository::new();
    let created = repo.upsert_link(sample_linear_link()).await.unwrap();

    let mut update = sample_linear_link();
    update.local_state = Some("approved".to_string());
    let updated = repo.upsert_link(update).await.unwrap();

    assert_eq!(updated.id, created.id);
    assert_eq!(updated.local_state.as_deref(), Some("approved"));

    let local_links = repo
        .list_links_for_local(&ExternalIssueLocalObject::task("task-1"))
        .await
        .unwrap();
    assert_eq!(local_links.len(), 1);

    let by_external = repo
        .find_link_by_external_identity("linear", "issue", "lin_123")
        .await
        .unwrap()
        .expect("external identity should resolve");
    assert_eq!(by_external.id, created.id);

    let by_idempotency = repo
        .find_link_by_idempotency_key("linear:issue:lin_123:task:task-1")
        .await
        .unwrap()
        .expect("idempotency key should resolve");
    assert_eq!(by_idempotency.id, created.id);
}

#[tokio::test]
async fn sqlite_repo_persists_links_and_sync_records() {
    let db = SqliteTestDb::new("external-issue-links");
    let repo = SqliteExternalIssueLinkRepository::from_shared(db.shared_conn());
    let link = repo.upsert_link(sample_linear_link()).await.unwrap();

    let pending = repo
        .upsert_sync_record(ExternalIssueSyncRecordUpsert {
            link_id: link.id.clone(),
            sync_kind: "comment".to_string(),
            idempotency_key: "linear:comment:task-1:abc123".to_string(),
            local_sha: Some("abc123".to_string()),
            local_state: Some("merged".to_string()),
            external_version: None,
            status: ExternalIssueSyncStatus::Pending,
            error_message: None,
            metadata_json: None,
        })
        .await
        .unwrap();

    let succeeded = repo
        .update_sync_status(
            &pending.id,
            ExternalIssueSyncStatus::Succeeded,
            Some("comment_456"),
            None,
        )
        .await
        .unwrap()
        .expect("sync record should update");

    assert_eq!(succeeded.status, ExternalIssueSyncStatus::Succeeded);
    assert_eq!(succeeded.external_version.as_deref(), Some("comment_456"));

    let by_key = repo
        .find_sync_record_by_idempotency_key("linear:comment:task-1:abc123")
        .await
        .unwrap()
        .expect("sync idempotency key should resolve");
    assert_eq!(by_key.id, pending.id);
}

#[tokio::test]
async fn service_derives_jira_compatibility_from_metadata_before_title() {
    let repo: Arc<dyn ExternalIssueLinkRepository> =
        Arc::new(MemoryExternalIssueLinkRepository::new());
    let service = ExternalIssueLinkService::new(Arc::clone(&repo));
    let metadata = r#"{
        "composer_integration_references": [
            { "provider": "atlassian", "kind": "jira", "id": "rx-42", "url": "https://example.atlassian.net/browse/RX-42" }
        ]
    }"#;

    let link = service
        .ensure_jira_task_link_from_metadata_or_title(
            "task-1",
            Some("project-1"),
            Some(metadata),
            "WRONG-99: title fallback must not win",
            Some("abc123"),
            Some("executing"),
        )
        .await
        .unwrap()
        .expect("metadata should create a Jira compatibility link");

    assert_eq!(link.provider, "atlassian");
    assert_eq!(link.external_kind, "jira");
    assert_eq!(link.external_key.as_deref(), Some("RX-42"));
    assert_eq!(link.local_object, ExternalIssueLocalObject::task("task-1"));

    let duplicate = service
        .ensure_jira_task_link_from_metadata_or_title(
            "task-1",
            Some("project-1"),
            None,
            "RX-42: title fallback",
            Some("def456"),
            Some("reviewing"),
        )
        .await
        .unwrap()
        .expect("title fallback should update the existing compatibility link");

    assert_eq!(duplicate.id, link.id);
    assert_eq!(duplicate.local_sha.as_deref(), Some("def456"));
    assert_eq!(
        repo.list_links_for_local(&ExternalIssueLocalObject::task("task-1"))
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn service_derives_linear_issue_link_from_metadata() {
    let repo: Arc<dyn ExternalIssueLinkRepository> =
        Arc::new(MemoryExternalIssueLinkRepository::new());
    let service = ExternalIssueLinkService::new(Arc::clone(&repo));
    let metadata = r#"{
        "composer_integration_references": [
            {
                "provider": "linear",
                "kind": "linear",
                "id": "539068e2-ae88-4d09-bd75-22eb4a59612f",
                "key": "LIN-123",
                "url": "https://linear.app/acme/issue/LIN-123/example"
            }
        ]
    }"#;

    let link = service
        .ensure_linear_task_link_from_metadata(
            "task-1",
            Some("project-1"),
            Some(metadata),
            Some("abc123"),
            Some("executing"),
        )
        .await
        .unwrap()
        .expect("metadata should create a Linear issue link");

    assert_eq!(link.provider, "linear");
    assert_eq!(link.external_kind, "issue");
    assert_eq!(link.external_id, "539068e2-ae88-4d09-bd75-22eb4a59612f");
    assert_eq!(link.external_key.as_deref(), Some("LIN-123"));
    assert_eq!(link.local_object, ExternalIssueLocalObject::task("task-1"));
}

#[tokio::test]
async fn service_exposes_provider_neutral_read_and_sync_update_apis() {
    let repo: Arc<dyn ExternalIssueLinkRepository> =
        Arc::new(MemoryExternalIssueLinkRepository::new());
    let service = ExternalIssueLinkService::new(repo);
    let link = service.upsert_link(sample_linear_link()).await.unwrap();

    let by_id = service
        .get_link(&link.id)
        .await
        .unwrap()
        .expect("service should read by local link id");
    assert_eq!(by_id.id, link.id);

    let by_external = service
        .find_link_by_external_identity("linear", "issue", "lin_123")
        .await
        .unwrap()
        .expect("service should read by provider identity");
    assert_eq!(by_external.id, link.id);

    let by_idempotency = service
        .find_link_by_idempotency_key("linear:issue:lin_123:task:task-1")
        .await
        .unwrap()
        .expect("service should read by idempotency key");
    assert_eq!(by_idempotency.id, link.id);

    let pending = service
        .upsert_sync_record(ExternalIssueSyncRecordUpsert {
            link_id: link.id.clone(),
            sync_kind: "comment".to_string(),
            idempotency_key: "linear:service-comment:task-1:abc123".to_string(),
            local_sha: Some("abc123".to_string()),
            local_state: Some("merged".to_string()),
            external_version: None,
            status: ExternalIssueSyncStatus::Pending,
            error_message: None,
            metadata_json: None,
        })
        .await
        .unwrap();

    let succeeded = service
        .update_sync_status(
            &pending.id,
            ExternalIssueSyncStatus::Succeeded,
            Some("comment_789"),
            None,
        )
        .await
        .unwrap()
        .expect("sync record should update through service");

    assert_eq!(succeeded.status, ExternalIssueSyncStatus::Succeeded);
    assert_eq!(succeeded.external_version.as_deref(), Some("comment_789"));

    let records = service.list_sync_records_for_link(&link.id).await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, pending.id);
}

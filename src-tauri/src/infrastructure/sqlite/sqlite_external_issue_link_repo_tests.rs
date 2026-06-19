use crate::domain::integrations::{
    ExternalIssueLinkRepository, ExternalIssueLinkUpsert, ExternalIssueLocalObject,
    ExternalIssueSyncRecordUpsert, ExternalIssueSyncStatus,
};
use crate::infrastructure::sqlite::SqliteExternalIssueLinkRepository;
use crate::testing::SqliteTestDb;

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
async fn sqlite_repo_covers_link_identity_reads_and_updates() {
    let db = SqliteTestDb::new("lib-external-issue-links");
    let repo = SqliteExternalIssueLinkRepository::from_shared(db.shared_conn());
    let created = repo.upsert_link(sample_linear_link()).await.unwrap();

    let mut update = sample_linear_link();
    update.local_sha = Some("def456".to_string());
    update.local_state = Some("reviewing".to_string());
    let updated = repo.upsert_link(update).await.unwrap();

    assert_eq!(updated.id, created.id);
    assert_eq!(updated.local_sha.as_deref(), Some("def456"));
    assert_eq!(updated.local_state.as_deref(), Some("reviewing"));

    let by_id = repo
        .get_link(&created.id)
        .await
        .unwrap()
        .expect("link should resolve by id");
    assert_eq!(by_id.id, created.id);

    let by_external = repo
        .find_link_by_external_identity("linear", "issue", "lin_123")
        .await
        .unwrap()
        .expect("link should resolve by external identity");
    assert_eq!(by_external.id, created.id);

    let by_idempotency = repo
        .find_link_by_idempotency_key("linear:issue:lin_123:task:task-1")
        .await
        .unwrap()
        .expect("link should resolve by idempotency key");
    assert_eq!(by_idempotency.id, created.id);

    let local_links = repo
        .list_links_for_local(&ExternalIssueLocalObject::task("task-1"))
        .await
        .unwrap();
    assert_eq!(local_links.len(), 1);
    assert_eq!(local_links[0].id, created.id);

    assert!(repo.get_link("missing").await.unwrap().is_none());
    assert!(repo
        .find_link_by_external_identity("linear", "issue", "missing")
        .await
        .unwrap()
        .is_none());
    assert!(repo
        .find_link_by_idempotency_key("missing-key")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn sqlite_repo_covers_sync_record_lifecycle() {
    let db = SqliteTestDb::new("lib-external-issue-sync-records");
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
    assert!(pending.completed_at.is_none());

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
    assert!(succeeded.completed_at.is_some());

    let retried = repo
        .upsert_sync_record(ExternalIssueSyncRecordUpsert {
            link_id: link.id.clone(),
            sync_kind: "comment".to_string(),
            idempotency_key: "linear:comment:task-1:abc123".to_string(),
            local_sha: Some("def456".to_string()),
            local_state: Some("reviewing".to_string()),
            external_version: Some("comment_456".to_string()),
            status: ExternalIssueSyncStatus::Failed,
            error_message: Some("Linear rejected update".to_string()),
            metadata_json: Some(r#"{"attempt":2}"#.to_string()),
        })
        .await
        .unwrap();
    assert_eq!(retried.id, pending.id);
    assert_eq!(retried.status, ExternalIssueSyncStatus::Failed);
    assert_eq!(
        retried.error_message.as_deref(),
        Some("Linear rejected update")
    );

    let by_key = repo
        .find_sync_record_by_idempotency_key("linear:comment:task-1:abc123")
        .await
        .unwrap()
        .expect("sync idempotency key should resolve");
    assert_eq!(by_key.id, pending.id);

    let records = repo.list_sync_records_for_link(&link.id).await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, pending.id);

    assert!(repo
        .find_sync_record_by_idempotency_key("missing-sync-key")
        .await
        .unwrap()
        .is_none());
    assert!(repo
        .update_sync_status(
            "missing-sync-record",
            ExternalIssueSyncStatus::Skipped,
            None,
            Some("not found"),
        )
        .await
        .unwrap()
        .is_none());
}

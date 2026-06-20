use crate::domain::integrations::{
    ExternalIssueLinkRepository, ExternalIssueLinkUpsert, ExternalIssueLocalObject,
    ExternalIssueSyncRecordUpsert, ExternalIssueSyncStatus, ProviderTicketOperationKind,
    ProviderTicketOperationStatus, ProviderTicketOperationUpsert,
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

fn sample_ticket_operation(link_id: Option<String>) -> ProviderTicketOperationUpsert {
    ProviderTicketOperationUpsert {
        provider: "linear".to_string(),
        external_kind: "issue".to_string(),
        external_id: "lin_123".to_string(),
        external_key: Some("LIN-42".to_string()),
        link_id,
        local_project_id: Some("project-1".to_string()),
        operation: ProviderTicketOperationKind::Comment,
        client_operation_id: "client-operation-1".to_string(),
        status: ProviderTicketOperationStatus::Pending,
        provider_operation_id: None,
        error_message: None,
        metadata_json: Some(r#"{"body":"Looks good"}"#.to_string()),
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

#[tokio::test]
async fn sqlite_repo_records_unlinked_ticket_operation_idempotently() {
    let db = SqliteTestDb::new("lib-provider-ticket-operations-unlinked");
    let repo = SqliteExternalIssueLinkRepository::from_shared(db.shared_conn());

    let pending = repo
        .upsert_provider_ticket_operation(sample_ticket_operation(None))
        .await
        .unwrap();
    assert_eq!(pending.operation, ProviderTicketOperationKind::Comment);
    assert_eq!(pending.status, ProviderTicketOperationStatus::Pending);
    assert_eq!(pending.client_operation_id, "client-operation-1");
    assert!(pending.link_id.is_none());
    assert!(pending.completed_at.is_none());

    let mut retry = sample_ticket_operation(None);
    retry.status = ProviderTicketOperationStatus::Succeeded;
    retry.provider_operation_id = Some("linear-comment-1".to_string());
    retry.metadata_json = Some(r#"{"attempt":2}"#.to_string());
    let completed = repo.upsert_provider_ticket_operation(retry).await.unwrap();

    assert_eq!(completed.id, pending.id);
    assert_eq!(completed.status, ProviderTicketOperationStatus::Succeeded);
    assert_eq!(
        completed.provider_operation_id.as_deref(),
        Some("linear-comment-1")
    );
    assert!(completed.completed_at.is_some());

    let by_client_id = repo
        .find_provider_ticket_operation_by_client_operation_id("client-operation-1")
        .await
        .unwrap()
        .expect("client operation id should resolve");
    assert_eq!(by_client_id.id, pending.id);

    let operations = repo
        .list_provider_ticket_operations_for_ticket(
            "linear",
            "issue",
            "lin_123",
            Some("LIN-42"),
            Some("project-1"),
        )
        .await
        .unwrap();
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].id, pending.id);
}

#[tokio::test]
async fn sqlite_repo_records_linked_ticket_operation_without_fabricating_links() {
    let db = SqliteTestDb::new("lib-provider-ticket-operations-linked");
    let repo = SqliteExternalIssueLinkRepository::from_shared(db.shared_conn());
    let link = repo.upsert_link(sample_linear_link()).await.unwrap();

    let created = repo
        .upsert_provider_ticket_operation(sample_ticket_operation(Some(link.id.clone())))
        .await
        .unwrap();

    assert_eq!(created.link_id.as_deref(), Some(link.id.as_str()));
    assert_eq!(created.external_id, "lin_123");

    let missing = repo
        .find_link_by_external_identity("linear", "issue", "missing")
        .await
        .unwrap();
    assert!(missing.is_none());
}

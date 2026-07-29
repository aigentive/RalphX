use std::str::FromStr;

use super::rows::{ticket_operation_completed_at_for, ticket_operation_select_sql};
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

#[tokio::test]
async fn sqlite_repo_pending_ticket_operation_sets_last_attempt_without_completion() {
    let db = SqliteTestDb::new("lib-provider-ticket-operations-pending");
    let repo = SqliteExternalIssueLinkRepository::from_shared(db.shared_conn());

    // Pending upserts stamp last_attempt_at (production sets it on every upsert)
    // but must leave completed_at unset because Pending is not terminal.
    let pending = repo
        .upsert_provider_ticket_operation(sample_ticket_operation(None))
        .await
        .unwrap();

    assert_eq!(pending.status, ProviderTicketOperationStatus::Pending);
    assert!(
        pending.last_attempt_at.is_some(),
        "last_attempt_at must be stamped on every upsert"
    );
    assert!(
        pending.completed_at.is_none(),
        "pending operations are not terminal so completed_at stays None"
    );
}

#[tokio::test]
async fn sqlite_repo_failed_ticket_operation_sets_completion_and_error() {
    let db = SqliteTestDb::new("lib-provider-ticket-operations-failed");
    let repo = SqliteExternalIssueLinkRepository::from_shared(db.shared_conn());

    let mut failed = sample_ticket_operation(None);
    failed.status = ProviderTicketOperationStatus::Failed;
    failed.error_message = Some("provider rejected transition".to_string());
    let stored = repo.upsert_provider_ticket_operation(failed).await.unwrap();

    assert_eq!(stored.status, ProviderTicketOperationStatus::Failed);
    assert!(
        stored.last_attempt_at.is_some(),
        "last_attempt_at is stamped for every upsert"
    );
    assert!(
        stored.completed_at.is_some(),
        "failed is terminal so completed_at must be set"
    );
    assert_eq!(
        stored.error_message.as_deref(),
        Some("provider rejected transition")
    );
}

#[tokio::test]
async fn sqlite_repo_succeeded_ticket_operation_sets_completion() {
    let db = SqliteTestDb::new("lib-provider-ticket-operations-succeeded");
    let repo = SqliteExternalIssueLinkRepository::from_shared(db.shared_conn());

    let mut succeeded = sample_ticket_operation(None);
    succeeded.status = ProviderTicketOperationStatus::Succeeded;
    succeeded.provider_operation_id = Some("linear-comment-9".to_string());
    let stored = repo
        .upsert_provider_ticket_operation(succeeded)
        .await
        .unwrap();

    assert_eq!(stored.status, ProviderTicketOperationStatus::Succeeded);
    assert!(
        stored.completed_at.is_some(),
        "succeeded is terminal so completed_at must be set"
    );
    assert_eq!(
        stored.provider_operation_id.as_deref(),
        Some("linear-comment-9")
    );
}

#[tokio::test]
async fn sqlite_repo_upsert_updates_existing_operation_by_client_operation_id() {
    let db = SqliteTestDb::new("lib-provider-ticket-operations-update-existing");
    let repo = SqliteExternalIssueLinkRepository::from_shared(db.shared_conn());

    let created = repo
        .upsert_provider_ticket_operation(sample_ticket_operation(None))
        .await
        .unwrap();
    assert!(created.provider_operation_id.is_none());

    // Re-upsert with the same client_operation_id but different mutable fields:
    // the row is keyed on client_operation_id so the id must be preserved while
    // provider_operation_id / metadata / external_key are overwritten.
    let mut update = sample_ticket_operation(None);
    update.status = ProviderTicketOperationStatus::Succeeded;
    update.provider_operation_id = Some("linear-comment-77".to_string());
    update.metadata_json = Some(r#"{"attempt":3}"#.to_string());
    update.external_key = Some("LIN-99".to_string());
    let updated = repo.upsert_provider_ticket_operation(update).await.unwrap();

    assert_eq!(updated.id, created.id, "client_operation_id keys the row");
    assert_eq!(updated.status, ProviderTicketOperationStatus::Succeeded);
    assert_eq!(
        updated.provider_operation_id.as_deref(),
        Some("linear-comment-77")
    );
    assert_eq!(updated.metadata_json.as_deref(), Some(r#"{"attempt":3}"#));
    assert_eq!(updated.external_key.as_deref(), Some("LIN-99"));
    assert!(updated.completed_at.is_some());

    // Exactly one row exists for that client_operation_id.
    let operations = repo
        .list_provider_ticket_operations_for_ticket("linear", "issue", "lin_123", None, None)
        .await
        .unwrap();
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].id, created.id);
}

#[tokio::test]
async fn sqlite_repo_find_ticket_operation_by_client_operation_id_present_and_missing() {
    let db = SqliteTestDb::new("lib-provider-ticket-operations-find");
    let repo = SqliteExternalIssueLinkRepository::from_shared(db.shared_conn());

    let created = repo
        .upsert_provider_ticket_operation(sample_ticket_operation(None))
        .await
        .unwrap();

    let found = repo
        .find_provider_ticket_operation_by_client_operation_id("client-operation-1")
        .await
        .unwrap()
        .expect("existing client operation id should resolve");
    assert_eq!(found.id, created.id);

    assert!(
        repo.find_provider_ticket_operation_by_client_operation_id("missing-client-op")
            .await
            .unwrap()
            .is_none(),
        "missing client operation id resolves to None"
    );
}

#[tokio::test]
async fn sqlite_repo_list_ticket_operations_honors_optional_filters() {
    let db = SqliteTestDb::new("lib-provider-ticket-operations-list-filters");
    let repo = SqliteExternalIssueLinkRepository::from_shared(db.shared_conn());

    let created = repo
        .upsert_provider_ticket_operation(sample_ticket_operation(None))
        .await
        .unwrap();

    // Exact match across every dimension returns the row.
    let exact = repo
        .list_provider_ticket_operations_for_ticket(
            "linear",
            "issue",
            "lin_123",
            Some("LIN-42"),
            Some("project-1"),
        )
        .await
        .unwrap();
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].id, created.id);

    // None external_key filter ("?4 IS NULL OR external_key = ?4") returns the match.
    let no_key_filter = repo
        .list_provider_ticket_operations_for_ticket(
            "linear",
            "issue",
            "lin_123",
            None,
            Some("project-1"),
        )
        .await
        .unwrap();
    assert_eq!(no_key_filter.len(), 1);
    assert_eq!(no_key_filter[0].id, created.id);

    // None local_project_id filter ("?5 IS NULL OR local_project_id = ?5") returns the match.
    let no_project_filter = repo
        .list_provider_ticket_operations_for_ticket(
            "linear",
            "issue",
            "lin_123",
            Some("LIN-42"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(no_project_filter.len(), 1);
    assert_eq!(no_project_filter[0].id, created.id);

    // A non-matching external_key still excludes the row even with other dims matching.
    let mismatched_key = repo
        .list_provider_ticket_operations_for_ticket(
            "linear",
            "issue",
            "lin_123",
            Some("OTHER-1"),
            None,
        )
        .await
        .unwrap();
    assert!(mismatched_key.is_empty());
}

#[tokio::test]
async fn sqlite_repo_update_ticket_operation_status_success_failure_and_missing() {
    let db = SqliteTestDb::new("lib-provider-ticket-operations-update-status");
    let repo = SqliteExternalIssueLinkRepository::from_shared(db.shared_conn());

    let created = repo
        .upsert_provider_ticket_operation(sample_ticket_operation(None))
        .await
        .unwrap();
    assert!(created.completed_at.is_none());

    // Success path: status + provider_operation_id + completed_at recorded.
    let succeeded = repo
        .update_provider_ticket_operation_status(
            &created.id,
            ProviderTicketOperationStatus::Succeeded,
            Some("provider-op-1"),
            None,
        )
        .await
        .unwrap()
        .expect("operation should update");
    assert_eq!(succeeded.status, ProviderTicketOperationStatus::Succeeded);
    assert_eq!(
        succeeded.provider_operation_id.as_deref(),
        Some("provider-op-1")
    );
    assert!(succeeded.completed_at.is_some());

    // Failure path: status + error_message + completed_at recorded.
    let failed = repo
        .update_provider_ticket_operation_status(
            &created.id,
            ProviderTicketOperationStatus::Failed,
            None,
            Some("downstream 500"),
        )
        .await
        .unwrap()
        .expect("operation should update");
    assert_eq!(failed.status, ProviderTicketOperationStatus::Failed);
    assert_eq!(failed.error_message.as_deref(), Some("downstream 500"));
    assert!(failed.completed_at.is_some());

    // Missing operation id resolves to None.
    assert!(
        repo.update_provider_ticket_operation_status(
            "missing-operation",
            ProviderTicketOperationStatus::Canceled,
            None,
            Some("not found"),
        )
        .await
        .unwrap()
        .is_none(),
        "updating an unknown operation id returns None"
    );
}

#[tokio::test]
async fn sqlite_repo_ticket_operation_round_trip_parses_optional_and_date_fields() {
    let db = SqliteTestDb::new("lib-provider-ticket-operations-round-trip");
    let repo = SqliteExternalIssueLinkRepository::from_shared(db.shared_conn());

    // Upsert with all optional fields set to None to prove null columns read back as None
    // and required date columns (created_at/updated_at/last_attempt_at) parse cleanly.
    let null_optionals = ProviderTicketOperationUpsert {
        provider: "linear".to_string(),
        external_kind: "issue".to_string(),
        external_id: "lin_999".to_string(),
        external_key: None,
        link_id: None,
        local_project_id: None,
        operation: ProviderTicketOperationKind::Assign,
        client_operation_id: "client-null-optionals".to_string(),
        status: ProviderTicketOperationStatus::Pending,
        provider_operation_id: None,
        error_message: None,
        metadata_json: None,
    };
    let stored = repo
        .upsert_provider_ticket_operation(null_optionals)
        .await
        .unwrap();

    assert_eq!(stored.operation, ProviderTicketOperationKind::Assign);
    assert!(stored.external_key.is_none());
    assert!(stored.link_id.is_none());
    assert!(stored.local_project_id.is_none());
    assert!(stored.provider_operation_id.is_none());
    assert!(stored.error_message.is_none());
    assert!(stored.metadata_json.is_none());
    // Read back through find_* exercises row_to_ticket_operation including date parsing.
    let read_back = repo
        .find_provider_ticket_operation_by_client_operation_id("client-null-optionals")
        .await
        .unwrap()
        .expect("operation should resolve");
    assert_eq!(read_back.id, stored.id);
    assert_eq!(read_back.last_attempt_at, stored.last_attempt_at);
    assert_eq!(read_back.created_at, stored.created_at);
    assert_eq!(read_back.updated_at, stored.updated_at);
    assert!(read_back.last_attempt_at.is_some());
}

#[test]
fn ticket_operation_select_sql_lists_all_columns() {
    let sql = ticket_operation_select_sql();
    for column in [
        "id",
        "provider",
        "external_kind",
        "external_id",
        "external_key",
        "link_id",
        "local_project_id",
        "operation",
        "client_operation_id",
        "status",
        "provider_operation_id",
        "error_message",
        "metadata_json",
        "last_attempt_at",
        "completed_at",
        "created_at",
        "updated_at",
    ] {
        assert!(
            sql.contains(column),
            "ticket_operation_select_sql must select column `{column}`"
        );
    }
    assert!(sql.contains("FROM provider_ticket_operations"));
}

#[test]
fn ticket_operation_completed_at_for_only_stamps_terminal_statuses() {
    for terminal in [
        ProviderTicketOperationStatus::Succeeded,
        ProviderTicketOperationStatus::Failed,
        ProviderTicketOperationStatus::TimedOut,
        ProviderTicketOperationStatus::Canceled,
    ] {
        let stamped = ticket_operation_completed_at_for(terminal);
        assert!(
            stamped.as_deref().is_some_and(|value| !value.is_empty()),
            "terminal status {terminal:?} must produce a non-empty completed_at"
        );
    }
    assert!(
        ticket_operation_completed_at_for(ProviderTicketOperationStatus::Pending).is_none(),
        "pending must not produce a completed_at"
    );
}

#[test]
fn ticket_operation_enum_parsing_rejects_unknown_strings() {
    // These are the parse branches row_to_ticket_operation maps to AppError::Database when
    // a row holds a corrupt operation/status string.
    assert!(ProviderTicketOperationKind::from_str("bogus-operation").is_err());
    assert!(ProviderTicketOperationStatus::from_str("bogus-status").is_err());

    // Valid round-trip strings still parse.
    assert_eq!(
        ProviderTicketOperationKind::from_str("set_labels").unwrap(),
        ProviderTicketOperationKind::SetLabels
    );
    assert_eq!(
        ProviderTicketOperationStatus::from_str("timed_out").unwrap(),
        ProviderTicketOperationStatus::TimedOut
    );
}

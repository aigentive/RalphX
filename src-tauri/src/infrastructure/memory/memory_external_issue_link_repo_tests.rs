use crate::domain::integrations::{
    ExternalIssueLinkRepository, ProviderTicketOperationKind, ProviderTicketOperationStatus,
    ProviderTicketOperationUpsert,
};
use crate::infrastructure::memory::MemoryExternalIssueLinkRepository;

fn sample_ticket_operation() -> ProviderTicketOperationUpsert {
    ProviderTicketOperationUpsert {
        provider: "jira".to_string(),
        external_kind: "issue".to_string(),
        external_id: "PROJ-123".to_string(),
        external_key: Some("PROJ-123".to_string()),
        link_id: None,
        local_project_id: Some("project-1".to_string()),
        operation: ProviderTicketOperationKind::Transition,
        client_operation_id: "client-operation-1".to_string(),
        status: ProviderTicketOperationStatus::Pending,
        provider_operation_id: None,
        error_message: None,
        metadata_json: None,
    }
}

#[tokio::test]
async fn memory_repo_records_ticket_operation_idempotently() {
    let repo = MemoryExternalIssueLinkRepository::new();
    let pending = repo
        .upsert_provider_ticket_operation(sample_ticket_operation())
        .await
        .unwrap();

    let mut retry = sample_ticket_operation();
    retry.status = ProviderTicketOperationStatus::Failed;
    retry.error_message = Some("workflow transition unavailable".to_string());
    let failed = repo.upsert_provider_ticket_operation(retry).await.unwrap();

    assert_eq!(failed.id, pending.id);
    assert_eq!(failed.status, ProviderTicketOperationStatus::Failed);
    assert_eq!(
        failed.error_message.as_deref(),
        Some("workflow transition unavailable")
    );
    assert!(failed.completed_at.is_some());

    let by_client_id = repo
        .find_provider_ticket_operation_by_client_operation_id("client-operation-1")
        .await
        .unwrap()
        .expect("client operation id should resolve");
    assert_eq!(by_client_id.id, pending.id);

    let operations = repo
        .list_provider_ticket_operations_for_ticket(
            "jira",
            "issue",
            "PROJ-123",
            Some("PROJ-123"),
            Some("project-1"),
        )
        .await
        .unwrap();
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0].id, pending.id);
}

#[tokio::test]
async fn memory_repo_upsert_records_pending_succeeded_and_failed_completion() {
    let repo = MemoryExternalIssueLinkRepository::new();

    // Pending: last_attempt_at stamped, completed_at left None (non-terminal).
    let pending = repo
        .upsert_provider_ticket_operation(sample_ticket_operation())
        .await
        .unwrap();
    assert!(pending.last_attempt_at.is_some());
    assert!(pending.completed_at.is_none());

    // Succeeded: terminal so completed_at must be set.
    let mut succeeded_input = sample_ticket_operation();
    succeeded_input.client_operation_id = "client-operation-succeeded".to_string();
    succeeded_input.status = ProviderTicketOperationStatus::Succeeded;
    let succeeded = repo
        .upsert_provider_ticket_operation(succeeded_input)
        .await
        .unwrap();
    assert_eq!(succeeded.status, ProviderTicketOperationStatus::Succeeded);
    assert!(succeeded.completed_at.is_some());

    // Failed + error_message: both recorded, completed_at set.
    let mut failed_input = sample_ticket_operation();
    failed_input.client_operation_id = "client-operation-failed".to_string();
    failed_input.status = ProviderTicketOperationStatus::Failed;
    failed_input.error_message = Some("provider unavailable".to_string());
    let failed = repo
        .upsert_provider_ticket_operation(failed_input)
        .await
        .unwrap();
    assert_eq!(failed.status, ProviderTicketOperationStatus::Failed);
    assert_eq!(failed.error_message.as_deref(), Some("provider unavailable"));
    assert!(failed.completed_at.is_some());
}

#[tokio::test]
async fn memory_repo_update_status_completes_only_terminal_statuses() {
    let repo = MemoryExternalIssueLinkRepository::new();
    let created = repo
        .upsert_provider_ticket_operation(sample_ticket_operation())
        .await
        .unwrap();
    assert!(created.completed_at.is_none());

    // Terminal status sets completed_at and records provider_operation_id.
    let canceled = repo
        .update_provider_ticket_operation_status(
            &created.id,
            ProviderTicketOperationStatus::Canceled,
            Some("provider-op-cancel"),
            Some("user canceled"),
        )
        .await
        .unwrap()
        .expect("operation should update");
    assert_eq!(canceled.status, ProviderTicketOperationStatus::Canceled);
    assert_eq!(
        canceled.provider_operation_id.as_deref(),
        Some("provider-op-cancel")
    );
    assert_eq!(canceled.error_message.as_deref(), Some("user canceled"));
    assert!(canceled.completed_at.is_some());

    // Re-setting to Pending clears completed_at (non-terminal).
    let back_to_pending = repo
        .update_provider_ticket_operation_status(
            &created.id,
            ProviderTicketOperationStatus::Pending,
            None,
            None,
        )
        .await
        .unwrap()
        .expect("operation should update");
    assert_eq!(back_to_pending.status, ProviderTicketOperationStatus::Pending);
    assert!(
        back_to_pending.completed_at.is_none(),
        "pending status must clear completed_at"
    );

    // Missing operation id resolves to None.
    assert!(repo
        .update_provider_ticket_operation_status(
            "missing-operation",
            ProviderTicketOperationStatus::Succeeded,
            None,
            None,
        )
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn memory_repo_list_ticket_operations_filters_by_all_dimensions() {
    let repo = MemoryExternalIssueLinkRepository::new();

    // Seed a target operation plus several rows that differ on one dimension each.
    repo.upsert_provider_ticket_operation(sample_ticket_operation())
        .await
        .unwrap();

    let mut other_provider = sample_ticket_operation();
    other_provider.client_operation_id = "op-other-provider".to_string();
    other_provider.provider = "linear".to_string();
    repo.upsert_provider_ticket_operation(other_provider)
        .await
        .unwrap();

    let mut other_kind = sample_ticket_operation();
    other_kind.client_operation_id = "op-other-kind".to_string();
    other_kind.external_kind = "comment".to_string();
    repo.upsert_provider_ticket_operation(other_kind)
        .await
        .unwrap();

    let mut other_id = sample_ticket_operation();
    other_id.client_operation_id = "op-other-id".to_string();
    other_id.external_id = "PROJ-999".to_string();
    repo.upsert_provider_ticket_operation(other_id)
        .await
        .unwrap();

    let mut other_key = sample_ticket_operation();
    other_key.client_operation_id = "op-other-key".to_string();
    other_key.external_key = Some("PROJ-OTHER".to_string());
    repo.upsert_provider_ticket_operation(other_key)
        .await
        .unwrap();

    let mut other_project = sample_ticket_operation();
    other_project.client_operation_id = "op-other-project".to_string();
    other_project.local_project_id = Some("project-2".to_string());
    repo.upsert_provider_ticket_operation(other_project)
        .await
        .unwrap();

    // Full filter set isolates exactly the target row.
    let exact = repo
        .list_provider_ticket_operations_for_ticket(
            "jira",
            "issue",
            "PROJ-123",
            Some("PROJ-123"),
            Some("project-1"),
        )
        .await
        .unwrap();
    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].client_operation_id, "client-operation-1");

    // None external_key matches any external_key on the same provider/kind/id/project:
    // the target row plus the row that only differed by external_key.
    let no_key = repo
        .list_provider_ticket_operations_for_ticket(
            "jira",
            "issue",
            "PROJ-123",
            None,
            Some("project-1"),
        )
        .await
        .unwrap();
    assert_eq!(no_key.len(), 2);

    // None local_project_id matches any project on the same provider/kind/id/key:
    // the target row plus the row that only differed by local_project_id.
    let no_project = repo
        .list_provider_ticket_operations_for_ticket(
            "jira",
            "issue",
            "PROJ-123",
            Some("PROJ-123"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(no_project.len(), 2);

    // A mismatching required dimension (provider) returns nothing.
    let no_match = repo
        .list_provider_ticket_operations_for_ticket(
            "github",
            "issue",
            "PROJ-123",
            None,
            None,
        )
        .await
        .unwrap();
    assert!(no_match.is_empty());
}

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

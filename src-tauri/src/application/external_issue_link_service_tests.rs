use std::sync::Arc;

use super::{ExternalIssueLinkService, TicketConversationLinkInput};
use crate::domain::integrations::{
    ExternalIssueLinkRepository, ExternalIssueLinkUpsert, ExternalIssueLocalObject,
    ExternalIssueSyncRecordUpsert, ExternalIssueSyncStatus, ProviderTicketOperationKind,
    ProviderTicketOperationStatus, ProviderTicketOperationUpsert,
};
use crate::infrastructure::memory::MemoryExternalIssueLinkRepository;

fn sample_ticket_operation() -> ProviderTicketOperationUpsert {
    ProviderTicketOperationUpsert {
        provider: "linear".to_string(),
        external_kind: "issue".to_string(),
        external_id: "lin_123".to_string(),
        external_key: Some("LIN-42".to_string()),
        link_id: None,
        local_project_id: Some("project-1".to_string()),
        operation: ProviderTicketOperationKind::Comment,
        client_operation_id: "client-operation-1".to_string(),
        status: ProviderTicketOperationStatus::Pending,
        provider_operation_id: None,
        error_message: None,
        metadata_json: Some(r#"{"body":"hello"}"#.to_string()),
    }
}

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
async fn ticket_conversation_links_are_idempotent_and_read_from_session_storage() {
    let repo: Arc<dyn ExternalIssueLinkRepository> =
        Arc::new(MemoryExternalIssueLinkRepository::new());
    let service = ExternalIssueLinkService::new(Arc::clone(&repo));
    let input = TicketConversationLinkInput {
        provider: "clickup".to_string(),
        external_kind: "clickup".to_string(),
        external_id: "8689abc".to_string(),
        external_key: Some("DEV-42".to_string()),
        external_url: Some("https://app.clickup.com/t/8689abc".to_string()),
        conversation_id: "conversation-1".to_string(),
        project_id: "project-1".to_string(),
        local_sha: Some("abc123".to_string()),
        local_state: Some("open".to_string()),
        metadata_json: Some(r#"{"source":"pr_title","matched_token":"DEV-42"}"#.to_string()),
    };

    let first = service
        .upsert_ticket_conversation_link(input.clone())
        .await
        .expect("first session link should persist");
    let second = service
        .upsert_ticket_conversation_link(input)
        .await
        .expect("repeated session link should update in place");

    assert_eq!(first.id, second.id);
    assert_eq!(
        second.local_object,
        ExternalIssueLocalObject::session("conversation-1")
    );
    assert_eq!(second.local_project_id.as_deref(), Some("project-1"));
    assert_eq!(second.local_sha.as_deref(), Some("abc123"));
    assert_eq!(
        service
            .list_ticket_links_for_conversation("conversation-1")
            .await
            .expect("session links should read")
            .len(),
        1
    );
    assert_eq!(
        repo.list_links_for_local(&ExternalIssueLocalObject::session("conversation-1"))
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn service_covers_provider_neutral_link_and_sync_apis() {
    let repo: Arc<dyn ExternalIssueLinkRepository> =
        Arc::new(MemoryExternalIssueLinkRepository::new());
    let service = ExternalIssueLinkService::new(repo);
    let link = service.upsert_link(sample_linear_link()).await.unwrap();

    assert_eq!(
        service
            .get_link(&link.id)
            .await
            .unwrap()
            .expect("link should resolve by id")
            .id,
        link.id
    );
    assert_eq!(
        service
            .find_link_by_external_identity("linear", "issue", "lin_123")
            .await
            .unwrap()
            .expect("link should resolve by provider identity")
            .id,
        link.id
    );
    assert_eq!(
        service
            .find_link_by_idempotency_key("linear:issue:lin_123:task:task-1")
            .await
            .unwrap()
            .expect("link should resolve by idempotency key")
            .id,
        link.id
    );
    assert_eq!(
        service
            .list_links_for_local(&ExternalIssueLocalObject::task("task-1"))
            .await
            .unwrap()
            .len(),
        1
    );

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
    assert_eq!(
        service
            .find_sync_record_by_idempotency_key("linear:service-comment:task-1:abc123")
            .await
            .unwrap()
            .expect("sync should resolve by key")
            .id,
        pending.id
    );
    assert_eq!(
        service
            .list_sync_records_for_link(&link.id)
            .await
            .unwrap()
            .len(),
        1
    );
    let skipped = service
        .update_sync_status(
            &pending.id,
            ExternalIssueSyncStatus::Skipped,
            Some("external-version"),
            Some("no mapped status"),
        )
        .await
        .unwrap()
        .expect("sync should update");
    assert_eq!(skipped.status, ExternalIssueSyncStatus::Skipped);
    assert_eq!(
        skipped.external_version.as_deref(),
        Some("external-version")
    );
}

#[tokio::test]
async fn service_derives_jira_and_linear_links_from_metadata() {
    let repo: Arc<dyn ExternalIssueLinkRepository> =
        Arc::new(MemoryExternalIssueLinkRepository::new());
    let service = ExternalIssueLinkService::new(Arc::clone(&repo));
    let jira_metadata = r#"{
        "composer_integration_references": [
            { "provider": "atlassian", "kind": "jira", "id": "rx-42", "url": "https://example.atlassian.net/browse/RX-42" }
        ]
    }"#;

    let jira = service
        .ensure_jira_task_link_from_metadata_or_title(
            "task-1",
            Some("project-1"),
            Some(jira_metadata),
            "WRONG-99 title fallback must not win",
            Some("abc123"),
            Some("executing"),
        )
        .await
        .unwrap()
        .expect("metadata should create a Jira link");
    assert_eq!(jira.provider, "atlassian");
    assert_eq!(jira.external_key.as_deref(), Some("RX-42"));
    assert_eq!(
        jira.external_url.as_deref(),
        Some("https://example.atlassian.net/browse/RX-42")
    );

    let jira_from_title = service
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
        .expect("title should update the same Jira link");
    assert_eq!(jira_from_title.id, jira.id);

    let linear_metadata = r#"{
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
    let linear = service
        .ensure_linear_task_link_from_metadata(
            "task-1",
            Some("project-1"),
            Some(linear_metadata),
            Some("abc123"),
            Some("executing"),
        )
        .await
        .unwrap()
        .expect("metadata should create a Linear link");
    assert_eq!(linear.provider, "linear");
    assert_eq!(linear.external_kind, "issue");
    assert_eq!(linear.external_key.as_deref(), Some("LIN-123"));

    assert!(service
        .ensure_linear_task_link_from_metadata(
            "task-2",
            Some("project-1"),
            Some(r#"{"composer_integration_references":[]}"#),
            Some("abc123"),
            Some("executing"),
        )
        .await
        .unwrap()
        .is_none());

    assert_eq!(
        repo.list_links_for_local(&ExternalIssueLocalObject::task("task-1"))
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn service_delegates_provider_ticket_operation_apis_to_repo() {
    let repo: Arc<dyn ExternalIssueLinkRepository> =
        Arc::new(MemoryExternalIssueLinkRepository::new());
    let service = ExternalIssueLinkService::new(Arc::clone(&repo));

    // upsert_provider_ticket_operation forwards the input and returns the repo result.
    let created = service
        .upsert_provider_ticket_operation(sample_ticket_operation())
        .await
        .unwrap();
    assert_eq!(created.client_operation_id, "client-operation-1");
    assert_eq!(created.operation, ProviderTicketOperationKind::Comment);
    assert_eq!(created.status, ProviderTicketOperationStatus::Pending);
    // Same row is visible directly through the backing repo (proves delegation, not a copy).
    assert_eq!(
        repo.find_provider_ticket_operation_by_client_operation_id("client-operation-1")
            .await
            .unwrap()
            .expect("repo should hold the upserted operation")
            .id,
        created.id
    );

    // find_provider_ticket_operation_by_client_operation_id forwards the lookup key.
    let found = service
        .find_provider_ticket_operation_by_client_operation_id("client-operation-1")
        .await
        .unwrap()
        .expect("service should resolve the operation");
    assert_eq!(found.id, created.id);
    assert!(service
        .find_provider_ticket_operation_by_client_operation_id("missing")
        .await
        .unwrap()
        .is_none());

    // list_provider_ticket_operations_for_ticket forwards all five filter args unchanged.
    let listed = service
        .list_provider_ticket_operations_for_ticket(
            "linear",
            "issue",
            "lin_123",
            Some("LIN-42"),
            Some("project-1"),
        )
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.id);
    // A mismatched required arg forwards through and yields no rows.
    assert!(service
        .list_provider_ticket_operations_for_ticket("github", "issue", "lin_123", None, None)
        .await
        .unwrap()
        .is_empty());

    // update_provider_ticket_operation_status forwards id/status/provider id/error and
    // returns the repo result.
    let updated = service
        .update_provider_ticket_operation_status(
            &created.id,
            ProviderTicketOperationStatus::Succeeded,
            Some("provider-op-1"),
            None,
        )
        .await
        .unwrap()
        .expect("operation should update");
    assert_eq!(updated.id, created.id);
    assert_eq!(updated.status, ProviderTicketOperationStatus::Succeeded);
    assert_eq!(
        updated.provider_operation_id.as_deref(),
        Some("provider-op-1")
    );
    assert!(updated.completed_at.is_some());
    // Missing id forwards through and returns None.
    assert!(service
        .update_provider_ticket_operation_status(
            "missing-operation",
            ProviderTicketOperationStatus::Failed,
            None,
            Some("nope"),
        )
        .await
        .unwrap()
        .is_none());
}

// Tests for MemoryWebhookRegistrationRepository
use crate::domain::repositories::{WebhookRegistration, WebhookRegistrationRepository};
use crate::infrastructure::memory::MemoryWebhookRegistrationRepository;

fn make_repo() -> MemoryWebhookRegistrationRepository {
    MemoryWebhookRegistrationRepository::new()
}

fn make_reg(id: &str, api_key_id: &str, url: &str) -> WebhookRegistration {
    WebhookRegistration {
        id: id.to_string(),
        api_key_id: api_key_id.to_string(),
        url: url.to_string(),
        event_types: None,
        project_ids: "[\"proj-1\"]".to_string(),
        secret: "deadbeef".repeat(8),
        active: true,
        failure_count: 0,
        last_failure_at: None,
        created_at: "2024-01-01T00:00:00Z".to_string(),
        updated_at: "2024-01-01T00:00:00Z".to_string(),
    }
}

#[tokio::test]
async fn test_upsert_refreshes_project_ids() {
    let repo = make_repo();
    // Initial registration with proj-1 only
    let reg1 = make_reg("wh-1", "key-1", "http://example.com/hook");
    let result1 = repo.upsert(reg1).await.unwrap();
    assert_eq!(result1.id, "wh-1");
    assert_eq!(result1.project_ids, "[\"proj-1\"]");

    // Re-register same url+api_key_id with expanded project scope
    let mut reg2 = make_reg("wh-NEW", "key-1", "http://example.com/hook");
    reg2.project_ids = "[\"proj-1\",\"proj-2\"]".to_string();
    let result2 = repo.upsert(reg2).await.unwrap();

    // Existing id preserved
    assert_eq!(
        result2.id, "wh-1",
        "Re-registration must return the existing id"
    );
    // project_ids updated
    assert_eq!(
        result2.project_ids, "[\"proj-1\",\"proj-2\"]",
        "project_ids must be refreshed on re-registration"
    );
    // active and failure_count reset
    assert!(result2.active);
    assert_eq!(result2.failure_count, 0);

    // Verify via list_active_for_project
    let for_proj2 = repo.list_active_for_project("proj-2").await.unwrap();
    assert_eq!(
        for_proj2.len(),
        1,
        "Webhook must appear for newly-scoped project after re-registration"
    );
    assert_eq!(for_proj2[0].id, "wh-1");
}

#[tokio::test]
async fn test_upsert_refreshes_event_types() {
    let repo = make_repo();
    let reg1 = make_reg("wh-1", "key-1", "http://example.com/hook");
    repo.upsert(reg1).await.unwrap();

    let mut reg2 = make_reg("wh-NEW", "key-1", "http://example.com/hook");
    reg2.event_types = Some("[\"task:status_changed\"]".to_string());
    let result = repo.upsert(reg2).await.unwrap();

    assert_eq!(result.id, "wh-1");
    assert_eq!(
        result.event_types,
        Some("[\"task:status_changed\"]".to_string()),
        "event_types must be refreshed on re-registration"
    );
}

#[tokio::test]
async fn test_upsert_preserves_secret_on_reregistration() {
    let repo = make_repo();
    let mut reg1 = make_reg("wh-1", "key-1", "http://example.com/hook");
    reg1.secret = "original-secret-value".to_string();
    repo.upsert(reg1).await.unwrap();

    let mut reg2 = make_reg("wh-NEW", "key-1", "http://example.com/hook");
    reg2.secret = "new-secret-should-be-ignored".to_string();
    let result = repo.upsert(reg2).await.unwrap();

    assert_eq!(
        result.secret, "original-secret-value",
        "Secret must be preserved across re-registration"
    );
}

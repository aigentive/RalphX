// Tests for SqliteWebhookRegistrationRepository
use crate::domain::repositories::{WebhookRegistration, WebhookRegistrationRepository};
use crate::infrastructure::sqlite::{
    open_connection, run_migrations, SqliteWebhookRegistrationRepository,
};
use std::path::PathBuf;

fn make_repo() -> SqliteWebhookRegistrationRepository {
    let conn = open_connection(&PathBuf::from(":memory:")).unwrap();
    run_migrations(&conn).unwrap();
    SqliteWebhookRegistrationRepository::new(conn)
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
async fn test_upsert_and_get() {
    let repo = make_repo();
    let reg = make_reg("wh-1", "key-1", "http://localhost:18789/hooks/ralphx");
    let result = repo.upsert(reg).await.unwrap();
    assert_eq!(result.id, "wh-1");
    let fetched = repo.get_by_id("wh-1").await.unwrap().unwrap();
    assert_eq!(fetched.url, "http://localhost:18789/hooks/ralphx");
}

#[tokio::test]
async fn test_upsert_idempotent() {
    let repo = make_repo();
    let reg1 = make_reg("wh-1", "key-1", "http://localhost:18789/hooks/ralphx");
    repo.upsert(reg1).await.unwrap();
    // Second upsert with same url+api_key_id returns same id
    let reg2 = make_reg("wh-NEW", "key-1", "http://localhost:18789/hooks/ralphx");
    let result = repo.upsert(reg2).await.unwrap();
    assert_eq!(result.id, "wh-1"); // Returns existing id, not wh-NEW
}

#[tokio::test]
async fn test_deactivate() {
    let repo = make_repo();
    let reg = make_reg("wh-1", "key-1", "http://localhost:18789/hooks/ralphx");
    repo.upsert(reg).await.unwrap();
    let found = repo.deactivate("wh-1", "key-1").await.unwrap();
    assert!(found);
    // Wrong api_key_id returns false
    let not_found = repo.deactivate("wh-1", "wrong-key").await.unwrap();
    assert!(!not_found);
}

#[tokio::test]
async fn test_list_by_api_key() {
    let repo = make_repo();
    repo.upsert(make_reg("wh-1", "key-1", "http://a.com"))
        .await
        .unwrap();
    repo.upsert(make_reg("wh-2", "key-1", "http://b.com"))
        .await
        .unwrap();
    repo.upsert(make_reg("wh-3", "key-2", "http://c.com"))
        .await
        .unwrap();
    let list = repo.list_by_api_key("key-1").await.unwrap();
    assert_eq!(list.len(), 2);
}

#[tokio::test]
async fn test_get_by_url_and_key() {
    let repo = make_repo();
    repo.upsert(make_reg("wh-1", "key-1", "http://a.com"))
        .await
        .unwrap();
    let found = repo
        .get_by_url_and_key("http://a.com", "key-1")
        .await
        .unwrap();
    assert!(found.is_some());
    let not_found = repo
        .get_by_url_and_key("http://a.com", "key-2")
        .await
        .unwrap();
    assert!(not_found.is_none());
}

#[tokio::test]
async fn test_increment_failure_deactivates_at_10() {
    let repo = make_repo();
    repo.upsert(make_reg("wh-1", "key-1", "http://a.com"))
        .await
        .unwrap();
    for _ in 0..10 {
        repo.increment_failure("wh-1").await.unwrap();
    }
    let reg = repo.get_by_id("wh-1").await.unwrap().unwrap();
    assert!(!reg.active);
    assert_eq!(reg.failure_count, 10);
}

#[tokio::test]
async fn test_reset_failures() {
    let repo = make_repo();
    repo.upsert(make_reg("wh-1", "key-1", "http://a.com"))
        .await
        .unwrap();
    for _ in 0..10 {
        repo.increment_failure("wh-1").await.unwrap();
    }
    repo.reset_failures("wh-1").await.unwrap();
    let reg = repo.get_by_id("wh-1").await.unwrap().unwrap();
    assert!(reg.active);
    assert_eq!(reg.failure_count, 0);
}

#[tokio::test]
async fn test_upsert_refreshes_project_ids_and_preserves_id_and_secret() {
    let repo = make_repo();
    // Initial registration: proj-1 only, known secret
    let mut reg1 = make_reg("wh-1", "key-1", "http://example.com/hook");
    reg1.secret = "original-secret-abcdef".to_string();
    let created = repo.upsert(reg1).await.unwrap();
    assert_eq!(created.id, "wh-1");
    assert_eq!(created.project_ids, "[\"proj-1\"]");

    // Re-register same url+api_key_id with expanded scope including proj-2
    let mut reg2 = make_reg("wh-NEW", "key-1", "http://example.com/hook");
    reg2.project_ids = "[\"proj-1\",\"proj-2\"]".to_string();
    reg2.event_types = Some("[\"task:status_changed\"]".to_string());
    reg2.secret = "new-secret-should-not-replace".to_string();
    let result = repo.upsert(reg2).await.unwrap();

    // Same id preserved
    assert_eq!(result.id, "wh-1", "Existing id must be preserved");
    // Secret preserved (not regenerated on re-registration)
    assert_eq!(result.secret, "original-secret-abcdef", "Secret must be preserved");
    // project_ids refreshed
    assert_eq!(
        result.project_ids, "[\"proj-1\",\"proj-2\"]",
        "project_ids must be refreshed on re-registration"
    );
    // event_types refreshed
    assert_eq!(
        result.event_types,
        Some("[\"task:status_changed\"]".to_string()),
        "event_types must be refreshed on re-registration"
    );
    // active and failure_count reset
    assert!(result.active);
    assert_eq!(result.failure_count, 0);

    // Verify via list_active_for_project — new project must appear
    let for_proj2 = repo.list_active_for_project("proj-2").await.unwrap();
    assert_eq!(
        for_proj2.len(),
        1,
        "Webhook must appear for newly-scoped project after re-registration"
    );
    assert_eq!(for_proj2[0].id, "wh-1");
}

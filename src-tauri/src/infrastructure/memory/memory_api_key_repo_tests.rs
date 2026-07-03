use super::*;
use crate::domain::entities::{ApiKey, ApiKeyId};
use crate::domain::repositories::ApiKeyRepository;

fn make_api_key(id: &str, permissions: i32) -> ApiKey {
    ApiKey {
        id: ApiKeyId::from_string(id),
        name: format!("Key {id}"),
        key_hash: format!("hash-{id}"),
        key_prefix: format!("prefix-{id}"),
        permissions,
        created_at: "2026-06-25T00:00:00Z".to_string(),
        revoked_at: None,
        last_used_at: None,
        grace_expires_at: None,
        metadata: None,
    }
}

#[tokio::test]
async fn audit_log_filters_latest_entries_and_preserves_fields() {
    let repo = MemoryApiKeyRepository::new();

    repo.log_audit("key-1", "start_agent", Some("project-1"), true, Some(123))
        .await
        .unwrap();
    repo.log_audit("key-2", "ignored", None, false, None)
        .await
        .unwrap();
    repo.log_audit("key-1", "create_followup", None, false, Some(7))
        .await
        .unwrap();

    let entries = repo.get_audit_log("key-1", Some(1)).await.unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, 0);
    assert_eq!(entries[0].api_key_id, "key-1");
    assert_eq!(entries[0].tool_name, "create_followup");
    assert_eq!(entries[0].project_id, None);
    assert!(!entries[0].success);
    assert_eq!(entries[0].latency_ms, Some(7));
    assert!(entries[0].created_at.ends_with('Z'));
}

#[tokio::test]
async fn update_api_key_permissions_updates_existing_key_and_ignores_missing() {
    let repo = MemoryApiKeyRepository::new();
    let key = repo.create(make_api_key("key-1", 3)).await.unwrap();

    repo.update_api_key_permissions(key.id.as_str(), 7)
        .await
        .unwrap();
    repo.update_api_key_permissions("missing-key", 1)
        .await
        .unwrap();

    let updated = repo.get_by_id(&key.id).await.unwrap().unwrap();
    assert_eq!(updated.permissions, 7);
}

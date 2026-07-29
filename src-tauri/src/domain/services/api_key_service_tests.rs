// Unit tests for ApiKeyService

use super::*;
use crate::infrastructure::memory::MemoryApiKeyRepository;

fn make_repo() -> MemoryApiKeyRepository {
    MemoryApiKeyRepository::new()
}

// ---------------------------------------------------------------------------
// create_key
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_create_key_settings_ui_default_permissions() {
    let repo = make_repo();
    let result = ApiKeyService::create_key(&repo, "my-key", None, &[], KeySource::SettingsUi)
        .await
        .expect("create_key should succeed");

    assert_eq!(result.key.permissions, 7, "SettingsUi default is 7 (read+write+admin)");
    assert!(!result.raw_key.is_empty());
    assert!(result.raw_key.starts_with("rxk_live_"));
}

#[tokio::test]
async fn test_create_key_http_api_default_permissions() {
    let repo = make_repo();
    let result = ApiKeyService::create_key(&repo, "api-key", None, &[], KeySource::HttpApi)
        .await
        .expect("create_key should succeed");

    assert_eq!(result.key.permissions, 3, "HttpApi default is 3 (read+write)");
}

#[tokio::test]
async fn test_create_key_custom_permissions() {
    let repo = make_repo();
    let result =
        ApiKeyService::create_key(&repo, "custom-key", Some(5), &[], KeySource::HttpApi)
            .await
            .expect("create_key should succeed");

    assert_eq!(result.key.permissions, 5);
}

#[tokio::test]
async fn test_create_key_invalid_permissions() {
    // permissions=16 is above PERMISSION_MAX (15) and must be rejected
    let repo = make_repo();
    let err = ApiKeyService::create_key(&repo, "bad-key", Some(16), &[], KeySource::HttpApi)
        .await
        .expect_err("permissions=16 should fail");

    assert!(
        matches!(err, crate::error::AppError::Validation(_)),
        "expected Validation error, got {:?}",
        err
    );
}

#[tokio::test]
async fn test_create_key_create_project_permission_allowed() {
    // PERMISSION_CREATE_PROJECT = 8 must be accepted after range fix
    let repo = make_repo();
    let result = ApiKeyService::create_key(&repo, "cp-key", Some(8), &[], KeySource::SettingsUi)
        .await
        .expect("permissions=8 (PERMISSION_CREATE_PROJECT) should succeed");
    assert_eq!(result.key.permissions, 8);
}

#[tokio::test]
async fn test_create_key_permission_max_allowed() {
    // PERMISSION_MAX = 15 (all bits set) must be accepted
    let repo = make_repo();
    let result = ApiKeyService::create_key(&repo, "max-key", Some(15), &[], KeySource::SettingsUi)
        .await
        .expect("permissions=15 (PERMISSION_MAX) should succeed");
    assert_eq!(result.key.permissions, 15);
}

#[tokio::test]
async fn test_create_key_above_permission_max_rejected() {
    // permissions=16 is above PERMISSION_MAX and must return a Validation error
    let repo = make_repo();
    let err = ApiKeyService::create_key(&repo, "over-key", Some(16), &[], KeySource::HttpApi)
        .await
        .expect_err("permissions=16 should fail");
    assert!(
        matches!(err, crate::error::AppError::Validation(_)),
        "expected Validation error, got {:?}",
        err
    );
}

#[tokio::test]
async fn test_create_key_zero_permissions_allowed() {
    // 0 is a valid bitmask (no permissions) — must not error
    let repo = make_repo();
    let result = ApiKeyService::create_key(&repo, "zero-perm", Some(0), &[], KeySource::HttpApi)
        .await
        .expect("permissions=0 should be valid");
    assert_eq!(result.key.permissions, 0);
}

#[tokio::test]
async fn test_create_key_project_ids_stored() {
    let repo = make_repo();
    let project_ids = vec!["proj-1".to_string(), "proj-2".to_string()];
    let result =
        ApiKeyService::create_key(&repo, "scoped-key", None, &project_ids, KeySource::SettingsUi)
            .await
            .expect("create_key should succeed");

    let stored = repo.get_projects(&result.key.id).await.unwrap();
    assert_eq!(stored, project_ids);
}

// ---------------------------------------------------------------------------
// rotate_key
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_rotate_key() {
    let repo = make_repo();

    // Create initial key
    let created =
        ApiKeyService::create_key(&repo, "rotate-me", None, &[], KeySource::SettingsUi)
            .await
            .expect("create should succeed");
    let old_key_id = created.key.id.as_str().to_string();
    let old_raw = created.raw_key.clone();

    // Rotate
    let rotated =
        ApiKeyService::rotate_key(&repo, &old_key_id, KeySource::SettingsUi)
            .await
            .expect("rotate should succeed");

    // New raw key must differ
    assert_ne!(rotated.raw_key, old_raw);
    assert!(rotated.raw_key.starts_with("rxk_live_"));

    // Old key should now have a grace period set (revoked_at set + grace_expires_at set)
    let old_key = repo
        .get_by_id(&crate::domain::entities::ApiKeyId::from_string(&old_key_id))
        .await
        .unwrap()
        .expect("old key should still exist");
    assert!(old_key.revoked_at.is_some(), "old key should be revoked");
    assert!(
        old_key.grace_expires_at.is_some(),
        "old key should have a grace period"
    );

    // New key should be active
    assert!(rotated.key.revoked_at.is_none());
}

#[tokio::test]
async fn test_rotate_nonexistent_key_returns_not_found() {
    let repo = make_repo();
    let err = ApiKeyService::rotate_key(&repo, "does-not-exist", KeySource::HttpApi)
        .await
        .expect_err("rotating nonexistent key should fail");

    assert!(
        matches!(err, crate::error::AppError::NotFound(_)),
        "expected NotFound, got {:?}",
        err
    );
}

// ---------------------------------------------------------------------------
// revoke_key
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_revoke_key() {
    let repo = make_repo();

    // Create a key then revoke it
    let created =
        ApiKeyService::create_key(&repo, "to-revoke", None, &[], KeySource::SettingsUi)
            .await
            .expect("create should succeed");
    let key_id = created.key.id.as_str().to_string();

    ApiKeyService::revoke_key(&repo, &key_id, KeySource::SettingsUi)
        .await
        .expect("revoke should succeed");

    // Fetch the key — it should be revoked
    let stored = repo
        .get_by_id(&crate::domain::entities::ApiKeyId::from_string(&key_id))
        .await
        .unwrap()
        .expect("key should still exist in store");

    assert!(stored.revoked_at.is_some(), "key should be revoked");
    assert!(!stored.is_active());
}

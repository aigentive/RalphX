// Tests for SqliteApiKeyRepository
// Tests run against in-memory SQLite with full migrations

use std::ops::Deref;

use crate::domain::entities::{
    ApiKey, ApiKeyId, Project, ProjectId, PERMISSION_ADMIN, PERMISSION_CREATE_PROJECT,
    PERMISSION_READ, PERMISSION_WRITE,
};
use crate::domain::repositories::{ApiKeyRepository, CreateKeyParams};
use crate::domain::services::key_crypto::{generate_raw_key, hash_key, key_prefix};
use crate::infrastructure::sqlite::sqlite_api_key_repo::SqliteApiKeyRepository;
use crate::testing::SqliteTestDb;

struct ApiKeyRepoFixture {
    db: SqliteTestDb,
    repo: SqliteApiKeyRepository,
}

impl ApiKeyRepoFixture {
    fn db(&self) -> &SqliteTestDb {
        &self.db
    }
}

impl Deref for ApiKeyRepoFixture {
    type Target = SqliteApiKeyRepository;

    fn deref(&self) -> &Self::Target {
        &self.repo
    }
}

fn setup_repo() -> ApiKeyRepoFixture {
    let db = SqliteTestDb::new("sqlite-api-key-repo");
    let repo = SqliteApiKeyRepository::from_shared(db.shared_conn());
    ApiKeyRepoFixture { db, repo }
}

fn insert_test_project(db: &SqliteTestDb, project_id: &str) {
    let mut project = Project::new(
        "test-project".to_string(),
        format!("/tmp/test/{}", project_id),
    );
    project.id = ProjectId::from_string(project_id.to_string());
    db.insert_project(project);
}

fn make_key(name: &str) -> ApiKey {
    let raw_key = generate_raw_key();
    ApiKey {
        id: ApiKeyId::new(),
        name: name.to_string(),
        key_hash: hash_key(&raw_key),
        key_prefix: key_prefix(&raw_key),
        permissions: 3,
        created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        revoked_at: None,
        last_used_at: None,
        grace_expires_at: None,
        metadata: None,
    }
}

#[test]
fn test_generate_raw_key_format() {
    let key = generate_raw_key();
    assert!(key.starts_with("rxk_live_"), "key should start with rxk_live_");
    assert_eq!(key.len(), 41, "rxk_live_ (9) + 32 chars = 41");
}

#[test]
fn test_hash_key_deterministic() {
    let hash1 = hash_key("test_key_value");
    let hash2 = hash_key("test_key_value");
    assert_eq!(hash1, hash2, "same input must produce same hash");
    assert_ne!(hash_key("a"), hash_key("b"), "different inputs must produce different hashes");
}

#[test]
fn test_key_prefix_length() {
    let raw = "rxk_live_abcdefgh12345";
    let prefix = key_prefix(raw);
    assert_eq!(prefix, "rxk_live_abc", "prefix should be first 12 chars");
    assert_eq!(prefix.len(), 12);
}

#[test]
fn test_permissions_bitmask() {
    let key = ApiKey {
        id: ApiKeyId::new(),
        name: "test".to_string(),
        key_hash: "x".to_string(),
        key_prefix: "x".to_string(),
        permissions: 5, // read(1) + admin(4)
        created_at: "2026-01-01T00:00:00Z".to_string(),
        revoked_at: None,
        last_used_at: None,
        grace_expires_at: None,
        metadata: None,
    };
    assert!(key.has_permission(PERMISSION_READ), "should have read");
    assert!(!key.has_permission(PERMISSION_WRITE), "should NOT have write");
    assert!(key.has_permission(PERMISSION_ADMIN), "should have admin");
}

#[test]
fn test_is_in_grace_period_true() {
    let future = (chrono::Utc::now() + chrono::Duration::seconds(60))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let key = ApiKey {
        id: ApiKeyId::new(),
        name: "test".to_string(),
        key_hash: "x".to_string(),
        key_prefix: "x".to_string(),
        permissions: 3,
        created_at: "2026-01-01T00:00:00Z".to_string(),
        revoked_at: Some("2026-01-01T00:00:01Z".to_string()),
        last_used_at: None,
        grace_expires_at: Some(future),
        metadata: None,
    };
    assert!(key.is_in_grace_period(), "key within grace period should return true");
}

#[test]
fn test_is_in_grace_period_false_expired() {
    let past = "2020-01-01T00:00:00Z".to_string();
    let key = ApiKey {
        id: ApiKeyId::new(),
        name: "test".to_string(),
        key_hash: "x".to_string(),
        key_prefix: "x".to_string(),
        permissions: 3,
        created_at: "2020-01-01T00:00:00Z".to_string(),
        revoked_at: Some("2020-01-01T00:00:01Z".to_string()),
        last_used_at: None,
        grace_expires_at: Some(past),
        metadata: None,
    };
    assert!(!key.is_in_grace_period(), "expired grace period should return false");
}

#[tokio::test]
async fn test_create_and_get_by_id() {
    let repo = setup_repo();
    let key = make_key("Test Key");
    let id = key.id.clone();

    let created = repo.create(key).await.expect("create failed");
    let found = repo.get_by_id(&id).await.expect("get_by_id failed").expect("not found");

    assert_eq!(found.id, id);
    assert_eq!(found.name, "Test Key");
    assert_eq!(found.permissions, 3);
    assert!(found.revoked_at.is_none());
    assert!(found.last_used_at.is_none());
    assert_eq!(found.key_prefix, created.key_prefix);
}

#[tokio::test]
async fn test_get_by_id_missing_returns_none() {
    let repo = setup_repo();
    let missing_id = ApiKeyId::from_string("nonexistent-id");
    let result = repo.get_by_id(&missing_id).await.expect("get should succeed");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_get_by_hash() {
    let repo = setup_repo();
    let raw = generate_raw_key();
    let hash = hash_key(&raw);
    let prefix = key_prefix(&raw);

    let key = ApiKey {
        id: ApiKeyId::new(),
        name: "Hash Test".to_string(),
        key_hash: hash.clone(),
        key_prefix: prefix,
        permissions: 3,
        created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        revoked_at: None,
        last_used_at: None,
        grace_expires_at: None,
        metadata: None,
    };

    repo.create(key).await.expect("create failed");
    let found = repo.get_by_hash(&hash).await.expect("get_by_hash failed").expect("not found");
    assert_eq!(found.key_hash, hash);
    assert_eq!(found.name, "Hash Test");
}

#[tokio::test]
async fn test_get_by_hash_missing_returns_none() {
    let repo = setup_repo();
    let result = repo.get_by_hash("nonexistent_hash").await.expect("should succeed");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_list_returns_all_keys() {
    let repo = setup_repo();
    repo.create(make_key("Key A")).await.expect("create A");
    repo.create(make_key("Key B")).await.expect("create B");
    repo.create(make_key("Key C")).await.expect("create C");

    let keys = repo.list().await.expect("list failed");
    assert_eq!(keys.len(), 3);
    let names: Vec<&str> = keys.iter().map(|k| k.name.as_str()).collect();
    assert!(names.contains(&"Key A"));
    assert!(names.contains(&"Key B"));
    assert!(names.contains(&"Key C"));
}

#[tokio::test]
async fn test_revoke_sets_revoked_at() {
    let repo = setup_repo();
    let key = make_key("Revoke Me");
    let id = key.id.clone();

    repo.create(key).await.expect("create");
    repo.revoke(&id).await.expect("revoke");

    let found = repo.get_by_id(&id).await.expect("get").expect("found");
    assert!(found.revoked_at.is_some(), "revoked_at must be set after revoke");
    assert!(!found.is_active(), "is_active should be false after revoke");
}

#[tokio::test]
async fn test_set_and_get_projects() {
    let repo = setup_repo();
    let key = make_key("Project Key");
    let id = key.id.clone();
    repo.create(key).await.expect("create");

    insert_test_project(repo.db(), "proj-1");
    insert_test_project(repo.db(), "proj-2");

    let project_ids = vec!["proj-1".to_string(), "proj-2".to_string()];
    repo.set_projects(&id, &project_ids).await.expect("set_projects");

    let mut found = repo.get_projects(&id).await.expect("get_projects");
    found.sort();
    let mut expected = project_ids.clone();
    expected.sort();
    assert_eq!(found, expected);
}

#[tokio::test]
async fn test_set_projects_replaces_existing() {
    let repo = setup_repo();
    let key = make_key("Replace Projects");
    let id = key.id.clone();
    repo.create(key).await.expect("create");

    insert_test_project(repo.db(), "old-proj");
    insert_test_project(repo.db(), "new-proj-1");
    insert_test_project(repo.db(), "new-proj-2");

    repo.set_projects(&id, &["old-proj".to_string()]).await.expect("first set");
    repo.set_projects(&id, &["new-proj-1".to_string(), "new-proj-2".to_string()])
        .await
        .expect("second set");

    let mut found = repo.get_projects(&id).await.expect("get_projects");
    found.sort();
    assert_eq!(found, vec!["new-proj-1".to_string(), "new-proj-2".to_string()]);
    assert!(!found.contains(&"old-proj".to_string()), "old project should be replaced");
}

#[tokio::test]
async fn test_get_projects_empty_for_new_key() {
    let repo = setup_repo();
    let key = make_key("No Projects");
    let id = key.id.clone();
    repo.create(key).await.expect("create");

    let projects = repo.get_projects(&id).await.expect("get_projects");
    assert!(projects.is_empty(), "new key should have no projects");
}

#[tokio::test]
async fn test_set_grace_period() {
    let repo = setup_repo();
    let key = make_key("Grace Key");
    let id = key.id.clone();
    repo.create(key).await.expect("create");

    let grace_time = (chrono::Utc::now() + chrono::Duration::seconds(60))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    repo.set_grace_period(&id, &grace_time).await.expect("set_grace_period");

    let found = repo.get_by_id(&id).await.expect("get").expect("found");
    assert!(found.grace_expires_at.is_some(), "grace_expires_at should be set");
    assert_eq!(found.grace_expires_at.unwrap(), grace_time);
}

#[tokio::test]
async fn test_update_last_used() {
    let repo = setup_repo();
    let key = make_key("Used Key");
    let id = key.id.clone();
    repo.create(key).await.expect("create");

    let ts = "2026-03-06T12:00:00Z".to_string();
    repo.update_last_used(&id, &ts).await.expect("update_last_used");

    let found = repo.get_by_id(&id).await.expect("get").expect("found");
    assert_eq!(found.last_used_at.as_deref(), Some("2026-03-06T12:00:00Z"));
}

#[tokio::test]
async fn test_log_audit_success() {
    let repo = setup_repo();
    let key = make_key("Audit Key");
    let id = key.id.clone();
    repo.create(key).await.expect("create");

    // Should not error
    repo.log_audit(id.as_str(), "validate_key", Some("proj-1"), true, Some(42))
        .await
        .expect("log_audit failed");

    // Log another entry
    repo.log_audit(id.as_str(), "list_tasks", None, false, None)
        .await
        .expect("second log_audit failed");
}

// ============================================================================
// rotate_key_atomic tests
// ============================================================================

#[tokio::test]
async fn test_rotate_key_atomic_success() {
    use crate::domain::repositories::RotateKeyParams;

    let repo = setup_repo();
    let old_key_raw = generate_raw_key();
    let old_key = ApiKey {
        id: ApiKeyId::new(),
        name: "Original Key".to_string(),
        key_hash: hash_key(&old_key_raw),
        key_prefix: key_prefix(&old_key_raw),
        permissions: PERMISSION_READ | PERMISSION_WRITE,
        created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        revoked_at: None,
        last_used_at: None,
        grace_expires_at: None,
        metadata: None,
    };
    let old_key_id = old_key.id.clone();
    repo.create(old_key).await.expect("create old key");

    let new_key_raw = generate_raw_key();
    let new_key = ApiKey {
        id: ApiKeyId::new(),
        name: "Original Key".to_string(),
        key_hash: hash_key(&new_key_raw),
        key_prefix: key_prefix(&new_key_raw),
        permissions: PERMISSION_READ | PERMISSION_WRITE,
        created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        revoked_at: None,
        last_used_at: None,
        grace_expires_at: None,
        metadata: None,
    };
    let new_key_id = new_key.id.clone();

    let grace_expires_at = (chrono::Utc::now() + chrono::Duration::seconds(60))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    repo.rotate_key_atomic(RotateKeyParams {
        new_key,
        project_ids: vec![],
        old_key_id: old_key_id.clone(),
        grace_expires_at: grace_expires_at.clone(),
    })
    .await
    .expect("rotate_key_atomic failed");

    // New key must exist
    let found_new = repo
        .get_by_id(&new_key_id)
        .await
        .expect("get new key")
        .expect("new key not found");
    assert_eq!(found_new.name, "Original Key");
    assert!(found_new.revoked_at.is_none(), "new key should not be revoked");

    // Old key must have grace period AND revoked_at set
    let found_old = repo
        .get_by_id(&old_key_id)
        .await
        .expect("get old key")
        .expect("old key not found");
    assert_eq!(
        found_old.grace_expires_at.as_deref(),
        Some(grace_expires_at.as_str()),
        "old key must have grace_expires_at set"
    );
    assert!(found_old.is_in_grace_period(), "old key must be in grace period");
    assert!(
        found_old.revoked_at.is_some(),
        "old key must have revoked_at set after rotation"
    );
    assert!(
        !found_old.is_active(),
        "old key must not be active (revoked_at set) after rotation"
    );
}

#[tokio::test]
async fn test_rotate_key_atomic_rollback_on_duplicate_key_hash() {
    use crate::domain::repositories::RotateKeyParams;

    // Simulate a partial failure: if we try to insert a new key with a hash that already
    // exists (UNIQUE constraint violation), the transaction must roll back and the old
    // key must NOT have its grace period updated.
    let repo = setup_repo();

    let old_key_raw = generate_raw_key();
    let old_key_hash = hash_key(&old_key_raw);
    let old_key = ApiKey {
        id: ApiKeyId::new(),
        name: "Key To Rotate".to_string(),
        key_hash: old_key_hash.clone(),
        key_prefix: key_prefix(&old_key_raw),
        permissions: PERMISSION_ADMIN,
        created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        revoked_at: None,
        last_used_at: None,
        grace_expires_at: None,
        metadata: None,
    };
    let old_key_id = old_key.id.clone();
    repo.create(old_key).await.expect("create old key");

    // Attempt to rotate with a new key that has the SAME hash as the old key,
    // triggering a UNIQUE constraint violation on key_hash.
    let conflicting_new_key = ApiKey {
        id: ApiKeyId::new(),
        name: "Key To Rotate".to_string(),
        key_hash: old_key_hash.clone(), // duplicate hash — will cause UNIQUE violation
        key_prefix: "rxk_live_ddd".to_string(),
        permissions: PERMISSION_ADMIN,
        created_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        revoked_at: None,
        last_used_at: None,
        grace_expires_at: None,
        metadata: None,
    };
    let conflicting_new_id = conflicting_new_key.id.clone();

    let grace_expires_at = (chrono::Utc::now() + chrono::Duration::seconds(60))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    let result = repo
        .rotate_key_atomic(RotateKeyParams {
            new_key: conflicting_new_key,
            project_ids: vec![],
            old_key_id: old_key_id.clone(),
            grace_expires_at,
        })
        .await;

    // The rotation must fail
    assert!(result.is_err(), "rotation with duplicate hash must fail");

    // The conflicting new key must NOT exist (transaction rolled back)
    let found_new = repo
        .get_by_id(&conflicting_new_id)
        .await
        .expect("get_by_id should not error");
    assert!(
        found_new.is_none(),
        "new key must not exist after failed rotation"
    );

    // The old key must NOT have a grace period set (transaction rolled back)
    let found_old = repo
        .get_by_id(&old_key_id)
        .await
        .expect("get old key")
        .expect("old key must still exist");
    assert!(
        found_old.grace_expires_at.is_none(),
        "old key grace_expires_at must not be set after failed rotation"
    );
}

// After grace period expires, old key should be fully inactive
#[tokio::test]
async fn test_old_key_inactive_after_grace_expires() {
    use crate::domain::repositories::RotateKeyParams;

    let repo = setup_repo();
    let old_key = make_key("Old Key");
    let old_key_id = old_key.id.clone();
    repo.create(old_key).await.expect("create old key");

    let new_key = make_key("New Key");

    // Grace period set to a time in the past — already expired
    let grace_expires_at = (chrono::Utc::now() - chrono::Duration::seconds(10))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    repo.rotate_key_atomic(RotateKeyParams {
        new_key,
        project_ids: vec![],
        old_key_id: old_key_id.clone(),
        grace_expires_at,
    })
    .await
    .expect("rotate_key_atomic failed");

    let found_old = repo
        .get_by_id(&old_key_id)
        .await
        .expect("get old key")
        .expect("old key not found");

    assert!(found_old.revoked_at.is_some(), "old key must have revoked_at set");
    assert!(!found_old.is_active(), "old key must not be active after revocation");
    assert!(
        !found_old.is_in_grace_period(),
        "grace period has expired, is_in_grace_period() must return false"
    );
}

// During grace period, old key is revoked but still usable via grace period check
#[tokio::test]
async fn test_old_key_usable_during_grace_period() {
    use crate::domain::repositories::RotateKeyParams;

    let repo = setup_repo();
    let old_key = make_key("Old Key");
    let old_key_id = old_key.id.clone();
    repo.create(old_key).await.expect("create old key");

    let new_key = make_key("New Key");

    // Grace period set to a time in the future — still active
    let grace_expires_at = (chrono::Utc::now() + chrono::Duration::seconds(60))
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    repo.rotate_key_atomic(RotateKeyParams {
        new_key,
        project_ids: vec![],
        old_key_id: old_key_id.clone(),
        grace_expires_at,
    })
    .await
    .expect("rotate_key_atomic failed");

    let found_old = repo
        .get_by_id(&old_key_id)
        .await
        .expect("get old key")
        .expect("old key not found");

    // revoked_at is eagerly set during rotation
    assert!(
        found_old.revoked_at.is_some(),
        "old key must have revoked_at set immediately after rotation"
    );
    // is_active() checks revoked_at only — key is revoked
    assert!(!found_old.is_active(), "old key must not be active (revoked_at is set)");
    // but the grace window is still open, so the key remains usable
    assert!(
        found_old.is_in_grace_period(),
        "old key must still be in grace period (grace_expires_at is in the future)"
    );
}

// ============================================================================
// set_projects transaction safety tests
// ============================================================================

#[tokio::test]
async fn test_set_projects_transaction_rollback_preserves_existing_associations() {
    // If any INSERT in set_projects fails (e.g. duplicate project_id triggers PRIMARY KEY
    // violation), the DELETE must also be rolled back — existing associations must be intact.
    let repo = setup_repo();
    let key = make_key("Transaction Safety Key");
    let id = key.id.clone();
    repo.create(key).await.expect("create");

    insert_test_project(repo.db(), "safe-proj");

    // Set initial association
    repo.set_projects(&id, &["safe-proj".to_string()])
        .await
        .expect("initial set_projects");

    let initial = repo.get_projects(&id).await.expect("get initial");
    assert_eq!(initial, vec!["safe-proj".to_string()]);

    // Trigger failure: duplicate project_id causes PRIMARY KEY violation on the second INSERT
    let result = repo
        .set_projects(
            &id,
            &["safe-proj".to_string(), "safe-proj".to_string()],
        )
        .await;

    assert!(result.is_err(), "set_projects with duplicate project_id must fail");

    // The DELETE must have been rolled back — original association still present
    let after = repo.get_projects(&id).await.expect("get after rollback");
    assert_eq!(
        after,
        vec!["safe-proj".to_string()],
        "existing project associations must be preserved after failed set_projects"
    );
}

// ============================================================================
// CREATE_PROJECT permission tests
// ============================================================================

#[test]
fn test_has_permission_create_project_combinations() {
    // true cases: bit 3 (value 8) is set
    let true_cases: &[(i32, &str)] = &[
        (8, "permissions=8 (CREATE_PROJECT only)"),
        (11, "permissions=11 (READ|WRITE|CREATE_PROJECT)"),
        (15, "permissions=15 (all bits)"),
        (9, "permissions=9 (READ|CREATE_PROJECT)"),
        (12, "permissions=12 (ADMIN|CREATE_PROJECT)"),
    ];
    for (perms, label) in true_cases {
        let key = ApiKey {
            id: ApiKeyId::new(),
            name: "test".to_string(),
            key_hash: "x".to_string(),
            key_prefix: "x".to_string(),
            permissions: *perms,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            revoked_at: None,
            last_used_at: None,
            grace_expires_at: None,
            metadata: None,
        };
        assert!(
            key.has_permission(PERMISSION_CREATE_PROJECT),
            "should have CREATE_PROJECT: {}",
            label
        );
    }

    // false cases: bit 3 is not set
    let false_cases: &[(i32, &str)] = &[
        (3, "permissions=3 (READ|WRITE)"),
        (7, "permissions=7 (READ|WRITE|ADMIN)"),
        (0, "permissions=0 (none)"),
        (1, "permissions=1 (READ only)"),
        (4, "permissions=4 (ADMIN only)"),
    ];
    for (perms, label) in false_cases {
        let key = ApiKey {
            id: ApiKeyId::new(),
            name: "test".to_string(),
            key_hash: "x".to_string(),
            key_prefix: "x".to_string(),
            permissions: *perms,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            revoked_at: None,
            last_used_at: None,
            grace_expires_at: None,
            metadata: None,
        };
        assert!(
            !key.has_permission(PERMISSION_CREATE_PROJECT),
            "should NOT have CREATE_PROJECT: {}",
            label
        );
    }
}

#[tokio::test]
async fn test_update_api_key_permissions_create_project_roundtrip() {
    let repo = setup_repo();
    let mut key = make_key("Permissions Roundtrip Key");
    key.permissions = 3; // READ|WRITE, no CREATE_PROJECT
    let id = key.id.clone();

    repo.create(key).await.expect("create failed");

    // Verify CREATE_PROJECT is NOT set initially
    let before = repo.get_by_id(&id).await.expect("get").expect("found");
    assert_eq!(before.permissions, 3);
    assert!(
        !before.has_permission(PERMISSION_CREATE_PROJECT),
        "should NOT have CREATE_PROJECT before update"
    );

    // Add CREATE_PROJECT: 3 | 8 = 11
    repo.update_api_key_permissions(id.as_str(), 11)
        .await
        .expect("update_api_key_permissions failed");

    // Verify CREATE_PROJECT is set after update
    let after = repo.get_by_id(&id).await.expect("get").expect("found");
    assert_eq!(after.permissions, 11, "permissions should be 11 (READ|WRITE|CREATE_PROJECT)");
    assert!(
        after.has_permission(PERMISSION_CREATE_PROJECT),
        "should have CREATE_PROJECT after update"
    );
    assert!(
        after.has_permission(PERMISSION_READ),
        "should still have READ after update"
    );
    assert!(
        after.has_permission(PERMISSION_WRITE),
        "should still have WRITE after update"
    );
    assert!(
        !after.has_permission(PERMISSION_ADMIN),
        "should NOT have ADMIN (was not set)"
    );
}

// ============================================================================
// create_key_atomic tests
// ============================================================================

#[tokio::test]
async fn test_create_key_atomic_success_no_projects() {
    let repo = setup_repo();
    let key = make_key("Atomic Create No Projects");
    let key_id = key.id.clone();

    let created = repo
        .create_key_atomic(CreateKeyParams {
            new_key: key,
            project_ids: vec![],
        })
        .await
        .expect("create_key_atomic failed");

    assert_eq!(created.id, key_id);
    assert_eq!(created.name, "Atomic Create No Projects");

    let found = repo.get_by_id(&key_id).await.expect("get_by_id").expect("not found");
    assert_eq!(found.id, key_id);
    let projects = repo.get_projects(&key_id).await.expect("get_projects");
    assert!(projects.is_empty(), "should have no project associations");
}

#[tokio::test]
async fn test_create_key_atomic_success_with_projects() {
    let repo = setup_repo();
    insert_test_project(repo.db(), "proj-a");
    insert_test_project(repo.db(), "proj-b");

    let key = make_key("Atomic Create With Projects");
    let key_id = key.id.clone();

    let created = repo
        .create_key_atomic(CreateKeyParams {
            new_key: key,
            project_ids: vec!["proj-a".to_string(), "proj-b".to_string()],
        })
        .await
        .expect("create_key_atomic failed");

    assert_eq!(created.id, key_id);

    let found = repo.get_by_id(&key_id).await.expect("get_by_id").expect("not found");
    assert_eq!(found.name, "Atomic Create With Projects");

    let mut projects = repo.get_projects(&key_id).await.expect("get_projects");
    projects.sort();
    assert_eq!(projects, vec!["proj-a".to_string(), "proj-b".to_string()]);
}

#[tokio::test]
async fn test_create_key_atomic_rollback_on_duplicate_project() {
    // If the project association step fails mid-transaction (PRIMARY KEY violation
    // from duplicate project_ids), the key insert must be rolled back — no orphaned keys.
    let repo = setup_repo();
    insert_test_project(repo.db(), "proj-dup");

    let key = make_key("Orphan Candidate");
    let key_id = key.id.clone();

    // Duplicate project IDs trigger a PRIMARY KEY violation on (api_key_id, project_id)
    // during step 2 of the transaction, causing rollback of the step 1 key insert.
    let result = repo
        .create_key_atomic(CreateKeyParams {
            new_key: key,
            project_ids: vec!["proj-dup".to_string(), "proj-dup".to_string()],
        })
        .await;

    assert!(result.is_err(), "create_key_atomic must fail on duplicate project_id");

    // The key must NOT exist — entire transaction was rolled back
    let found = repo.get_by_id(&key_id).await.expect("get_by_id should not error");
    assert!(
        found.is_none(),
        "key must not exist after failed atomic create (no orphaned keys)"
    );
}

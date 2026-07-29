// Tests for SqliteMemoryEntryRepository (sqlite_memory_entry_repo.rs)
// Included via #[path = "sqlite_memory_entry_repo_tests.rs"] in sqlite_memory_entry_repo.rs

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{MemoryBucket, MemoryEntry, MemoryEntryId, MemoryStatus};
use crate::domain::repositories::MemoryEntryRepository;
use crate::testing::SqliteTestDb;
use chrono::Utc;
use std::sync::Arc;

use super::SqliteMemoryEntryRepository;

fn setup_test_db() -> SqliteTestDb {
    let db = SqliteTestDb::new("sqlite_memory_entry_repo_tests");
    // memory_entries has FK to projects(id) — insert required parent records
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO projects (id, name, working_directory) VALUES ('proj-1', 'Project 1', '/test/proj1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, working_directory) VALUES ('proj-2', 'Project 2', '/test/proj2')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, working_directory) VALUES ('proj-a', 'Project A', '/test/proja')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, working_directory) VALUES ('proj-b', 'Project B', '/test/projb')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, working_directory) VALUES ('proj-shared', 'Shared Project', '/test/shared')",
            [],
        )
        .unwrap();
    });
    db
}

fn make_entry(id: &str, project_id: &str) -> MemoryEntry {
    let now = Utc::now();
    MemoryEntry {
        id: MemoryEntryId::from(id.to_string()),
        project_id: ProjectId::from_string(project_id.to_string()),
        bucket: MemoryBucket::ArchitecturePatterns,
        title: "Test Entry".to_string(),
        summary: "Test summary".to_string(),
        details_markdown: "# Test\nDetails here".to_string(),
        scope_paths: vec!["src/**".to_string()],
        source_context_type: None,
        source_context_id: None,
        source_conversation_id: None,
        source_rule_file: None,
        quality_score: None,
        status: MemoryStatus::Active,
        content_hash: "hash-abc123".to_string(),
        created_at: now,
        updated_at: now,
    }
}

// --- create ---

#[tokio::test]
async fn test_create_returns_entry() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let entry = make_entry("entry-1", "proj-1");
    let result = repo.create(entry.clone()).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.id.as_str(), "entry-1");
    assert_eq!(created.title, "Test Entry");
}

#[tokio::test]
async fn test_create_preserves_all_optional_fields() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let now = Utc::now();
    let entry = MemoryEntry {
        id: MemoryEntryId::from("entry-full".to_string()),
        project_id: ProjectId::from_string("proj-1".to_string()),
        bucket: MemoryBucket::ImplementationDiscoveries,
        title: "Full Entry".to_string(),
        summary: "Summary".to_string(),
        details_markdown: "Details".to_string(),
        scope_paths: vec!["src/api/**".to_string(), "tests/**".to_string()],
        source_context_type: Some("task".to_string()),
        source_context_id: Some("task-99".to_string()),
        source_conversation_id: Some("conv-77".to_string()),
        source_rule_file: Some("rules/api.md".to_string()),
        quality_score: Some(0.95),
        status: MemoryStatus::Active,
        content_hash: "fullhash".to_string(),
        created_at: now,
        updated_at: now,
    };

    repo.create(entry.clone()).await.unwrap();

    let loaded = repo.get_by_id(&entry.id).await.unwrap().unwrap();
    assert_eq!(loaded.source_context_type, Some("task".to_string()));
    assert_eq!(loaded.source_context_id, Some("task-99".to_string()));
    assert_eq!(loaded.source_conversation_id, Some("conv-77".to_string()));
    assert_eq!(loaded.source_rule_file, Some("rules/api.md".to_string()));
    assert!((loaded.quality_score.unwrap() - 0.95).abs() < 1e-9);
    assert_eq!(loaded.scope_paths, vec!["src/api/**", "tests/**"]);
    assert!(matches!(loaded.bucket, MemoryBucket::ImplementationDiscoveries));
}

// --- get_by_id ---

#[tokio::test]
async fn test_get_by_id_found() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let entry = make_entry("entry-2", "proj-1");
    repo.create(entry.clone()).await.unwrap();

    let result = repo.get_by_id(&entry.id).await;
    assert!(result.is_ok());
    let found = result.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().id.as_str(), "entry-2");
}

#[tokio::test]
async fn test_get_by_id_not_found() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let missing_id = MemoryEntryId::from("does-not-exist".to_string());
    let result = repo.get_by_id(&missing_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

// --- find_by_content_hash ---

#[tokio::test]
async fn test_find_by_content_hash_found() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let entry = make_entry("entry-hash", "proj-1");
    repo.create(entry.clone()).await.unwrap();

    let result = repo
        .find_by_content_hash(
            &ProjectId::from_string("proj-1".to_string()),
            &MemoryBucket::ArchitecturePatterns,
            "hash-abc123",
        )
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_some());
}

#[tokio::test]
async fn test_find_by_content_hash_not_found() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let result = repo
        .find_by_content_hash(
            &ProjectId::from_string("proj-1".to_string()),
            &MemoryBucket::ArchitecturePatterns,
            "no-such-hash",
        )
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_find_by_content_hash_inactive_not_returned() {
    // find_by_content_hash filters status = 'active'; obsolete entries must not appear
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let entry = make_entry("entry-obsolete", "proj-1");
    repo.create(entry.clone()).await.unwrap();
    repo.update_status(&entry.id, MemoryStatus::Obsolete).await.unwrap();

    let result = repo
        .find_by_content_hash(
            &ProjectId::from_string("proj-1".to_string()),
            &MemoryBucket::ArchitecturePatterns,
            "hash-abc123",
        )
        .await
        .unwrap();

    assert!(result.is_none());
}

// --- get_by_project ---

#[tokio::test]
async fn test_get_by_project_empty() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let result = repo
        .get_by_project(&ProjectId::from_string("empty-proj".to_string()))
        .await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_by_project_returns_matching_entries() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    repo.create(make_entry("entry-a", "proj-a")).await.unwrap();
    repo.create(make_entry("entry-b", "proj-a")).await.unwrap();
    repo.create(make_entry("entry-other", "proj-b")).await.unwrap();

    let result = repo
        .get_by_project(&ProjectId::from_string("proj-a".to_string()))
        .await
        .unwrap();

    assert_eq!(result.len(), 2);
    assert!(result.iter().all(|e| e.project_id.as_str() == "proj-a"));
}

#[tokio::test]
async fn test_get_by_project_includes_all_statuses() {
    // This repo's get_by_project does NOT filter by status
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    repo.create(make_entry("active-e", "proj-1")).await.unwrap();

    let mut obsolete = make_entry("obsolete-e", "proj-1");
    obsolete.status = MemoryStatus::Obsolete;
    repo.create(obsolete).await.unwrap();

    let results = repo
        .get_by_project(&ProjectId::from_string("proj-1".to_string()))
        .await
        .unwrap();

    // Both active and obsolete are returned (no status filter in this impl)
    assert_eq!(results.len(), 2);
}

// --- get_by_project_and_status ---

#[tokio::test]
async fn test_get_by_project_and_status_filters_correctly() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    repo.create(make_entry("active-1", "proj-1")).await.unwrap();
    repo.create(make_entry("active-2", "proj-1")).await.unwrap();

    let mut obsolete = make_entry("obsolete-1", "proj-1");
    obsolete.status = MemoryStatus::Obsolete;
    repo.create(obsolete).await.unwrap();

    let active = repo
        .get_by_project_and_status(
            &ProjectId::from_string("proj-1".to_string()),
            MemoryStatus::Active,
        )
        .await
        .unwrap();
    assert_eq!(active.len(), 2);

    let obsolete_result = repo
        .get_by_project_and_status(
            &ProjectId::from_string("proj-1".to_string()),
            MemoryStatus::Obsolete,
        )
        .await
        .unwrap();
    assert_eq!(obsolete_result.len(), 1);
}

// --- get_by_project_and_bucket ---

#[tokio::test]
async fn test_get_by_project_and_bucket_filters_correctly() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    repo.create(make_entry("arch-1", "proj-1")).await.unwrap();

    let now = Utc::now();
    let impl_entry = MemoryEntry {
        id: MemoryEntryId::from("impl-1".to_string()),
        project_id: ProjectId::from_string("proj-1".to_string()),
        bucket: MemoryBucket::ImplementationDiscoveries,
        title: "Impl".to_string(),
        summary: "Summary".to_string(),
        details_markdown: "Details".to_string(),
        scope_paths: vec![],
        source_context_type: None,
        source_context_id: None,
        source_conversation_id: None,
        source_rule_file: None,
        quality_score: None,
        status: MemoryStatus::Active,
        content_hash: "impl-hash".to_string(),
        created_at: now,
        updated_at: now,
    };
    repo.create(impl_entry).await.unwrap();

    let arch_results = repo
        .get_by_project_and_bucket(
            &ProjectId::from_string("proj-1".to_string()),
            MemoryBucket::ArchitecturePatterns,
        )
        .await
        .unwrap();
    assert_eq!(arch_results.len(), 1);
    assert!(matches!(
        arch_results[0].bucket,
        MemoryBucket::ArchitecturePatterns
    ));
}

// --- get_by_rule_file ---

#[tokio::test]
async fn test_get_by_rule_file_returns_matching() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let now = Utc::now();
    let entry_with_rule = MemoryEntry {
        id: MemoryEntryId::from("rule-entry".to_string()),
        project_id: ProjectId::from_string("proj-1".to_string()),
        bucket: MemoryBucket::ArchitecturePatterns,
        title: "Rule Entry".to_string(),
        summary: "Summary".to_string(),
        details_markdown: "Details".to_string(),
        scope_paths: vec![],
        source_context_type: None,
        source_context_id: None,
        source_conversation_id: None,
        source_rule_file: Some("rules/arch.md".to_string()),
        quality_score: None,
        status: MemoryStatus::Active,
        content_hash: "rule-hash".to_string(),
        created_at: now,
        updated_at: now,
    };
    repo.create(entry_with_rule).await.unwrap();
    repo.create(make_entry("no-rule", "proj-1")).await.unwrap();

    let results = repo
        .get_by_rule_file(
            &ProjectId::from_string("proj-1".to_string()),
            "rules/arch.md",
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].source_rule_file, Some("rules/arch.md".to_string()));
}

// --- get_by_content_hash (global) ---

#[tokio::test]
async fn test_get_by_content_hash_global_finds_across_projects() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let mut e1 = make_entry("e1", "proj-1");
    e1.content_hash = "shared-hash".to_string();
    let mut e2 = make_entry("e2", "proj-2");
    e2.content_hash = "shared-hash".to_string();
    repo.create(e1).await.unwrap();
    repo.create(e2).await.unwrap();

    let results = repo.get_by_content_hash("shared-hash").await.unwrap();
    assert_eq!(results.len(), 2);
}

#[tokio::test]
async fn test_get_by_content_hash_global_empty() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let results = repo.get_by_content_hash("no-such-hash").await.unwrap();
    assert!(results.is_empty());
}

// --- update_status ---

#[tokio::test]
async fn test_update_status_changes_status() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let entry = make_entry("entry-status", "proj-1");
    repo.create(entry.clone()).await.unwrap();

    repo.update_status(&entry.id, MemoryStatus::Obsolete)
        .await
        .unwrap();

    let loaded = repo.get_by_id(&entry.id).await.unwrap().unwrap();
    assert!(matches!(loaded.status, MemoryStatus::Obsolete));
}

#[tokio::test]
async fn test_update_status_not_found_returns_error() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let missing_id = MemoryEntryId::from("ghost-id".to_string());
    let result = repo.update_status(&missing_id, MemoryStatus::Archived).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, crate::error::AppError::NotFound(_)));
}

// --- update (12-column) ---

#[tokio::test]
async fn test_update_modifies_all_fields() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let entry = make_entry("entry-upd", "proj-1");
    repo.create(entry.clone()).await.unwrap();

    let now = Utc::now();
    let updated = MemoryEntry {
        id: entry.id.clone(),
        project_id: entry.project_id.clone(),
        bucket: MemoryBucket::OperationalPlaybooks,
        title: "Updated Title".to_string(),
        summary: "Updated summary".to_string(),
        details_markdown: "Updated details".to_string(),
        scope_paths: vec!["tests/**".to_string(), "docs/**".to_string()],
        source_context_type: Some("review".to_string()),
        source_context_id: Some("review-42".to_string()),
        source_conversation_id: Some("conv-99".to_string()),
        source_rule_file: Some("rules/ops.md".to_string()),
        quality_score: Some(0.75),
        status: MemoryStatus::Archived,
        content_hash: "new-hash-xyz".to_string(),
        created_at: now,
        updated_at: now,
    };

    repo.update(&updated).await.unwrap();

    let loaded = repo.get_by_id(&entry.id).await.unwrap().unwrap();
    assert!(matches!(loaded.bucket, MemoryBucket::OperationalPlaybooks));
    assert_eq!(loaded.title, "Updated Title");
    assert_eq!(loaded.summary, "Updated summary");
    assert_eq!(loaded.details_markdown, "Updated details");
    assert_eq!(loaded.scope_paths, vec!["tests/**", "docs/**"]);
    assert_eq!(loaded.source_context_type, Some("review".to_string()));
    assert_eq!(loaded.source_context_id, Some("review-42".to_string()));
    assert_eq!(loaded.source_conversation_id, Some("conv-99".to_string()));
    assert_eq!(loaded.source_rule_file, Some("rules/ops.md".to_string()));
    assert!((loaded.quality_score.unwrap() - 0.75).abs() < 1e-9);
    assert!(matches!(loaded.status, MemoryStatus::Archived));
    assert_eq!(loaded.content_hash, "new-hash-xyz");
}

#[tokio::test]
async fn test_update_not_found_returns_error() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let entry = make_entry("ghost-upd", "proj-1");
    let result = repo.update(&entry).await;

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        crate::error::AppError::NotFound(_)
    ));
}

// --- delete ---

#[tokio::test]
async fn test_delete_removes_entry() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let entry = make_entry("entry-del", "proj-1");
    repo.create(entry.clone()).await.unwrap();

    repo.delete(&entry.id).await.unwrap();

    let found = repo.get_by_id(&entry.id).await.unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn test_delete_not_found_returns_error() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let missing_id = MemoryEntryId::from("ghost-del".to_string());
    let result = repo.delete(&missing_id).await;

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        crate::error::AppError::NotFound(_)
    ));
}

// --- get_by_paths (in-memory glob matching) ---

#[tokio::test]
async fn test_get_by_paths_matches_prefix() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    // scope_paths = ["src/**"] → prefix is "src/"
    let entry = make_entry("path-entry", "proj-1");
    repo.create(entry).await.unwrap();

    // Path "src/utils/foo.rs" starts with "src/" → should match
    let results = repo
        .get_by_paths(
            &ProjectId::from_string("proj-1".to_string()),
            &["src/utils/foo.rs".to_string()],
        )
        .await
        .unwrap();

    assert_eq!(results.len(), 1);
}

#[tokio::test]
async fn test_get_by_paths_no_match_returns_empty() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    // scope_paths = ["src/**"] → prefix "src/"
    let entry = make_entry("path-entry-2", "proj-1");
    repo.create(entry).await.unwrap();

    // "vendor/something.rs" does not start with "src/"
    let results = repo
        .get_by_paths(
            &ProjectId::from_string("proj-1".to_string()),
            &["vendor/something.rs".to_string()],
        )
        .await
        .unwrap();

    assert!(results.is_empty());
}

#[tokio::test]
async fn test_get_by_paths_excludes_inactive() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let entry = make_entry("inactive-path", "proj-1");
    repo.create(entry.clone()).await.unwrap();
    repo.update_status(&entry.id, MemoryStatus::Obsolete)
        .await
        .unwrap();

    let results = repo
        .get_by_paths(
            &ProjectId::from_string("proj-1".to_string()),
            &["src/utils/foo.rs".to_string()],
        )
        .await
        .unwrap();

    // Inactive entries are excluded (WHERE status = 'active')
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_get_by_paths_empty_paths_returns_empty() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let entry = make_entry("path-entry-3", "proj-1");
    repo.create(entry).await.unwrap();

    let results = repo
        .get_by_paths(
            &ProjectId::from_string("proj-1".to_string()),
            &[],
        )
        .await
        .unwrap();

    assert!(results.is_empty());
}

#[tokio::test]
async fn test_get_by_paths_nested_glob() {
    let db = setup_test_db();
    let repo = SqliteMemoryEntryRepository::from_shared(db.shared_conn());

    let now = Utc::now();
    let entry = MemoryEntry {
        id: MemoryEntryId::from("nested-glob".to_string()),
        project_id: ProjectId::from_string("proj-1".to_string()),
        bucket: MemoryBucket::ArchitecturePatterns,
        title: "Nested".to_string(),
        summary: "Summary".to_string(),
        details_markdown: "Details".to_string(),
        scope_paths: vec!["src/components/**".to_string()],
        source_context_type: None,
        source_context_id: None,
        source_conversation_id: None,
        source_rule_file: None,
        quality_score: None,
        status: MemoryStatus::Active,
        content_hash: "nested-hash".to_string(),
        created_at: now,
        updated_at: now,
    };
    repo.create(entry).await.unwrap();

    // "src/components/Button.tsx" starts with "src/components/" → match
    let matches = repo
        .get_by_paths(
            &ProjectId::from_string("proj-1".to_string()),
            &["src/components/Button.tsx".to_string()],
        )
        .await
        .unwrap();
    assert_eq!(matches.len(), 1);

    // "src/utils/helper.rs" does NOT start with "src/components/" → no match
    let no_match = repo
        .get_by_paths(
            &ProjectId::from_string("proj-1".to_string()),
            &["src/utils/helper.rs".to_string()],
        )
        .await
        .unwrap();
    assert!(no_match.is_empty());
}

// --- from_shared_connection ---

#[tokio::test]
async fn test_from_shared_connection() {
    let db = setup_test_db();
    let shared = db.shared_conn();

    let repo1 = SqliteMemoryEntryRepository::from_shared(Arc::clone(&shared));
    let repo2 = SqliteMemoryEntryRepository::from_shared(Arc::clone(&shared));

    // Create via repo1, read via repo2
    let entry = make_entry("shared-entry", "proj-shared");
    repo1.create(entry.clone()).await.unwrap();

    let found = repo2.get_by_id(&entry.id).await.unwrap();
    assert!(found.is_some());
}

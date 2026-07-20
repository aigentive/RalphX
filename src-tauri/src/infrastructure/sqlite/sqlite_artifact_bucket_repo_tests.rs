use super::*;
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite-artifact-bucket-repo")
}

fn create_test_bucket() -> ArtifactBucket {
    ArtifactBucket::new("Test Bucket")
        .accepts(ArtifactType::Prd)
        .accepts(ArtifactType::DesignDoc)
        .with_writer("user")
        .with_writer("orchestrator")
}

fn create_system_bucket() -> ArtifactBucket {
    ArtifactBucket::system("test-system-bucket", "Test System Bucket")
        .accepts(ArtifactType::ResearchDocument)
        .with_writer("researcher")
}

// ==================== CREATE TESTS ====================

#[tokio::test]
async fn test_create_bucket() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());
    let bucket = create_test_bucket();

    let result = repo.create(bucket.clone()).await;
    assert!(result.is_ok());

    let created = result.unwrap();
    assert_eq!(created.id, bucket.id);
    assert_eq!(created.name, "Test Bucket");
}

#[tokio::test]
async fn test_create_system_bucket() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());
    let bucket = create_system_bucket();

    let result = repo.create(bucket.clone()).await;
    assert!(result.is_ok());

    let created = result.unwrap();
    assert!(created.is_system);
}

// ==================== GET BY ID TESTS ====================

#[tokio::test]
async fn test_get_by_id_found() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());
    let bucket = create_test_bucket();

    repo.create(bucket.clone()).await.unwrap();

    let result = repo.get_by_id(&bucket.id).await;
    assert!(result.is_ok());

    let found = result.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "Test Bucket");
}

#[tokio::test]
async fn test_get_by_id_not_found() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());
    let id = ArtifactBucketId::new();

    let result = repo.get_by_id(&id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_get_by_id_preserves_accepted_types() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());
    let bucket = create_test_bucket();

    repo.create(bucket.clone()).await.unwrap();

    let loaded = repo.get_by_id(&bucket.id).await.unwrap().unwrap();
    assert!(loaded.accepts_type(ArtifactType::Prd));
    assert!(loaded.accepts_type(ArtifactType::DesignDoc));
    assert!(!loaded.accepts_type(ArtifactType::CodeChange));
}

#[tokio::test]
async fn test_get_by_id_preserves_writers() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());
    let bucket = create_test_bucket();

    repo.create(bucket.clone()).await.unwrap();

    let loaded = repo.get_by_id(&bucket.id).await.unwrap().unwrap();
    assert!(loaded.can_write("user"));
    assert!(loaded.can_write("orchestrator"));
    assert!(!loaded.can_write("worker"));
}

// ==================== GET ALL TESTS ====================

/// Clears seeded buckets so tests can start from empty state
fn clear_seeded_buckets(db: &SqliteTestDb) {
    db.with_connection(|conn| {
        conn.execute("DELETE FROM artifact_buckets", []).unwrap();
    });
}

#[tokio::test]
async fn test_get_all_empty() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());

    // v25 seeds 4 system buckets; v37 and persona history add one each.
    let result = repo.get_all().await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 6);
}

#[tokio::test]
async fn test_get_all_with_buckets() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());

    let bucket1 = create_test_bucket();
    let bucket2 = create_system_bucket();

    repo.create(bucket1).await.unwrap();
    repo.create(bucket2).await.unwrap();

    let result = repo.get_all().await;
    assert!(result.is_ok());
    // 6 seeded + 2 created
    assert_eq!(result.unwrap().len(), 8);
}

#[tokio::test]
async fn test_get_all_returns_sorted_by_name() {
    let db = setup_test_db();
    clear_seeded_buckets(&db);
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());

    let mut bucket1 = create_test_bucket();
    bucket1.name = "Zebra Bucket".to_string();

    let mut bucket2 = create_test_bucket();
    bucket2.id = ArtifactBucketId::new();
    bucket2.name = "Alpha Bucket".to_string();

    repo.create(bucket1).await.unwrap();
    repo.create(bucket2).await.unwrap();

    let result = repo.get_all().await.unwrap();
    // Should be sorted: Alpha Bucket, Code Changes, PRD Library, Research Outputs, Work Context, Zebra Bucket
    assert_eq!(result[0].name, "Alpha Bucket");
    assert_eq!(result.last().unwrap().name, "Zebra Bucket");
}

// ==================== GET SYSTEM BUCKETS TESTS ====================

#[tokio::test]
async fn test_get_system_buckets_empty() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());

    // Create only non-system bucket
    let bucket = create_test_bucket();
    repo.create(bucket).await.unwrap();

    // v25 seeds 4 system buckets; v37 and persona history add one each.
    let result = repo.get_system_buckets().await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 6);
}

#[tokio::test]
async fn test_get_system_buckets_returns_only_system() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());

    let custom = create_test_bucket();
    let system = create_system_bucket();

    repo.create(custom).await.unwrap();
    repo.create(system.clone()).await.unwrap();

    let result = repo.get_system_buckets().await;
    assert!(result.is_ok());

    let buckets = result.unwrap();
    // 6 seeded + 1 created
    assert_eq!(buckets.len(), 7);
    assert!(buckets.iter().all(|b| b.is_system));
    assert!(buckets.iter().any(|b| b.id == system.id));
}

// ==================== UPDATE TESTS ====================

#[tokio::test]
async fn test_update_bucket() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());

    let mut bucket = create_test_bucket();
    repo.create(bucket.clone()).await.unwrap();

    bucket.name = "Updated Name".to_string();
    bucket.accepted_types.push(ArtifactType::CodeChange);

    let result = repo.update(&bucket).await;
    assert!(result.is_ok());

    let updated = repo.get_by_id(&bucket.id).await.unwrap().unwrap();
    assert_eq!(updated.name, "Updated Name");
    assert!(updated.accepts_type(ArtifactType::CodeChange));
}

// ==================== DELETE TESTS ====================

#[tokio::test]
async fn test_delete_custom_bucket() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());

    let bucket = create_test_bucket();
    repo.create(bucket.clone()).await.unwrap();

    let result = repo.delete(&bucket.id).await;
    assert!(result.is_ok());

    let found = repo.get_by_id(&bucket.id).await.unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn test_delete_system_bucket_fails() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());

    let bucket = create_system_bucket();
    repo.create(bucket.clone()).await.unwrap();

    let result = repo.delete(&bucket.id).await;
    assert!(result.is_err());

    // Bucket should still exist
    let found = repo.get_by_id(&bucket.id).await.unwrap();
    assert!(found.is_some());
}

// ==================== EXISTS TESTS ====================

#[tokio::test]
async fn test_exists_true() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());

    let bucket = create_test_bucket();
    repo.create(bucket.clone()).await.unwrap();

    let result = repo.exists(&bucket.id).await;
    assert!(result.is_ok());
    assert!(result.unwrap());
}

#[tokio::test]
async fn test_exists_false() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());
    let id = ArtifactBucketId::new();

    let result = repo.exists(&id).await;
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

// ==================== SHARED CONNECTION TESTS ====================

#[tokio::test]
async fn test_from_shared_connection() {
    let db = setup_test_db();
    let shared = db.shared_conn();

    let repo1 = SqliteArtifactBucketRepository::from_shared(shared.clone());
    let repo2 = SqliteArtifactBucketRepository::from_shared(shared.clone());

    // Create via repo1
    let bucket = create_test_bucket();
    repo1.create(bucket.clone()).await.unwrap();

    // Read via repo2
    let found = repo2.get_by_id(&bucket.id).await.unwrap();
    assert!(found.is_some());
}

// ==================== SEEDING TESTS ====================

#[tokio::test]
async fn test_seed_builtin_buckets_creates_all_six() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());

    // v25+v37 migration already seeded them, so seed_builtin_buckets returns 0
    let count = repo.seed_builtin_buckets().await.unwrap();
    assert_eq!(count, 0);

    let all = repo.get_all().await.unwrap();
    assert_eq!(all.len(), 6);

    let system = repo.get_system_buckets().await.unwrap();
    assert_eq!(system.len(), 6);
}

#[tokio::test]
async fn test_seed_builtin_buckets_is_idempotent() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());

    // v25 already seeded, so both calls return 0
    let count1 = repo.seed_builtin_buckets().await.unwrap();
    let count2 = repo.seed_builtin_buckets().await.unwrap();

    assert_eq!(count1, 0);
    assert_eq!(count2, 0);

    // Still only the 6 seeded buckets
    let all = repo.get_all().await.unwrap();
    assert_eq!(all.len(), 6);
}

#[tokio::test]
async fn test_seed_builtin_buckets_creates_research_outputs() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());

    // Seeded by v25 migration
    let research_id = ArtifactBucketId::from_string("research-outputs");
    let bucket = repo.get_by_id(&research_id).await.unwrap();
    assert!(bucket.is_some());

    let bucket = bucket.unwrap();
    assert_eq!(bucket.name, "Research Outputs");
    assert!(bucket.is_system);
    assert!(bucket.accepts_type(ArtifactType::ResearchDocument));
    assert!(bucket.accepts_type(ArtifactType::Findings));
    assert!(bucket.can_write("deep-researcher"));
}

#[tokio::test]
async fn test_seed_builtin_buckets_creates_work_context() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());

    // Seeded by v25 migration
    let work_id = ArtifactBucketId::from_string("work-context");
    let bucket = repo.get_by_id(&work_id).await.unwrap();
    assert!(bucket.is_some());

    let bucket = bucket.unwrap();
    assert_eq!(bucket.name, "Work Context");
    assert!(bucket.is_system);
    assert!(bucket.accepts_type(ArtifactType::Context));
    assert!(bucket.accepts_type(ArtifactType::TaskSpec));
    assert!(bucket.can_write("orchestrator"));
}

#[tokio::test]
async fn test_seed_builtin_buckets_creates_code_changes() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());

    // Seeded by v25 migration
    let code_id = ArtifactBucketId::from_string("code-changes");
    let bucket = repo.get_by_id(&code_id).await.unwrap();
    assert!(bucket.is_some());

    let bucket = bucket.unwrap();
    assert_eq!(bucket.name, "Code Changes");
    assert!(bucket.is_system);
    assert!(bucket.accepts_type(ArtifactType::CodeChange));
    assert!(bucket.accepts_type(ArtifactType::Diff));
    assert!(bucket.can_write("worker"));
}

#[tokio::test]
async fn test_seed_builtin_buckets_creates_prd_library() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());

    // Seeded by v25 migration
    let prd_id = ArtifactBucketId::from_string("prd-library");
    let bucket = repo.get_by_id(&prd_id).await.unwrap();
    assert!(bucket.is_some());

    let bucket = bucket.unwrap();
    assert_eq!(bucket.name, "PRD Library");
    assert!(bucket.is_system);
    assert!(bucket.accepts_type(ArtifactType::Prd));
    assert!(bucket.accepts_type(ArtifactType::Specification));
    assert!(bucket.can_write("orchestrator"));
    assert!(bucket.can_write("user"));
}

#[tokio::test]
async fn test_seed_builtin_buckets_creates_persona_library() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());

    let bucket = repo
        .get_by_id(&ArtifactBucketId::from_string("persona-library"))
        .await
        .unwrap()
        .expect("persona library should be seeded");

    assert!(bucket.is_system);
    assert!(bucket.accepts_type(ArtifactType::Persona));
    assert!(bucket.can_write("agent"));
    assert!(bucket.can_write("user"));
    assert!(bucket.can_write("system"));
}

#[tokio::test]
async fn test_seed_builtin_buckets_preserves_existing() {
    let db = setup_test_db();
    let repo = SqliteArtifactBucketRepository::new(db.new_connection());

    // Create a custom bucket first
    let custom = create_test_bucket();
    repo.create(custom).await.unwrap();

    // Seed built-ins (already seeded by v25, so no new ones)
    repo.seed_builtin_buckets().await.unwrap();

    // Should have 7 buckets total (1 custom + 6 seeded system buckets)
    let all = repo.get_all().await.unwrap();
    assert_eq!(all.len(), 7);
}

use super::*;
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite-artifact-repo")
}

fn setup_repo() -> (SqliteTestDb, SqliteArtifactRepository) {
    let db = setup_test_db();
    let repo = SqliteArtifactRepository::new(db.new_connection());
    (db, repo)
}

fn seed_task(db: &SqliteTestDb, task_id: &TaskId) {
    let project = db.seed_project("Test Project");
    let mut task = crate::domain::entities::Task::new(project.id, "Test Task".to_string());
    task.id = task_id.clone();
    db.insert_task(task);
}

fn create_test_artifact() -> Artifact {
    Artifact::new_inline("Test PRD", ArtifactType::Prd, "PRD content here", "user")
}

fn create_file_artifact() -> Artifact {
    Artifact::new_file(
        "Design Doc",
        ArtifactType::DesignDoc,
        "/docs/design.md",
        "architect",
    )
}

// ==================== CREATE TESTS ====================

#[tokio::test]
async fn test_create_artifact_inline() {
    let (_db, repo) = setup_repo();
    let artifact = create_test_artifact();

    let result = repo.create(artifact.clone()).await;
    assert!(result.is_ok());

    let created = result.unwrap();
    assert_eq!(created.id, artifact.id);
    assert_eq!(created.name, "Test PRD");
}

#[tokio::test]
async fn test_create_artifact_file() {
    let (_db, repo) = setup_repo();
    let artifact = create_file_artifact();

    let result = repo.create(artifact.clone()).await;
    assert!(result.is_ok());

    let created = result.unwrap();
    assert!(created.content.is_file());
}

#[tokio::test]
async fn test_create_artifact_with_bucket() {
    let (_db, repo) = setup_repo();

    // prd-library bucket is seeded by v25 migration
    let artifact = create_test_artifact().with_bucket(ArtifactBucketId::from_string("prd-library"));

    let result = repo.create(artifact.clone()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_create_artifact_with_task() {
    let (db, repo) = setup_repo();

    let task_id = TaskId::from_string("task-123".to_string());

    seed_task(&db, &task_id);

    let artifact = create_test_artifact().with_task(task_id.clone());

    let result = repo.create(artifact).await;
    assert!(result.is_ok());
}

// ==================== GET BY ID TESTS ====================

#[tokio::test]
async fn test_get_by_id_found() {
    let (_db, repo) = setup_repo();
    let artifact = create_test_artifact();

    repo.create(artifact.clone()).await.unwrap();

    let result = repo.get_by_id(&artifact.id).await;
    assert!(result.is_ok());

    let found = result.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "Test PRD");
}

#[tokio::test]
async fn test_get_by_id_not_found() {
    let (_db, repo) = setup_repo();
    let id = ArtifactId::new();

    let result = repo.get_by_id(&id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[tokio::test]
async fn test_get_by_id_preserves_content_inline() {
    let (_db, repo) = setup_repo();
    let artifact = create_test_artifact();

    repo.create(artifact.clone()).await.unwrap();

    let loaded = repo.get_by_id(&artifact.id).await.unwrap().unwrap();
    if let ArtifactContent::Inline { text } = &loaded.content {
        assert_eq!(text, "PRD content here");
    } else {
        panic!("Expected inline content");
    }
}

#[tokio::test]
async fn test_get_by_id_preserves_content_file() {
    let (_db, repo) = setup_repo();
    let artifact = create_file_artifact();

    repo.create(artifact.clone()).await.unwrap();

    let loaded = repo.get_by_id(&artifact.id).await.unwrap().unwrap();
    if let ArtifactContent::File { path } = &loaded.content {
        assert_eq!(path, "/docs/design.md");
    } else {
        panic!("Expected file content");
    }
}

// ==================== GET BY BUCKET TESTS ====================

#[tokio::test]
async fn test_get_by_bucket_empty() {
    let (_db, repo) = setup_repo();
    let bucket_id = ArtifactBucketId::from_string("nonexistent");

    let result = repo.get_by_bucket(&bucket_id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_by_bucket_returns_matching() {
    let (_db, repo) = setup_repo();

    // prd-library bucket is seeded by v25 migration
    let bucket_id = ArtifactBucketId::from_string("prd-library");

    // Create artifacts in bucket
    let a1 = create_test_artifact().with_bucket(bucket_id.clone());
    let mut a2 = create_test_artifact();
    a2.id = ArtifactId::new();
    let a2 = a2.with_bucket(bucket_id.clone());

    // Create artifact not in bucket
    let a3 = create_test_artifact();

    repo.create(a1).await.unwrap();
    repo.create(a2).await.unwrap();
    repo.create(a3).await.unwrap();

    let result = repo.get_by_bucket(&bucket_id).await.unwrap();
    assert_eq!(result.len(), 2);
}

// ==================== GET BY TYPE TESTS ====================

#[tokio::test]
async fn test_get_by_type_empty() {
    let (_db, repo) = setup_repo();

    let result = repo.get_by_type(ArtifactType::CodeChange).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_by_type_returns_matching() {
    let (_db, repo) = setup_repo();

    // Create PRD artifacts
    let a1 = create_test_artifact();
    let mut a2 = create_test_artifact();
    a2.id = ArtifactId::new();

    // Create design doc artifact
    let a3 = create_file_artifact();

    repo.create(a1).await.unwrap();
    repo.create(a2).await.unwrap();
    repo.create(a3).await.unwrap();

    let prds = repo.get_by_type(ArtifactType::Prd).await.unwrap();
    assert_eq!(prds.len(), 2);

    let docs = repo.get_by_type(ArtifactType::DesignDoc).await.unwrap();
    assert_eq!(docs.len(), 1);
}

// ==================== GET BY TASK TESTS ====================

#[tokio::test]
async fn test_get_by_task_empty() {
    let (_db, repo) = setup_repo();
    let task_id = TaskId::from_string("task-999".to_string());

    let result = repo.get_by_task(&task_id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_by_task_returns_matching() {
    let (db, repo) = setup_repo();

    let task_id = TaskId::from_string("task-123".to_string());

    seed_task(&db, &task_id);

    let a1 = create_test_artifact().with_task(task_id.clone());
    let mut a2 = create_test_artifact();
    a2.id = ArtifactId::new(); // Give it a different ID

    repo.create(a1).await.unwrap();
    repo.create(a2).await.unwrap();

    let result = repo.get_by_task(&task_id).await.unwrap();
    assert_eq!(result.len(), 1);
}

// ==================== GET BY PROCESS TESTS ====================

#[tokio::test]
async fn test_get_by_process_empty() {
    let (_db, repo) = setup_repo();
    let process_id = ProcessId::from_string("process-999");

    let result = repo.get_by_process(&process_id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_by_process_returns_matching() {
    let (_db, repo) = setup_repo();

    let process_id = ProcessId::from_string("research-1");

    let a1 = create_test_artifact().with_process(process_id.clone());
    let a2 = create_test_artifact(); // No process

    repo.create(a1).await.unwrap();
    repo.create(a2).await.unwrap();

    let result = repo.get_by_process(&process_id).await.unwrap();
    assert_eq!(result.len(), 1);
}

// ==================== UPDATE TESTS ====================

#[tokio::test]
async fn test_update_artifact() {
    let (_db, repo) = setup_repo();

    let mut artifact = create_test_artifact();
    repo.create(artifact.clone()).await.unwrap();

    artifact.name = "Updated Name".to_string();
    artifact.content = ArtifactContent::inline("Updated content");

    let result = repo.update(&artifact).await;
    assert!(result.is_ok());

    let updated = repo.get_by_id(&artifact.id).await.unwrap().unwrap();
    assert_eq!(updated.name, "Updated Name");
    if let ArtifactContent::Inline { text } = &updated.content {
        assert_eq!(text, "Updated content");
    }
}

// ==================== DELETE TESTS ====================

#[tokio::test]
async fn test_delete_artifact() {
    let (_db, repo) = setup_repo();

    let artifact = create_test_artifact();
    repo.create(artifact.clone()).await.unwrap();

    let result = repo.delete(&artifact.id).await;
    assert!(result.is_ok());

    let found = repo.get_by_id(&artifact.id).await.unwrap();
    assert!(found.is_none());
}

// ==================== RELATION TESTS ====================

#[tokio::test]
async fn test_add_relation_derived_from() {
    let (_db, repo) = setup_repo();

    let parent = create_test_artifact();
    let mut child = create_test_artifact();
    child.id = ArtifactId::new();

    repo.create(parent.clone()).await.unwrap();
    repo.create(child.clone()).await.unwrap();

    let relation = ArtifactRelation::derived_from(child.id.clone(), parent.id.clone());
    let result = repo.add_relation(relation.clone()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_add_relation_related_to() {
    let (_db, repo) = setup_repo();

    let a1 = create_test_artifact();
    let mut a2 = create_test_artifact();
    a2.id = ArtifactId::new();

    repo.create(a1.clone()).await.unwrap();
    repo.create(a2.clone()).await.unwrap();

    let relation = ArtifactRelation::related_to(a1.id.clone(), a2.id.clone());
    let result = repo.add_relation(relation).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_get_derived_from() {
    let (_db, repo) = setup_repo();

    let parent1 = create_test_artifact();
    let mut parent2 = create_test_artifact();
    parent2.id = ArtifactId::new();
    let mut child = create_test_artifact();
    child.id = ArtifactId::new();

    repo.create(parent1.clone()).await.unwrap();
    repo.create(parent2.clone()).await.unwrap();
    repo.create(child.clone()).await.unwrap();

    // Child derived from both parents
    repo.add_relation(ArtifactRelation::derived_from(
        child.id.clone(),
        parent1.id.clone(),
    ))
    .await
    .unwrap();
    repo.add_relation(ArtifactRelation::derived_from(
        child.id.clone(),
        parent2.id.clone(),
    ))
    .await
    .unwrap();

    let parents = repo.get_derived_from(&child.id).await.unwrap();
    assert_eq!(parents.len(), 2);
}

#[tokio::test]
async fn test_get_related() {
    let (_db, repo) = setup_repo();

    let a1 = create_test_artifact();
    let mut a2 = create_test_artifact();
    a2.id = ArtifactId::new();
    let mut a3 = create_test_artifact();
    a3.id = ArtifactId::new();

    repo.create(a1.clone()).await.unwrap();
    repo.create(a2.clone()).await.unwrap();
    repo.create(a3.clone()).await.unwrap();

    // a1 related to a2 and a3
    repo.add_relation(ArtifactRelation::related_to(a1.id.clone(), a2.id.clone()))
        .await
        .unwrap();
    repo.add_relation(ArtifactRelation::related_to(a3.id.clone(), a1.id.clone()))
        .await
        .unwrap();

    let related = repo.get_related(&a1.id).await.unwrap();
    assert_eq!(related.len(), 2);
}

#[tokio::test]
async fn test_get_relations() {
    let (_db, repo) = setup_repo();

    let a1 = create_test_artifact();
    let mut a2 = create_test_artifact();
    a2.id = ArtifactId::new();

    repo.create(a1.clone()).await.unwrap();
    repo.create(a2.clone()).await.unwrap();

    repo.add_relation(ArtifactRelation::derived_from(a2.id.clone(), a1.id.clone()))
        .await
        .unwrap();

    let relations = repo.get_relations(&a1.id).await.unwrap();
    assert_eq!(relations.len(), 1);
}

#[tokio::test]
async fn test_get_relations_by_type() {
    let (_db, repo) = setup_repo();

    let a1 = create_test_artifact();
    let mut a2 = create_test_artifact();
    a2.id = ArtifactId::new();
    let mut a3 = create_test_artifact();
    a3.id = ArtifactId::new();

    repo.create(a1.clone()).await.unwrap();
    repo.create(a2.clone()).await.unwrap();
    repo.create(a3.clone()).await.unwrap();

    // Different relation types
    repo.add_relation(ArtifactRelation::derived_from(a2.id.clone(), a1.id.clone()))
        .await
        .unwrap();
    repo.add_relation(ArtifactRelation::related_to(a1.id.clone(), a3.id.clone()))
        .await
        .unwrap();

    let derived = repo
        .get_relations_by_type(&a1.id, ArtifactRelationType::DerivedFrom)
        .await
        .unwrap();
    assert_eq!(derived.len(), 1);

    let related = repo
        .get_relations_by_type(&a1.id, ArtifactRelationType::RelatedTo)
        .await
        .unwrap();
    assert_eq!(related.len(), 1);
}

#[tokio::test]
async fn test_delete_relation() {
    let (_db, repo) = setup_repo();

    let a1 = create_test_artifact();
    let mut a2 = create_test_artifact();
    a2.id = ArtifactId::new();

    repo.create(a1.clone()).await.unwrap();
    repo.create(a2.clone()).await.unwrap();

    repo.add_relation(ArtifactRelation::related_to(a1.id.clone(), a2.id.clone()))
        .await
        .unwrap();

    // Verify relation exists
    let relations = repo.get_relations(&a1.id).await.unwrap();
    assert_eq!(relations.len(), 1);

    // Delete relation
    repo.delete_relation(&a1.id, &a2.id).await.unwrap();

    // Verify relation deleted
    let relations = repo.get_relations(&a1.id).await.unwrap();
    assert!(relations.is_empty());
}

// ==================== VERSION CHAIN TRAVERSAL TESTS ====================

#[tokio::test]
async fn test_get_at_version_traverses_chain_v1_via_v2() {
    let (_db, repo) = setup_repo();

    // Create V1
    let v1 = Artifact::new_inline(
        "Plan",
        ArtifactType::Specification,
        "V1 content",
        "orchestrator",
    );
    let v1_id = v1.id.clone();
    repo.create(v1).await.unwrap();

    // Create V2 chained to V1
    let mut v2 = Artifact::new_inline(
        "Plan",
        ArtifactType::Specification,
        "V2 content",
        "orchestrator",
    );
    v2.metadata.version = 2;
    let v2_id = v2.id.clone();
    repo.create_with_previous_version(v2, v1_id.clone())
        .await
        .unwrap();

    // Fetch V1 using V2's ID — should traverse chain and return V1
    let result = repo.get_by_id_at_version(&v2_id, 1).await.unwrap();
    assert!(result.is_some(), "Should find V1 via V2's chain");
    let artifact = result.unwrap();
    assert_eq!(artifact.metadata.version, 1);
    assert_eq!(artifact.id, v1_id);
    if let ArtifactContent::Inline { text } = &artifact.content {
        assert_eq!(text, "V1 content");
    } else {
        panic!("Expected inline content");
    }
}

#[tokio::test]
async fn test_get_at_version_returns_current_without_traversal() {
    let (_db, repo) = setup_repo();

    // Create V1
    let v1 = Artifact::new_inline(
        "Plan",
        ArtifactType::Specification,
        "V1 content",
        "orchestrator",
    );
    let v1_id = v1.id.clone();
    repo.create(v1).await.unwrap();

    // Create V2 chained to V1
    let mut v2 = Artifact::new_inline(
        "Plan",
        ArtifactType::Specification,
        "V2 content",
        "orchestrator",
    );
    v2.metadata.version = 2;
    let v2_id = v2.id.clone();
    repo.create_with_previous_version(v2, v1_id).await.unwrap();

    // Fetch V2 using V2's ID — should return directly (no traversal)
    let result = repo.get_by_id_at_version(&v2_id, 2).await.unwrap();
    assert!(result.is_some(), "Should find V2 directly");
    let artifact = result.unwrap();
    assert_eq!(artifact.metadata.version, 2);
    if let ArtifactContent::Inline { text } = &artifact.content {
        assert_eq!(text, "V2 content");
    } else {
        panic!("Expected inline content");
    }
}

#[tokio::test]
async fn test_get_at_version_three_level_chain() {
    let (_db, repo) = setup_repo();

    // Create V1 → V2 → V3 chain
    let v1 = Artifact::new_inline("Plan", ArtifactType::Specification, "V1", "orchestrator");
    let v1_id = v1.id.clone();
    repo.create(v1).await.unwrap();

    let mut v2 = Artifact::new_inline("Plan", ArtifactType::Specification, "V2", "orchestrator");
    v2.metadata.version = 2;
    let v2_id = v2.id.clone();
    repo.create_with_previous_version(v2, v1_id).await.unwrap();

    let mut v3 = Artifact::new_inline("Plan", ArtifactType::Specification, "V3", "orchestrator");
    v3.metadata.version = 3;
    let v3_id = v3.id.clone();
    repo.create_with_previous_version(v3, v2_id).await.unwrap();

    // Fetch V1 using V3's ID — should traverse V3 → V2 → V1
    let result = repo.get_by_id_at_version(&v3_id, 1).await.unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().metadata.version, 1);

    // Fetch V2 using V3's ID
    let result = repo.get_by_id_at_version(&v3_id, 2).await.unwrap();
    assert!(result.is_some());
    assert_eq!(result.unwrap().metadata.version, 2);
}

#[tokio::test]
async fn test_get_at_version_nonexistent_version() {
    let (_db, repo) = setup_repo();

    let v1 = Artifact::new_inline("Plan", ArtifactType::Specification, "V1", "orchestrator");
    let v1_id = v1.id.clone();
    repo.create(v1).await.unwrap();

    // Ask for version 5 — doesn't exist
    let result = repo.get_by_id_at_version(&v1_id, 5).await.unwrap();
    assert!(
        result.is_none(),
        "Should return None for nonexistent version"
    );
}

// ==================== SHARED CONNECTION TESTS ====================

#[tokio::test]
async fn test_from_shared_connection() {
    let db = setup_test_db();
    let shared = db.shared_conn();

    let repo1 = SqliteArtifactRepository::from_shared(shared.clone());
    let repo2 = SqliteArtifactRepository::from_shared(shared.clone());

    // Create via repo1
    let artifact = create_test_artifact();
    repo1.create(artifact.clone()).await.unwrap();

    // Read via repo2
    let found = repo2.get_by_id(&artifact.id).await.unwrap();
    assert!(found.is_some());
}

// ==================== RESOLVE LATEST ARTIFACT ID TESTS ====================

#[tokio::test]
async fn test_resolve_latest_single_version_returns_itself() {
    let (_db, repo) = setup_repo();

    let v1 = Artifact::new_inline("Plan", ArtifactType::Specification, "V1", "orchestrator");
    let v1_id = v1.id.clone();
    repo.create(v1).await.unwrap();

    let resolved = repo.resolve_latest_artifact_id(&v1_id).await.unwrap();
    assert_eq!(resolved, v1_id, "Single version should resolve to itself");
}

#[tokio::test]
async fn test_resolve_latest_three_version_chain_from_v1() {
    let (_db, repo) = setup_repo();

    // Create V1 → V2 → V3 chain
    let v1 = Artifact::new_inline("Plan", ArtifactType::Specification, "V1", "orchestrator");
    let v1_id = v1.id.clone();
    repo.create(v1).await.unwrap();

    let mut v2 = Artifact::new_inline("Plan", ArtifactType::Specification, "V2", "orchestrator");
    v2.metadata.version = 2;
    let v2_id = v2.id.clone();
    repo.create_with_previous_version(v2, v1_id.clone())
        .await
        .unwrap();

    let mut v3 = Artifact::new_inline("Plan", ArtifactType::Specification, "V3", "orchestrator");
    v3.metadata.version = 3;
    let v3_id = v3.id.clone();
    repo.create_with_previous_version(v3, v2_id).await.unwrap();

    let resolved = repo.resolve_latest_artifact_id(&v1_id).await.unwrap();
    assert_eq!(resolved, v3_id, "V1 should resolve to V3 (latest)");
}

#[tokio::test]
async fn test_resolve_latest_three_version_chain_from_middle() {
    let (_db, repo) = setup_repo();

    // Create V1 → V2 → V3 chain
    let v1 = Artifact::new_inline("Plan", ArtifactType::Specification, "V1", "orchestrator");
    let v1_id = v1.id.clone();
    repo.create(v1).await.unwrap();

    let mut v2 = Artifact::new_inline("Plan", ArtifactType::Specification, "V2", "orchestrator");
    v2.metadata.version = 2;
    let v2_id = v2.id.clone();
    repo.create_with_previous_version(v2, v1_id).await.unwrap();

    let mut v3 = Artifact::new_inline("Plan", ArtifactType::Specification, "V3", "orchestrator");
    v3.metadata.version = 3;
    let v3_id = v3.id.clone();
    repo.create_with_previous_version(v3, v2_id.clone())
        .await
        .unwrap();

    let resolved = repo.resolve_latest_artifact_id(&v2_id).await.unwrap();
    assert_eq!(resolved, v3_id, "V2 (middle) should resolve to V3 (latest)");
}

#[tokio::test]
async fn test_resolve_stale_id_for_link_proposals_scenario() {
    // Simulates the link_proposals_to_plan fix: agent passes stale V1 ID,
    // resolve_latest_artifact_id returns V3 ID, proposals get linked to V3.
    let (_db, repo) = setup_repo();

    // Create V1 → V2 → V3 chain
    let v1 = Artifact::new_inline(
        "Plan",
        ArtifactType::Specification,
        "V1 content",
        "orchestrator",
    );
    let v1_id = v1.id.clone();
    repo.create(v1).await.unwrap();

    let mut v2 = Artifact::new_inline(
        "Plan",
        ArtifactType::Specification,
        "V2 content",
        "orchestrator",
    );
    v2.metadata.version = 2;
    let v2_id = v2.id.clone();
    repo.create_with_previous_version(v2, v1_id.clone())
        .await
        .unwrap();

    let mut v3 = Artifact::new_inline(
        "Plan",
        ArtifactType::Specification,
        "V3 content",
        "orchestrator",
    );
    v3.metadata.version = 3;
    let v3_id = v3.id.clone();
    repo.create_with_previous_version(v3, v2_id).await.unwrap();

    // Agent holds stale V1 ID — resolve to latest
    let resolved_id = repo.resolve_latest_artifact_id(&v1_id).await.unwrap();
    assert_eq!(resolved_id, v3_id, "Stale V1 ID should resolve to V3");

    // Fetch resolved artifact — this is what proposals would be linked to
    let resolved_artifact = repo.get_by_id(&resolved_id).await.unwrap().unwrap();
    assert_eq!(
        resolved_artifact.metadata.version, 3,
        "Resolved artifact should be version 3"
    );
    assert_eq!(
        resolved_artifact.id, v3_id,
        "Resolved artifact ID should be V3"
    );
}

/// Integration test: simulates the update_plan_artifact flow twice using the original v1 ID.
/// First update: v1 → resolve to v1 (latest) → create v2
/// Second update: v1 → resolve to v2 (latest) → create v3
/// Validates that resolve_latest_artifact_id makes stale IDs work for repeated updates.
#[tokio::test]
async fn test_two_updates_with_original_id_both_succeed() {
    let (_db, repo) = setup_repo();

    // Create V1 (original artifact)
    let v1 = Artifact::new_inline(
        "Plan",
        ArtifactType::Specification,
        "V1 content",
        "orchestrator",
    );
    let v1_id = v1.id.clone();
    repo.create(v1).await.unwrap();

    // --- First update: agent passes v1_id ---
    let resolved1 = repo.resolve_latest_artifact_id(&v1_id).await.unwrap();
    assert_eq!(resolved1, v1_id, "First resolve should return v1 itself");

    let old1 = repo.get_by_id(&resolved1).await.unwrap().unwrap();
    let mut v2 = Artifact::new_inline(
        "Plan",
        ArtifactType::Specification,
        "V2 content",
        "orchestrator",
    );
    v2.metadata.version = old1.metadata.version + 1;
    let v2_id = v2.id.clone();
    repo.create_with_previous_version(v2, resolved1)
        .await
        .unwrap();

    // --- Second update: agent passes SAME v1_id (stale) ---
    let resolved2 = repo.resolve_latest_artifact_id(&v1_id).await.unwrap();
    assert_eq!(
        resolved2, v2_id,
        "Second resolve should walk v1 → v2 (latest)"
    );

    let old2 = repo.get_by_id(&resolved2).await.unwrap().unwrap();
    assert_eq!(old2.metadata.version, 2, "Resolved artifact should be v2");

    let mut v3 = Artifact::new_inline(
        "Plan",
        ArtifactType::Specification,
        "V3 content",
        "orchestrator",
    );
    v3.metadata.version = old2.metadata.version + 1;
    let v3_id = v3.id.clone();
    repo.create_with_previous_version(v3, resolved2)
        .await
        .unwrap();

    // Verify final state: v1 → v2 → v3 chain
    let final_resolved = repo.resolve_latest_artifact_id(&v1_id).await.unwrap();
    assert_eq!(
        final_resolved, v3_id,
        "After two updates, v1 should resolve to v3"
    );

    let v3_artifact = repo.get_by_id(&v3_id).await.unwrap().unwrap();
    assert_eq!(v3_artifact.metadata.version, 3);

    // Verify version history from v3
    let history = repo.get_version_history(&v3_id).await.unwrap();
    assert_eq!(history.len(), 3, "Should have 3 versions in history");
    assert_eq!(history[0].version, 3);
    assert_eq!(history[1].version, 2);
    assert_eq!(history[2].version, 1);
}

// ==================== TEAM METADATA PERSISTENCE TESTS ====================

#[tokio::test]
async fn test_create_artifact_with_team_metadata_persists() {
    let (_db, repo) = setup_repo();

    let mut artifact = create_test_artifact();
    artifact.metadata.team_metadata = Some(TeamArtifactMetadata {
        team_name: "ideation-team".to_string(),
        author_teammate: "researcher".to_string(),
        session_id: Some("session-123".to_string()),
        team_phase: Some("active".to_string()),
    });

    repo.create(artifact.clone()).await.unwrap();

    let loaded = repo.get_by_id(&artifact.id).await.unwrap().unwrap();
    let tm = loaded
        .metadata
        .team_metadata
        .expect("team_metadata should be persisted");
    assert_eq!(tm.team_name, "ideation-team");
    assert_eq!(tm.author_teammate, "researcher");
    assert_eq!(tm.session_id, Some("session-123".to_string()));
    assert_eq!(tm.team_phase, Some("active".to_string()));
}

#[tokio::test]
async fn test_artifact_without_team_metadata_loads_none() {
    let (_db, repo) = setup_repo();

    let artifact = create_test_artifact();
    repo.create(artifact.clone()).await.unwrap();

    let loaded = repo.get_by_id(&artifact.id).await.unwrap().unwrap();
    assert!(loaded.metadata.team_metadata.is_none());
}

#[tokio::test]
async fn test_update_artifact_preserves_team_metadata() {
    let (_db, repo) = setup_repo();

    let mut artifact = create_test_artifact();
    artifact.metadata.team_metadata = Some(TeamArtifactMetadata {
        team_name: "team-alpha".to_string(),
        author_teammate: "worker-1".to_string(),
        session_id: None,
        team_phase: None,
    });

    repo.create(artifact.clone()).await.unwrap();

    artifact.name = "Updated Name".to_string();
    repo.update(&artifact).await.unwrap();

    let loaded = repo.get_by_id(&artifact.id).await.unwrap().unwrap();
    assert_eq!(loaded.name, "Updated Name");
    let tm = loaded
        .metadata
        .team_metadata
        .expect("team_metadata should survive update");
    assert_eq!(tm.team_name, "team-alpha");
    assert_eq!(tm.author_teammate, "worker-1");
}

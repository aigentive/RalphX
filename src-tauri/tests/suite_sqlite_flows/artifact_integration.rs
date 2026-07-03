// Integration test: Artifact creation and bucket routing
//
// Tests end-to-end artifact operations:
// - Create artifact in research-outputs bucket
// - Copy artifact to prd-library bucket (create with same content in another bucket)
// - Create artifact relation (derived_from)
// - Query artifacts by bucket and type
//
// Both memory and SQLite repositories are tested to ensure consistent behavior.

use std::sync::Arc;

use ralphx_lib::application::AppState;
use ralphx_lib::domain::entities::{
    Artifact, ArtifactBucket, ArtifactBucketId, ArtifactRelation, ArtifactRelationType,
    ArtifactType,
};
use ralphx_lib::infrastructure::sqlite::{
    SqliteArtifactBucketRepository, SqliteArtifactRepository,
};
use ralphx_lib::testing::SqliteStateFixture;

// ============================================================================
// Test Setup Helpers
// ============================================================================

/// Helper to create AppState with memory repositories
fn create_memory_state() -> AppState {
    AppState::new_test()
}

/// Helper to create AppState with SQLite repositories (file-backed temp database)
fn create_sqlite_state() -> SqliteStateFixture {
    SqliteStateFixture::new("artifact-integration", |db, state| {
        db.with_connection(|conn| {
            conn.execute("DELETE FROM artifact_buckets", [])
                .expect("Failed to clear seeded artifact buckets");
        });

        let shared_conn = db.shared_conn();
        state.artifact_repo = Arc::new(SqliteArtifactRepository::from_shared(Arc::clone(
            &shared_conn,
        )));
        state.artifact_bucket_repo =
            Arc::new(SqliteArtifactBucketRepository::from_shared(shared_conn));
    })
}

/// Helper to create the research-outputs system bucket
fn create_research_outputs_bucket() -> ArtifactBucket {
    ArtifactBucket::system("research-outputs", "Research Outputs")
        .accepts(ArtifactType::ResearchDocument)
        .accepts(ArtifactType::Findings)
        .accepts(ArtifactType::Recommendations)
        .with_writer("deep-researcher")
        .with_writer("orchestrator")
}

/// Helper to create the prd-library system bucket
fn create_prd_library_bucket() -> ArtifactBucket {
    ArtifactBucket::system("prd-library", "PRD Library")
        .accepts(ArtifactType::Prd)
        .accepts(ArtifactType::Specification)
        .accepts(ArtifactType::DesignDoc)
        .accepts(ArtifactType::Recommendations)
        .with_writer("orchestrator")
        .with_writer("user")
}

/// Helper to ensure a bucket exists (idempotent -- skips if already seeded by migration)
async fn ensure_bucket(state: &AppState, bucket: ArtifactBucket) {
    if !state.artifact_bucket_repo.exists(&bucket.id).await.unwrap() {
        state.artifact_bucket_repo.create(bucket).await.unwrap();
    }
}

// ============================================================================
// Shared Test Logic (works with any repository implementation)
// ============================================================================

/// Test 1: Create artifact in research-outputs bucket
async fn test_create_artifact_in_research_outputs_bucket(state: &AppState) {
    // Ensure the research-outputs bucket exists
    let bucket = create_research_outputs_bucket();
    ensure_bucket(state, bucket.clone()).await;

    // Create a research findings artifact
    let artifact = Artifact::new_inline(
        "Authentication Analysis Findings",
        ArtifactType::Findings,
        "After analyzing the codebase, we found that OAuth2 is the best approach...",
        "deep-researcher",
    )
    .with_bucket(ArtifactBucketId::from_string("research-outputs"));

    let artifact_id = artifact.id.clone();

    // Create the artifact
    let created = state.artifact_repo.create(artifact).await.unwrap();

    // Verify artifact was created with correct attributes
    assert_eq!(created.name, "Authentication Analysis Findings");
    assert_eq!(created.artifact_type, ArtifactType::Findings);
    assert_eq!(created.metadata.created_by, "deep-researcher");
    assert!(created.content.is_inline());
    assert_eq!(
        created.bucket_id,
        Some(ArtifactBucketId::from_string("research-outputs"))
    );

    // Verify we can retrieve it
    let found = state.artifact_repo.get_by_id(&artifact_id).await.unwrap();
    assert!(found.is_some());
    let found = found.unwrap();
    assert_eq!(found.name, "Authentication Analysis Findings");
    assert_eq!(found.artifact_type, ArtifactType::Findings);

    // Verify we can find it by bucket
    let bucket_artifacts = state
        .artifact_repo
        .get_by_bucket(&ArtifactBucketId::from_string("research-outputs"))
        .await
        .unwrap();
    assert_eq!(bucket_artifacts.len(), 1);
    assert_eq!(bucket_artifacts[0].id, artifact_id);
}

/// Test 2: Copy artifact to prd-library bucket (simulate routing)
async fn test_copy_artifact_to_another_bucket(state: &AppState) {
    // Ensure both buckets exist
    let research_bucket = create_research_outputs_bucket();
    let prd_bucket = create_prd_library_bucket();
    ensure_bucket(state, research_bucket).await;
    ensure_bucket(state, prd_bucket).await;

    // Create original artifact in research-outputs
    let original = Artifact::new_inline(
        "Technology Recommendations",
        ArtifactType::Recommendations,
        "Based on our research, we recommend using Rust for the backend...",
        "deep-researcher",
    )
    .with_bucket(ArtifactBucketId::from_string("research-outputs"));

    let original_id = original.id.clone();
    state.artifact_repo.create(original).await.unwrap();

    // Verify original is in research-outputs
    let research_artifacts = state
        .artifact_repo
        .get_by_bucket(&ArtifactBucketId::from_string("research-outputs"))
        .await
        .unwrap();
    assert_eq!(research_artifacts.len(), 1);

    // "Copy" to prd-library by creating a new artifact with same content
    // (In a real system, ArtifactFlowService would handle this)
    let copied = Artifact::new_inline(
        "Technology Recommendations",
        ArtifactType::Recommendations,
        "Based on our research, we recommend using Rust for the backend...",
        "orchestrator", // Different creator since it's a copy action
    )
    .with_bucket(ArtifactBucketId::from_string("prd-library"));

    let copied_id = copied.id.clone();
    state.artifact_repo.create(copied).await.unwrap();

    // Create a relation to track that this was derived from the original
    // (The derived_from field on Artifact is for convenience, relations are the source of truth)
    let relation = ArtifactRelation::derived_from(copied_id.clone(), original_id.clone());
    state.artifact_repo.add_relation(relation).await.unwrap();

    // Verify copy is in prd-library
    let prd_artifacts = state
        .artifact_repo
        .get_by_bucket(&ArtifactBucketId::from_string("prd-library"))
        .await
        .unwrap();
    assert_eq!(prd_artifacts.len(), 1);
    assert_eq!(prd_artifacts[0].id, copied_id);

    // Verify derived_from relation exists via get_derived_from
    let sources = state
        .artifact_repo
        .get_derived_from(&copied_id)
        .await
        .unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, original_id);

    // Both buckets should have their artifacts
    let all_recommendations = state
        .artifact_repo
        .get_by_type(ArtifactType::Recommendations)
        .await
        .unwrap();
    assert_eq!(all_recommendations.len(), 2);
}

/// Test 3: Create artifact relation (derived_from)
async fn test_create_artifact_relation_derived_from(state: &AppState) {
    // Ensure bucket exists
    let bucket = create_research_outputs_bucket();
    ensure_bucket(state, bucket).await;

    // Create source artifact (research document)
    let source = Artifact::new_inline(
        "OAuth2 Research Document",
        ArtifactType::ResearchDocument,
        "Comprehensive analysis of OAuth2 implementation patterns...",
        "deep-researcher",
    )
    .with_bucket(ArtifactBucketId::from_string("research-outputs"));

    let source_id = source.id.clone();
    state.artifact_repo.create(source).await.unwrap();

    // Create derived artifact (findings based on research)
    let derived = Artifact::new_inline(
        "OAuth2 Findings Summary",
        ArtifactType::Findings,
        "Key findings from the OAuth2 research...",
        "deep-researcher",
    )
    .with_bucket(ArtifactBucketId::from_string("research-outputs"));

    let derived_id = derived.id.clone();
    state.artifact_repo.create(derived).await.unwrap();

    // Create the derived_from relation
    let relation = ArtifactRelation::derived_from(derived_id.clone(), source_id.clone());
    let relation_id = relation.id.clone();
    let created_relation = state.artifact_repo.add_relation(relation).await.unwrap();

    // Verify relation was created
    assert_eq!(created_relation.id, relation_id);
    assert_eq!(created_relation.from_artifact_id, derived_id);
    assert_eq!(created_relation.to_artifact_id, source_id);
    assert_eq!(
        created_relation.relation_type,
        ArtifactRelationType::DerivedFrom
    );

    // Verify we can retrieve relations
    let relations = state
        .artifact_repo
        .get_relations(&derived_id)
        .await
        .unwrap();
    assert_eq!(relations.len(), 1);
    assert_eq!(relations[0].to_artifact_id, source_id);

    // Verify we can query by relation type
    let derived_relations = state
        .artifact_repo
        .get_relations_by_type(&derived_id, ArtifactRelationType::DerivedFrom)
        .await
        .unwrap();
    assert_eq!(derived_relations.len(), 1);

    // Test get_derived_from helper
    let sources = state
        .artifact_repo
        .get_derived_from(&derived_id)
        .await
        .unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, source_id);
}

/// Test 4: Query artifacts by bucket and type
async fn test_query_artifacts_by_bucket_and_type(state: &AppState) {
    // Ensure buckets exist
    let research_bucket = create_research_outputs_bucket();
    let prd_bucket = create_prd_library_bucket();
    ensure_bucket(state, research_bucket).await;
    ensure_bucket(state, prd_bucket).await;

    // Create multiple artifacts in research-outputs
    let findings1 = Artifact::new_inline(
        "Findings 1",
        ArtifactType::Findings,
        "Content 1",
        "researcher",
    )
    .with_bucket(ArtifactBucketId::from_string("research-outputs"));
    let findings2 = Artifact::new_inline(
        "Findings 2",
        ArtifactType::Findings,
        "Content 2",
        "researcher",
    )
    .with_bucket(ArtifactBucketId::from_string("research-outputs"));
    let recommendations = Artifact::new_inline(
        "Recs",
        ArtifactType::Recommendations,
        "Recs content",
        "researcher",
    )
    .with_bucket(ArtifactBucketId::from_string("research-outputs"));

    state.artifact_repo.create(findings1).await.unwrap();
    state.artifact_repo.create(findings2).await.unwrap();
    state.artifact_repo.create(recommendations).await.unwrap();

    // Create artifacts in prd-library
    let prd = Artifact::new_inline("Product PRD", ArtifactType::Prd, "PRD content", "user")
        .with_bucket(ArtifactBucketId::from_string("prd-library"));
    let spec = Artifact::new_inline(
        "API Spec",
        ArtifactType::Specification,
        "Spec content",
        "orchestrator",
    )
    .with_bucket(ArtifactBucketId::from_string("prd-library"));

    state.artifact_repo.create(prd).await.unwrap();
    state.artifact_repo.create(spec).await.unwrap();

    // Query by bucket: research-outputs
    let research_artifacts = state
        .artifact_repo
        .get_by_bucket(&ArtifactBucketId::from_string("research-outputs"))
        .await
        .unwrap();
    assert_eq!(research_artifacts.len(), 3);

    // Query by bucket: prd-library
    let prd_artifacts = state
        .artifact_repo
        .get_by_bucket(&ArtifactBucketId::from_string("prd-library"))
        .await
        .unwrap();
    assert_eq!(prd_artifacts.len(), 2);

    // Query by type: Findings (should be 2)
    let findings = state
        .artifact_repo
        .get_by_type(ArtifactType::Findings)
        .await
        .unwrap();
    assert_eq!(findings.len(), 2);

    // Query by type: Recommendations (should be 1)
    let recs = state
        .artifact_repo
        .get_by_type(ArtifactType::Recommendations)
        .await
        .unwrap();
    assert_eq!(recs.len(), 1);

    // Query by type: Prd (should be 1)
    let prds = state
        .artifact_repo
        .get_by_type(ArtifactType::Prd)
        .await
        .unwrap();
    assert_eq!(prds.len(), 1);
    assert_eq!(prds[0].name, "Product PRD");

    // Query by type: Specification (should be 1)
    let specs = state
        .artifact_repo
        .get_by_type(ArtifactType::Specification)
        .await
        .unwrap();
    assert_eq!(specs.len(), 1);
    assert_eq!(specs[0].name, "API Spec");
}

/// Test 5: Artifact CRUD cycle
async fn test_artifact_crud_cycle(state: &AppState) {
    // Ensure bucket exists
    let bucket = create_research_outputs_bucket();
    ensure_bucket(state, bucket).await;

    // CREATE
    let artifact = Artifact::new_inline(
        "Initial Name",
        ArtifactType::ResearchDocument,
        "Initial content",
        "researcher",
    )
    .with_bucket(ArtifactBucketId::from_string("research-outputs"));
    let artifact_id = artifact.id.clone();

    let created = state.artifact_repo.create(artifact).await.unwrap();
    assert_eq!(created.name, "Initial Name");

    // READ
    let found = state.artifact_repo.get_by_id(&artifact_id).await.unwrap();
    assert!(found.is_some());
    let mut artifact = found.unwrap();

    // UPDATE
    artifact.name = "Updated Name".to_string();
    artifact.metadata.version = 2;
    state.artifact_repo.update(&artifact).await.unwrap();

    // Verify update
    let updated = state
        .artifact_repo
        .get_by_id(&artifact_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.name, "Updated Name");
    assert_eq!(updated.metadata.version, 2);

    // DELETE
    state.artifact_repo.delete(&artifact_id).await.unwrap();

    // Verify deletion
    let deleted = state.artifact_repo.get_by_id(&artifact_id).await.unwrap();
    assert!(deleted.is_none());
}

/// Test 6: Multiple artifacts coexist in buckets
async fn test_multiple_artifacts_coexist(state: &AppState) {
    // Ensure all system buckets exist
    for bucket in ArtifactBucket::system_buckets() {
        ensure_bucket(state, bucket).await;
    }

    // Create artifacts in different buckets
    let code_artifact =
        Artifact::new_inline("Code Changes", ArtifactType::CodeChange, "diff", "worker")
            .with_bucket(ArtifactBucketId::from_string("code-changes"));
    let context_artifact =
        Artifact::new_inline("Task Context", ArtifactType::Context, "ctx", "orchestrator")
            .with_bucket(ArtifactBucketId::from_string("work-context"));
    let prd_artifact = Artifact::new_inline("Feature PRD", ArtifactType::Prd, "prd", "user")
        .with_bucket(ArtifactBucketId::from_string("prd-library"));
    let research_artifact = Artifact::new_inline(
        "Research",
        ArtifactType::ResearchDocument,
        "research",
        "deep-researcher",
    )
    .with_bucket(ArtifactBucketId::from_string("research-outputs"));

    state.artifact_repo.create(code_artifact).await.unwrap();
    state.artifact_repo.create(context_artifact).await.unwrap();
    state.artifact_repo.create(prd_artifact).await.unwrap();
    state.artifact_repo.create(research_artifact).await.unwrap();

    // Verify each bucket has its artifact
    let code = state
        .artifact_repo
        .get_by_bucket(&ArtifactBucketId::from_string("code-changes"))
        .await
        .unwrap();
    assert_eq!(code.len(), 1);
    assert_eq!(code[0].artifact_type, ArtifactType::CodeChange);

    let context = state
        .artifact_repo
        .get_by_bucket(&ArtifactBucketId::from_string("work-context"))
        .await
        .unwrap();
    assert_eq!(context.len(), 1);
    assert_eq!(context[0].artifact_type, ArtifactType::Context);

    let prd = state
        .artifact_repo
        .get_by_bucket(&ArtifactBucketId::from_string("prd-library"))
        .await
        .unwrap();
    assert_eq!(prd.len(), 1);
    assert_eq!(prd[0].artifact_type, ArtifactType::Prd);

    let research = state
        .artifact_repo
        .get_by_bucket(&ArtifactBucketId::from_string("research-outputs"))
        .await
        .unwrap();
    assert_eq!(research.len(), 1);
    assert_eq!(research[0].artifact_type, ArtifactType::ResearchDocument);
}

/// Test 7: Related artifacts (not just derived_from)
async fn test_related_artifacts(state: &AppState) {
    // Ensure bucket exists
    let bucket = create_prd_library_bucket();
    ensure_bucket(state, bucket).await;

    // Create two related PRDs
    let prd1 = Artifact::new_inline("Auth PRD", ArtifactType::Prd, "Auth features", "user")
        .with_bucket(ArtifactBucketId::from_string("prd-library"));
    let prd2 = Artifact::new_inline(
        "Permissions PRD",
        ArtifactType::Prd,
        "Permission features",
        "user",
    )
    .with_bucket(ArtifactBucketId::from_string("prd-library"));

    let prd1_id = prd1.id.clone();
    let prd2_id = prd2.id.clone();

    state.artifact_repo.create(prd1).await.unwrap();
    state.artifact_repo.create(prd2).await.unwrap();

    // Create related_to relation (bidirectional conceptual link)
    let relation = ArtifactRelation::related_to(prd1_id.clone(), prd2_id.clone());
    state.artifact_repo.add_relation(relation).await.unwrap();

    // Verify relation exists
    let relations = state
        .artifact_repo
        .get_relations_by_type(&prd1_id, ArtifactRelationType::RelatedTo)
        .await
        .unwrap();
    assert_eq!(relations.len(), 1);
    assert_eq!(relations[0].to_artifact_id, prd2_id);

    // Test get_related helper
    let related = state.artifact_repo.get_related(&prd1_id).await.unwrap();
    assert_eq!(related.len(), 1);
    assert_eq!(related[0].id, prd2_id);
}

/// Test 8: Delete relation
async fn test_delete_artifact_relation(state: &AppState) {
    // Ensure bucket exists
    let bucket = create_research_outputs_bucket();
    ensure_bucket(state, bucket).await;

    // Create two artifacts
    let artifact1 =
        Artifact::new_inline("Doc 1", ArtifactType::ResearchDocument, "c1", "researcher")
            .with_bucket(ArtifactBucketId::from_string("research-outputs"));
    let artifact2 = Artifact::new_inline("Doc 2", ArtifactType::Findings, "c2", "researcher")
        .with_bucket(ArtifactBucketId::from_string("research-outputs"));

    let id1 = artifact1.id.clone();
    let id2 = artifact2.id.clone();

    state.artifact_repo.create(artifact1).await.unwrap();
    state.artifact_repo.create(artifact2).await.unwrap();

    // Add relation
    let relation = ArtifactRelation::derived_from(id2.clone(), id1.clone());
    state.artifact_repo.add_relation(relation).await.unwrap();

    // Verify relation exists
    let relations = state.artifact_repo.get_relations(&id2).await.unwrap();
    assert_eq!(relations.len(), 1);

    // Delete relation
    state
        .artifact_repo
        .delete_relation(&id2, &id1)
        .await
        .unwrap();

    // Verify relation is gone
    let relations_after = state.artifact_repo.get_relations(&id2).await.unwrap();
    assert!(relations_after.is_empty());
}

/// Test 9: Bucket access control
async fn test_bucket_access_control(state: &AppState) {
    // Ensure research-outputs bucket with specific writers exists
    let bucket = create_research_outputs_bucket();
    let bucket_id = bucket.id.clone();
    ensure_bucket(state, bucket).await;

    // Verify bucket properties
    let found = state
        .artifact_bucket_repo
        .get_by_id(&bucket_id)
        .await
        .unwrap();
    assert!(found.is_some());
    let bucket = found.unwrap();

    // Check access control
    assert!(bucket.can_write("deep-researcher"));
    assert!(bucket.can_write("orchestrator"));
    assert!(!bucket.can_write("random-agent")); // Not in writers list

    // Check type acceptance
    assert!(bucket.accepts_type(ArtifactType::ResearchDocument));
    assert!(bucket.accepts_type(ArtifactType::Findings));
    assert!(bucket.accepts_type(ArtifactType::Recommendations));
    assert!(!bucket.accepts_type(ArtifactType::CodeChange)); // Not in accepted types

    // Check readers
    assert!(bucket.can_read("anyone")); // "all" is in readers
}

/// Test 10: System buckets are flagged correctly
async fn test_system_buckets_flagged(state: &AppState) {
    // Ensure system buckets exist
    for bucket in ArtifactBucket::system_buckets() {
        ensure_bucket(state, bucket).await;
    }

    // Get all system buckets
    let system_buckets = state
        .artifact_bucket_repo
        .get_system_buckets()
        .await
        .unwrap();
    assert_eq!(system_buckets.len(), 5);

    // Verify all are marked as system
    for bucket in &system_buckets {
        assert!(bucket.is_system);
    }

    // Create a custom bucket
    let custom = ArtifactBucket::new("Custom Bucket").accepts(ArtifactType::Prd);
    ensure_bucket(state, custom).await;

    // System buckets should still be 5
    let system_buckets = state
        .artifact_bucket_repo
        .get_system_buckets()
        .await
        .unwrap();
    assert_eq!(system_buckets.len(), 5);

    // But total buckets should be 6
    let all_buckets = state.artifact_bucket_repo.get_all().await.unwrap();
    assert_eq!(all_buckets.len(), 6);
}

// ============================================================================
// Memory Repository Tests
// ============================================================================

#[tokio::test]
async fn test_create_artifact_in_research_outputs_bucket_with_memory() {
    let state = create_memory_state();
    test_create_artifact_in_research_outputs_bucket(&state).await;
}

#[tokio::test]
async fn test_copy_artifact_to_another_bucket_with_memory() {
    let state = create_memory_state();
    test_copy_artifact_to_another_bucket(&state).await;
}

#[tokio::test]
async fn test_create_artifact_relation_derived_from_with_memory() {
    let state = create_memory_state();
    test_create_artifact_relation_derived_from(&state).await;
}

#[tokio::test]
async fn test_query_artifacts_by_bucket_and_type_with_memory() {
    let state = create_memory_state();
    test_query_artifacts_by_bucket_and_type(&state).await;
}

#[tokio::test]
async fn test_artifact_crud_cycle_with_memory() {
    let state = create_memory_state();
    test_artifact_crud_cycle(&state).await;
}

#[tokio::test]
async fn test_multiple_artifacts_coexist_with_memory() {
    let state = create_memory_state();
    test_multiple_artifacts_coexist(&state).await;
}

#[tokio::test]
async fn test_related_artifacts_with_memory() {
    let state = create_memory_state();
    test_related_artifacts(&state).await;
}

#[tokio::test]
async fn test_delete_artifact_relation_with_memory() {
    let state = create_memory_state();
    test_delete_artifact_relation(&state).await;
}

#[tokio::test]
async fn test_bucket_access_control_with_memory() {
    let state = create_memory_state();
    test_bucket_access_control(&state).await;
}

#[tokio::test]
async fn test_system_buckets_flagged_with_memory() {
    let state = create_memory_state();
    test_system_buckets_flagged(&state).await;
}

// ============================================================================
// SQLite Repository Tests
// ============================================================================

#[tokio::test]
async fn test_create_artifact_in_research_outputs_bucket_with_sqlite() {
    let state = create_sqlite_state();
    test_create_artifact_in_research_outputs_bucket(&state).await;
}

#[tokio::test]
async fn test_copy_artifact_to_another_bucket_with_sqlite() {
    let state = create_sqlite_state();
    test_copy_artifact_to_another_bucket(&state).await;
}

#[tokio::test]
async fn test_create_artifact_relation_derived_from_with_sqlite() {
    let state = create_sqlite_state();
    test_create_artifact_relation_derived_from(&state).await;
}

#[tokio::test]
async fn test_query_artifacts_by_bucket_and_type_with_sqlite() {
    let state = create_sqlite_state();
    test_query_artifacts_by_bucket_and_type(&state).await;
}

#[tokio::test]
async fn test_artifact_crud_cycle_with_sqlite() {
    let state = create_sqlite_state();
    test_artifact_crud_cycle(&state).await;
}

#[tokio::test]
async fn test_multiple_artifacts_coexist_with_sqlite() {
    let state = create_sqlite_state();
    test_multiple_artifacts_coexist(&state).await;
}

#[tokio::test]
async fn test_related_artifacts_with_sqlite() {
    let state = create_sqlite_state();
    test_related_artifacts(&state).await;
}

#[tokio::test]
async fn test_delete_artifact_relation_with_sqlite() {
    let state = create_sqlite_state();
    test_delete_artifact_relation(&state).await;
}

#[tokio::test]
async fn test_bucket_access_control_with_sqlite() {
    let state = create_sqlite_state();
    test_bucket_access_control(&state).await;
}

#[tokio::test]
async fn test_system_buckets_flagged_with_sqlite() {
    let state = create_sqlite_state();
    test_system_buckets_flagged(&state).await;
}

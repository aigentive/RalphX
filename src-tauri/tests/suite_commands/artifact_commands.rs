use ralphx_lib::application::AppState;
use ralphx_lib::commands::artifact_commands::{
    get_system_buckets, ArtifactResponse, BucketResponse,
};
use ralphx_lib::domain::entities::{
    Artifact, ArtifactBucket, ArtifactBucketId, ArtifactId, ArtifactRelation, ArtifactType, TaskId,
};

fn setup_test_state() -> AppState {
    AppState::new_test()
}

#[tokio::test]
async fn test_create_artifact() {
    let state = setup_test_state();

    let artifact = Artifact::new_inline("Test PRD", ArtifactType::Prd, "Content", "user");
    let created = state
        .artifact_repo
        .create(artifact)
        .await
        .expect("Failed to create artifact in test");

    assert_eq!(created.name, "Test PRD");
    assert_eq!(created.artifact_type, ArtifactType::Prd);
}

#[tokio::test]
async fn test_get_artifact_by_id() {
    let state = setup_test_state();

    let artifact = Artifact::new_inline("Find Me", ArtifactType::Prd, "Content", "user");
    let id = artifact.id.clone();

    state
        .artifact_repo
        .create(artifact)
        .await
        .expect("Failed to create artifact in test");

    let found = state
        .artifact_repo
        .get_by_id(&id)
        .await
        .expect("Failed to get artifact by id in test");
    assert!(found.is_some());
    assert_eq!(found.expect("Expected to find artifact").name, "Find Me");
}

#[tokio::test]
async fn test_get_artifacts_by_bucket() {
    let state = setup_test_state();

    let bucket_id = ArtifactBucketId::from_string("test-bucket");
    let artifact = Artifact::new_inline("In Bucket", ArtifactType::Prd, "Content", "user")
        .with_bucket(bucket_id.clone());

    state
        .artifact_repo
        .create(artifact)
        .await
        .expect("Failed to create artifact in test");

    let found = state
        .artifact_repo
        .get_by_bucket(&bucket_id)
        .await
        .expect("Failed to get artifacts by bucket in test");
    assert_eq!(found.len(), 1);
}

#[tokio::test]
async fn test_get_artifacts_by_task() {
    let state = setup_test_state();

    let task_id = TaskId::from_string("task-123".to_string());
    let artifact = Artifact::new_inline("For Task", ArtifactType::CodeChange, "diff", "worker")
        .with_task(task_id.clone());

    state
        .artifact_repo
        .create(artifact)
        .await
        .expect("Failed to create artifact in test");

    let found = state
        .artifact_repo
        .get_by_task(&task_id)
        .await
        .expect("Failed to get artifacts by task in test");
    assert_eq!(found.len(), 1);
}

#[tokio::test]
async fn test_archive_artifact() {
    let state = setup_test_state();

    let artifact = Artifact::new_inline("Archive Me", ArtifactType::Prd, "Content", "user");
    let id = artifact.id.clone();

    state
        .artifact_repo
        .create(artifact)
        .await
        .expect("Failed to create artifact in test");
    state
        .artifact_repo
        .archive(&id)
        .await
        .expect("Failed to archive artifact in test");

    let found = state
        .artifact_repo
        .get_by_id(&id)
        .await
        .expect("Failed to get artifact by id in test")
        .expect("Expected artifact to still exist after archive");
    assert!(found.archived_at.is_some());
}

#[tokio::test]
async fn test_create_bucket() {
    let state = setup_test_state();

    let bucket = ArtifactBucket::new("Test Bucket")
        .accepts(ArtifactType::Prd)
        .with_writer("user");

    let created = state
        .artifact_bucket_repo
        .create(bucket)
        .await
        .expect("Failed to create bucket in test");
    assert_eq!(created.name, "Test Bucket");
}

#[tokio::test]
async fn test_get_all_buckets() {
    let state = setup_test_state();

    state
        .artifact_bucket_repo
        .create(ArtifactBucket::new("Bucket 1"))
        .await
        .expect("Failed to create bucket 1 in test");
    state
        .artifact_bucket_repo
        .create(ArtifactBucket::new("Bucket 2"))
        .await
        .expect("Failed to create bucket 2 in test");

    let all = state
        .artifact_bucket_repo
        .get_all()
        .await
        .expect("Failed to get all buckets in test");
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn test_add_artifact_relation() {
    let state = setup_test_state();

    let artifact1 = Artifact::new_inline("Parent", ArtifactType::Prd, "Content", "user");
    let artifact2 = Artifact::new_inline("Child", ArtifactType::Findings, "Derived", "agent");

    let id1 = artifact1.id.clone();
    let id2 = artifact2.id.clone();

    state
        .artifact_repo
        .create(artifact1)
        .await
        .expect("Failed to create parent artifact in test");
    state
        .artifact_repo
        .create(artifact2)
        .await
        .expect("Failed to create child artifact in test");

    let relation = ArtifactRelation::derived_from(id2.clone(), id1.clone());
    state
        .artifact_repo
        .add_relation(relation)
        .await
        .expect("Failed to add artifact relation in test");

    let relations = state
        .artifact_repo
        .get_relations(&id2)
        .await
        .expect("Failed to get artifact relations in test");
    assert_eq!(relations.len(), 1);
}

#[tokio::test]
async fn test_artifact_response_serialization() {
    let artifact = Artifact::new_inline("Test", ArtifactType::Prd, "Content", "user")
        .with_bucket(ArtifactBucketId::from_string("bucket-1"))
        .derived_from_artifact(ArtifactId::from_string("parent-1"));

    let response = ArtifactResponse::from(artifact);

    assert_eq!(response.name, "Test");
    assert_eq!(response.artifact_type, "prd");
    assert_eq!(response.content_type, "inline");
    assert_eq!(response.bucket_id, Some("bucket-1".to_string()));
    assert_eq!(response.derived_from.len(), 1);

    let json =
        serde_json::to_string(&response).expect("Failed to serialize artifact response in test");
    assert!(json.contains("\"name\":\"Test\""));
}

#[tokio::test]
async fn test_bucket_response_serialization() {
    let bucket = ArtifactBucket::new("Test Bucket")
        .accepts(ArtifactType::Prd)
        .accepts(ArtifactType::DesignDoc)
        .with_writer("user");

    let response = BucketResponse::from(bucket);

    assert_eq!(response.name, "Test Bucket");
    assert_eq!(response.accepted_types.len(), 2);
    assert!(!response.is_system);

    let json =
        serde_json::to_string(&response).expect("Failed to serialize bucket response in test");
    assert!(json.contains("\"name\":\"Test Bucket\""));
}

#[tokio::test]
async fn test_get_system_buckets() {
    let result = get_system_buckets()
        .await
        .expect("Failed to get system buckets in test");

    assert_eq!(result.len(), 5);

    let names: Vec<&str> = result.iter().map(|b| b.name.as_str()).collect();
    assert!(names.contains(&"Research Outputs"));
    assert!(names.contains(&"Work Context"));
    assert!(names.contains(&"Code Changes"));
    assert!(names.contains(&"PRD Library"));
    assert!(names.contains(&"Team Findings"));
}

#[tokio::test]
async fn test_get_team_artifacts_by_session_filters_correctly() {
    use ralphx_lib::domain::entities::TeamArtifactMetadata;

    let state = setup_test_state();
    let bucket_id = ArtifactBucketId::from_string("team-findings");

    // Create artifact WITH matching session_id
    let mut matching = Artifact::new_inline(
        "Research Finding",
        ArtifactType::TeamResearch,
        "Some research content here",
        "team-lead",
    )
    .with_bucket(bucket_id.clone());
    matching.metadata = matching.metadata.with_team_metadata(TeamArtifactMetadata {
        team_name: "test-team".into(),
        author_teammate: "researcher".into(),
        session_id: Some("session-abc".into()),
        team_phase: None,
        verification_finding: None,
    });

    // Create artifact with DIFFERENT session_id
    let mut other = Artifact::new_inline(
        "Other Finding",
        ArtifactType::TeamAnalysis,
        "Different session content",
        "team-lead",
    )
    .with_bucket(bucket_id.clone());
    other.metadata = other.metadata.with_team_metadata(TeamArtifactMetadata {
        team_name: "test-team".into(),
        author_teammate: "analyst".into(),
        session_id: Some("session-xyz".into()),
        team_phase: None,
        verification_finding: None,
    });

    // Create artifact with NO team_metadata
    let no_meta = Artifact::new_inline(
        "No Meta",
        ArtifactType::TeamSummary,
        "No team metadata",
        "system",
    )
    .with_bucket(bucket_id.clone());

    state
        .artifact_repo
        .create(matching)
        .await
        .expect("create matching");
    state
        .artifact_repo
        .create(other)
        .await
        .expect("create other");
    state
        .artifact_repo
        .create(no_meta)
        .await
        .expect("create no_meta");

    // Query for session-abc — should return only the matching artifact
    let all = state
        .artifact_repo
        .get_by_bucket(&bucket_id)
        .await
        .expect("get_by_bucket");
    assert_eq!(all.len(), 3);

    // Filter like the command does
    let filtered: Vec<_> = all
        .into_iter()
        .filter(|a| {
            a.metadata
                .team_metadata
                .as_ref()
                .and_then(|tm| tm.session_id.as_deref())
                == Some("session-abc")
        })
        .collect();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "Research Finding");
}

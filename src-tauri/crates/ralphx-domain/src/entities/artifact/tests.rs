use super::types::*;
use crate::entities::types::TaskId;

// ===== ArtifactId Tests =====

#[test]
fn artifact_id_new_generates_valid_uuid() {
    let id = ArtifactId::new();
    assert_eq!(id.as_str().len(), 36);
    assert!(uuid::Uuid::parse_str(id.as_str()).is_ok());
}

#[test]
fn artifact_id_from_string_preserves_value() {
    let id = ArtifactId::from_string("artifact-123");
    assert_eq!(id.as_str(), "artifact-123");
}

#[test]
fn artifact_id_equality_works() {
    let id1 = ArtifactId::from_string("a1");
    let id2 = ArtifactId::from_string("a1");
    let id3 = ArtifactId::from_string("a2");
    assert_eq!(id1, id2);
    assert_ne!(id1, id3);
}

#[test]
fn artifact_id_serializes() {
    let id = ArtifactId::from_string("serialize-test");
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, "\"serialize-test\"");
}

#[test]
fn artifact_id_deserializes() {
    let json = "\"deserialize-test\"";
    let id: ArtifactId = serde_json::from_str(json).unwrap();
    assert_eq!(id.as_str(), "deserialize-test");
}

// ===== ArtifactBucketId Tests =====

#[test]
fn artifact_bucket_id_new_generates_valid_uuid() {
    let id = ArtifactBucketId::new();
    assert_eq!(id.as_str().len(), 36);
    assert!(uuid::Uuid::parse_str(id.as_str()).is_ok());
}

#[test]
fn artifact_bucket_id_from_string_preserves_value() {
    let id = ArtifactBucketId::from_string("bucket-123");
    assert_eq!(id.as_str(), "bucket-123");
}

// ===== ProcessId Tests =====

#[test]
fn process_id_new_generates_valid_uuid() {
    let id = ProcessId::new();
    assert_eq!(id.as_str().len(), 36);
    assert!(uuid::Uuid::parse_str(id.as_str()).is_ok());
}

#[test]
fn process_id_from_string_preserves_value() {
    let id = ProcessId::from_string("process-123");
    assert_eq!(id.as_str(), "process-123");
}

// ===== ArtifactType Tests =====

#[test]
fn artifact_type_all_includes_persona() {
    let all = ArtifactType::all();
    assert_eq!(all.len(), 23);
    assert!(all.contains(&ArtifactType::Persona));
}

#[test]
fn artifact_type_serializes_snake_case() {
    assert_eq!(
        serde_json::to_string(&ArtifactType::Prd).unwrap(),
        "\"prd\""
    );
    assert_eq!(
        serde_json::to_string(&ArtifactType::ResearchDocument).unwrap(),
        "\"research_document\""
    );
    assert_eq!(
        serde_json::to_string(&ArtifactType::CodeChange).unwrap(),
        "\"code_change\""
    );
    assert_eq!(
        serde_json::to_string(&ArtifactType::ReviewFeedback).unwrap(),
        "\"review_feedback\""
    );
    assert_eq!(
        serde_json::to_string(&ArtifactType::PrReview).unwrap(),
        "\"pr_review\""
    );
    assert_eq!(
        serde_json::to_string(&ArtifactType::Persona).unwrap(),
        "\"persona\""
    );
}

#[test]
fn artifact_type_deserializes() {
    let t: ArtifactType = serde_json::from_str("\"prd\"").unwrap();
    assert_eq!(t, ArtifactType::Prd);
    let t: ArtifactType = serde_json::from_str("\"research_document\"").unwrap();
    assert_eq!(t, ArtifactType::ResearchDocument);
    assert!(serde_json::from_str::<ArtifactType>("\"verification_finding\"").is_err());
    let t: ArtifactType = serde_json::from_str("\"pr_review\"").unwrap();
    assert_eq!(t, ArtifactType::PrReview);
    let t: ArtifactType = serde_json::from_str("\"persona\"").unwrap();
    assert_eq!(t, ArtifactType::Persona);
}

#[test]
fn artifact_type_from_str() {
    use std::str::FromStr;
    assert_eq!(ArtifactType::from_str("prd").unwrap(), ArtifactType::Prd);
    assert_eq!(
        ArtifactType::from_str("research_document").unwrap(),
        ArtifactType::ResearchDocument
    );
    assert_eq!(
        ArtifactType::from_str("design_doc").unwrap(),
        ArtifactType::DesignDoc
    );
    assert_eq!(
        ArtifactType::from_str("pr_review").unwrap(),
        ArtifactType::PrReview
    );
}

#[test]
fn artifact_type_from_str_error() {
    use std::str::FromStr;
    let err = ArtifactType::from_str("invalid").unwrap_err();
    assert_eq!(err.value, "invalid");
    assert!(err.to_string().contains("invalid"));
}

#[test]
fn artifact_type_display() {
    assert_eq!(ArtifactType::Prd.to_string(), "prd");
    assert_eq!(
        ArtifactType::ResearchDocument.to_string(),
        "research_document"
    );
}

// ===== ArtifactContent Tests =====

#[test]
fn artifact_content_inline_creates_correctly() {
    let content = ArtifactContent::inline("Hello world");
    assert!(content.is_inline());
    assert!(!content.is_file());
    assert_eq!(content.content_type(), "inline");
}

#[test]
fn artifact_content_file_creates_correctly() {
    let content = ArtifactContent::file("/path/to/file.md");
    assert!(content.is_file());
    assert!(!content.is_inline());
    assert_eq!(content.content_type(), "file");
}

#[test]
fn artifact_content_inline_serializes() {
    let content = ArtifactContent::inline("Test content");
    let json = serde_json::to_string(&content).unwrap();
    assert!(json.contains("\"type\":\"inline\""));
    assert!(json.contains("\"text\":\"Test content\""));
}

#[test]
fn artifact_content_file_serializes() {
    let content = ArtifactContent::file("/path/to/file");
    let json = serde_json::to_string(&content).unwrap();
    assert!(json.contains("\"type\":\"file\""));
    assert!(json.contains("\"path\":\"/path/to/file\""));
}

#[test]
fn artifact_content_deserializes_inline() {
    let json = r#"{"type":"inline","text":"Hello"}"#;
    let content: ArtifactContent = serde_json::from_str(json).unwrap();
    assert!(content.is_inline());
    if let ArtifactContent::Inline { text } = content {
        assert_eq!(text, "Hello");
    } else {
        panic!("Expected inline content");
    }
}

#[test]
fn artifact_content_deserializes_file() {
    let json = r#"{"type":"file","path":"/test/path"}"#;
    let content: ArtifactContent = serde_json::from_str(json).unwrap();
    assert!(content.is_file());
    if let ArtifactContent::File { path } = content {
        assert_eq!(path, "/test/path");
    } else {
        panic!("Expected file content");
    }
}

// ===== ArtifactMetadata Tests =====

#[test]
fn artifact_metadata_new_sets_defaults() {
    let meta = ArtifactMetadata::new("user");
    assert_eq!(meta.created_by, "user");
    assert_eq!(meta.version, 1);
    assert!(meta.task_id.is_none());
    assert!(meta.process_id.is_none());
}

#[test]
fn artifact_metadata_with_task() {
    let task_id = TaskId::from_string("task-1".to_string());
    let meta = ArtifactMetadata::new("user").with_task(task_id.clone());
    assert_eq!(meta.task_id, Some(task_id));
}

#[test]
fn artifact_metadata_with_process() {
    let process_id = ProcessId::from_string("process-1");
    let meta = ArtifactMetadata::new("user").with_process(process_id.clone());
    assert_eq!(meta.process_id, Some(process_id));
}

#[test]
fn artifact_metadata_with_version() {
    let meta = ArtifactMetadata::new("user").with_version(5);
    assert_eq!(meta.version, 5);
}

// ===== Artifact Tests =====

#[test]
fn artifact_new_inline_creates_correctly() {
    let artifact = Artifact::new_inline("Test PRD", ArtifactType::Prd, "PRD content", "user");
    assert_eq!(artifact.name, "Test PRD");
    assert_eq!(artifact.artifact_type, ArtifactType::Prd);
    assert!(artifact.content.is_inline());
    assert_eq!(artifact.metadata.created_by, "user");
    assert!(artifact.derived_from.is_empty());
    assert!(artifact.bucket_id.is_none());
}

#[test]
fn artifact_new_file_creates_correctly() {
    let artifact = Artifact::new_file(
        "Design Doc",
        ArtifactType::DesignDoc,
        "/docs/design.md",
        "system",
    );
    assert_eq!(artifact.name, "Design Doc");
    assert_eq!(artifact.artifact_type, ArtifactType::DesignDoc);
    assert!(artifact.content.is_file());
    assert_eq!(artifact.metadata.created_by, "system");
}

#[test]
fn artifact_with_bucket() {
    let bucket_id = ArtifactBucketId::from_string("bucket-1");
    let artifact = Artifact::new_inline("Test", ArtifactType::Prd, "Content", "user")
        .with_bucket(bucket_id.clone());
    assert_eq!(artifact.bucket_id, Some(bucket_id));
}

#[test]
fn artifact_derived_from_artifact() {
    let parent_id = ArtifactId::from_string("parent-1");
    let artifact = Artifact::new_inline("Child", ArtifactType::Findings, "Content", "agent")
        .derived_from_artifact(parent_id.clone());
    assert_eq!(artifact.derived_from.len(), 1);
    assert_eq!(artifact.derived_from[0], parent_id);
}

#[test]
fn artifact_with_task() {
    let task_id = TaskId::from_string("task-1".to_string());
    let artifact = Artifact::new_inline("Test", ArtifactType::CodeChange, "Code", "worker")
        .with_task(task_id.clone());
    assert_eq!(artifact.metadata.task_id, Some(task_id));
}

#[test]
fn artifact_serializes_roundtrip() {
    let artifact = Artifact::new_inline("Test", ArtifactType::Prd, "Content", "user")
        .with_bucket(ArtifactBucketId::from_string("bucket-1"))
        .derived_from_artifact(ArtifactId::from_string("parent-1"));
    let json = serde_json::to_string(&artifact).unwrap();
    let parsed: Artifact = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.name, artifact.name);
    assert_eq!(parsed.artifact_type, artifact.artifact_type);
    assert_eq!(parsed.bucket_id, artifact.bucket_id);
    assert_eq!(parsed.derived_from.len(), 1);
}

// ===== ArtifactBucket Tests =====

#[test]
fn artifact_bucket_new_creates_custom() {
    let bucket = ArtifactBucket::new("My Bucket");
    assert_eq!(bucket.name, "My Bucket");
    assert!(!bucket.is_system);
    assert!(bucket.accepted_types.is_empty());
}

#[test]
fn artifact_bucket_system_creates_with_id() {
    let bucket = ArtifactBucket::system("research-outputs", "Research Outputs");
    assert_eq!(bucket.id.as_str(), "research-outputs");
    assert_eq!(bucket.name, "Research Outputs");
    assert!(bucket.is_system);
}

#[test]
fn artifact_bucket_accepts_type() {
    let bucket = ArtifactBucket::new("Test")
        .accepts(ArtifactType::Prd)
        .accepts(ArtifactType::DesignDoc);
    assert!(bucket.accepts_type(ArtifactType::Prd));
    assert!(bucket.accepts_type(ArtifactType::DesignDoc));
    assert!(!bucket.accepts_type(ArtifactType::CodeChange));
}

#[test]
fn artifact_bucket_accepts_all_types_when_empty() {
    let bucket = ArtifactBucket::new("Test");
    assert!(bucket.accepts_type(ArtifactType::Prd));
    assert!(bucket.accepts_type(ArtifactType::CodeChange));
}

#[test]
fn artifact_bucket_with_writer() {
    let bucket = ArtifactBucket::new("Test")
        .with_writer("worker")
        .with_writer("reviewer");
    assert!(bucket.can_write("worker"));
    assert!(bucket.can_write("reviewer"));
    assert!(!bucket.can_write("user"));
}

#[test]
fn artifact_bucket_allows_all_writers_when_empty() {
    let bucket = ArtifactBucket::new("Test");
    assert!(bucket.can_write("anyone"));
}

#[test]
fn artifact_bucket_with_reader() {
    let bucket = ArtifactBucket::new("Test").with_reader("worker");
    assert!(bucket.can_read("worker"));
    assert!(bucket.can_read("all")); // "all" is added by default
}

#[test]
fn artifact_bucket_system_buckets_returns_5() {
    let buckets = ArtifactBucket::system_buckets();
    assert_eq!(buckets.len(), 5);

    let ids: Vec<&str> = buckets.iter().map(|b| b.id.as_str()).collect();
    assert!(ids.contains(&"research-outputs"));
    assert!(ids.contains(&"work-context"));
    assert!(ids.contains(&"code-changes"));
    assert!(ids.contains(&"prd-library"));
    assert!(ids.contains(&"team-findings"));
}

#[test]
fn artifact_bucket_research_outputs_has_correct_types() {
    let buckets = ArtifactBucket::system_buckets();
    let research = buckets
        .iter()
        .find(|b| b.id.as_str() == "research-outputs")
        .unwrap();
    assert!(research.accepts_type(ArtifactType::ResearchDocument));
    assert!(research.accepts_type(ArtifactType::Findings));
    assert!(research.accepts_type(ArtifactType::Recommendations));
    assert!(!research.accepts_type(ArtifactType::CodeChange));
}

#[test]
fn artifact_bucket_team_findings_accepts_only_current_team_artifacts() {
    let buckets = ArtifactBucket::system_buckets();
    let team = buckets
        .iter()
        .find(|b| b.id.as_str() == "team-findings")
        .unwrap();
    assert!(team.accepts_type(ArtifactType::TeamResearch));
    assert!(team.accepts_type(ArtifactType::TeamAnalysis));
    assert!(team.accepts_type(ArtifactType::TeamSummary));
    assert!(!team.accepts_type(ArtifactType::Findings));
}

#[test]
fn artifact_bucket_serializes() {
    let bucket = ArtifactBucket::new("Test").accepts(ArtifactType::Prd);
    let json = serde_json::to_string(&bucket).unwrap();
    assert!(json.contains("\"name\":\"Test\""));
    assert!(json.contains("\"prd\""));
}

// ===== ArtifactRelationType Tests =====

#[test]
fn artifact_relation_type_serializes() {
    assert_eq!(
        serde_json::to_string(&ArtifactRelationType::DerivedFrom).unwrap(),
        "\"derived_from\""
    );
    assert_eq!(
        serde_json::to_string(&ArtifactRelationType::RelatedTo).unwrap(),
        "\"related_to\""
    );
}

#[test]
fn artifact_relation_type_deserializes() {
    let t: ArtifactRelationType = serde_json::from_str("\"derived_from\"").unwrap();
    assert_eq!(t, ArtifactRelationType::DerivedFrom);
}

#[test]
fn artifact_relation_type_from_str() {
    use std::str::FromStr;
    assert_eq!(
        ArtifactRelationType::from_str("derived_from").unwrap(),
        ArtifactRelationType::DerivedFrom
    );
    assert_eq!(
        ArtifactRelationType::from_str("related_to").unwrap(),
        ArtifactRelationType::RelatedTo
    );
}

#[test]
fn artifact_relation_type_from_str_error() {
    use std::str::FromStr;
    let err = ArtifactRelationType::from_str("invalid").unwrap_err();
    assert_eq!(err.value, "invalid");
}

#[test]
fn artifact_relation_type_display() {
    assert_eq!(
        ArtifactRelationType::DerivedFrom.to_string(),
        "derived_from"
    );
    assert_eq!(ArtifactRelationType::RelatedTo.to_string(), "related_to");
}

// ===== ArtifactRelationId Tests =====

#[test]
fn artifact_relation_id_new_generates_valid_uuid() {
    let id = ArtifactRelationId::new();
    assert_eq!(id.as_str().len(), 36);
    assert!(uuid::Uuid::parse_str(id.as_str()).is_ok());
}

#[test]
fn artifact_relation_id_from_string_preserves_value() {
    let id = ArtifactRelationId::from_string("rel-123");
    assert_eq!(id.as_str(), "rel-123");
}

// ===== ArtifactRelation Tests =====

#[test]
fn artifact_relation_new_creates_correctly() {
    let from = ArtifactId::from_string("from-1");
    let to = ArtifactId::from_string("to-1");
    let rel = ArtifactRelation::new(from.clone(), to.clone(), ArtifactRelationType::DerivedFrom);
    assert_eq!(rel.from_artifact_id, from);
    assert_eq!(rel.to_artifact_id, to);
    assert_eq!(rel.relation_type, ArtifactRelationType::DerivedFrom);
}

#[test]
fn artifact_relation_derived_from_helper() {
    let derived = ArtifactId::from_string("derived");
    let source = ArtifactId::from_string("source");
    let rel = ArtifactRelation::derived_from(derived.clone(), source.clone());
    assert_eq!(rel.from_artifact_id, derived);
    assert_eq!(rel.to_artifact_id, source);
    assert_eq!(rel.relation_type, ArtifactRelationType::DerivedFrom);
}

#[test]
fn artifact_relation_related_to_helper() {
    let a = ArtifactId::from_string("a");
    let b = ArtifactId::from_string("b");
    let rel = ArtifactRelation::related_to(a.clone(), b.clone());
    assert_eq!(rel.from_artifact_id, a);
    assert_eq!(rel.to_artifact_id, b);
    assert_eq!(rel.relation_type, ArtifactRelationType::RelatedTo);
}

#[test]
fn artifact_relation_serializes() {
    let rel = ArtifactRelation::derived_from(
        ArtifactId::from_string("from"),
        ArtifactId::from_string("to"),
    );
    let json = serde_json::to_string(&rel).unwrap();
    assert!(json.contains("\"from_artifact_id\":\"from\""));
    assert!(json.contains("\"to_artifact_id\":\"to\""));
    assert!(json.contains("\"relation_type\":\"derived_from\""));
}

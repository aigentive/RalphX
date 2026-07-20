use super::*;
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::Mutex;

// ==================== Mock Artifact Repository ====================

struct MockArtifactRepository {
    artifacts: Mutex<HashMap<String, Artifact>>,
    relations: Mutex<Vec<ArtifactRelation>>,
}

impl MockArtifactRepository {
    fn new() -> Self {
        Self {
            artifacts: Mutex::new(HashMap::new()),
            relations: Mutex::new(Vec::new()),
        }
    }

    async fn add_artifact(&self, artifact: Artifact) {
        let mut artifacts = self.artifacts.lock().await;
        artifacts.insert(artifact.id.as_str().to_string(), artifact);
    }
}

#[async_trait]
impl ArtifactRepository for MockArtifactRepository {
    async fn create(&self, artifact: Artifact) -> AppResult<Artifact> {
        self.add_artifact(artifact.clone()).await;
        Ok(artifact)
    }

    async fn get_by_id(&self, id: &ArtifactId) -> AppResult<Option<Artifact>> {
        let artifacts = self.artifacts.lock().await;
        Ok(artifacts.get(id.as_str()).cloned())
    }

    async fn get_by_id_at_version(
        &self,
        id: &ArtifactId,
        version: u32,
    ) -> AppResult<Option<Artifact>> {
        let artifacts = self.artifacts.lock().await;
        if let Some(artifact) = artifacts.get(id.as_str()) {
            if artifact.metadata.version == version {
                Ok(Some(artifact.clone()))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    async fn get_by_bucket(&self, bucket_id: &ArtifactBucketId) -> AppResult<Vec<Artifact>> {
        let artifacts = self.artifacts.lock().await;
        Ok(artifacts
            .values()
            .filter(|a| a.bucket_id.as_ref() == Some(bucket_id))
            .cloned()
            .collect())
    }

    async fn get_by_type(&self, artifact_type: ArtifactType) -> AppResult<Vec<Artifact>> {
        let artifacts = self.artifacts.lock().await;
        Ok(artifacts
            .values()
            .filter(|a| a.artifact_type == artifact_type)
            .cloned()
            .collect())
    }

    async fn get_by_task(&self, task_id: &TaskId) -> AppResult<Vec<Artifact>> {
        let artifacts = self.artifacts.lock().await;
        Ok(artifacts
            .values()
            .filter(|a| a.metadata.task_id.as_ref() == Some(task_id))
            .cloned()
            .collect())
    }

    async fn get_by_process(&self, process_id: &ProcessId) -> AppResult<Vec<Artifact>> {
        let artifacts = self.artifacts.lock().await;
        Ok(artifacts
            .values()
            .filter(|a| a.metadata.process_id.as_ref() == Some(process_id))
            .cloned()
            .collect())
    }

    async fn update(&self, artifact: &Artifact) -> AppResult<()> {
        let mut artifacts = self.artifacts.lock().await;
        artifacts.insert(artifact.id.as_str().to_string(), artifact.clone());
        Ok(())
    }

    async fn delete(&self, id: &ArtifactId) -> AppResult<()> {
        let mut artifacts = self.artifacts.lock().await;
        artifacts.remove(id.as_str());
        Ok(())
    }

    async fn get_derived_from(&self, artifact_id: &ArtifactId) -> AppResult<Vec<Artifact>> {
        let relations = self.relations.lock().await;
        let artifacts = self.artifacts.lock().await;

        let derived_ids: Vec<_> = relations
            .iter()
            .filter(|r| {
                r.from_artifact_id == *artifact_id
                    && r.relation_type == ArtifactRelationType::DerivedFrom
            })
            .map(|r| r.to_artifact_id.as_str().to_string())
            .collect();

        Ok(derived_ids
            .iter()
            .filter_map(|id| artifacts.get(id).cloned())
            .collect())
    }

    async fn get_related(&self, artifact_id: &ArtifactId) -> AppResult<Vec<Artifact>> {
        let relations = self.relations.lock().await;
        let artifacts = self.artifacts.lock().await;

        let related_ids: Vec<_> = relations
            .iter()
            .filter(|r| r.from_artifact_id == *artifact_id || r.to_artifact_id == *artifact_id)
            .flat_map(|r| {
                if r.from_artifact_id == *artifact_id {
                    vec![r.to_artifact_id.as_str().to_string()]
                } else {
                    vec![r.from_artifact_id.as_str().to_string()]
                }
            })
            .collect();

        Ok(related_ids
            .iter()
            .filter_map(|id| artifacts.get(id).cloned())
            .collect())
    }

    async fn add_relation(&self, relation: ArtifactRelation) -> AppResult<ArtifactRelation> {
        let mut relations = self.relations.lock().await;
        relations.push(relation.clone());
        Ok(relation)
    }

    async fn get_relations(&self, artifact_id: &ArtifactId) -> AppResult<Vec<ArtifactRelation>> {
        let relations = self.relations.lock().await;
        Ok(relations
            .iter()
            .filter(|r| r.from_artifact_id == *artifact_id || r.to_artifact_id == *artifact_id)
            .cloned()
            .collect())
    }

    async fn get_relations_by_type(
        &self,
        artifact_id: &ArtifactId,
        relation_type: ArtifactRelationType,
    ) -> AppResult<Vec<ArtifactRelation>> {
        let relations = self.relations.lock().await;
        Ok(relations
            .iter()
            .filter(|r| {
                (r.from_artifact_id == *artifact_id || r.to_artifact_id == *artifact_id)
                    && r.relation_type == relation_type
            })
            .cloned()
            .collect())
    }

    async fn delete_relation(&self, from_id: &ArtifactId, to_id: &ArtifactId) -> AppResult<()> {
        let mut relations = self.relations.lock().await;
        relations.retain(|r| r.from_artifact_id != *from_id || r.to_artifact_id != *to_id);
        Ok(())
    }

    async fn create_with_previous_version(
        &self,
        artifact: Artifact,
        _previous_version_id: ArtifactId,
    ) -> AppResult<Artifact> {
        self.add_artifact(artifact.clone()).await;
        Ok(artifact)
    }

    async fn get_version_history(
        &self,
        id: &ArtifactId,
    ) -> AppResult<Vec<crate::domain::repositories::ArtifactVersionSummary>> {
        let artifacts = self.artifacts.lock().await;
        if let Some(artifact) = artifacts.get(id.as_str()) {
            Ok(vec![crate::domain::repositories::ArtifactVersionSummary {
                id: artifact.id.clone(),
                version: artifact.metadata.version,
                name: artifact.name.clone(),
                created_at: artifact.metadata.created_at,
                created_by: artifact.metadata.created_by.clone(),
                metadata: artifact.metadata.custom_metadata.clone(),
            }])
        } else {
            Ok(vec![])
        }
    }

    async fn resolve_latest_artifact_id(&self, id: &ArtifactId) -> AppResult<ArtifactId> {
        Ok(id.clone())
    }

    async fn archive(&self, _id: &ArtifactId) -> AppResult<Artifact> {
        unimplemented!()
    }
}

// ==================== Mock Bucket Repository ====================

struct MockBucketRepository {
    buckets: Mutex<HashMap<String, ArtifactBucket>>,
}

impl MockBucketRepository {
    fn new() -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
        }
    }

    async fn add_bucket(&self, bucket: ArtifactBucket) {
        let mut buckets = self.buckets.lock().await;
        buckets.insert(bucket.id.as_str().to_string(), bucket);
    }
}

#[async_trait]
impl ArtifactBucketRepository for MockBucketRepository {
    async fn create(&self, bucket: ArtifactBucket) -> AppResult<ArtifactBucket> {
        self.add_bucket(bucket.clone()).await;
        Ok(bucket)
    }

    async fn get_by_id(&self, id: &ArtifactBucketId) -> AppResult<Option<ArtifactBucket>> {
        let buckets = self.buckets.lock().await;
        Ok(buckets.get(id.as_str()).cloned())
    }

    async fn get_all(&self) -> AppResult<Vec<ArtifactBucket>> {
        let buckets = self.buckets.lock().await;
        Ok(buckets.values().cloned().collect())
    }

    async fn get_system_buckets(&self) -> AppResult<Vec<ArtifactBucket>> {
        let buckets = self.buckets.lock().await;
        Ok(buckets.values().filter(|b| b.is_system).cloned().collect())
    }

    async fn update(&self, bucket: &ArtifactBucket) -> AppResult<()> {
        let mut buckets = self.buckets.lock().await;
        buckets.insert(bucket.id.as_str().to_string(), bucket.clone());
        Ok(())
    }

    async fn delete(&self, id: &ArtifactBucketId) -> AppResult<()> {
        let mut buckets = self.buckets.lock().await;
        buckets.remove(id.as_str());
        Ok(())
    }

    async fn exists(&self, id: &ArtifactBucketId) -> AppResult<bool> {
        let buckets = self.buckets.lock().await;
        Ok(buckets.contains_key(id.as_str()))
    }
}

// ==================== Test Helpers ====================

fn create_service() -> (
    ArtifactService<MockArtifactRepository, MockBucketRepository>,
    Arc<MockArtifactRepository>,
    Arc<MockBucketRepository>,
) {
    let artifact_repo = Arc::new(MockArtifactRepository::new());
    let bucket_repo = Arc::new(MockBucketRepository::new());
    let service = ArtifactService::new(artifact_repo.clone(), bucket_repo.clone());
    (service, artifact_repo, bucket_repo)
}

fn create_prd_bucket() -> ArtifactBucket {
    ArtifactBucket::system("prd-library", "PRD Library")
        .accepts(ArtifactType::Prd)
        .accepts(ArtifactType::Specification)
        .with_writer("orchestrator")
        .with_writer("user")
}

fn create_code_bucket() -> ArtifactBucket {
    ArtifactBucket::system("code-changes", "Code Changes")
        .accepts(ArtifactType::CodeChange)
        .accepts(ArtifactType::Diff)
        .with_writer("worker")
}

fn create_test_artifact() -> Artifact {
    Artifact::new_inline("Test PRD", ArtifactType::Prd, "PRD content", "user")
}

// ==================== create_artifact Tests ====================

#[tokio::test]
async fn create_artifact_without_bucket() {
    let (service, _artifact_repo, _bucket_repo) = create_service();

    let artifact = create_test_artifact();
    let result = service.create_artifact(artifact.clone(), "user").await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.name, "Test PRD");
    assert!(created.bucket_id.is_none());
}

#[tokio::test]
async fn create_artifact_with_valid_bucket() {
    let (service, _artifact_repo, bucket_repo) = create_service();

    let bucket = create_prd_bucket();
    bucket_repo.add_bucket(bucket.clone()).await;

    let artifact = create_test_artifact().with_bucket(bucket.id.clone());
    let result = service.create_artifact(artifact, "user").await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.bucket_id, Some(bucket.id));
}

#[tokio::test]
async fn create_artifact_bucket_not_found() {
    let (service, _artifact_repo, _bucket_repo) = create_service();

    let bucket_id = ArtifactBucketId::from_string("nonexistent");
    let artifact = create_test_artifact().with_bucket(bucket_id);
    let result = service.create_artifact(artifact, "user").await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[tokio::test]
async fn create_artifact_type_not_accepted() {
    let (service, _artifact_repo, bucket_repo) = create_service();

    let bucket = create_code_bucket(); // Only accepts code types
    bucket_repo.add_bucket(bucket.clone()).await;

    let artifact = create_test_artifact().with_bucket(bucket.id.clone()); // PRD type
    let result = service.create_artifact(artifact, "worker").await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("does not accept"));
}

#[tokio::test]
async fn create_artifact_writer_not_allowed() {
    let (service, _artifact_repo, bucket_repo) = create_service();

    let bucket = create_code_bucket(); // Only "worker" can write
    bucket_repo.add_bucket(bucket.clone()).await;

    let artifact = Artifact::new_inline("Code", ArtifactType::CodeChange, "code", "user")
        .with_bucket(bucket.id.clone());
    let result = service.create_artifact(artifact, "user").await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cannot write"));
}

// ==================== get_artifact Tests ====================

#[tokio::test]
async fn get_artifact_found() {
    let (service, artifact_repo, _bucket_repo) = create_service();

    let artifact = create_test_artifact();
    let id = artifact.id.clone();
    artifact_repo.add_artifact(artifact).await;

    let result = service.get_artifact(&id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_some());
}

#[tokio::test]
async fn get_artifact_not_found() {
    let (service, _artifact_repo, _bucket_repo) = create_service();

    let id = ArtifactId::new();
    let result = service.get_artifact(&id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

// ==================== get_artifacts_for_task Tests ====================

#[tokio::test]
async fn get_artifacts_for_task_empty() {
    let (service, _artifact_repo, _bucket_repo) = create_service();

    let task_id = TaskId::from_string("task-1".to_string());
    let result = service.get_artifacts_for_task(&task_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn get_artifacts_for_task_returns_matching() {
    let (service, artifact_repo, _bucket_repo) = create_service();

    let task_id = TaskId::from_string("task-1".to_string());
    let artifact = create_test_artifact().with_task(task_id.clone());
    artifact_repo.add_artifact(artifact).await;

    let result = service.get_artifacts_for_task(&task_id).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 1);
}

// ==================== get_artifacts_for_process Tests ====================

#[tokio::test]
async fn get_artifacts_for_process_empty() {
    let (service, _artifact_repo, _bucket_repo) = create_service();

    let process_id = ProcessId::new();
    let result = service.get_artifacts_for_process(&process_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn get_artifacts_for_process_returns_matching() {
    let (service, artifact_repo, _bucket_repo) = create_service();

    let process_id = ProcessId::from_string("process-1");
    let artifact = create_test_artifact().with_process(process_id.clone());
    artifact_repo.add_artifact(artifact).await;

    let result = service.get_artifacts_for_process(&process_id).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 1);
}

// ==================== get_artifacts_in_bucket Tests ====================

#[tokio::test]
async fn get_artifacts_in_bucket_empty() {
    let (service, _artifact_repo, _bucket_repo) = create_service();

    let bucket_id = ArtifactBucketId::new();
    let result = service.get_artifacts_in_bucket(&bucket_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn get_artifacts_in_bucket_returns_matching() {
    let (service, artifact_repo, _bucket_repo) = create_service();

    let bucket_id = ArtifactBucketId::from_string("test-bucket");
    let artifact = create_test_artifact().with_bucket(bucket_id.clone());
    artifact_repo.add_artifact(artifact).await;

    let result = service.get_artifacts_in_bucket(&bucket_id).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 1);
}

// ==================== get_artifacts_by_type Tests ====================

#[tokio::test]
async fn get_artifacts_by_type_empty() {
    let (service, _artifact_repo, _bucket_repo) = create_service();

    let result = service.get_artifacts_by_type(ArtifactType::Prd).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn get_artifacts_by_type_returns_matching() {
    let (service, artifact_repo, _bucket_repo) = create_service();

    let artifact = create_test_artifact(); // PRD type
    artifact_repo.add_artifact(artifact).await;

    let result = service.get_artifacts_by_type(ArtifactType::Prd).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 1);
}

// ==================== copy_to_bucket Tests ====================

#[tokio::test]
async fn copy_to_bucket_success() {
    let (service, artifact_repo, bucket_repo) = create_service();

    // Create source bucket and artifact
    let source_bucket = create_prd_bucket();
    bucket_repo.add_bucket(source_bucket.clone()).await;

    let artifact = create_test_artifact().with_bucket(source_bucket.id.clone());
    let artifact_id = artifact.id.clone();
    artifact_repo.add_artifact(artifact).await;

    // Create target bucket
    let target_bucket = ArtifactBucket::new("Target Bucket")
        .accepts(ArtifactType::Prd)
        .with_writer("copier");
    let target_id = target_bucket.id.clone();
    bucket_repo.add_bucket(target_bucket).await;

    // Copy
    let result = service
        .copy_to_bucket(&artifact_id, &target_id, "copier")
        .await;

    assert!(result.is_ok());
    let copied = result.unwrap();
    assert_ne!(copied.id, artifact_id);
    assert_eq!(copied.bucket_id, Some(target_id));
    assert_eq!(copied.name, "Test PRD");
    assert_eq!(copied.derived_from.len(), 1);
    assert_eq!(copied.derived_from[0], artifact_id);
}

#[tokio::test]
async fn copy_to_bucket_source_not_found() {
    let (service, _artifact_repo, bucket_repo) = create_service();

    let target_bucket = create_prd_bucket();
    bucket_repo.add_bucket(target_bucket.clone()).await;

    let artifact_id = ArtifactId::new();
    let result = service
        .copy_to_bucket(&artifact_id, &target_bucket.id, "user")
        .await;

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Artifact not found"));
}

#[tokio::test]
async fn copy_to_bucket_target_not_found() {
    let (service, artifact_repo, _bucket_repo) = create_service();

    let artifact = create_test_artifact();
    let artifact_id = artifact.id.clone();
    artifact_repo.add_artifact(artifact).await;

    let target_id = ArtifactBucketId::new();
    let result = service
        .copy_to_bucket(&artifact_id, &target_id, "user")
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Bucket not found"));
}

#[tokio::test]
async fn copy_to_bucket_type_not_accepted() {
    let (service, artifact_repo, bucket_repo) = create_service();

    let artifact = create_test_artifact(); // PRD type
    let artifact_id = artifact.id.clone();
    artifact_repo.add_artifact(artifact).await;

    let target_bucket = create_code_bucket(); // Only accepts code types
    bucket_repo.add_bucket(target_bucket.clone()).await;

    let result = service
        .copy_to_bucket(&artifact_id, &target_bucket.id, "worker")
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("does not accept"));
}

#[tokio::test]
async fn copy_to_bucket_copier_not_allowed() {
    let (service, artifact_repo, bucket_repo) = create_service();

    let artifact = Artifact::new_inline("Code", ArtifactType::CodeChange, "code", "worker");
    let artifact_id = artifact.id.clone();
    artifact_repo.add_artifact(artifact).await;

    let target_bucket = create_code_bucket(); // Only worker can write
    bucket_repo.add_bucket(target_bucket.clone()).await;

    let result = service
        .copy_to_bucket(&artifact_id, &target_bucket.id, "user")
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cannot write"));
}

// ==================== version_artifact Tests ====================

#[tokio::test]
async fn version_artifact_success() {
    let (service, artifact_repo, _bucket_repo) = create_service();

    let artifact = create_test_artifact();
    let artifact_id = artifact.id.clone();
    artifact_repo.add_artifact(artifact).await;

    let new_content = ArtifactContent::inline("Updated PRD content");
    let result = service
        .version_artifact(&artifact_id, new_content, "user")
        .await;

    assert!(result.is_ok());
    let versioned = result.unwrap();
    assert_ne!(versioned.id, artifact_id);
    assert_eq!(versioned.metadata.version, 2);
    assert_eq!(versioned.derived_from.len(), 1);
    assert_eq!(versioned.derived_from[0], artifact_id);
}

#[tokio::test]
async fn version_artifact_not_found() {
    let (service, _artifact_repo, _bucket_repo) = create_service();

    let artifact_id = ArtifactId::new();
    let new_content = ArtifactContent::inline("content");
    let result = service
        .version_artifact(&artifact_id, new_content, "user")
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[tokio::test]
async fn version_artifact_preserves_bucket() {
    let (service, artifact_repo, bucket_repo) = create_service();

    let bucket = create_prd_bucket();
    bucket_repo.add_bucket(bucket.clone()).await;

    let artifact = create_test_artifact().with_bucket(bucket.id.clone());
    let artifact_id = artifact.id.clone();
    artifact_repo.add_artifact(artifact).await;

    let new_content = ArtifactContent::inline("Updated content");
    let result = service
        .version_artifact(&artifact_id, new_content, "user")
        .await;

    assert!(result.is_ok());
    let versioned = result.unwrap();
    assert_eq!(versioned.bucket_id, Some(bucket.id));
}

#[tokio::test]
async fn version_artifact_preserves_task_association() {
    let (service, artifact_repo, _bucket_repo) = create_service();

    let task_id = TaskId::from_string("task-123".to_string());
    let artifact = create_test_artifact().with_task(task_id.clone());
    let artifact_id = artifact.id.clone();
    artifact_repo.add_artifact(artifact).await;

    let new_content = ArtifactContent::inline("Updated");
    let result = service
        .version_artifact(&artifact_id, new_content, "user")
        .await;

    assert!(result.is_ok());
    let versioned = result.unwrap();
    assert_eq!(versioned.metadata.task_id, Some(task_id));
}

#[tokio::test]
async fn version_artifact_increments_version() {
    let (service, artifact_repo, _bucket_repo) = create_service();

    // Create v1
    let artifact = create_test_artifact();
    let artifact_id = artifact.id.clone();
    artifact_repo.add_artifact(artifact).await;

    // Create v2
    let v2 = service
        .version_artifact(&artifact_id, ArtifactContent::inline("v2"), "user")
        .await
        .unwrap();
    assert_eq!(v2.metadata.version, 2);

    // Create v3
    let v3 = service
        .version_artifact(&v2.id, ArtifactContent::inline("v3"), "user")
        .await
        .unwrap();
    assert_eq!(v3.metadata.version, 3);
}

// ==================== get_buckets Tests ====================

#[tokio::test]
async fn get_buckets_empty() {
    let (service, _artifact_repo, _bucket_repo) = create_service();

    let result = service.get_buckets().await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn get_buckets_returns_all() {
    let (service, _artifact_repo, bucket_repo) = create_service();

    bucket_repo.add_bucket(create_prd_bucket()).await;
    bucket_repo.add_bucket(create_code_bucket()).await;

    let result = service.get_buckets().await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 2);
}

// ==================== get_bucket Tests ====================

#[tokio::test]
async fn get_bucket_found() {
    let (service, _artifact_repo, bucket_repo) = create_service();

    let bucket = create_prd_bucket();
    let id = bucket.id.clone();
    bucket_repo.add_bucket(bucket).await;

    let result = service.get_bucket(&id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_some());
}

#[tokio::test]
async fn get_bucket_not_found() {
    let (service, _artifact_repo, _bucket_repo) = create_service();

    let id = ArtifactBucketId::new();
    let result = service.get_bucket(&id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

// ==================== add_relation Tests ====================

#[tokio::test]
async fn add_relation_success() {
    let (service, artifact_repo, _bucket_repo) = create_service();

    let artifact1 = create_test_artifact();
    let artifact2 = Artifact::new_inline("Second", ArtifactType::Specification, "spec", "user");
    let id1 = artifact1.id.clone();
    let id2 = artifact2.id.clone();
    artifact_repo.add_artifact(artifact1).await;
    artifact_repo.add_artifact(artifact2).await;

    let result = service
        .add_relation(id1.clone(), id2.clone(), ArtifactRelationType::RelatedTo)
        .await;

    assert!(result.is_ok());
    let relation = result.unwrap();
    assert_eq!(relation.from_artifact_id, id1);
    assert_eq!(relation.to_artifact_id, id2);
    assert_eq!(relation.relation_type, ArtifactRelationType::RelatedTo);
}

#[tokio::test]
async fn add_relation_from_not_found() {
    let (service, artifact_repo, _bucket_repo) = create_service();

    let artifact = create_test_artifact();
    let id = artifact.id.clone();
    artifact_repo.add_artifact(artifact).await;

    let nonexistent = ArtifactId::new();
    let result = service
        .add_relation(nonexistent, id, ArtifactRelationType::DerivedFrom)
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[tokio::test]
async fn add_relation_to_not_found() {
    let (service, artifact_repo, _bucket_repo) = create_service();

    let artifact = create_test_artifact();
    let id = artifact.id.clone();
    artifact_repo.add_artifact(artifact).await;

    let nonexistent = ArtifactId::new();
    let result = service
        .add_relation(id, nonexistent, ArtifactRelationType::DerivedFrom)
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

// ==================== get_derived_from Tests ====================

#[tokio::test]
async fn get_derived_from_empty() {
    let (service, artifact_repo, _bucket_repo) = create_service();

    let artifact = create_test_artifact();
    let id = artifact.id.clone();
    artifact_repo.add_artifact(artifact).await;

    let result = service.get_derived_from(&id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

// ==================== get_related Tests ====================

#[tokio::test]
async fn get_related_empty() {
    let (service, artifact_repo, _bucket_repo) = create_service();

    let artifact = create_test_artifact();
    let id = artifact.id.clone();
    artifact_repo.add_artifact(artifact).await;

    let result = service.get_related(&id).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

// ==================== Content Handling Tests ====================

#[tokio::test]
async fn create_artifact_with_inline_content() {
    let (service, _artifact_repo, _bucket_repo) = create_service();

    let artifact = Artifact::new_inline(
        "Inline Test",
        ArtifactType::Prd,
        "This is inline content",
        "user",
    );
    let result = service.create_artifact(artifact, "user").await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert!(created.content.is_inline());
}

#[tokio::test]
async fn create_artifact_with_file_content() {
    let (service, _artifact_repo, _bucket_repo) = create_service();

    let artifact = Artifact::new_file(
        "File Test",
        ArtifactType::Prd,
        "/path/to/document.md",
        "user",
    );
    let result = service.create_artifact(artifact, "user").await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert!(created.content.is_file());
}

#[tokio::test]
async fn version_artifact_changes_content_type() {
    let (service, artifact_repo, _bucket_repo) = create_service();

    // Start with inline content
    let artifact = Artifact::new_inline("Test", ArtifactType::Prd, "inline", "user");
    let artifact_id = artifact.id.clone();
    artifact_repo.add_artifact(artifact).await;

    // Version with file content
    let file_content = ArtifactContent::file("/path/to/file.md");
    let result = service
        .version_artifact(&artifact_id, file_content, "user")
        .await;

    assert!(result.is_ok());
    let versioned = result.unwrap();
    assert!(versioned.content.is_file());
}

// Memory-based ArtifactRepository implementation for testing
// Uses RwLock<HashMap> for thread-safe storage

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use async_trait::async_trait;
use chrono::Utc;

use crate::domain::entities::{
    Artifact, ArtifactBucketId, ArtifactId, ArtifactRelation, ArtifactRelationType, ArtifactType,
    ProcessId, TaskId,
};
use crate::domain::repositories::ArtifactRepository;
use crate::error::AppResult;

/// In-memory implementation of ArtifactRepository for testing
pub struct MemoryArtifactRepository {
    artifacts: Arc<RwLock<HashMap<ArtifactId, Artifact>>>,
    relations: Arc<RwLock<HashMap<String, ArtifactRelation>>>,
    /// Fault injection for the latest-version resolver. Callers that must not silently fall
    /// back to a stale seed id need the failure to be reachable in a test.
    resolve_latest_failure: Arc<RwLock<Option<String>>>,
}

impl Default for MemoryArtifactRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryArtifactRepository {
    pub fn new() -> Self {
        Self {
            artifacts: Arc::new(RwLock::new(HashMap::new())),
            relations: Arc::new(RwLock::new(HashMap::new())),
            resolve_latest_failure: Arc::new(RwLock::new(None)),
        }
    }

    /// Make `resolve_latest_artifact_id` fail with `message`.
    pub async fn fail_resolve_latest_artifact_id(&self, message: impl Into<String>) {
        *self.resolve_latest_failure.write().await = Some(message.into());
    }

    pub fn with_artifacts(artifacts: Vec<Artifact>) -> Self {
        let map: HashMap<ArtifactId, Artifact> =
            artifacts.into_iter().map(|a| (a.id.clone(), a)).collect();
        Self {
            artifacts: Arc::new(RwLock::new(map)),
            relations: Arc::new(RwLock::new(HashMap::new())),
            resolve_latest_failure: Arc::new(RwLock::new(None)),
        }
    }
}

#[async_trait]
impl ArtifactRepository for MemoryArtifactRepository {
    async fn create(&self, artifact: Artifact) -> AppResult<Artifact> {
        let mut artifacts = self.artifacts.write().await;
        artifacts.insert(artifact.id.clone(), artifact.clone());
        Ok(artifact)
    }

    async fn get_by_id(&self, id: &ArtifactId) -> AppResult<Option<Artifact>> {
        let artifacts = self.artifacts.read().await;
        Ok(artifacts.get(id).cloned())
    }

    async fn get_by_id_at_version(
        &self,
        id: &ArtifactId,
        version: u32,
    ) -> AppResult<Option<Artifact>> {
        let artifacts = self.artifacts.read().await;
        if let Some(artifact) = artifacts.get(id) {
            // For memory implementation, just return the artifact if the version matches
            // In a real implementation, this would traverse version history
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
        let artifacts = self.artifacts.read().await;
        Ok(artifacts
            .values()
            .filter(|a| a.bucket_id.as_ref() == Some(bucket_id))
            .cloned()
            .collect())
    }

    async fn get_by_type(&self, artifact_type: ArtifactType) -> AppResult<Vec<Artifact>> {
        let artifacts = self.artifacts.read().await;
        Ok(artifacts
            .values()
            .filter(|a| a.artifact_type == artifact_type)
            .cloned()
            .collect())
    }

    async fn get_by_task(&self, task_id: &TaskId) -> AppResult<Vec<Artifact>> {
        let artifacts = self.artifacts.read().await;
        Ok(artifacts
            .values()
            .filter(|a| a.metadata.task_id.as_ref() == Some(task_id))
            .cloned()
            .collect())
    }

    async fn get_by_process(&self, process_id: &ProcessId) -> AppResult<Vec<Artifact>> {
        let artifacts = self.artifacts.read().await;
        Ok(artifacts
            .values()
            .filter(|a| a.metadata.process_id.as_ref() == Some(process_id))
            .cloned()
            .collect())
    }

    async fn update(&self, artifact: &Artifact) -> AppResult<()> {
        let mut artifacts = self.artifacts.write().await;
        artifacts.insert(artifact.id.clone(), artifact.clone());
        Ok(())
    }

    async fn delete(&self, id: &ArtifactId) -> AppResult<()> {
        let mut artifacts = self.artifacts.write().await;
        artifacts.remove(id);
        Ok(())
    }

    async fn get_derived_from(&self, artifact_id: &ArtifactId) -> AppResult<Vec<Artifact>> {
        let artifacts = self.artifacts.read().await;
        let relations = self.relations.read().await;

        let parent_ids: Vec<ArtifactId> = relations
            .values()
            .filter(|r| {
                r.from_artifact_id == *artifact_id
                    && r.relation_type == ArtifactRelationType::DerivedFrom
            })
            .map(|r| r.to_artifact_id.clone())
            .collect();

        Ok(artifacts
            .values()
            .filter(|a| parent_ids.contains(&a.id))
            .cloned()
            .collect())
    }

    async fn get_related(&self, artifact_id: &ArtifactId) -> AppResult<Vec<Artifact>> {
        let artifacts = self.artifacts.read().await;
        let relations = self.relations.read().await;

        let related_ids: Vec<ArtifactId> = relations
            .values()
            .filter(|r| r.from_artifact_id == *artifact_id || r.to_artifact_id == *artifact_id)
            .flat_map(|r| {
                if r.from_artifact_id == *artifact_id {
                    vec![r.to_artifact_id.clone()]
                } else {
                    vec![r.from_artifact_id.clone()]
                }
            })
            .collect();

        Ok(artifacts
            .values()
            .filter(|a| related_ids.contains(&a.id) && a.id != *artifact_id)
            .cloned()
            .collect())
    }

    async fn add_relation(&self, relation: ArtifactRelation) -> AppResult<ArtifactRelation> {
        let mut relations = self.relations.write().await;
        let key = format!("{}-{}", relation.from_artifact_id, relation.to_artifact_id);
        relations.insert(key, relation.clone());
        Ok(relation)
    }

    async fn get_relations(&self, artifact_id: &ArtifactId) -> AppResult<Vec<ArtifactRelation>> {
        let relations = self.relations.read().await;
        Ok(relations
            .values()
            .filter(|r| r.from_artifact_id == *artifact_id || r.to_artifact_id == *artifact_id)
            .cloned()
            .collect())
    }

    async fn get_relations_by_type(
        &self,
        artifact_id: &ArtifactId,
        relation_type: ArtifactRelationType,
    ) -> AppResult<Vec<ArtifactRelation>> {
        let relations = self.relations.read().await;
        Ok(relations
            .values()
            .filter(|r| {
                (r.from_artifact_id == *artifact_id || r.to_artifact_id == *artifact_id)
                    && r.relation_type == relation_type
            })
            .cloned()
            .collect())
    }

    async fn delete_relation(&self, from_id: &ArtifactId, to_id: &ArtifactId) -> AppResult<()> {
        let mut relations = self.relations.write().await;
        let key = format!("{}-{}", from_id, to_id);
        relations.remove(&key);
        Ok(())
    }

    async fn create_with_previous_version(
        &self,
        artifact: Artifact,
        _previous_version_id: ArtifactId,
    ) -> AppResult<Artifact> {
        // Memory implementation doesn't track previous_version_id, just create normally
        self.create(artifact).await
    }

    async fn get_version_history(
        &self,
        id: &ArtifactId,
    ) -> AppResult<Vec<crate::domain::repositories::ArtifactVersionSummary>> {
        // Memory implementation just returns single artifact summary if it exists
        let artifacts = self.artifacts.read().await;
        if let Some(artifact) = artifacts.get(id) {
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
        if let Some(message) = self.resolve_latest_failure.read().await.clone() {
            return Err(crate::error::AppError::Infrastructure(message));
        }
        Ok(id.clone())
    }

    async fn archive(&self, id: &ArtifactId) -> AppResult<Artifact> {
        let mut artifacts = self.artifacts.write().await;
        if let Some(artifact) = artifacts.get_mut(id) {
            artifact.archived_at = Some(Utc::now());
            Ok(artifact.clone())
        } else {
            Err(crate::error::AppError::NotFound(format!(
                "Artifact with id {} not found",
                id
            )))
        }
    }
}

#[cfg(test)]
#[path = "memory_artifact_repo_tests.rs"]
mod tests;

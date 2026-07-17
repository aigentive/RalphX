// Artifact repository trait - domain layer abstraction
//
// This trait defines the contract for artifact persistence.
// Implementations can use SQLite, PostgreSQL, in-memory, etc.

use async_trait::async_trait;

use crate::domain::entities::{
    Artifact, ArtifactBucketId, ArtifactId, ArtifactRelation, ArtifactRelationType, ArtifactType,
    ProcessId, TaskId,
};
use crate::error::AppResult;

/// Repository trait for Artifact persistence.
/// Implementations can use SQLite, in-memory, etc.
#[async_trait]
pub trait ArtifactRepository: Send + Sync {
    /// Create a new artifact
    async fn create(&self, artifact: Artifact) -> AppResult<Artifact>;

    /// Get artifact by ID
    async fn get_by_id(&self, id: &ArtifactId) -> AppResult<Option<Artifact>>;

    /// Get artifact at a specific version by traversing the version history
    /// Returns None if the artifact doesn't exist or the version is not found
    async fn get_by_id_at_version(
        &self,
        id: &ArtifactId,
        version: u32,
    ) -> AppResult<Option<Artifact>>;

    /// Get all artifacts in a bucket
    async fn get_by_bucket(&self, bucket_id: &ArtifactBucketId) -> AppResult<Vec<Artifact>>;

    /// Get all artifacts of a specific type
    async fn get_by_type(&self, artifact_type: ArtifactType) -> AppResult<Vec<Artifact>>;

    /// Get all artifacts for a task
    async fn get_by_task(&self, task_id: &TaskId) -> AppResult<Vec<Artifact>>;

    /// Get all artifacts for a process
    async fn get_by_process(&self, process_id: &ProcessId) -> AppResult<Vec<Artifact>>;

    /// Update an artifact
    async fn update(&self, artifact: &Artifact) -> AppResult<()>;

    /// Delete an artifact
    async fn delete(&self, id: &ArtifactId) -> AppResult<()>;

    /// Get all artifacts that this artifact was derived from
    async fn get_derived_from(&self, artifact_id: &ArtifactId) -> AppResult<Vec<Artifact>>;

    /// Get all related artifacts (both directions)
    async fn get_related(&self, artifact_id: &ArtifactId) -> AppResult<Vec<Artifact>>;

    /// Add a relation between two artifacts
    async fn add_relation(&self, relation: ArtifactRelation) -> AppResult<ArtifactRelation>;

    /// Get relations for an artifact
    async fn get_relations(&self, artifact_id: &ArtifactId) -> AppResult<Vec<ArtifactRelation>>;

    /// Get relations of a specific type for an artifact
    async fn get_relations_by_type(
        &self,
        artifact_id: &ArtifactId,
        relation_type: ArtifactRelationType,
    ) -> AppResult<Vec<ArtifactRelation>>;

    /// Delete a relation
    async fn delete_relation(&self, from_id: &ArtifactId, to_id: &ArtifactId) -> AppResult<()>;

    /// Create a new artifact with a link to the previous version
    /// This is used for version chaining in plan artifacts
    async fn create_with_previous_version(
        &self,
        artifact: Artifact,
        previous_version_id: ArtifactId,
    ) -> AppResult<Artifact>;

    /// Get version history for an artifact by walking the previous_version_id chain
    /// Returns summaries in order from newest to oldest
    async fn get_version_history(&self, id: &ArtifactId) -> AppResult<Vec<ArtifactVersionSummary>>;

    /// Walk the version chain forward from any artifact ID to find the latest version.
    /// Given A(v1) → B(v2) → C(v3): resolve_latest(A) → C, resolve_latest(C) → C.
    async fn resolve_latest_artifact_id(&self, id: &ArtifactId) -> AppResult<ArtifactId>;

    /// Archive an artifact (soft delete)
    async fn archive(&self, id: &ArtifactId) -> AppResult<Artifact>;
}

/// Summary of an artifact version for history display
#[derive(Debug, Clone)]
pub struct ArtifactVersionSummary {
    pub id: ArtifactId,
    pub version: u32,
    pub name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub created_by: String,
    pub metadata: Option<serde_json::Value>,
}

#[cfg(test)]
#[path = "artifact_repository_tests.rs"]
mod tests;

// Project repository trait - domain layer abstraction
//
// This trait defines the contract for project persistence.
// Implementations can use SQLite, PostgreSQL, in-memory, etc.

use async_trait::async_trait;

use crate::domain::entities::{Project, ProjectId};
use crate::error::AppResult;

/// Repository trait for Project persistence.
/// Implementations can use SQLite, PostgreSQL, in-memory, etc.
#[async_trait]
pub trait ProjectRepository: Send + Sync {
    /// Create a new project
    async fn create(&self, project: Project) -> AppResult<Project>;

    /// Get project by ID
    async fn get_by_id(&self, id: &ProjectId) -> AppResult<Option<Project>>;

    /// Get all projects
    async fn get_all(&self) -> AppResult<Vec<Project>>;

    /// Update a project
    async fn update(&self, project: &Project) -> AppResult<()>;

    /// Delete a project
    async fn delete(&self, id: &ProjectId) -> AppResult<()>;

    /// Delete a project with implementation-owned dependent-data cleanup.
    async fn delete_with_dependent_sweep(&self, id: &ProjectId) -> AppResult<()> {
        self.delete(id).await
    }

    /// Find project by working directory path
    async fn get_by_working_directory(&self, path: &str) -> AppResult<Option<Project>>;

    /// Archive a project (soft delete)
    async fn archive(&self, id: &ProjectId) -> AppResult<Project>;
}

#[cfg(test)]
#[path = "project_repository_tests.rs"]
mod tests;

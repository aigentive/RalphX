// Permission repository trait - domain layer abstraction for permission persistence
//
// This trait defines the contract for persisting pending permission requests.
// SQLite stores data for restart resilience + audit trail; in-memory channels remain for signaling.
// Records live in crate::domain::entities::permission_request.

use async_trait::async_trait;

use crate::domain::entities::permission_request::{PendingPermissionInfo, PermissionDecision};
use crate::error::AppResult;

/// Repository trait for pending permission persistence.
/// Implementations can use SQLite, in-memory, etc.
#[async_trait]
pub trait PermissionRepository: Send + Sync {
    /// Persist a new pending permission request
    async fn create_pending(&self, info: &PendingPermissionInfo) -> AppResult<()>;

    /// Mark a permission request as resolved with the given decision
    async fn resolve(&self, request_id: &str, decision: &PermissionDecision) -> AppResult<bool>;

    /// Get all currently pending permission requests
    async fn get_pending(&self) -> AppResult<Vec<PendingPermissionInfo>>;

    /// Get a single permission request by its request_id
    async fn get_by_request_id(&self, request_id: &str)
        -> AppResult<Option<PendingPermissionInfo>>;

    /// Expire all pending permission requests (e.g., on startup — agents that asked are gone)
    async fn expire_all_pending(&self) -> AppResult<u64>;

    /// Remove a permission record by request_id
    async fn remove(&self, request_id: &str) -> AppResult<bool>;
}

#[cfg(test)]
#[path = "permission_repository_tests.rs"]
mod tests;

use crate::domain::entities::{ApiKey, ApiKeyId, AuditLogEntry};
use crate::error::AppResult;
use async_trait::async_trait;

/// Parameters for atomic key rotation.
pub struct RotateKeyParams {
    /// The new API key to insert.
    pub new_key: ApiKey,
    /// Project IDs to associate with the new key (may be empty).
    pub project_ids: Vec<String>,
    /// The old key's ID — its grace_expires_at will be set atomically.
    pub old_key_id: ApiKeyId,
    /// The grace expiry timestamp for the old key (RFC3339 string).
    pub grace_expires_at: String,
}

/// Parameters for atomic key creation.
pub struct CreateKeyParams {
    /// The API key to insert.
    pub new_key: ApiKey,
    /// Project IDs to associate with the new key (may be empty).
    pub project_ids: Vec<String>,
}

/// Repository trait for ApiKey persistence
#[async_trait]
pub trait ApiKeyRepository: Send + Sync {
    /// Create a new API key record (key_hash must already be computed)
    async fn create(&self, api_key: ApiKey) -> AppResult<ApiKey>;

    /// List all non-revoked API keys
    async fn list(&self) -> AppResult<Vec<ApiKey>>;

    /// Get by ID
    async fn get_by_id(&self, id: &ApiKeyId) -> AppResult<Option<ApiKey>>;

    /// Get by SHA-256 hash of the raw key (for validation)
    async fn get_by_hash(&self, key_hash: &str) -> AppResult<Option<ApiKey>>;

    /// Revoke a key by setting revoked_at
    async fn revoke(&self, id: &ApiKeyId) -> AppResult<()>;

    /// Set grace_expires_at for key rotation (old key valid until this time)
    async fn set_grace_period(&self, id: &ApiKeyId, grace_expires_at: &str) -> AppResult<()>;

    /// Update last_used_at timestamp
    async fn update_last_used(&self, id: &ApiKeyId, timestamp: &str) -> AppResult<()>;

    /// Get all project IDs associated with a key
    async fn get_projects(&self, id: &ApiKeyId) -> AppResult<Vec<String>>;

    /// Replace project associations for a key (delete old, insert new)
    async fn set_projects(&self, id: &ApiKeyId, project_ids: &[String]) -> AppResult<()>;

    /// Insert an audit log entry
    async fn log_audit(
        &self,
        api_key_id: &str,
        tool_name: &str,
        project_id: Option<&str>,
        success: bool,
        latency_ms: Option<i64>,
    ) -> AppResult<()>;

    /// Retrieve audit log entries for a key (most recent first, default limit 100)
    async fn get_audit_log(
        &self,
        key_id: &str,
        limit: Option<i64>,
    ) -> AppResult<Vec<AuditLogEntry>>;

    /// Update the permissions bitmask for a key; returns Err if key not found
    async fn update_api_key_permissions(&self, key_id: &str, permissions: i64) -> AppResult<()>;

    /// Atomically rotate a key: create the new key, set its project associations,
    /// and set the old key's grace period — all in a single SQLite transaction.
    /// If any step fails the entire operation is rolled back.
    ///
    /// # Errors
    /// Returns `AppError::Database` if any of the underlying SQL operations fail.
    async fn rotate_key_atomic(&self, params: RotateKeyParams) -> AppResult<()>;

    /// Atomically create a key and set its project associations in a single
    /// SQLite transaction. If the project association step fails, the key
    /// insert is rolled back — no orphaned keys.
    ///
    /// # Errors
    /// Returns `AppError::Database` if any of the underlying SQL operations fail.
    async fn create_key_atomic(&self, params: CreateKeyParams) -> AppResult<ApiKey>;
}

// Repository seam for the client-side remote environment registry (§6.1).

use async_trait::async_trait;

use crate::domain::entities::remote_environment::{
    RemoteEnvironment, RemoteEnvironmentId, RemoteEnvironmentStatus,
};
use crate::error::AppResult;

/// Inputs for the transactional pairing upsert.
///
/// The repository owns row identity: on first pairing it mints the client-local id and
/// the Keychain `token_secret_ref`; on re-pairing an already-known `environment_id` it
/// keeps both and merges `url` into `candidate_urls` instead of inserting a second row.
#[derive(Debug, Clone)]
pub struct UpsertPairedEnvironment {
    /// Host-reported identity from the descriptor/pair response.
    pub environment_id: String,
    /// User-facing display name.
    pub name: String,
    /// The endpoint the pairing exchange just used.
    pub url: String,
    /// Scopes granted by the host, serialized as the protocol scope strings.
    pub scopes: Vec<ralphx_remote_protocol::Scope>,
    /// Protocol version the host reported.
    pub protocol_version: u32,
}

/// Client registry of paired remote environments.
///
/// All write methods are plain row writes; the staged add/remove ORDERING
/// (row → Keychain → activate, revoke → Keychain → row) is owned by the
/// application service, not the repository.
#[async_trait]
pub trait RemoteEnvironmentRepository: Send + Sync {
    /// Transactional upsert on `environment_id` (§6.1 dedup).
    ///
    /// Inserts a new row as `pending_add`, or — when the host identity already
    /// exists — merges `url` into `candidate_urls`, refreshes name/scopes/protocol
    /// version, and resets the row to `pending_add` for the staged re-pair.
    /// Exactly one row per host identity in both branches.
    async fn upsert_paired(&self, params: UpsertPairedEnvironment)
        -> AppResult<RemoteEnvironment>;

    async fn get(&self, id: &RemoteEnvironmentId) -> AppResult<Option<RemoteEnvironment>>;

    async fn get_by_environment_id(
        &self,
        environment_id: &str,
    ) -> AppResult<Option<RemoteEnvironment>>;

    async fn list(&self) -> AppResult<Vec<RemoteEnvironment>>;

    /// Sets the lifecycle status. Errors with `AppError::NotFound` when the row is gone.
    async fn set_status(
        &self,
        id: &RemoteEnvironmentId,
        status: RemoteEnvironmentStatus,
    ) -> AppResult<()>;

    /// Deletes the row. Deleting an absent row is a no-op (idempotent removal).
    async fn delete(&self, id: &RemoteEnvironmentId) -> AppResult<()>;

    async fn touch_last_connected(
        &self,
        id: &RemoteEnvironmentId,
        timestamp: &str,
    ) -> AppResult<()>;
}

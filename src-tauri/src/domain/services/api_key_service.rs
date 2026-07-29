// Domain service for API key management with source-aware defaults.
//
// Centralizes key creation, rotation, and revocation logic so that
// both the HTTP API and the Tauri settings UI share the same rules.

use crate::domain::entities::api_key::{ApiKey, PERMISSION_MAX};
use crate::domain::entities::types::ApiKeyId;
use crate::domain::repositories::api_key_repository::{
    ApiKeyRepository, CreateKeyParams, RotateKeyParams,
};
use crate::domain::services::key_crypto::{generate_raw_key, hash_key, key_prefix};
use crate::error::{AppError, AppResult};

/// ISO 8601 / RFC 3339 timestamp format used throughout this module.
const TIMESTAMP_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";

/// Identifies the caller context so that appropriate permission defaults are applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    /// Key created via the native settings UI (trusted context — gets full read/write/admin).
    SettingsUi,
    /// Key created via the HTTP API (less-trusted context — gets read/write only).
    HttpApi,
}

impl KeySource {
    /// Default permission bitmask when the caller does not specify permissions explicitly.
    pub fn default_permissions(self) -> i32 {
        match self {
            KeySource::SettingsUi => 7, // read + write + admin
            KeySource::HttpApi => 3,    // read + write
        }
    }

    /// Human-readable source name written into audit log entries.
    pub fn audit_source_name(self) -> &'static str {
        match self {
            KeySource::SettingsUi => "settings_ui",
            KeySource::HttpApi => "http_api",
        }
    }
}

/// The result of a successful key creation or rotation — includes the one-time raw key.
#[derive(Debug)]
pub struct ApiKeyCreated {
    /// The persisted key record (raw key is NOT present — only hash/prefix).
    pub key: ApiKey,
    /// The full raw key value. Must be shown to the user exactly once.
    pub raw_key: String,
}

/// Pure-domain service: no I/O state — all methods accept a repo reference.
pub struct ApiKeyService;

impl ApiKeyService {
    /// Create a new API key.
    ///
    /// `permissions` defaults to `source.default_permissions()` when `None`.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Validation`] if `permissions` is outside `0..=PERMISSION_MAX`.
    /// Returns [`AppError::Database`] if the repository call fails.
    pub async fn create_key(
        repo: &dyn ApiKeyRepository,
        name: &str,
        permissions: Option<i32>,
        project_ids: &[String],
        source: KeySource,
    ) -> AppResult<ApiKeyCreated> {
        let permissions = permissions.unwrap_or_else(|| source.default_permissions());
        if !(0..=PERMISSION_MAX).contains(&permissions) {
            return Err(AppError::Validation(format!(
                "permissions must be between 0 and {}, got {}",
                PERMISSION_MAX, permissions
            )));
        }

        let raw_key = generate_raw_key();
        let key_hash = hash_key(&raw_key);
        let prefix = key_prefix(&raw_key);
        let now = chrono::Utc::now().format(TIMESTAMP_FORMAT).to_string();

        let new_key = ApiKey {
            id: ApiKeyId::new(),
            name: name.to_string(),
            key_hash,
            key_prefix: prefix,
            permissions,
            created_at: now,
            revoked_at: None,
            last_used_at: None,
            grace_expires_at: None,
            metadata: None,
        };

        let created = repo
            .create_key_atomic(CreateKeyParams {
                new_key,
                project_ids: project_ids.to_vec(),
            })
            .await?;

        let key_id = created.id.as_str().to_string();
        let _ = repo
            .log_audit(&key_id, source.audit_source_name(), None, true, None)
            .await;

        Ok(ApiKeyCreated {
            key: created,
            raw_key,
        })
    }

    /// Rotate an existing key.
    ///
    /// The old key receives a 60-second grace period so in-flight requests
    /// using it continue to succeed during the transition.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::NotFound`] if no key with `key_id` exists.
    /// Returns [`AppError::Validation`] if the key has already been revoked (outside grace period).
    /// Returns [`AppError::Database`] if the repository call fails.
    pub async fn rotate_key(
        repo: &dyn ApiKeyRepository,
        key_id: &str,
        source: KeySource,
    ) -> AppResult<ApiKeyCreated> {
        let old_key_id = ApiKeyId::from_string(key_id);

        let old_key = repo
            .get_by_id(&old_key_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("API key not found: {}", key_id)))?;

        if old_key.revoked_at.is_some() && !old_key.is_in_grace_period() {
            return Err(AppError::Validation(format!(
                "API key {} has been revoked and cannot be rotated",
                key_id
            )));
        }

        let raw_key = generate_raw_key();
        let new_key_hash = hash_key(&raw_key);
        let new_prefix = key_prefix(&raw_key);
        let now = chrono::Utc::now();
        let grace_expires_at = (now + chrono::Duration::seconds(60))
            .format(TIMESTAMP_FORMAT)
            .to_string();
        let now_str = now.format(TIMESTAMP_FORMAT).to_string();

        let project_ids = repo
            .get_projects(&old_key_id)
            .await
            .unwrap_or_default();

        let new_key = ApiKey {
            id: ApiKeyId::new(),
            name: old_key.name.clone(),
            key_hash: new_key_hash,
            key_prefix: new_prefix,
            permissions: old_key.permissions,
            created_at: now_str,
            revoked_at: None,
            last_used_at: None,
            grace_expires_at: None,
            metadata: None,
        };

        let new_key_id = new_key.id.clone();

        repo.rotate_key_atomic(RotateKeyParams {
            new_key: new_key.clone(),
            project_ids,
            old_key_id,
            grace_expires_at,
        })
        .await?;

        let _ = repo
            .log_audit(new_key_id.as_str(), source.audit_source_name(), None, true, None)
            .await;

        Ok(ApiKeyCreated {
            key: new_key,
            raw_key,
        })
    }

    /// Revoke a key immediately.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Database`] if the repository call fails.
    pub async fn revoke_key(
        repo: &dyn ApiKeyRepository,
        key_id: &str,
        source: KeySource,
    ) -> AppResult<()> {
        let id = ApiKeyId::from_string(key_id);
        repo.revoke(&id).await?;
        let _ = repo
            .log_audit(key_id, source.audit_source_name(), None, true, None)
            .await;
        Ok(())
    }
}

#[cfg(test)]
#[path = "api_key_service_tests.rs"]
mod tests;

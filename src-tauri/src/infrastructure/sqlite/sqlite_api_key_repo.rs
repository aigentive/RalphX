// SQLite-based ApiKeyRepository implementation for production use
// Uses rusqlite with connection pooling for thread-safe access

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use rusqlite::Connection;

use super::DbConnection;
use crate::domain::entities::{ApiKey, ApiKeyId, AuditLogEntry};
use crate::domain::repositories::{ApiKeyRepository, CreateKeyParams, RotateKeyParams};
use crate::error::{AppError, AppResult};

/// SQLite implementation of ApiKeyRepository for production use
/// Uses a mutex-protected connection for thread-safe access
pub struct SqliteApiKeyRepository {
    db: DbConnection,
}

impl SqliteApiKeyRepository {
    /// Create a new SQLite API key repository with the given connection
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }

    /// Create from an Arc-wrapped mutex connection (for sharing)
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }

    /// Parse an ApiKey from a database row
    fn api_key_from_row(row: &rusqlite::Row<'_>) -> Result<ApiKey, rusqlite::Error> {
        Ok(ApiKey {
            id: ApiKeyId::from_string(row.get::<_, String>(0)?),
            name: row.get(1)?,
            key_hash: row.get(2)?,
            key_prefix: row.get(3)?,
            permissions: row.get(4)?,
            created_at: row.get(5)?,
            revoked_at: row.get(6)?,
            last_used_at: row.get(7)?,
            grace_expires_at: row.get(8)?,
            metadata: row.get(9)?,
        })
    }
}

/// Insert a single api_keys row. Caller must be inside a db.run() or db.run_transaction() closure.
fn insert_key_row(conn: &Connection, key: &ApiKey) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO api_keys (id, name, key_hash, key_prefix, permissions, created_at, revoked_at, last_used_at, grace_expires_at, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            key.id.as_str(),
            &key.name,
            &key.key_hash,
            &key.key_prefix,
            key.permissions,
            &key.created_at,
            &key.revoked_at,
            &key.last_used_at,
            &key.grace_expires_at,
            &key.metadata,
        ],
    )?;
    Ok(())
}

/// Insert api_key_projects rows for the given key_id/project_ids pairs.
/// Caller must be inside a db.run() or db.run_transaction() closure.
fn insert_project_associations(
    conn: &Connection,
    key_id: &str,
    project_ids: &[String],
) -> Result<(), rusqlite::Error> {
    for project_id in project_ids {
        conn.execute(
            "INSERT INTO api_key_projects (api_key_id, project_id) VALUES (?1, ?2)",
            rusqlite::params![key_id, project_id],
        )?;
    }
    Ok(())
}

#[async_trait]
impl ApiKeyRepository for SqliteApiKeyRepository {
    async fn create(&self, api_key: ApiKey) -> AppResult<ApiKey> {
        self.db
            .run(move |conn| {
                insert_key_row(conn, &api_key)?;
                Ok(api_key)
            })
            .await
    }

    async fn list(&self) -> AppResult<Vec<ApiKey>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, name, key_hash, key_prefix, permissions, created_at, revoked_at, last_used_at, grace_expires_at, metadata
                     FROM api_keys WHERE revoked_at IS NULL ORDER BY created_at DESC",
                )?;
                let keys = stmt
                    .query_map([], SqliteApiKeyRepository::api_key_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(keys)
            })
            .await
    }

    async fn get_by_id(&self, id: &ApiKeyId) -> AppResult<Option<ApiKey>> {
        let id = id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, name, key_hash, key_prefix, permissions, created_at, revoked_at, last_used_at, grace_expires_at, metadata
                     FROM api_keys WHERE id = ?1",
                    [id.as_str()],
                    SqliteApiKeyRepository::api_key_from_row,
                )
            })
            .await
    }

    async fn get_by_hash(&self, key_hash: &str) -> AppResult<Option<ApiKey>> {
        let key_hash = key_hash.to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, name, key_hash, key_prefix, permissions, created_at, revoked_at, last_used_at, grace_expires_at, metadata
                     FROM api_keys WHERE key_hash = ?1",
                    [key_hash.as_str()],
                    SqliteApiKeyRepository::api_key_from_row,
                )
            })
            .await
    }

    async fn revoke(&self, id: &ApiKeyId) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE api_keys SET revoked_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1",
                    [id.as_str()],
                )?;
                Ok(())
            })
            .await
    }

    async fn set_grace_period(&self, id: &ApiKeyId, grace_expires_at: &str) -> AppResult<()> {
        let id = id.as_str().to_string();
        let grace_expires_at = grace_expires_at.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE api_keys SET grace_expires_at = ?2 WHERE id = ?1",
                    rusqlite::params![id, grace_expires_at],
                )?;
                Ok(())
            })
            .await
    }

    async fn update_last_used(&self, id: &ApiKeyId, timestamp: &str) -> AppResult<()> {
        let id = id.as_str().to_string();
        let timestamp = timestamp.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE api_keys SET last_used_at = ?2 WHERE id = ?1",
                    rusqlite::params![id, timestamp],
                )?;
                Ok(())
            })
            .await
    }

    async fn get_projects(&self, id: &ApiKeyId) -> AppResult<Vec<String>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT project_id FROM api_key_projects WHERE api_key_id = ?1",
                )?;
                let project_ids = stmt
                    .query_map([id.as_str()], |row| row.get(0))?
                    .collect::<Result<Vec<String>, _>>()?;
                Ok(project_ids)
            })
            .await
    }

    async fn set_projects(&self, id: &ApiKeyId, project_ids: &[String]) -> AppResult<()> {
        let id = id.as_str().to_string();
        let project_ids = project_ids.to_vec();
        self.db
            .run_transaction(move |conn| {
                conn.execute(
                    "DELETE FROM api_key_projects WHERE api_key_id = ?1",
                    [id.as_str()],
                )?;
                insert_project_associations(conn, &id, &project_ids)?;
                Ok(())
            })
            .await
    }

    async fn log_audit(
        &self,
        api_key_id: &str,
        tool_name: &str,
        project_id: Option<&str>,
        success: bool,
        latency_ms: Option<i64>,
    ) -> AppResult<()> {
        let api_key_id = api_key_id.to_string();
        let tool_name = tool_name.to_string();
        let project_id = project_id.map(|s| s.to_string());
        let success_int = if success { 1i64 } else { 0i64 };

        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO api_audit_log (api_key_id, tool_name, project_id, success, latency_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![
                        api_key_id,
                        tool_name,
                        project_id,
                        success_int,
                        latency_ms,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn get_audit_log(
        &self,
        key_id: &str,
        limit: Option<i64>,
    ) -> AppResult<Vec<AuditLogEntry>> {
        let key_id = key_id.to_string();
        let limit = limit.unwrap_or(100);
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, api_key_id, tool_name, project_id, success, latency_ms, created_at
                     FROM api_audit_log WHERE api_key_id = ?1
                     ORDER BY created_at DESC LIMIT ?2",
                )?;
                let entries = stmt
                    .query_map(rusqlite::params![key_id, limit], |row| {
                        let success_int: i64 = row.get(4)?;
                        Ok(AuditLogEntry {
                            id: row.get(0)?,
                            api_key_id: row.get(1)?,
                            tool_name: row.get(2)?,
                            project_id: row.get(3)?,
                            success: success_int != 0,
                            latency_ms: row.get(5)?,
                            created_at: row.get(6)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(entries)
            })
            .await
    }

    async fn update_api_key_permissions(
        &self,
        key_id: &str,
        permissions: i64,
    ) -> AppResult<()> {
        let key_id = key_id.to_string();
        self.db
            .run(move |conn| {
                let rows_updated = conn.execute(
                    "UPDATE api_keys SET permissions = ?1 WHERE id = ?2",
                    rusqlite::params![permissions, key_id],
                )?;
                if rows_updated == 0 {
                    return Err(AppError::NotFound(format!(
                        "API key not found: {}",
                        key_id
                    )));
                }
                Ok(())
            })
            .await
    }

    async fn create_key_atomic(&self, params: CreateKeyParams) -> AppResult<ApiKey> {
        let new_key = params.new_key;
        let project_ids = params.project_ids;
        let returned_key = new_key.clone();

        self.db
            .run_transaction(move |conn| {
                insert_key_row(conn, &new_key)?;
                insert_project_associations(conn, new_key.id.as_str(), &project_ids)?;
                Ok(returned_key)
            })
            .await
    }

    async fn rotate_key_atomic(&self, params: RotateKeyParams) -> AppResult<()> {
        let new_key = params.new_key;
        let project_ids = params.project_ids;
        let old_key_id = params.old_key_id.as_str().to_string();
        let grace_expires_at = params.grace_expires_at;

        self.db
            .run_transaction(move |conn| {
                insert_key_row(conn, &new_key)?;
                insert_project_associations(conn, new_key.id.as_str(), &project_ids)?;

                // Revoke the old key and set its grace period atomically.
                // revoked_at marks the key as rotated; grace_expires_at allows it to
                // remain usable via is_in_grace_period() until the grace window elapses.
                conn.execute(
                    "UPDATE api_keys \
                     SET revoked_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), \
                         grace_expires_at = ?2 \
                     WHERE id = ?1",
                    rusqlite::params![old_key_id, grace_expires_at],
                )?;

                Ok(())
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_api_key_repo_tests.rs"]
mod tests;

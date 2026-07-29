// SQLite-based ArtifactBucketRepository implementation for production use
// Uses rusqlite with connection pooling for thread-safe access

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use rusqlite::Connection;

use super::DbConnection;
use crate::domain::entities::{ArtifactBucket, ArtifactBucketId, ArtifactType};
use crate::domain::repositories::ArtifactBucketRepository;
use crate::error::{AppError, AppResult};

/// Configuration stored in config_json column
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct BucketConfig {
    accepted_types: Vec<String>,
    writers: Vec<String>,
    readers: Vec<String>,
}

/// SQLite implementation of ArtifactBucketRepository for production use
/// Uses a mutex-protected connection for thread-safe access
pub struct SqliteArtifactBucketRepository {
    db: DbConnection,
}

impl SqliteArtifactBucketRepository {
    /// Create a new SQLite artifact bucket repository with the given connection
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

    /// Parse an ArtifactBucket from a database row
    fn bucket_from_row(row: &rusqlite::Row<'_>) -> Result<ArtifactBucket, rusqlite::Error> {
        let id: String = row.get(0)?;
        let name: String = row.get(1)?;
        let config_json: String = row.get(2)?;
        let is_system: i32 = row.get(3)?;

        let config: BucketConfig = serde_json::from_str(&config_json)
            .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;

        let accepted_types: Vec<ArtifactType> = config
            .accepted_types
            .iter()
            .filter_map(|s| s.parse().ok())
            .collect();

        Ok(ArtifactBucket {
            id: ArtifactBucketId::from_string(id),
            name,
            accepted_types,
            writers: config.writers,
            readers: config.readers,
            is_system: is_system != 0,
        })
    }

    /// Convert bucket to config JSON for storage
    fn bucket_to_config_json(bucket: &ArtifactBucket) -> AppResult<String> {
        let config = BucketConfig {
            accepted_types: bucket
                .accepted_types
                .iter()
                .map(|t| t.as_str().to_string())
                .collect(),
            writers: bucket.writers.clone(),
            readers: bucket.readers.clone(),
        };
        serde_json::to_string(&config)
            .map_err(|e| AppError::Database(format!("JSON serialization error: {}", e)))
    }

    /// Seeds the 4 built-in system buckets, creating or updating as needed.
    /// Returns the number of buckets seeded (created, not updated).
    pub async fn seed_builtin_buckets(&self) -> AppResult<usize> {
        let system_buckets = ArtifactBucket::system_buckets();
        let mut seeded_count = 0;

        for bucket in system_buckets {
            if self.exists(&bucket.id).await? {
                self.update(&bucket).await?;
            } else {
                self.create(bucket).await?;
                seeded_count += 1;
            }
        }

        Ok(seeded_count)
    }
}

#[async_trait]
impl ArtifactBucketRepository for SqliteArtifactBucketRepository {
    async fn create(&self, bucket: ArtifactBucket) -> AppResult<ArtifactBucket> {
        let config_json = Self::bucket_to_config_json(&bucket)?;
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO artifact_buckets (id, name, config_json, is_system)
                     VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![
                        bucket.id.as_str(),
                        bucket.name,
                        config_json,
                        if bucket.is_system { 1 } else { 0 },
                    ],
                )?;
                Ok(bucket)
            })
            .await
    }

    async fn get_by_id(&self, id: &ArtifactBucketId) -> AppResult<Option<ArtifactBucket>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let result = conn.query_row(
                    "SELECT id, name, config_json, is_system
                     FROM artifact_buckets WHERE id = ?1",
                    [id.as_str()],
                    SqliteArtifactBucketRepository::bucket_from_row,
                );
                match result {
                    Ok(bucket) => Ok(Some(bucket)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(AppError::Database(e.to_string())),
                }
            })
            .await
    }

    async fn get_all(&self) -> AppResult<Vec<ArtifactBucket>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, name, config_json, is_system
                     FROM artifact_buckets ORDER BY name ASC",
                )?;
                let buckets = stmt
                    .query_map([], SqliteArtifactBucketRepository::bucket_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(buckets)
            })
            .await
    }

    async fn get_system_buckets(&self) -> AppResult<Vec<ArtifactBucket>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, name, config_json, is_system
                     FROM artifact_buckets WHERE is_system = 1 ORDER BY name ASC",
                )?;
                let buckets = stmt
                    .query_map([], SqliteArtifactBucketRepository::bucket_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(buckets)
            })
            .await
    }

    async fn update(&self, bucket: &ArtifactBucket) -> AppResult<()> {
        let config_json = Self::bucket_to_config_json(bucket)?;
        let id = bucket.id.as_str().to_string();
        let name = bucket.name.clone();
        let is_system = bucket.is_system;
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE artifact_buckets SET name = ?2, config_json = ?3, is_system = ?4
                     WHERE id = ?1",
                    rusqlite::params![
                        id,
                        name,
                        config_json,
                        if is_system { 1 } else { 0 },
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn delete(&self, id: &ArtifactBucketId) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let is_system: Option<i32> = conn
                    .query_row(
                        "SELECT is_system FROM artifact_buckets WHERE id = ?1",
                        [id.as_str()],
                        |row| row.get(0),
                    )
                    .ok();

                if is_system == Some(1) {
                    return Err(AppError::Validation(
                        "Cannot delete system bucket".to_string(),
                    ));
                }

                conn.execute("DELETE FROM artifact_buckets WHERE id = ?1", [id.as_str()])?;
                Ok(())
            })
            .await
    }

    async fn exists(&self, id: &ArtifactBucketId) -> AppResult<bool> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let count: i32 = conn.query_row(
                    "SELECT COUNT(*) FROM artifact_buckets WHERE id = ?1",
                    [id.as_str()],
                    |row| row.get(0),
                )?;
                Ok(count > 0)
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_artifact_bucket_repo_tests.rs"]
mod tests;

// SQLite-based ArtifactRepository implementation for production use
// Uses rusqlite with connection pooling for thread-safe access

use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::Connection;

use crate::domain::entities::{
    artifact::TeamArtifactMetadata, Artifact, ArtifactBucketId, ArtifactContent, ArtifactId,
    ArtifactMetadata, ArtifactRelation, ArtifactRelationId, ArtifactRelationType, ArtifactType,
    ProcessId, TaskId,
};
use crate::domain::repositories::ArtifactRepository;
use crate::error::AppResult;
use crate::infrastructure::sqlite::DbConnection;

/// SQLite implementation of ArtifactRepository for production use
/// Uses a mutex-protected connection for thread-safe access
pub struct SqliteArtifactRepository {
    db: DbConnection,
}

impl SqliteArtifactRepository {
    // custom_metadata mirrors the shared metadata_json column and takes precedence over
    // team_metadata; future load-mutate-update flows must not rely on team_metadata alone.
    fn metadata_json(metadata: &ArtifactMetadata) -> AppResult<Option<String>> {
        metadata
            .custom_metadata
            .as_ref()
            .map(serde_json::to_string)
            .or_else(|| metadata.team_metadata.as_ref().map(serde_json::to_string))
            .transpose()
            .map_err(|error| crate::error::AppError::Validation(error.to_string()))
    }

    /// Create a new SQLite artifact repository with the given connection
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

    /// Parse an Artifact from a database row
    ///
    /// Expected column order:
    /// 0:id, 1:type, 2:name, 3:content_type, 4:content_text, 5:content_path,
    /// 6:bucket_id, 7:task_id, 8:process_id, 9:created_by, 10:version,
    /// 11:previous_version_id, 12:created_at, 13:metadata_json, 14:archived_at
    fn artifact_from_row(row: &rusqlite::Row<'_>) -> Result<Artifact, rusqlite::Error> {
        let id: String = row.get(0)?;
        let type_str: String = row.get(1)?;
        let name: String = row.get(2)?;
        let content_type: String = row.get(3)?;
        let content_text: Option<String> = row.get(4)?;
        let content_path: Option<String> = row.get(5)?;
        let bucket_id: Option<String> = row.get(6)?;
        let task_id: Option<String> = row.get(7)?;
        let process_id: Option<String> = row.get(8)?;
        let created_by: String = row.get(9)?;
        let version: i32 = row.get(10)?;
        let created_at_str: String = row.get(12)?;
        let metadata_json: Option<String> = row.get(13)?;
        let archived_at_str: Option<String> = row.get(14)?;

        // Parse artifact type
        let artifact_type = ArtifactType::from_str(&type_str)
            .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;

        // Parse content
        let content = match content_type.as_str() {
            "inline" => ArtifactContent::inline(content_text.unwrap_or_default()),
            "file" => ArtifactContent::file(content_path.unwrap_or_default()),
            _ => {
                return Err(rusqlite::Error::InvalidParameterName(format!(
                    "Unknown content type: {}",
                    content_type
                )))
            }
        };

        // Parse created_at
        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        // Parse team_metadata from metadata_json
        let team_metadata: Option<TeamArtifactMetadata> = metadata_json
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok());
        let custom_metadata = metadata_json
            .as_deref()
            .and_then(|json| serde_json::from_str(json).ok());

        // Build metadata
        let metadata = ArtifactMetadata {
            created_at,
            created_by,
            task_id: task_id.map(TaskId::from_string),
            process_id: process_id.map(ProcessId::from_string),
            version: version as u32,
            team_metadata,
            custom_metadata,
        };

        // Parse archived_at
        let archived_at = archived_at_str.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        });

        Ok(Artifact {
            id: ArtifactId::from_string(id),
            artifact_type,
            name,
            content,
            metadata,
            derived_from: vec![], // Loaded separately via relations
            bucket_id: bucket_id.map(ArtifactBucketId::from_string),
            archived_at,
        })
    }

    // ============================================================================
    // Sync helpers — pub(crate) methods containing SQL logic.
    // Part of the sync-helper pattern: batch callers (e.g., artifact HTTP handlers)
    // call these directly with &Connection inside a db.run_transaction() closure.
    // Async trait methods wrap these in db.run() for single-operation use.
    // ============================================================================

    /// Walk the version chain forward to find the latest artifact ID.
    pub(crate) fn resolve_latest_sync(conn: &Connection, id: &str) -> AppResult<String> {
        let mut current_id = id.to_string();
        loop {
            match conn.query_row(
                "SELECT id FROM artifacts WHERE previous_version_id = ?1",
                [current_id.as_str()],
                |row| row.get::<_, String>(0),
            ) {
                Ok(next_id) => current_id = next_id,
                Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(current_id),
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Fetch a single artifact by ID; returns None if not found.
    pub(crate) fn get_by_id_sync(conn: &Connection, id: &str) -> AppResult<Option<Artifact>> {
        match conn.query_row(
            "SELECT id, type, name, content_type, content_text, content_path,
                    bucket_id, task_id, process_id, created_by, version,
                    previous_version_id, created_at, metadata_json, archived_at
             FROM artifacts WHERE id = ?1",
            [id],
            Self::artifact_from_row,
        ) {
            Ok(artifact) => Ok(Some(artifact)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Insert a new artifact row (no previous_version_id).
    pub(crate) fn create_sync(conn: &Connection, artifact: Artifact) -> AppResult<Artifact> {
        let (content_type, content_text, content_path) = match &artifact.content {
            ArtifactContent::Inline { text } => ("inline", Some(text.clone()), None),
            ArtifactContent::File { path } => ("file", None, Some(path.clone())),
        };
        let created_at = artifact.metadata.created_at.to_rfc3339();
        let metadata_json = Self::metadata_json(&artifact.metadata)?;

        conn.execute(
            "INSERT INTO artifacts (id, type, name, content_type, content_text, content_path,
             bucket_id, task_id, process_id, created_by, version, created_at, metadata_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                artifact.id.as_str(),
                artifact.artifact_type.as_str(),
                artifact.name,
                content_type,
                content_text,
                content_path,
                artifact.bucket_id.as_ref().map(|b| b.as_str()),
                artifact.metadata.task_id.as_ref().map(|t| t.as_str()),
                artifact.metadata.process_id.as_ref().map(|p| p.as_str()),
                artifact.metadata.created_by,
                artifact.metadata.version as i32,
                created_at,
                metadata_json,
            ],
        )?;
        Ok(artifact)
    }

    /// Insert a new artifact row with a previous_version_id link (version chain).
    pub(crate) fn create_with_previous_version_sync(
        conn: &Connection,
        artifact: Artifact,
        previous_version_id: &str,
    ) -> AppResult<Artifact> {
        let (content_type, content_text, content_path) = match &artifact.content {
            ArtifactContent::Inline { text } => ("inline", Some(text.clone()), None),
            ArtifactContent::File { path } => ("file", None, Some(path.clone())),
        };
        let created_at = artifact.metadata.created_at.to_rfc3339();
        let metadata_json = Self::metadata_json(&artifact.metadata)?;

        conn.execute(
            "INSERT INTO artifacts (id, type, name, content_type, content_text, content_path,
             bucket_id, task_id, process_id, created_by, version, previous_version_id, created_at, metadata_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            rusqlite::params![
                artifact.id.as_str(),
                artifact.artifact_type.as_str(),
                artifact.name,
                content_type,
                content_text,
                content_path,
                artifact.bucket_id.as_ref().map(|b| b.as_str()),
                artifact.metadata.task_id.as_ref().map(|t| t.as_str()),
                artifact.metadata.process_id.as_ref().map(|p| p.as_str()),
                artifact.metadata.created_by,
                artifact.metadata.version as i32,
                previous_version_id,
                created_at,
                metadata_json,
            ],
        )?;
        Ok(artifact)
    }

    pub(crate) fn add_relation_sync(
        conn: &Connection,
        relation: ArtifactRelation,
    ) -> AppResult<ArtifactRelation> {
        conn.execute(
            "INSERT INTO artifact_relations (id, from_artifact_id, to_artifact_id, relation_type)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                relation.id.as_str(),
                relation.from_artifact_id.as_str(),
                relation.to_artifact_id.as_str(),
                relation.relation_type.as_str(),
            ],
        )?;
        Ok(relation)
    }

    /// Archive an artifact by setting its archived_at timestamp.
    pub(crate) fn archive_sync(conn: &Connection, id: &ArtifactId) -> AppResult<Artifact> {
        let now = Utc::now();
        conn.execute(
            "UPDATE artifacts SET archived_at = ?2 WHERE id = ?1 AND archived_at IS NULL",
            rusqlite::params![id.as_str(), now.to_rfc3339()],
        )?;
        let artifact = conn
            .query_row(
                "SELECT id, type, name, content_type, content_text, content_path,
                        bucket_id, task_id, process_id, created_by, version,
                        previous_version_id, created_at, metadata_json, archived_at
                 FROM artifacts WHERE id = ?1",
                [id.as_str()],
                Self::artifact_from_row,
            )
            .map_err(crate::error::AppError::from)?;
        Ok(artifact)
    }

    /// Parse an ArtifactRelation from a database row
    fn relation_from_row(row: &rusqlite::Row<'_>) -> Result<ArtifactRelation, rusqlite::Error> {
        let id: String = row.get(0)?;
        let from_id: String = row.get(1)?;
        let to_id: String = row.get(2)?;
        let rel_type_str: String = row.get(3)?;

        let relation_type = ArtifactRelationType::from_str(&rel_type_str)
            .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;

        Ok(ArtifactRelation {
            id: ArtifactRelationId::from_string(id),
            from_artifact_id: ArtifactId::from_string(from_id),
            to_artifact_id: ArtifactId::from_string(to_id),
            relation_type,
        })
    }
}

#[async_trait]
impl ArtifactRepository for SqliteArtifactRepository {
    async fn create(&self, artifact: Artifact) -> AppResult<Artifact> {
        self.db
            .run(move |conn| {
                let (content_type, content_text, content_path) = match &artifact.content {
                    ArtifactContent::Inline { text } => ("inline", Some(text.clone()), None),
                    ArtifactContent::File { path } => ("file", None, Some(path.clone())),
                };

                let created_at = artifact.metadata.created_at.to_rfc3339();
                let metadata_json = Self::metadata_json(&artifact.metadata)?;

                conn.execute(
                    "INSERT INTO artifacts (id, type, name, content_type, content_text, content_path,
                     bucket_id, task_id, process_id, created_by, version, created_at, metadata_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    rusqlite::params![
                        artifact.id.as_str(),
                        artifact.artifact_type.as_str(),
                        artifact.name,
                        content_type,
                        content_text,
                        content_path,
                        artifact.bucket_id.as_ref().map(|b| b.as_str()),
                        artifact.metadata.task_id.as_ref().map(|t| t.as_str()),
                        artifact.metadata.process_id.as_ref().map(|p| p.as_str()),
                        artifact.metadata.created_by,
                        artifact.metadata.version as i32,
                        created_at,
                        metadata_json,
                    ],
                )?;
                Ok(artifact)
            })
            .await
    }

    async fn get_by_id(&self, id: &ArtifactId) -> AppResult<Option<Artifact>> {
        let id = id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, type, name, content_type, content_text, content_path,
                            bucket_id, task_id, process_id, created_by, version,
                            previous_version_id, created_at, metadata_json, archived_at
                     FROM artifacts WHERE id = ?1",
                    [id.as_str()],
                    Self::artifact_from_row,
                )
            })
            .await
    }

    async fn get_by_id_at_version(
        &self,
        id: &ArtifactId,
        target_version: u32,
    ) -> AppResult<Option<Artifact>> {
        let mut current_id = id.clone();
        self.db
            .run(move |conn| loop {
                let result = conn.query_row(
                    "SELECT id, type, name, content_type, content_text, content_path,
                                bucket_id, task_id, process_id, created_by, version,
                                previous_version_id, created_at, metadata_json, archived_at
                         FROM artifacts WHERE id = ?1",
                    [current_id.as_str()],
                    |row| {
                        let version: u32 = row.get(10)?;
                        let previous_version_id: Option<String> = row.get(11)?;
                        Ok((version, previous_version_id, Self::artifact_from_row(row)?))
                    },
                );

                match result {
                    Ok((version, previous_version_id, artifact)) => {
                        if version == target_version {
                            return Ok(Some(artifact));
                        }
                        if version < target_version {
                            return Ok(None);
                        }
                        if let Some(prev_id) = previous_version_id {
                            current_id = ArtifactId::from_string(prev_id);
                        } else {
                            return Ok(None);
                        }
                    }
                    Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
                    Err(e) => return Err(e.into()),
                }
            })
            .await
    }

    async fn get_by_bucket(&self, bucket_id: &ArtifactBucketId) -> AppResult<Vec<Artifact>> {
        let bucket_id = bucket_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, type, name, content_type, content_text, content_path,
                            bucket_id, task_id, process_id, created_by, version,
                            previous_version_id, created_at, metadata_json, archived_at
                     FROM artifacts WHERE bucket_id = ?1 AND archived_at IS NULL
                     ORDER BY created_at DESC",
                )?;
                let artifacts = stmt
                    .query_map([bucket_id.as_str()], Self::artifact_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(artifacts)
            })
            .await
    }

    async fn get_by_type(&self, artifact_type: ArtifactType) -> AppResult<Vec<Artifact>> {
        let type_str = artifact_type.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, type, name, content_type, content_text, content_path,
                            bucket_id, task_id, process_id, created_by, version,
                            previous_version_id, created_at, metadata_json, archived_at
                     FROM artifacts WHERE type = ?1 AND archived_at IS NULL
                     ORDER BY created_at DESC",
                )?;
                let artifacts = stmt
                    .query_map([type_str.as_str()], Self::artifact_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(artifacts)
            })
            .await
    }

    async fn get_by_task(&self, task_id: &TaskId) -> AppResult<Vec<Artifact>> {
        let task_id = task_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, type, name, content_type, content_text, content_path,
                            bucket_id, task_id, process_id, created_by, version,
                            previous_version_id, created_at, metadata_json, archived_at
                     FROM artifacts WHERE task_id = ?1 AND archived_at IS NULL
                     ORDER BY created_at DESC",
                )?;
                let artifacts = stmt
                    .query_map([task_id.as_str()], Self::artifact_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(artifacts)
            })
            .await
    }

    async fn get_by_process(&self, process_id: &ProcessId) -> AppResult<Vec<Artifact>> {
        let process_id = process_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, type, name, content_type, content_text, content_path,
                            bucket_id, task_id, process_id, created_by, version,
                            previous_version_id, created_at, metadata_json, archived_at
                     FROM artifacts WHERE process_id = ?1 AND archived_at IS NULL
                     ORDER BY created_at DESC",
                )?;
                let artifacts = stmt
                    .query_map([process_id.as_str()], Self::artifact_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(artifacts)
            })
            .await
    }

    async fn update(&self, artifact: &Artifact) -> AppResult<()> {
        let artifact = artifact.clone();
        self.db
            .run(move |conn| {
                let (content_type, content_text, content_path) = match &artifact.content {
                    ArtifactContent::Inline { text } => ("inline", Some(text.clone()), None),
                    ArtifactContent::File { path } => ("file", None, Some(path.clone())),
                };

                let metadata_json = Self::metadata_json(&artifact.metadata)?;

                conn.execute(
                    "UPDATE artifacts SET type = ?2, name = ?3, content_type = ?4,
                     content_text = ?5, content_path = ?6, bucket_id = ?7, task_id = ?8,
                     process_id = ?9, created_by = ?10, version = ?11, metadata_json = ?12
                     WHERE id = ?1",
                    rusqlite::params![
                        artifact.id.as_str(),
                        artifact.artifact_type.as_str(),
                        artifact.name,
                        content_type,
                        content_text,
                        content_path,
                        artifact.bucket_id.as_ref().map(|b| b.as_str()),
                        artifact.metadata.task_id.as_ref().map(|t| t.as_str()),
                        artifact.metadata.process_id.as_ref().map(|p| p.as_str()),
                        artifact.metadata.created_by,
                        artifact.metadata.version as i32,
                        metadata_json,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn delete(&self, id: &ArtifactId) -> AppResult<()> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute("DELETE FROM artifacts WHERE id = ?1", [id.as_str()])?;
                Ok(())
            })
            .await
    }

    async fn get_derived_from(&self, artifact_id: &ArtifactId) -> AppResult<Vec<Artifact>> {
        let artifact_id = artifact_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT a.id, a.type, a.name, a.content_type, a.content_text, a.content_path,
                            a.bucket_id, a.task_id, a.process_id, a.created_by, a.version,
                            a.previous_version_id, a.created_at, a.metadata_json, a.archived_at
                     FROM artifacts a
                     INNER JOIN artifact_relations r ON a.id = r.to_artifact_id
                     WHERE r.from_artifact_id = ?1 AND r.relation_type = 'derived_from'
                     ORDER BY a.created_at DESC",
                )?;
                let artifacts = stmt
                    .query_map([artifact_id.as_str()], Self::artifact_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(artifacts)
            })
            .await
    }

    async fn get_related(&self, artifact_id: &ArtifactId) -> AppResult<Vec<Artifact>> {
        let artifact_id = artifact_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT DISTINCT a.id, a.type, a.name, a.content_type, a.content_text,
                            a.content_path, a.bucket_id, a.task_id, a.process_id, a.created_by,
                            a.version, a.previous_version_id, a.created_at, a.metadata_json, a.archived_at
                     FROM artifacts a
                     INNER JOIN artifact_relations r ON
                        (a.id = r.to_artifact_id AND r.from_artifact_id = ?1) OR
                        (a.id = r.from_artifact_id AND r.to_artifact_id = ?1)
                     WHERE r.relation_type = 'related_to' AND a.id != ?1
                     ORDER BY a.created_at DESC",
                )?;
                let artifacts = stmt
                    .query_map([artifact_id.as_str()], Self::artifact_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(artifacts)
            })
            .await
    }

    async fn add_relation(&self, relation: ArtifactRelation) -> AppResult<ArtifactRelation> {
        self.db
            .run(move |conn| Self::add_relation_sync(conn, relation))
            .await
    }

    async fn get_relations(&self, artifact_id: &ArtifactId) -> AppResult<Vec<ArtifactRelation>> {
        let artifact_id = artifact_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, from_artifact_id, to_artifact_id, relation_type
                     FROM artifact_relations
                     WHERE from_artifact_id = ?1 OR to_artifact_id = ?1
                     ORDER BY created_at DESC",
                )?;
                let relations = stmt
                    .query_map([artifact_id.as_str()], Self::relation_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(relations)
            })
            .await
    }

    async fn get_relations_by_type(
        &self,
        artifact_id: &ArtifactId,
        relation_type: ArtifactRelationType,
    ) -> AppResult<Vec<ArtifactRelation>> {
        let artifact_id = artifact_id.as_str().to_string();
        let rel_type_str = relation_type.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, from_artifact_id, to_artifact_id, relation_type
                     FROM artifact_relations
                     WHERE (from_artifact_id = ?1 OR to_artifact_id = ?1)
                       AND relation_type = ?2
                     ORDER BY created_at DESC",
                )?;
                let relations = stmt
                    .query_map(
                        rusqlite::params![artifact_id.as_str(), rel_type_str.as_str()],
                        Self::relation_from_row,
                    )?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(relations)
            })
            .await
    }

    async fn delete_relation(&self, from_id: &ArtifactId, to_id: &ArtifactId) -> AppResult<()> {
        let from_id = from_id.as_str().to_string();
        let to_id = to_id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM artifact_relations
                     WHERE from_artifact_id = ?1 AND to_artifact_id = ?2",
                    rusqlite::params![from_id.as_str(), to_id.as_str()],
                )?;
                Ok(())
            })
            .await
    }

    async fn create_with_previous_version(
        &self,
        artifact: Artifact,
        previous_version_id: ArtifactId,
    ) -> AppResult<Artifact> {
        self.db
            .run(move |conn| {
                let (content_type, content_text, content_path) = match &artifact.content {
                    ArtifactContent::Inline { text } => ("inline", Some(text.clone()), None),
                    ArtifactContent::File { path } => ("file", None, Some(path.clone())),
                };

                let created_at = artifact.metadata.created_at.to_rfc3339();
                let metadata_json = Self::metadata_json(&artifact.metadata)?;

                conn.execute(
                    "INSERT INTO artifacts (id, type, name, content_type, content_text, content_path,
                     bucket_id, task_id, process_id, created_by, version, previous_version_id, created_at, metadata_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                    rusqlite::params![
                        artifact.id.as_str(),
                        artifact.artifact_type.as_str(),
                        artifact.name,
                        content_type,
                        content_text,
                        content_path,
                        artifact.bucket_id.as_ref().map(|b| b.as_str()),
                        artifact.metadata.task_id.as_ref().map(|t| t.as_str()),
                        artifact.metadata.process_id.as_ref().map(|p| p.as_str()),
                        artifact.metadata.created_by,
                        artifact.metadata.version as i32,
                        previous_version_id.as_str(),
                        created_at,
                        metadata_json,
                    ],
                )?;
                Ok(artifact)
            })
            .await
    }

    async fn get_version_history(
        &self,
        id: &ArtifactId,
    ) -> AppResult<Vec<crate::domain::repositories::ArtifactVersionSummary>> {
        let mut current_id = id.clone();
        self.db
            .run(move |conn| {
                let mut history = Vec::new();
                loop {
                    let result = conn.query_row(
                        "SELECT id, version, name, previous_version_id, created_at,
                                created_by, metadata_json
                         FROM artifacts WHERE id = ?1",
                        [current_id.as_str()],
                        |row| {
                            let id_str: String = row.get(0)?;
                            let version: i32 = row.get(1)?;
                            let name: String = row.get(2)?;
                            let previous_version_id: Option<String> = row.get(3)?;
                            let created_at_str: String = row.get(4)?;
                            let created_by: String = row.get(5)?;
                            let metadata_json: Option<String> = row.get(6)?;

                            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                                .map(|dt| dt.with_timezone(&Utc))
                                .unwrap_or_else(|_| Utc::now());

                            Ok((
                                crate::domain::repositories::ArtifactVersionSummary {
                                    id: ArtifactId::from_string(id_str),
                                    version: version as u32,
                                    name,
                                    created_at,
                                    created_by,
                                    metadata: metadata_json
                                        .and_then(|value| serde_json::from_str(&value).ok()),
                                },
                                previous_version_id,
                            ))
                        },
                    );

                    match result {
                        Ok((summary, previous_version_id)) => {
                            history.push(summary);
                            if let Some(prev_id) = previous_version_id {
                                current_id = ArtifactId::from_string(prev_id);
                            } else {
                                break;
                            }
                        }
                        Err(rusqlite::Error::QueryReturnedNoRows) => break,
                        Err(e) => return Err(e.into()),
                    }
                }
                Ok(history)
            })
            .await
    }

    async fn resolve_latest_artifact_id(&self, id: &ArtifactId) -> AppResult<ArtifactId> {
        let mut current_id = id.clone();
        self.db
            .run(move |conn| loop {
                let result = conn.query_row(
                    "SELECT id FROM artifacts WHERE previous_version_id = ?1",
                    [current_id.as_str()],
                    |row| {
                        let next_id: String = row.get(0)?;
                        Ok(next_id)
                    },
                );

                match result {
                    Ok(next_id) => {
                        current_id = ArtifactId::from_string(next_id);
                    }
                    Err(rusqlite::Error::QueryReturnedNoRows) => {
                        return Ok(current_id);
                    }
                    Err(e) => return Err(e.into()),
                }
            })
            .await
    }

    async fn archive(&self, id: &ArtifactId) -> AppResult<Artifact> {
        let id = id.clone();
        self.db
            .run(move |conn| SqliteArtifactRepository::archive_sync(conn, &id))
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_artifact_repo_tests.rs"]
mod tests;

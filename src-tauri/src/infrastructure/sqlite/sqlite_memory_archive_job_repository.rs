// SQLite implementation of MemoryArchiveJobRepository (legacy)
// New code should use SqliteMemoryArchiveRepository

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    ArchiveJobPayload, ArchiveJobStatus, ArchiveJobType, MemoryArchiveJob, MemoryArchiveJobId,
};
use crate::domain::repositories::MemoryArchiveJobRepository;
use crate::error::{AppError, AppResult};

/// SQLite-backed memory archive job repository (legacy)
pub struct SqliteMemoryArchiveJobRepository {
    db: DbConnection,
}

impl SqliteMemoryArchiveJobRepository {
    /// Create a new repository with the given connection
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

    /// Helper to parse a row into a MemoryArchiveJob
    fn row_to_memory_archive_job(row: &rusqlite::Row) -> rusqlite::Result<MemoryArchiveJob> {
        let job_type_str: String = row.get(2)?;
        let job_type = job_type_str.parse::<ArchiveJobType>().map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e))
        })?;

        let payload_json_str: String = row.get(3)?;
        let payload = ArchiveJobPayload::from_json(&payload_json_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    e.to_string(),
                )),
            )
        })?;

        let status_str: String = row.get(4)?;
        let status = status_str.parse::<ArchiveJobStatus>().map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
        })?;

        let created_at_str: String = row.get(6)?;
        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    6,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;

        let updated_at_str: String = row.get(7)?;
        let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    7,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;

        let started_at: Option<DateTime<Utc>> = row
            .get::<_, Option<String>>(8)?
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let completed_at: Option<DateTime<Utc>> = row
            .get::<_, Option<String>>(9)?
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        Ok(MemoryArchiveJob {
            id: MemoryArchiveJobId::from(row.get::<_, String>(0)?),
            project_id: ProjectId::from_string(row.get::<_, String>(1)?),
            job_type,
            payload,
            status,
            error_message: row.get(5)?,
            created_at,
            updated_at,
            started_at,
            completed_at,
        })
    }
}

#[async_trait]
impl MemoryArchiveJobRepository for SqliteMemoryArchiveJobRepository {
    async fn create(&self, job: MemoryArchiveJob) -> AppResult<MemoryArchiveJob> {
        let payload_json = job.payload.to_json()?;

        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO memory_archive_jobs (
                id, project_id, job_type, payload_json, status, error_message,
                created_at, updated_at, started_at, completed_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    rusqlite::params![
                        job.id.0.as_str(),
                        job.project_id.as_str(),
                        job.job_type.to_string(),
                        payload_json,
                        job.status.to_string(),
                        job.error_message,
                        job.created_at.to_rfc3339(),
                        job.updated_at.to_rfc3339(),
                        job.started_at.as_ref().map(|dt| dt.to_rfc3339()),
                        job.completed_at.as_ref().map(|dt| dt.to_rfc3339()),
                    ],
                )?;
                Ok(job)
            })
            .await
    }

    async fn get_by_id(&self, id: &MemoryArchiveJobId) -> AppResult<Option<MemoryArchiveJob>> {
        let id_str = id.0.clone();

        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, project_id, job_type, payload_json, status, error_message,
                    created_at, updated_at, started_at, completed_at
             FROM memory_archive_jobs WHERE id = ?1",
                    [id_str.as_str()],
                    Self::row_to_memory_archive_job,
                )
            })
            .await
    }

    async fn get_pending_by_project(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Vec<MemoryArchiveJob>> {
        let project_id_str = project_id.as_str().to_string();

        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, project_id, job_type, payload_json, status, error_message,
                    created_at, updated_at, started_at, completed_at
             FROM memory_archive_jobs
             WHERE project_id = ?1 AND status = 'pending'
             ORDER BY created_at ASC",
                )?;

                let jobs = stmt
                    .query_map([project_id_str.as_str()], Self::row_to_memory_archive_job)?
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(jobs)
            })
            .await
    }

    async fn update_status(
        &self,
        id: &MemoryArchiveJobId,
        status: ArchiveJobStatus,
        error_message: Option<String>,
    ) -> AppResult<()> {
        let id_str = id.0.clone();

        self.db
            .run(move |conn| {
                let now = Utc::now().to_rfc3339();

                let (started_at, completed_at) = match status {
                    ArchiveJobStatus::Running => (Some(now.clone()), None),
                    ArchiveJobStatus::Done | ArchiveJobStatus::Failed => {
                        (None, Some(now.clone()))
                    }
                    ArchiveJobStatus::Pending => (None, None),
                };

                let affected = conn.execute(
                    "UPDATE memory_archive_jobs
             SET status = ?1,
                 error_message = ?2,
                 updated_at = ?3,
                 started_at = COALESCE(?4, started_at),
                 completed_at = COALESCE(?5, completed_at)
             WHERE id = ?6",
                    rusqlite::params![
                        status.to_string(),
                        error_message,
                        now,
                        started_at,
                        completed_at,
                        id_str.as_str(),
                    ],
                )?;

                if affected == 0 {
                    return Err(AppError::NotFound(format!(
                        "Archive job not found: {}",
                        id_str
                    )));
                }

                Ok(())
            })
            .await
    }
}

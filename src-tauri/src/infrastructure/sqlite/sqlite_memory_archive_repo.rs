// SQLite-based MemoryArchiveRepository implementation for production use
// Uses DbConnection for non-blocking SQLite access via spawn_blocking

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use rusqlite::Connection;

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{
    ArchiveJobPayload, ArchiveJobStatus, ArchiveJobType, MemoryArchiveJob, MemoryArchiveJobId,
};
use crate::domain::repositories::MemoryArchiveRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

/// SQLite implementation of MemoryArchiveRepository for production use
pub struct SqliteMemoryArchiveRepository {
    db: DbConnection,
}

impl SqliteMemoryArchiveRepository {
    /// Create a new SQLite memory archive repository with the given connection
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
}

/// Helper to deserialize a MemoryArchiveJob from a database row
fn job_from_row(row: &rusqlite::Row) -> rusqlite::Result<MemoryArchiveJob> {
    let id: String = row.get(0)?;
    let project_id: String = row.get(1)?;
    let job_type_str: String = row.get(2)?;
    let payload_json: String = row.get(3)?;
    let status_str: String = row.get(4)?;
    let error_message: Option<String> = row.get(5)?;
    let created_at_str: String = row.get(6)?;
    let updated_at_str: String = row.get(7)?;
    let started_at_str: Option<String> = row.get(8)?;
    let completed_at_str: Option<String> = row.get(9)?;

    let job_type = job_type_str
        .parse::<ArchiveJobType>()
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

    let status = status_str
        .parse::<ArchiveJobStatus>()
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

    let payload = ArchiveJobPayload::from_json(&payload_json)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;

    let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
        .with_timezone(&Utc);

    let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at_str)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
        .with_timezone(&Utc);

    let started_at = started_at_str
        .as_ref()
        .map(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
        })
        .transpose()?;

    let completed_at = completed_at_str
        .as_ref()
        .map(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
        })
        .transpose()?;

    Ok(MemoryArchiveJob {
        id: MemoryArchiveJobId::from(id),
        project_id: ProjectId::from_string(project_id),
        job_type,
        payload,
        status,
        error_message,
        created_at,
        updated_at,
        started_at,
        completed_at,
    })
}

#[async_trait]
impl MemoryArchiveRepository for SqliteMemoryArchiveRepository {
    async fn create(&self, job: MemoryArchiveJob) -> AppResult<MemoryArchiveJob> {
        let payload_json = job.payload.to_json()?;
        let id = job.id.0.clone();
        let project_id = job.project_id.as_str().to_string();
        let job_type = job.job_type.to_string();
        let status = job.status.to_string();
        let error_message = job.error_message.clone();
        let created_at = job.created_at.to_rfc3339();
        let updated_at = job.updated_at.to_rfc3339();
        let started_at = job.started_at.map(|dt| dt.to_rfc3339());
        let completed_at = job.completed_at.map(|dt| dt.to_rfc3339());

        self.db.run(move |conn| {
            conn.execute(
                "INSERT INTO memory_archive_jobs (id, project_id, job_type, payload_json, status, error_message, created_at, updated_at, started_at, completed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![id, project_id, job_type, payload_json, status, error_message, created_at, updated_at, started_at, completed_at],
            )?;
            Ok(())
        }).await?;

        Ok(job)
    }

    async fn get_by_id(&self, id: &MemoryArchiveJobId) -> AppResult<Option<MemoryArchiveJob>> {
        let id_str = id.0.clone();
        self.db.query_optional(move |conn| {
            conn.query_row(
                "SELECT id, project_id, job_type, payload_json, status, error_message, created_at, updated_at, started_at, completed_at
                 FROM memory_archive_jobs WHERE id = ?1",
                [&id_str],
                job_from_row,
            )
        }).await
    }

    async fn update(&self, job: &MemoryArchiveJob) -> AppResult<()> {
        let payload_json = job.payload.to_json()?;
        let id = job.id.0.clone();
        let project_id = job.project_id.as_str().to_string();
        let job_type = job.job_type.to_string();
        let status = job.status.to_string();
        let error_message = job.error_message.clone();
        let updated_at = job.updated_at.to_rfc3339();
        let started_at = job.started_at.map(|dt| dt.to_rfc3339());
        let completed_at = job.completed_at.map(|dt| dt.to_rfc3339());

        self.db.run(move |conn| {
            conn.execute(
                "UPDATE memory_archive_jobs
                 SET project_id = ?2, job_type = ?3, payload_json = ?4, status = ?5, error_message = ?6, updated_at = ?7, started_at = ?8, completed_at = ?9
                 WHERE id = ?1",
                rusqlite::params![id, project_id, job_type, payload_json, status, error_message, updated_at, started_at, completed_at],
            )?;
            Ok(())
        }).await
    }

    async fn delete(&self, id: &MemoryArchiveJobId) -> AppResult<()> {
        let id_str = id.0.clone();
        self.db.run(move |conn| {
            conn.execute("DELETE FROM memory_archive_jobs WHERE id = ?1", [id_str])?;
            Ok(())
        }).await
    }

    async fn get_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<MemoryArchiveJob>> {
        let project_id_str = project_id.as_str().to_string();
        self.db.run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, job_type, payload_json, status, error_message, created_at, updated_at, started_at, completed_at
                 FROM memory_archive_jobs
                 WHERE project_id = ?1
                 ORDER BY created_at DESC",
            )?;
            let jobs = stmt
                .query_map([project_id_str], job_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(jobs)
        }).await
    }

    async fn get_by_status(&self, status: ArchiveJobStatus) -> AppResult<Vec<MemoryArchiveJob>> {
        let status_str = status.to_string();
        self.db.run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, job_type, payload_json, status, error_message, created_at, updated_at, started_at, completed_at
                 FROM memory_archive_jobs
                 WHERE status = ?1
                 ORDER BY created_at ASC",
            )?;
            let jobs = stmt
                .query_map([status_str], job_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(jobs)
        }).await
    }

    async fn get_by_project_and_status(
        &self,
        project_id: &ProjectId,
        status: ArchiveJobStatus,
    ) -> AppResult<Vec<MemoryArchiveJob>> {
        let project_id_str = project_id.as_str().to_string();
        let status_str = status.to_string();
        self.db.run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, job_type, payload_json, status, error_message, created_at, updated_at, started_at, completed_at
                 FROM memory_archive_jobs
                 WHERE project_id = ?1 AND status = ?2
                 ORDER BY created_at ASC",
            )?;
            let jobs = stmt
                .query_map(rusqlite::params![project_id_str, status_str], job_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(jobs)
        }).await
    }

    async fn get_by_project_and_type(
        &self,
        project_id: &ProjectId,
        job_type: ArchiveJobType,
    ) -> AppResult<Vec<MemoryArchiveJob>> {
        let project_id_str = project_id.as_str().to_string();
        let job_type_str = job_type.to_string();
        self.db.run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id, project_id, job_type, payload_json, status, error_message, created_at, updated_at, started_at, completed_at
                 FROM memory_archive_jobs
                 WHERE project_id = ?1 AND job_type = ?2
                 ORDER BY created_at DESC",
            )?;
            let jobs = stmt
                .query_map(rusqlite::params![project_id_str, job_type_str], job_from_row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(jobs)
        }).await
    }

    async fn claim_next(&self) -> AppResult<Option<MemoryArchiveJob>> {
        self.db.run(move |conn| {
            let tx = conn.unchecked_transaction()?;

            let result: rusqlite::Result<MemoryArchiveJob> = tx.query_row(
                "SELECT id, project_id, job_type, payload_json, status, error_message, created_at, updated_at, started_at, completed_at
                 FROM memory_archive_jobs
                 WHERE status IN ('pending', 'failed')
                 ORDER BY created_at ASC
                 LIMIT 1",
                [],
                job_from_row,
            );

            let mut job = match result {
                Ok(job) => job,
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    tx.commit()?;
                    return Ok(None);
                }
                Err(e) => return Err(AppError::Database(e.to_string())),
            };

            job.start();

            tx.execute(
                "UPDATE memory_archive_jobs
                 SET status = ?1, updated_at = ?2, started_at = ?3
                 WHERE id = ?4",
                rusqlite::params![
                    job.status.to_string(),
                    job.updated_at.to_rfc3339(),
                    job.started_at.map(|dt| dt.to_rfc3339()),
                    job.id.0,
                ],
            )?;

            tx.commit()?;
            Ok(Some(job))
        }).await
    }

    async fn claim_next_for_project(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Option<MemoryArchiveJob>> {
        let project_id_str = project_id.as_str().to_string();
        self.db.run(move |conn| {
            let tx = conn.unchecked_transaction()?;

            let result: rusqlite::Result<MemoryArchiveJob> = tx.query_row(
                "SELECT id, project_id, job_type, payload_json, status, error_message, created_at, updated_at, started_at, completed_at
                 FROM memory_archive_jobs
                 WHERE project_id = ?1 AND status IN ('pending', 'failed')
                 ORDER BY created_at ASC
                 LIMIT 1",
                [project_id_str],
                job_from_row,
            );

            let mut job = match result {
                Ok(job) => job,
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    tx.commit()?;
                    return Ok(None);
                }
                Err(e) => return Err(AppError::Database(e.to_string())),
            };

            job.start();

            tx.execute(
                "UPDATE memory_archive_jobs
                 SET status = ?1, updated_at = ?2, started_at = ?3
                 WHERE id = ?4",
                rusqlite::params![
                    job.status.to_string(),
                    job.updated_at.to_rfc3339(),
                    job.started_at.map(|dt| dt.to_rfc3339()),
                    job.id.0,
                ],
            )?;

            tx.commit()?;
            Ok(Some(job))
        }).await
    }

    async fn count_by_status(&self, status: ArchiveJobStatus) -> AppResult<u32> {
        let status_str = status.to_string();
        self.db.run(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memory_archive_jobs WHERE status = ?1",
                [status_str],
                |row| row.get(0),
            )?;
            Ok(count as u32)
        }).await
    }

    async fn count_claimable(&self) -> AppResult<u32> {
        self.db.run(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memory_archive_jobs WHERE status IN ('pending', 'failed')",
                [],
                |row| row.get(0),
            )?;
            Ok(count as u32)
        }).await
    }

    async fn count_claimable_for_project(&self, project_id: &ProjectId) -> AppResult<u32> {
        let project_id_str = project_id.as_str().to_string();
        self.db.run(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memory_archive_jobs WHERE project_id = ?1 AND status IN ('pending', 'failed')",
                [project_id_str],
                |row| row.get(0),
            )?;
            Ok(count as u32)
        }).await
    }

    async fn delete_completed_older_than(&self, days: u32) -> AppResult<u32> {
        let cutoff_str = (Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339();
        self.db.run(move |conn| {
            let deleted = conn.execute(
                "DELETE FROM memory_archive_jobs
                 WHERE status = 'done' AND completed_at < ?1",
                [cutoff_str],
            )?;
            Ok(deleted as u32)
        }).await
    }

    async fn delete_by_project(&self, project_id: &ProjectId) -> AppResult<()> {
        let project_id_str = project_id.as_str().to_string();
        self.db.run(move |conn| {
            conn.execute(
                "DELETE FROM memory_archive_jobs WHERE project_id = ?1",
                [project_id_str],
            )?;
            Ok(())
        }).await
    }
}

#[cfg(test)]
#[path = "sqlite_memory_archive_repo_tests.rs"]
mod tests;

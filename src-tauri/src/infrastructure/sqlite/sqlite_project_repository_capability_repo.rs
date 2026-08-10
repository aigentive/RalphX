use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::entities::{ProjectId, ProjectRepositoryCapability};
use crate::domain::repositories::ProjectRepositoryCapabilityRepository;
use crate::error::{AppError, AppResult};

pub struct SqliteProjectRepositoryCapabilityRepository {
    db: DbConnection,
}

impl SqliteProjectRepositoryCapabilityRepository {
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

#[async_trait]
impl ProjectRepositoryCapabilityRepository for SqliteProjectRepositoryCapabilityRepository {
    async fn get(&self, project_id: &ProjectId) -> AppResult<Option<ProjectRepositoryCapability>> {
        let id = project_id.as_str().to_string();
        self.db.run(move |conn| {
            let result = conn.query_row(
                "SELECT project_id, kind, fetch_url, push_url, message, inspected_at, working_directory FROM project_repository_capability WHERE project_id = ?1",
                [&id],
                |row| Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get::<_, String>(5)?, row.get(6)?)),
            );
            match result {
                Ok((project_id, kind, fetch_url, push_url, message, inspected_at, working_directory)) => Ok(Some(ProjectRepositoryCapability {
                    project_id: ProjectId::from_string(project_id), kind, fetch_url, push_url, message,
                    inspected_at: DateTime::parse_from_rfc3339(&inspected_at).map_err(|error| AppError::Database(error.to_string()))?.with_timezone(&Utc),
                    working_directory,
                })),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(error) => Err(AppError::Database(error.to_string())),
            }
        }).await
    }

    async fn upsert(&self, capability: &ProjectRepositoryCapability) -> AppResult<()> {
        let row = capability.clone();
        self.db.run(move |conn| {
            conn.execute(
                "INSERT INTO project_repository_capability (project_id, kind, fetch_url, push_url, message, inspected_at, working_directory) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(project_id) DO UPDATE SET kind=excluded.kind, fetch_url=excluded.fetch_url, push_url=excluded.push_url, message=excluded.message, inspected_at=excluded.inspected_at, working_directory=excluded.working_directory",
                rusqlite::params![row.project_id.as_str(), row.kind, row.fetch_url, row.push_url, row.message, row.inspected_at.to_rfc3339(), row.working_directory],
            ).map_err(|error| AppError::Database(error.to_string()))?;
            Ok(())
        }).await
    }
}

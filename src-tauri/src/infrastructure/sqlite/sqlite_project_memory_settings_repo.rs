use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::Connection;
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::entities::{ProjectId, ProjectMemorySettings};
use crate::domain::repositories::ProjectMemorySettingsRepository;
use crate::error::{AppError, AppResult};

pub struct SqliteProjectMemorySettingsRepository {
    db: DbConnection,
}

impl SqliteProjectMemorySettingsRepository {
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }

    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

#[async_trait]
impl ProjectMemorySettingsRepository for SqliteProjectMemorySettingsRepository {
    async fn get_for_project(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Option<ProjectMemorySettings>> {
        let project_id = project_id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT project_id, enabled, maintenance_categories_json, capture_categories_json
                     FROM project_memory_settings
                     WHERE project_id = ?1",
                    [project_id],
                    |row| {
                        let project_id = ProjectId::from_string(row.get::<_, String>(0)?);
                        let maintenance_categories = parse_categories(row.get::<_, String>(2)?)?;
                        let capture_categories = parse_categories(row.get::<_, String>(3)?)?;
                        Ok(ProjectMemorySettings {
                            project_id,
                            enabled: row.get::<_, i64>(1)? != 0,
                            maintenance_categories,
                            capture_categories,
                        })
                    },
                )
            })
            .await
    }
}

fn parse_categories(raw: String) -> rusqlite::Result<Vec<String>> {
    serde_json::from_str::<Vec<String>>(&raw).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(AppError::Database(format!(
                "invalid memory category json: {error}"
            ))),
        )
    })
}

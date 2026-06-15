use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::Connection;
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::entities::{ProjectId, ProjectSkillSettings};
use crate::domain::repositories::ProjectSkillSettingsRepository;
use crate::error::AppResult;

pub struct SqliteProjectSkillSettingsRepository {
    db: DbConnection,
}

impl SqliteProjectSkillSettingsRepository {
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }
}

#[async_trait]
impl ProjectSkillSettingsRepository for SqliteProjectSkillSettingsRepository {
    async fn get_for_project(
        &self,
        project_id: &ProjectId,
    ) -> AppResult<Option<ProjectSkillSettings>> {
        let project_id = project_id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT project_id, export_enabled
                     FROM project_skill_settings
                     WHERE project_id = ?1",
                    [project_id],
                    |row| {
                        Ok(ProjectSkillSettings {
                            project_id: ProjectId::from_string(row.get::<_, String>(0)?),
                            export_enabled: row.get::<_, i64>(1)? != 0,
                        })
                    },
                )
            })
            .await
    }

    async fn upsert(&self, settings: ProjectSkillSettings) -> AppResult<ProjectSkillSettings> {
        let project_id = settings.project_id.as_str().to_string();
        let export_enabled = i64::from(settings.export_enabled);
        let saved = settings.clone();
        self.db
            .run(move |conn| {
                Ok(conn.execute(
                    "INSERT INTO project_skill_settings (
                        project_id, export_enabled, created_at, updated_at
                     ) VALUES (
                        ?1, ?2, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
                     )
                     ON CONFLICT(project_id)
                     DO UPDATE SET
                        export_enabled = excluded.export_enabled,
                        updated_at = CURRENT_TIMESTAMP",
                    rusqlite::params![project_id, export_enabled],
                )?)
            })
            .await?;
        Ok(saved)
    }
}

use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use rusqlite::Connection;

use super::DbConnection;
use crate::domain::entities::ProjectId;
use crate::domain::ideation::{EffortLevel, IdeationEffortSettings};
use ralphx_domain::repositories::IdeationEffortSettingsRepository;
use crate::error::AppError;

pub struct SqliteIdeationEffortSettingsRepository {
    db: DbConnection,
}

impl SqliteIdeationEffortSettingsRepository {
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
impl IdeationEffortSettingsRepository for SqliteIdeationEffortSettingsRepository {
    async fn get_by_project_id(
        &self,
        project_id: Option<&str>,
    ) -> Result<Option<IdeationEffortSettings>, Box<dyn std::error::Error>> {
        let project_id_owned = project_id.map(|s| s.to_string());
        self.db
            .run(move |conn| {
                let result = if let Some(ref pid) = project_id_owned {
                    conn.query_row(
                        "SELECT id, project_id, primary_effort, verifier_effort, updated_at
                         FROM ideation_effort_settings WHERE project_id = ?1",
                        rusqlite::params![pid.as_str()],
                        parse_row,
                    )
                } else {
                    conn.query_row(
                        "SELECT id, project_id, primary_effort, verifier_effort, updated_at
                         FROM ideation_effort_settings WHERE project_id IS NULL",
                        [],
                        parse_row,
                    )
                };

                match result {
                    Ok(settings) => Ok(Some(settings)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(AppError::Database(e.to_string())),
                }
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn upsert(
        &self,
        project_id: Option<&str>,
        primary_effort: &str,
        verifier_effort: &str,
    ) -> Result<IdeationEffortSettings, Box<dyn std::error::Error>> {
        let project_id_owned = project_id.map(|s| s.to_string());
        let primary_effort_owned = primary_effort.to_string();
        let verifier_effort_owned = verifier_effort.to_string();

        self.db
            .run(move |conn| {
                // Check if a row already exists
                let exists: bool = if let Some(ref pid) = project_id_owned {
                    let count: i64 = conn.query_row(
                        "SELECT COUNT(*) FROM ideation_effort_settings WHERE project_id = ?1",
                        rusqlite::params![pid.as_str()],
                        |row| row.get(0),
                    )?;
                    count > 0
                } else {
                    let count: i64 = conn.query_row(
                        "SELECT COUNT(*) FROM ideation_effort_settings WHERE project_id IS NULL",
                        [],
                        |row| row.get(0),
                    )?;
                    count > 0
                };

                if exists {
                    if let Some(ref pid) = project_id_owned {
                        conn.execute(
                            "UPDATE ideation_effort_settings
                             SET primary_effort = ?1,
                                 verifier_effort = ?2,
                                 updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                             WHERE project_id = ?3",
                            rusqlite::params![
                                primary_effort_owned.as_str(),
                                verifier_effort_owned.as_str(),
                                pid.as_str(),
                            ],
                        )?;
                    } else {
                        conn.execute(
                            "UPDATE ideation_effort_settings
                             SET primary_effort = ?1,
                                 verifier_effort = ?2,
                                 updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                             WHERE project_id IS NULL",
                            rusqlite::params![
                                primary_effort_owned.as_str(),
                                verifier_effort_owned.as_str(),
                            ],
                        )?;
                    }
                } else {
                    conn.execute(
                        "INSERT INTO ideation_effort_settings (project_id, primary_effort, verifier_effort, updated_at)
                         VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
                        rusqlite::params![
                            project_id_owned.as_deref(),
                            primary_effort_owned.as_str(),
                            verifier_effort_owned.as_str(),
                        ],
                    )?;
                }

                // Re-query the row and return it
                let result = if let Some(ref pid) = project_id_owned {
                    conn.query_row(
                        "SELECT id, project_id, primary_effort, verifier_effort, updated_at
                         FROM ideation_effort_settings WHERE project_id = ?1",
                        rusqlite::params![pid.as_str()],
                        parse_row,
                    )
                } else {
                    conn.query_row(
                        "SELECT id, project_id, primary_effort, verifier_effort, updated_at
                         FROM ideation_effort_settings WHERE project_id IS NULL",
                        [],
                        parse_row,
                    )
                };

                result.map_err(|e| AppError::Database(e.to_string()))
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }
}

fn parse_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<IdeationEffortSettings> {
    let id: i64 = row.get(0)?;
    let project_id_str: Option<String> = row.get(1)?;
    let primary_effort_str: String = row.get(2)?;
    let verifier_effort_str: String = row.get(3)?;
    let updated_at_str: String = row.get(4)?;

    let project_id = project_id_str.map(ProjectId);
    let primary_effort =
        EffortLevel::from_str(&primary_effort_str).unwrap_or(EffortLevel::Inherit);
    let verifier_effort =
        EffortLevel::from_str(&verifier_effort_str).unwrap_or(EffortLevel::Inherit);
    let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(IdeationEffortSettings {
        id,
        project_id,
        primary_effort,
        verifier_effort,
        updated_at,
    })
}

#[cfg(test)]
#[path = "sqlite_ideation_effort_settings_repo_tests.rs"]
mod tests;

use super::DbConnection;
use crate::domain::entities::app_state::{AppSettings, ExecutionHaltMode};
use crate::domain::entities::ProjectId;
use crate::domain::repositories::AppStateRepository;
use crate::error::AppError;
use async_trait::async_trait;
use rusqlite::Connection;
use std::sync::Arc;
use tokio::sync::Mutex;

/// SQLite implementation of AppStateRepository
/// Manages the singleton app_state table (id=1)
pub struct SqliteAppStateRepository {
    db: DbConnection,
}

impl SqliteAppStateRepository {
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

fn parse_halt_mode(raw: Option<String>) -> ExecutionHaltMode {
    match raw.as_deref() {
        Some("paused") => ExecutionHaltMode::Paused,
        Some("stopped") => ExecutionHaltMode::Stopped,
        _ => ExecutionHaltMode::Running,
    }
}

fn halt_mode_to_db(mode: ExecutionHaltMode) -> &'static str {
    match mode {
        ExecutionHaltMode::Running => "running",
        ExecutionHaltMode::Paused => "paused",
        ExecutionHaltMode::Stopped => "stopped",
    }
}

#[async_trait]
impl AppStateRepository for SqliteAppStateRepository {
    async fn get(&self) -> Result<AppSettings, Box<dyn std::error::Error>> {
        self.db
            .run(move |conn| {
                let result = conn.query_row(
                    "SELECT active_project_id, execution_halt_mode, last_seen_release_notes_version FROM app_state WHERE id = 1",
                    [],
                    |row| {
                        let active_project_id: Option<String> = row.get(0)?;
                        let execution_halt_mode: Option<String> = row.get(1)?;
                        let last_seen_release_notes_version: Option<String> = row.get(2)?;
                        Ok(AppSettings {
                            active_project_id: active_project_id.map(ProjectId::from_string),
                            execution_halt_mode: parse_halt_mode(execution_halt_mode),
                            last_seen_release_notes_version,
                        })
                    },
                );

                match result {
                    Ok(settings) => Ok(settings),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(AppSettings::default()),
                    Err(e) => Err(AppError::Database(e.to_string())),
                }
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn set_active_project(
        &self,
        project_id: Option<&ProjectId>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let project_id_str = project_id.map(|p| p.as_str().to_string());

        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE app_state SET active_project_id = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now') WHERE id = 1",
                    rusqlite::params![project_id_str],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn set_execution_halt_mode(
        &self,
        halt_mode: ExecutionHaltMode,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let halt_mode = halt_mode_to_db(halt_mode).to_string();

        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE app_state SET execution_halt_mode = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now') WHERE id = 1",
                    rusqlite::params![halt_mode],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn set_last_seen_release_notes_version(
        &self,
        version: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let version = version.map(str::to_string);

        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE app_state SET last_seen_release_notes_version = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now') WHERE id = 1",
                    rusqlite::params![version],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }
}

#[cfg(test)]
#[path = "sqlite_app_state_repo_tests.rs"]
mod tests;

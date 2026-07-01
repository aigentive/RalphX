use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::Connection;
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::agents::{
    AgentHarnessKind, LogicalEffort, StoredWorkspaceReviewRuntimeSettings,
    WorkspaceReviewRuntimeSettings,
};
use crate::domain::repositories::WorkspaceReviewRuntimeSettingsRepository;
use crate::error::{AppError, AppResult};

const GLOBAL_SCOPE_ID: &str = "";

pub struct SqliteWorkspaceReviewRuntimeSettingsRepository {
    db: DbConnection,
}

impl SqliteWorkspaceReviewRuntimeSettingsRepository {
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

fn parse_datetime(s: &str) -> DateTime<Utc> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return dt.with_timezone(&Utc);
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Utc.from_utc_datetime(&ndt);
    }
    Utc::now()
}

fn parse_row(row: &rusqlite::Row<'_>) -> AppResult<StoredWorkspaceReviewRuntimeSettings> {
    let id: i64 = row
        .get("id")
        .map_err(|e| AppError::Database(e.to_string()))?;
    let project_id: Option<String> = row
        .get("scope_id")
        .map_err(|e| AppError::Database(e.to_string()))?;
    let provider = row
        .get::<_, String>("provider")
        .map_err(|e| AppError::Database(e.to_string()))?
        .parse::<AgentHarnessKind>()
        .map_err(AppError::Database)?;
    let effort = row
        .get::<_, Option<String>>("effort")
        .map_err(|e| AppError::Database(e.to_string()))?
        .map(|value| value.parse::<LogicalEffort>().map_err(AppError::Database))
        .transpose()?;
    let updated_at = parse_datetime(
        &row.get::<_, String>("updated_at")
            .map_err(|e| AppError::Database(e.to_string()))?,
    );

    Ok(StoredWorkspaceReviewRuntimeSettings {
        id,
        project_id,
        provider,
        settings: WorkspaceReviewRuntimeSettings {
            model: row
                .get("model")
                .map_err(|e| AppError::Database(e.to_string()))?,
            effort,
        },
        updated_at,
    })
}

fn fetch_optional<P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> AppResult<Option<StoredWorkspaceReviewRuntimeSettings>> {
    match conn.query_row(sql, params, |row| {
        parse_row(row).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
        })
    }) {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(AppError::Database(err.to_string())),
    }
}

fn fetch_many<P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> AppResult<Vec<StoredWorkspaceReviewRuntimeSettings>> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| AppError::Database(e.to_string()))?;
    let rows = stmt
        .query_map(params, |row| {
            parse_row(row).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })
        })
        .map_err(|e| AppError::Database(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(rows)
}

#[async_trait]
impl WorkspaceReviewRuntimeSettingsRepository for SqliteWorkspaceReviewRuntimeSettingsRepository {
    async fn get_global(
        &self,
        provider: AgentHarnessKind,
    ) -> Result<Option<StoredWorkspaceReviewRuntimeSettings>, Box<dyn std::error::Error>> {
        let provider = provider.to_string();
        self.db
            .run(move |conn| {
                fetch_optional(
                    conn,
                    "SELECT id,
                            CASE WHEN scope_type = 'project' THEN scope_id ELSE NULL END AS scope_id,
                            provider, model, effort, updated_at
                     FROM workspace_review_runtime_settings
                     WHERE scope_type = 'global' AND provider = ?1",
                    rusqlite::params![provider],
                )
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn get_for_project(
        &self,
        project_id: &str,
        provider: AgentHarnessKind,
    ) -> Result<Option<StoredWorkspaceReviewRuntimeSettings>, Box<dyn std::error::Error>> {
        let project_id = project_id.to_string();
        let provider = provider.to_string();
        self.db
            .run(move |conn| {
                fetch_optional(
                    conn,
                    "SELECT id,
                            CASE WHEN scope_type = 'project' THEN scope_id ELSE NULL END AS scope_id,
                            provider, model, effort, updated_at
                     FROM workspace_review_runtime_settings
                     WHERE scope_type = 'project' AND scope_id = ?1 AND provider = ?2",
                    rusqlite::params![project_id, provider],
                )
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn list_global(
        &self,
    ) -> Result<Vec<StoredWorkspaceReviewRuntimeSettings>, Box<dyn std::error::Error>> {
        self.db
            .run(move |conn| {
                fetch_many(
                    conn,
                    "SELECT id,
                            CASE WHEN scope_type = 'project' THEN scope_id ELSE NULL END AS scope_id,
                            provider, model, effort, updated_at
                     FROM workspace_review_runtime_settings
                     WHERE scope_type = 'global'
                     ORDER BY provider",
                    [],
                )
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn list_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<StoredWorkspaceReviewRuntimeSettings>, Box<dyn std::error::Error>> {
        let project_id = project_id.to_string();
        self.db
            .run(move |conn| {
                fetch_many(
                    conn,
                    "SELECT id,
                            CASE WHEN scope_type = 'project' THEN scope_id ELSE NULL END AS scope_id,
                            provider, model, effort, updated_at
                     FROM workspace_review_runtime_settings
                     WHERE scope_type = 'project' AND scope_id = ?1
                     ORDER BY provider",
                    rusqlite::params![project_id],
                )
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn upsert_global(
        &self,
        provider: AgentHarnessKind,
        settings: &WorkspaceReviewRuntimeSettings,
    ) -> Result<StoredWorkspaceReviewRuntimeSettings, Box<dyn std::error::Error>> {
        let provider_key = provider.to_string();
        let model = settings.model.clone();
        let effort = settings.effort.map(|value| value.to_string());
        let scope_id = GLOBAL_SCOPE_ID.to_string();

        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO workspace_review_runtime_settings (
                        scope_type, scope_id, provider, model, effort, updated_at
                     ) VALUES (
                        'global', ?1, ?2, ?3, ?4,
                        strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                     )
                     ON CONFLICT(scope_type, scope_id, provider) DO UPDATE SET
                        model = excluded.model,
                        effort = excluded.effort,
                        updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')",
                    rusqlite::params![scope_id.clone(), provider_key.clone(), model, effort],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;

                fetch_optional(
                    conn,
                    "SELECT id,
                            CASE WHEN scope_type = 'project' THEN scope_id ELSE NULL END AS scope_id,
                            provider, model, effort, updated_at
                     FROM workspace_review_runtime_settings
                     WHERE scope_type = 'global' AND scope_id = ?1 AND provider = ?2",
                    rusqlite::params![scope_id, provider_key],
                )?
                .ok_or_else(|| {
                    AppError::Database(
                        "Global Workspace Review runtime settings row missing after upsert"
                            .to_string(),
                    )
                })
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn upsert_for_project(
        &self,
        project_id: &str,
        provider: AgentHarnessKind,
        settings: &WorkspaceReviewRuntimeSettings,
    ) -> Result<StoredWorkspaceReviewRuntimeSettings, Box<dyn std::error::Error>> {
        let project_id = project_id.to_string();
        let provider_key = provider.to_string();
        let model = settings.model.clone();
        let effort = settings.effort.map(|value| value.to_string());

        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO workspace_review_runtime_settings (
                        scope_type, scope_id, provider, model, effort, updated_at
                     ) VALUES (
                        'project', ?1, ?2, ?3, ?4,
                        strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                     )
                     ON CONFLICT(scope_type, scope_id, provider) DO UPDATE SET
                        model = excluded.model,
                        effort = excluded.effort,
                        updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')",
                    rusqlite::params![project_id.clone(), provider_key.clone(), model, effort],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;

                fetch_optional(
                    conn,
                    "SELECT id,
                            CASE WHEN scope_type = 'project' THEN scope_id ELSE NULL END AS scope_id,
                            provider, model, effort, updated_at
                     FROM workspace_review_runtime_settings
                     WHERE scope_type = 'project' AND scope_id = ?1 AND provider = ?2",
                    rusqlite::params![project_id, provider_key],
                )?
                .ok_or_else(|| {
                    AppError::Database(
                        "Project Workspace Review runtime settings row missing after upsert"
                            .to_string(),
                    )
                })
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }
}

#[cfg(test)]
#[path = "sqlite_workspace_review_runtime_settings_repo_tests.rs"]
mod tests;

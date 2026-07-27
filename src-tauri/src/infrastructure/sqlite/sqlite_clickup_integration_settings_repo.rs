use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::{params, Connection};
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::integrations::{
    ClickUpIntegrationSettings, ClickUpIntegrationSettingsRepository, IntegrationValidationStatus,
};
use crate::error::{AppError, AppResult};

pub struct SqliteClickUpIntegrationSettingsRepository {
    db: DbConnection,
}

impl SqliteClickUpIntegrationSettingsRepository {
    pub fn from_db(db: DbConnection) -> Self {
        Self { db }
    }

    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
        }
    }

    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
        }
    }
}

fn parse_datetime(raw: Option<String>) -> Option<DateTime<Utc>> {
    let raw = raw?;
    if let Ok(dt) = DateTime::parse_from_rfc3339(&raw) {
        return Some(dt.with_timezone(&Utc));
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(&raw, "%Y-%m-%d %H:%M:%S") {
        return Some(Utc.from_utc_datetime(&ndt));
    }
    None
}

fn row_to_settings(row: &rusqlite::Row<'_>) -> AppResult<ClickUpIntegrationSettings> {
    let validation_status = row
        .get::<_, String>("validation_status")
        .map_err(|error| AppError::Database(error.to_string()))?
        .parse::<IntegrationValidationStatus>()
        .map_err(AppError::Database)?;
    Ok(ClickUpIntegrationSettings {
        enabled: row
            .get::<_, i64>("enabled")
            .map_err(|error| AppError::Database(error.to_string()))?
            != 0,
        token_secret_ref: row
            .get("token_secret_ref")
            .map_err(|error| AppError::Database(error.to_string()))?,
        workspace_id: row
            .get("workspace_id")
            .map_err(|error| AppError::Database(error.to_string()))?,
        validation_status,
        task_search_available: row
            .get::<_, i64>("task_search_available")
            .map_err(|error| AppError::Database(error.to_string()))?
            != 0,
        strict_git_naming_enabled: row
            .get::<_, i64>("strict_git_naming_enabled")
            .map_err(|error| AppError::Database(error.to_string()))?
            != 0,
        branch_name_template: row
            .get("branch_name_template")
            .map_err(|error| AppError::Database(error.to_string()))?,
        commit_subject_template: row
            .get("commit_subject_template")
            .map_err(|error| AppError::Database(error.to_string()))?,
        pr_title_template: row
            .get("pr_title_template")
            .map_err(|error| AppError::Database(error.to_string()))?,
        last_validated_at: parse_datetime(
            row.get("last_validated_at")
                .map_err(|error| AppError::Database(error.to_string()))?,
        ),
        last_error: row
            .get("last_error")
            .map_err(|error| AppError::Database(error.to_string()))?,
        updated_at: parse_datetime(
            row.get("updated_at")
                .map_err(|error| AppError::Database(error.to_string()))?,
        )
        .unwrap_or_else(Utc::now),
    })
}

#[async_trait]
impl ClickUpIntegrationSettingsRepository for SqliteClickUpIntegrationSettingsRepository {
    async fn get(&self) -> Result<ClickUpIntegrationSettings, Box<dyn std::error::Error>> {
        self.db
            .run(move |conn| {
                let result = conn.query_row(
                    "SELECT enabled, token_secret_ref, workspace_id, validation_status,
                            task_search_available, strict_git_naming_enabled,
                            branch_name_template, commit_subject_template, pr_title_template,
                            last_validated_at, last_error, updated_at
                       FROM clickup_integration_settings
                      WHERE id = 'default'",
                    [],
                    |row| {
                        row_to_settings(row).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })
                    },
                );
                match result {
                    Ok(settings) => Ok(settings),
                    Err(rusqlite::Error::QueryReturnedNoRows) => {
                        Ok(ClickUpIntegrationSettings::default())
                    }
                    Err(error) => Err(AppError::Database(error.to_string())),
                }
            })
            .await
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
    }

    async fn upsert(
        &self,
        settings: &ClickUpIntegrationSettings,
    ) -> Result<ClickUpIntegrationSettings, Box<dyn std::error::Error>> {
        let settings = settings.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO clickup_integration_settings (
                        id, enabled, token_secret_ref, workspace_id, validation_status,
                        task_search_available, strict_git_naming_enabled, branch_name_template,
                        commit_subject_template, pr_title_template, last_validated_at,
                        last_error, updated_at
                    ) VALUES (
                        'default', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12
                    )
                    ON CONFLICT(id) DO UPDATE SET
                        enabled = excluded.enabled,
                        token_secret_ref = excluded.token_secret_ref,
                        workspace_id = excluded.workspace_id,
                        validation_status = excluded.validation_status,
                        task_search_available = excluded.task_search_available,
                        strict_git_naming_enabled = excluded.strict_git_naming_enabled,
                        branch_name_template = excluded.branch_name_template,
                        commit_subject_template = excluded.commit_subject_template,
                        pr_title_template = excluded.pr_title_template,
                        last_validated_at = excluded.last_validated_at,
                        last_error = excluded.last_error,
                        updated_at = excluded.updated_at",
                    params![
                        settings.enabled as i64,
                        settings.token_secret_ref,
                        settings.workspace_id,
                        settings.validation_status.as_str(),
                        settings.task_search_available as i64,
                        settings.strict_git_naming_enabled as i64,
                        settings.branch_name_template,
                        settings.commit_subject_template,
                        settings.pr_title_template,
                        settings.last_validated_at.map(|value| value.to_rfc3339()),
                        settings.last_error,
                        settings.updated_at.to_rfc3339(),
                    ],
                )?;
                Ok(settings)
            })
            .await
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
    }
}

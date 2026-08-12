use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::{params, Connection};
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::integrations::{LinearIntegrationSettings, LinearIntegrationSettingsRepository};
use crate::domain::integrations::IntegrationValidationStatus;
use crate::error::{AppError, AppResult};

pub struct SqliteLinearIntegrationSettingsRepository {
    db: DbConnection,
}

impl SqliteLinearIntegrationSettingsRepository {
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

#[cfg(test)]
#[path = "sqlite_linear_integration_settings_repo_tests.rs"]
mod tests;

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

fn row_to_settings(row: &rusqlite::Row<'_>) -> AppResult<LinearIntegrationSettings> {
    let validation_status = row
        .get::<_, String>("validation_status")
        .map_err(|error| AppError::Database(error.to_string()))?
        .parse::<IntegrationValidationStatus>()
        .map_err(AppError::Database)?;
    Ok(LinearIntegrationSettings {
        enabled: row
            .get::<_, i64>("enabled")
            .map_err(|error| AppError::Database(error.to_string()))?
            != 0,
        token_secret_ref: row
            .get("token_secret_ref")
            .map_err(|error| AppError::Database(error.to_string()))?,
        validation_status,
        issue_search_available: row
            .get::<_, i64>("issue_search_available")
            .map_err(|error| AppError::Database(error.to_string()))?
            != 0,
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
impl LinearIntegrationSettingsRepository for SqliteLinearIntegrationSettingsRepository {
    async fn get(&self) -> Result<LinearIntegrationSettings, Box<dyn std::error::Error>> {
        self.db
            .run(move |conn| {
                let result = conn.query_row(
                    "SELECT enabled, token_secret_ref, validation_status, issue_search_available,
                            last_validated_at, last_error, updated_at
                       FROM linear_integration_settings
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
                        Ok(LinearIntegrationSettings::default())
                    }
                    Err(error) => Err(AppError::Database(error.to_string())),
                }
            })
            .await
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
    }

    async fn upsert(
        &self,
        settings: &LinearIntegrationSettings,
    ) -> Result<LinearIntegrationSettings, Box<dyn std::error::Error>> {
        let settings = settings.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO linear_integration_settings (
                        id, enabled, token_secret_ref, validation_status, issue_search_available,
                        last_validated_at, last_error, updated_at
                    ) VALUES (
                        'default', ?1, ?2, ?3, ?4, ?5, ?6, ?7
                    )
                    ON CONFLICT(id) DO UPDATE SET
                        enabled = excluded.enabled,
                        token_secret_ref = excluded.token_secret_ref,
                        validation_status = excluded.validation_status,
                        issue_search_available = excluded.issue_search_available,
                        last_validated_at = excluded.last_validated_at,
                        last_error = excluded.last_error,
                        updated_at = excluded.updated_at",
                    params![
                        settings.enabled as i64,
                        settings.token_secret_ref,
                        settings.validation_status.as_str(),
                        settings.issue_search_available as i64,
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

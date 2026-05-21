use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::{params, Connection};
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::integrations::{
    AtlassianAuthMethod, AtlassianIntegrationSettings, AtlassianIntegrationSettingsRepository,
    IntegrationValidationStatus,
};
use crate::error::{AppError, AppResult};

pub struct SqliteAtlassianIntegrationSettingsRepository {
    db: DbConnection,
}

impl SqliteAtlassianIntegrationSettingsRepository {
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

fn row_to_settings(row: &rusqlite::Row<'_>) -> AppResult<AtlassianIntegrationSettings> {
    let validation_status = row
        .get::<_, String>("validation_status")
        .map_err(|error| AppError::Database(error.to_string()))?
        .parse::<IntegrationValidationStatus>()
        .map_err(AppError::Database)?;
    let auth_method = row
        .get::<_, String>("auth_method")
        .map_err(|error| AppError::Database(error.to_string()))?
        .parse::<AtlassianAuthMethod>()
        .map_err(AppError::Database)?;
    Ok(AtlassianIntegrationSettings {
        enabled: row
            .get::<_, i64>("enabled")
            .map_err(|error| AppError::Database(error.to_string()))?
            != 0,
        auth_method,
        site_url: row
            .get("site_url")
            .map_err(|error| AppError::Database(error.to_string()))?,
        email: row
            .get("email")
            .map_err(|error| AppError::Database(error.to_string()))?,
        token_secret_ref: row
            .get("token_secret_ref")
            .map_err(|error| AppError::Database(error.to_string()))?,
        oauth_client_id: row
            .get("oauth_client_id")
            .map_err(|error| AppError::Database(error.to_string()))?,
        oauth_redirect_uri: row
            .get("oauth_redirect_uri")
            .map_err(|error| AppError::Database(error.to_string()))?,
        oauth_client_secret_ref: row
            .get("oauth_client_secret_ref")
            .map_err(|error| AppError::Database(error.to_string()))?,
        oauth_access_token_ref: row
            .get("oauth_access_token_ref")
            .map_err(|error| AppError::Database(error.to_string()))?,
        oauth_refresh_token_ref: row
            .get("oauth_refresh_token_ref")
            .map_err(|error| AppError::Database(error.to_string()))?,
        oauth_cloud_id: row
            .get("oauth_cloud_id")
            .map_err(|error| AppError::Database(error.to_string()))?,
        oauth_scopes: row
            .get("oauth_scopes")
            .map_err(|error| AppError::Database(error.to_string()))?,
        oauth_access_token_expires_at: parse_datetime(
            row.get("oauth_access_token_expires_at")
                .map_err(|error| AppError::Database(error.to_string()))?,
        ),
        validation_status,
        jira_available: row
            .get::<_, i64>("jira_available")
            .map_err(|error| AppError::Database(error.to_string()))?
            != 0,
        confluence_available: row
            .get::<_, i64>("confluence_available")
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
impl AtlassianIntegrationSettingsRepository for SqliteAtlassianIntegrationSettingsRepository {
    async fn get(&self) -> Result<AtlassianIntegrationSettings, Box<dyn std::error::Error>> {
        self.db
            .run(move |conn| {
                let result = conn.query_row(
                    "SELECT enabled, auth_method, site_url, email, token_secret_ref,
                            oauth_client_id, oauth_redirect_uri, oauth_client_secret_ref,
                            oauth_access_token_ref, oauth_refresh_token_ref, oauth_cloud_id,
                            oauth_scopes, oauth_access_token_expires_at,
                            validation_status, jira_available, confluence_available,
                            last_validated_at, last_error, updated_at
                       FROM atlassian_integration_settings
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
                        Ok(AtlassianIntegrationSettings::default())
                    }
                    Err(error) => Err(AppError::Database(error.to_string())),
                }
            })
            .await
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
    }

    async fn upsert(
        &self,
        settings: &AtlassianIntegrationSettings,
    ) -> Result<AtlassianIntegrationSettings, Box<dyn std::error::Error>> {
        let settings = settings.clone();
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO atlassian_integration_settings (
                        id, enabled, auth_method, site_url, email, token_secret_ref,
                        oauth_client_id, oauth_redirect_uri, oauth_client_secret_ref,
                        oauth_access_token_ref, oauth_refresh_token_ref, oauth_cloud_id,
                        oauth_scopes, oauth_access_token_expires_at,
                        validation_status, jira_available, confluence_available,
                        last_validated_at, last_error, updated_at
                    ) VALUES (
                        'default', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                        ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19
                    )
                    ON CONFLICT(id) DO UPDATE SET
                        enabled = excluded.enabled,
                        auth_method = excluded.auth_method,
                        site_url = excluded.site_url,
                        email = excluded.email,
                        token_secret_ref = excluded.token_secret_ref,
                        oauth_client_id = excluded.oauth_client_id,
                        oauth_redirect_uri = excluded.oauth_redirect_uri,
                        oauth_client_secret_ref = excluded.oauth_client_secret_ref,
                        oauth_access_token_ref = excluded.oauth_access_token_ref,
                        oauth_refresh_token_ref = excluded.oauth_refresh_token_ref,
                        oauth_cloud_id = excluded.oauth_cloud_id,
                        oauth_scopes = excluded.oauth_scopes,
                        oauth_access_token_expires_at = excluded.oauth_access_token_expires_at,
                        validation_status = excluded.validation_status,
                        jira_available = excluded.jira_available,
                        confluence_available = excluded.confluence_available,
                        last_validated_at = excluded.last_validated_at,
                        last_error = excluded.last_error,
                        updated_at = excluded.updated_at",
                    params![
                        settings.enabled as i64,
                        settings.auth_method.as_str(),
                        settings.site_url,
                        settings.email,
                        settings.token_secret_ref,
                        settings.oauth_client_id,
                        settings.oauth_redirect_uri,
                        settings.oauth_client_secret_ref,
                        settings.oauth_access_token_ref,
                        settings.oauth_refresh_token_ref,
                        settings.oauth_cloud_id,
                        settings.oauth_scopes,
                        settings
                            .oauth_access_token_expires_at
                            .map(|value| value.to_rfc3339()),
                        settings.validation_status.as_str(),
                        settings.jira_available as i64,
                        settings.confluence_available as i64,
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

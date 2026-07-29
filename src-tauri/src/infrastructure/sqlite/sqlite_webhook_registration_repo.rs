// SQLite-based WebhookRegistrationRepository implementation for production use
// Uses rusqlite with connection pooling for thread-safe access

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use rusqlite::Connection;

use super::DbConnection;
use crate::domain::repositories::{WebhookRegistration, WebhookRegistrationRepository};
use crate::error::AppResult;

/// SQLite implementation of WebhookRegistrationRepository for production use
pub struct SqliteWebhookRegistrationRepository {
    db: DbConnection,
}

impl SqliteWebhookRegistrationRepository {
    /// Create a new SQLite webhook registration repository with the given connection
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

    /// Parse a WebhookRegistration from a database row
    fn from_row(row: &rusqlite::Row<'_>) -> Result<WebhookRegistration, rusqlite::Error> {
        let active: i64 = row.get(6)?;
        Ok(WebhookRegistration {
            id: row.get(0)?,
            api_key_id: row.get(1)?,
            url: row.get(2)?,
            event_types: row.get(3)?,
            project_ids: row.get(4)?,
            secret: row.get(5)?,
            active: active != 0,
            failure_count: row.get(7)?,
            last_failure_at: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        })
    }
}

#[async_trait]
impl WebhookRegistrationRepository for SqliteWebhookRegistrationRepository {
    async fn upsert(&self, registration: WebhookRegistration) -> AppResult<WebhookRegistration> {
        let id = registration.id.clone();
        let api_key_id = registration.api_key_id.clone();
        let url = registration.url.clone();
        let event_types = registration.event_types.clone();
        let project_ids = registration.project_ids.clone();
        let secret = registration.secret.clone();

        self.db
            .run(move |conn| {
                // Check if URL+api_key_id already exists
                let existing: Option<String> = conn
                    .query_row(
                        "SELECT id FROM webhook_registrations WHERE url = ?1 AND api_key_id = ?2",
                        rusqlite::params![url, api_key_id],
                        |row| row.get(0),
                    )
                    .ok();

                if let Some(existing_id) = existing {
                    // Re-registration: refresh project_ids, event_types, reset failures, reactivate
                    conn.execute(
                        "UPDATE webhook_registrations SET active = 1, failure_count = 0, project_ids = ?2, event_types = ?3, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1",
                        rusqlite::params![existing_id, project_ids, event_types],
                    )?;
                    // Return updated row
                    conn.query_row(
                        "SELECT id, api_key_id, url, event_types, project_ids, secret, active, failure_count, last_failure_at, created_at, updated_at FROM webhook_registrations WHERE id = ?1",
                        [existing_id.as_str()],
                        SqliteWebhookRegistrationRepository::from_row,
                    ).map_err(Into::into)
                } else {
                    // Insert new
                    conn.execute(
                        "INSERT INTO webhook_registrations (id, api_key_id, url, event_types, project_ids, secret, active, failure_count, last_failure_at, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 0, NULL, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
                        rusqlite::params![id, api_key_id, url, event_types, project_ids, secret],
                    )?;
                    conn.query_row(
                        "SELECT id, api_key_id, url, event_types, project_ids, secret, active, failure_count, last_failure_at, created_at, updated_at FROM webhook_registrations WHERE id = ?1",
                        [id.as_str()],
                        SqliteWebhookRegistrationRepository::from_row,
                    ).map_err(Into::into)
                }
            })
            .await
    }

    async fn get_by_id(&self, id: &str) -> AppResult<Option<WebhookRegistration>> {
        let id = id.to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, api_key_id, url, event_types, project_ids, secret, active, failure_count, last_failure_at, created_at, updated_at FROM webhook_registrations WHERE id = ?1",
                    [id.as_str()],
                    SqliteWebhookRegistrationRepository::from_row,
                )
            })
            .await
    }

    async fn get_by_url_and_key(
        &self,
        url: &str,
        api_key_id: &str,
    ) -> AppResult<Option<WebhookRegistration>> {
        let url = url.to_string();
        let api_key_id = api_key_id.to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, api_key_id, url, event_types, project_ids, secret, active, failure_count, last_failure_at, created_at, updated_at FROM webhook_registrations WHERE url = ?1 AND api_key_id = ?2",
                    rusqlite::params![url, api_key_id],
                    SqliteWebhookRegistrationRepository::from_row,
                )
            })
            .await
    }

    async fn list_by_api_key(&self, api_key_id: &str) -> AppResult<Vec<WebhookRegistration>> {
        let api_key_id = api_key_id.to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, api_key_id, url, event_types, project_ids, secret, active, failure_count, last_failure_at, created_at, updated_at FROM webhook_registrations WHERE api_key_id = ?1 ORDER BY created_at DESC",
                )?;
                let rows = stmt
                    .query_map(
                        [api_key_id.as_str()],
                        SqliteWebhookRegistrationRepository::from_row,
                    )?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }

    async fn deactivate(&self, id: &str, api_key_id: &str) -> AppResult<bool> {
        let id = id.to_string();
        let api_key_id = api_key_id.to_string();
        self.db
            .run(move |conn| {
                let rows = conn.execute(
                    "UPDATE webhook_registrations SET active = 0, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1 AND api_key_id = ?2",
                    rusqlite::params![id, api_key_id],
                )?;
                Ok(rows > 0)
            })
            .await
    }

    async fn increment_failure(&self, id: &str) -> AppResult<()> {
        let id = id.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE webhook_registrations SET failure_count = failure_count + 1, last_failure_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), active = CASE WHEN failure_count + 1 >= 10 THEN 0 ELSE active END, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1",
                    [id.as_str()],
                )?;
                Ok(())
            })
            .await
    }

    async fn reset_failures(&self, id: &str) -> AppResult<()> {
        let id = id.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE webhook_registrations SET failure_count = 0, active = 1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1",
                    [id.as_str()],
                )?;
                Ok(())
            })
            .await
    }

    async fn list_active_for_project(&self, project_id: &str) -> AppResult<Vec<WebhookRegistration>> {
        let project_id = project_id.to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, api_key_id, url, event_types, project_ids, secret, active, failure_count, last_failure_at, created_at, updated_at \
                     FROM webhook_registrations \
                     WHERE active = 1 \
                     AND (project_ids = '[]' OR project_ids LIKE '%\"' || ?1 || '\"%') \
                     ORDER BY created_at DESC",
                )?;
                let rows = stmt
                    .query_map(
                        [project_id.as_str()],
                        SqliteWebhookRegistrationRepository::from_row,
                    )?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_webhook_registration_repo_tests.rs"]
mod tests;

use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::Connection;
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::entities::NotificationSettings;
use crate::domain::repositories::NotificationSettingsRepository;
use crate::error::{AppError, AppResult};

pub struct SqliteNotificationSettingsRepository {
    db: DbConnection,
}

impl SqliteNotificationSettingsRepository {
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
impl NotificationSettingsRepository for SqliteNotificationSettingsRepository {
    async fn get_settings(&self) -> AppResult<NotificationSettings> {
        self.db
            .run(|conn| {
                let result = conn.query_row(
                    "SELECT settings_json FROM notification_settings WHERE id = 1",
                    [],
                    |row| row.get::<_, String>(0),
                );

                match result {
                    Ok(json) => serde_json::from_str(&json)
                        .map_err(|error| AppError::Database(error.to_string())),
                    Err(rusqlite::Error::QueryReturnedNoRows) => {
                        Ok(NotificationSettings::default())
                    }
                    Err(error) => Err(AppError::Database(error.to_string())),
                }
            })
            .await
    }

    async fn update_settings(
        &self,
        settings: &NotificationSettings,
    ) -> AppResult<NotificationSettings> {
        let settings = settings.clone();
        self.db
            .run(move |conn| {
                let json = serde_json::to_string(&settings)
                    .map_err(|error| AppError::Database(error.to_string()))?;
                conn.execute(
                    "INSERT INTO notification_settings (id, settings_json)
                     VALUES (1, ?1)
                     ON CONFLICT(id) DO UPDATE SET
                         settings_json = excluded.settings_json,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')",
                    [json],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
                Ok(settings)
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_notification_settings_repo_tests.rs"]
mod tests;

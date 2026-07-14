use std::sync::Arc;

use async_trait::async_trait;
use rusqlite::Connection;
use tokio::sync::Mutex;

use crate::domain::entities::UiFeatureFlagOverrides;
use crate::domain::repositories::UiFeatureFlagOverridesRepository;
use crate::error::{AppError, AppResult};

use super::DbConnection;

pub struct SqliteUiFeatureFlagOverridesRepository {
    db: DbConnection,
}

impl SqliteUiFeatureFlagOverridesRepository {
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
impl UiFeatureFlagOverridesRepository for SqliteUiFeatureFlagOverridesRepository {
    async fn get(&self) -> AppResult<UiFeatureFlagOverrides> {
        self.db
            .run(|conn| {
                let result = conn.query_row(
                    "SELECT agent_personas FROM ui_feature_flag_overrides WHERE id = 1",
                    [],
                    |row| {
                        let value: Option<i64> = row.get(0)?;
                        Ok(UiFeatureFlagOverrides {
                            agent_personas: value.map(|value| value != 0),
                        })
                    },
                );

                match result {
                    Ok(overrides) => Ok(overrides),
                    Err(rusqlite::Error::QueryReturnedNoRows) => {
                        Ok(UiFeatureFlagOverrides::default())
                    }
                    Err(error) => Err(AppError::Database(error.to_string())),
                }
            })
            .await
    }

    async fn set_agent_personas(&self, value: Option<bool>) -> AppResult<()> {
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO ui_feature_flag_overrides (id, agent_personas) VALUES (1, NULL)",
                    [],
                )?;
                conn.execute(
                    "UPDATE ui_feature_flag_overrides SET agent_personas = ?1 WHERE id = 1",
                    [value.map(|value| if value { 1 } else { 0 })],
                )?;
                Ok(())
            })
            .await
    }
}

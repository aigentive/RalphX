// SQLite-based AgentProfileRepository implementation for production use
// All rusqlite calls go through DbConnection::run() (spawn_blocking + blocking_lock)
// to prevent blocking the tokio async runtime / timer driver.

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use rusqlite::Connection;

use crate::domain::agents::{AgentProfile, ProfileRole};
use crate::domain::repositories::{AgentProfileId, AgentProfileRepository};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

/// SQLite implementation of AgentProfileRepository for production use
pub struct SqliteAgentProfileRepository {
    db: DbConnection,
}

impl SqliteAgentProfileRepository {
    /// Create a new SQLite agent profile repository with the given connection
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
}

#[async_trait]
impl AgentProfileRepository for SqliteAgentProfileRepository {
    async fn create(
        &self,
        id: &AgentProfileId,
        profile: &AgentProfile,
        is_builtin: bool,
    ) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        let name = profile.name.clone();
        let role = profile.role.to_string();
        let profile_json = serde_json::to_string(profile)
            .map_err(|e| AppError::Database(format!("JSON serialization error: {}", e)))?;
        let is_builtin_int = if is_builtin { 1i32 } else { 0 };

        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO agent_profiles (id, name, role, profile_json, is_builtin)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![id_str, name, role, profile_json, is_builtin_int],
                )?;
                Ok(())
            })
            .await
    }

    async fn get_by_id(&self, id: &AgentProfileId) -> AppResult<Option<AgentProfile>> {
        let id_str = id.as_str().to_string();
        let maybe_json = self
            .db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT profile_json FROM agent_profiles WHERE id = ?1",
                    rusqlite::params![id_str],
                    |row| row.get::<_, String>(0),
                )
            })
            .await?;

        maybe_json
            .map(|json| {
                serde_json::from_str(&json)
                    .map_err(|e| AppError::Database(format!("JSON deserialization error: {}", e)))
            })
            .transpose()
    }

    async fn get_by_name(&self, name: &str) -> AppResult<Option<AgentProfile>> {
        let name_str = name.to_string();
        let maybe_json = self
            .db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT profile_json FROM agent_profiles WHERE name = ?1",
                    rusqlite::params![name_str],
                    |row| row.get::<_, String>(0),
                )
            })
            .await?;

        maybe_json
            .map(|json| {
                serde_json::from_str(&json)
                    .map_err(|e| AppError::Database(format!("JSON deserialization error: {}", e)))
            })
            .transpose()
    }

    async fn get_all(&self) -> AppResult<Vec<AgentProfile>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn
                    .prepare("SELECT profile_json FROM agent_profiles ORDER BY name ASC")?;
                let jsons = stmt
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<String>, _>>()?;
                jsons
                    .into_iter()
                    .map(|json| {
                        serde_json::from_str(&json).map_err(|e| {
                            AppError::Database(format!("JSON deserialization error: {}", e))
                        })
                    })
                    .collect()
            })
            .await
    }

    async fn get_by_role(&self, role: ProfileRole) -> AppResult<Vec<AgentProfile>> {
        let role_str = role.to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT profile_json FROM agent_profiles WHERE role = ?1 ORDER BY name ASC",
                )?;
                let jsons = stmt
                    .query_map(rusqlite::params![role_str], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<String>, _>>()?;
                jsons
                    .into_iter()
                    .map(|json| {
                        serde_json::from_str(&json).map_err(|e| {
                            AppError::Database(format!("JSON deserialization error: {}", e))
                        })
                    })
                    .collect()
            })
            .await
    }

    async fn get_builtin(&self) -> AppResult<Vec<AgentProfile>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT profile_json FROM agent_profiles WHERE is_builtin = 1 ORDER BY name ASC",
                )?;
                let jsons = stmt
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<String>, _>>()?;
                jsons
                    .into_iter()
                    .map(|json| {
                        serde_json::from_str(&json).map_err(|e| {
                            AppError::Database(format!("JSON deserialization error: {}", e))
                        })
                    })
                    .collect()
            })
            .await
    }

    async fn get_custom(&self) -> AppResult<Vec<AgentProfile>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT profile_json FROM agent_profiles WHERE is_builtin = 0 ORDER BY name ASC",
                )?;
                let jsons = stmt
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<Result<Vec<String>, _>>()?;
                jsons
                    .into_iter()
                    .map(|json| {
                        serde_json::from_str(&json).map_err(|e| {
                            AppError::Database(format!("JSON deserialization error: {}", e))
                        })
                    })
                    .collect()
            })
            .await
    }

    async fn update(&self, id: &AgentProfileId, profile: &AgentProfile) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        let name = profile.name.clone();
        let role = profile.role.to_string();
        let profile_json = serde_json::to_string(profile)
            .map_err(|e| AppError::Database(format!("JSON serialization error: {}", e)))?;

        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE agent_profiles SET name = ?2, role = ?3, profile_json = ?4, updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                     WHERE id = ?1",
                    rusqlite::params![id_str, name, role, profile_json],
                )?;
                Ok(())
            })
            .await
    }

    async fn delete(&self, id: &AgentProfileId) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM agent_profiles WHERE id = ?1",
                    rusqlite::params![id_str],
                )?;
                Ok(())
            })
            .await
    }

    async fn exists_by_name(&self, name: &str) -> AppResult<bool> {
        let name_str = name.to_string();
        self.db
            .run(move |conn| {
                let count: i32 = conn.query_row(
                    "SELECT COUNT(*) FROM agent_profiles WHERE name = ?1",
                    rusqlite::params![name_str],
                    |row| row.get(0),
                )?;
                Ok(count > 0)
            })
            .await
    }

    async fn seed_builtin_profiles(&self) -> AppResult<()> {
        let profiles = AgentProfile::builtin_profiles();
        self.db
            .run(move |conn| {
                for profile in profiles {
                    let profile_json = serde_json::to_string(&profile).map_err(|e| {
                        AppError::Database(format!("JSON serialization error: {}", e))
                    })?;
                    conn.execute(
                        "INSERT OR IGNORE INTO agent_profiles (id, name, role, profile_json, is_builtin)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![
                            profile.id,
                            profile.name,
                            profile.role.to_string(),
                            profile_json,
                            1i32,
                        ],
                    )?;
                }
                Ok(())
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_agent_profile_repo_tests.rs"]
mod tests;

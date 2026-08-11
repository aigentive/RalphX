use std::error::Error;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::Mutex;

use crate::domain::agents::{ManualRoleDefault, RoutingRole, StoredManualRoleDefault};
use crate::domain::repositories::{
    manual_role_default_repository::ManualRoleDefaultRepositoryResult, ManualRoleDefaultRepository,
};
use crate::error::{AppError, AppResult};

use super::DbConnection;

pub struct SqliteManualRoleDefaultRepository {
    db: DbConnection,
}

impl SqliteManualRoleDefaultRepository {
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

fn parse_row(row: &rusqlite::Row<'_>) -> AppResult<StoredManualRoleDefault> {
    let role = row
        .get::<_, String>("role")
        .map_err(database_error)?
        .parse::<RoutingRole>()
        .map_err(AppError::Database)?;
    let value_json = row.get::<_, String>("value_json").map_err(database_error)?;
    let value = serde_json::from_str::<ManualRoleDefault>(&value_json).map_err(|error| {
        AppError::Database(format!("invalid manual role default JSON: {error}"))
    })?;
    let updated_at = row
        .get::<_, String>("updated_at")
        .map_err(database_error)
        .and_then(|value| {
            DateTime::parse_from_rfc3339(&value)
                .map(|date| date.with_timezone(&Utc))
                .map_err(|error| {
                    AppError::Database(format!("invalid manual role default timestamp: {error}"))
                })
        })?;

    Ok(StoredManualRoleDefault {
        id: row.get("id").map_err(database_error)?,
        project_id: match row.get::<_, String>("scope_id").map_err(database_error)? {
            value if value.is_empty() => None,
            value => Some(value),
        },
        role,
        value,
        updated_at,
    })
}

fn database_error(error: rusqlite::Error) -> AppError {
    AppError::Database(error.to_string())
}

fn fetch_optional(
    conn: &Connection,
    scope_type: &str,
    scope_id: &str,
    role: RoutingRole,
) -> AppResult<Option<StoredManualRoleDefault>> {
    conn.query_row(
        "SELECT id, scope_id, role, value_json, updated_at
         FROM manual_role_defaults
         WHERE scope_type = ?1 AND scope_id = ?2 AND role = ?3",
        rusqlite::params![scope_type, scope_id, role.to_string()],
        |row| {
            parse_row(row).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        },
    )
    .optional()
    .map_err(database_error)
}

fn fetch_many(
    conn: &Connection,
    scope_type: &str,
    scope_id: &str,
) -> AppResult<Vec<StoredManualRoleDefault>> {
    let mut statement = conn
        .prepare(
            "SELECT id, scope_id, role, value_json, updated_at
             FROM manual_role_defaults
             WHERE scope_type = ?1 AND scope_id = ?2
             ORDER BY role",
        )
        .map_err(database_error)?;
    let rows = statement
        .query_map(rusqlite::params![scope_type, scope_id], |row| {
            parse_row(row).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .map_err(database_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_error)?;
    Ok(rows)
}

fn upsert(
    conn: &Connection,
    scope_type: &str,
    scope_id: &str,
    role: RoutingRole,
    value: &ManualRoleDefault,
) -> AppResult<StoredManualRoleDefault> {
    let value_json = serde_json::to_string(value)
        .map_err(|error| AppError::Database(format!("serialize manual role default: {error}")))?;
    conn.execute(
        "INSERT INTO manual_role_defaults (scope_type, scope_id, role, value_json)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(scope_type, scope_id, role) DO UPDATE SET
             value_json = excluded.value_json,
             updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')",
        rusqlite::params![scope_type, scope_id, role.to_string(), value_json],
    )
    .map_err(database_error)?;
    fetch_optional(conn, scope_type, scope_id, role)?.ok_or_else(|| {
        AppError::Database("manual role default disappeared after upsert".to_string())
    })
}

fn boxed<T>(result: AppResult<T>) -> ManualRoleDefaultRepositoryResult<T> {
    result.map_err(|error| Box::new(error) as Box<dyn Error + Send + Sync>)
}

#[async_trait]
impl ManualRoleDefaultRepository for SqliteManualRoleDefaultRepository {
    async fn get_global(
        &self,
        role: RoutingRole,
    ) -> ManualRoleDefaultRepositoryResult<Option<StoredManualRoleDefault>> {
        boxed(
            self.db
                .run(move |conn| fetch_optional(conn, "global", "", role))
                .await,
        )
    }

    async fn get_for_project(
        &self,
        project_id: &str,
        role: RoutingRole,
    ) -> ManualRoleDefaultRepositoryResult<Option<StoredManualRoleDefault>> {
        let project_id = project_id.to_string();
        boxed(
            self.db
                .run(move |conn| fetch_optional(conn, "project", &project_id, role))
                .await,
        )
    }

    async fn list_global(&self) -> ManualRoleDefaultRepositoryResult<Vec<StoredManualRoleDefault>> {
        boxed(self.db.run(|conn| fetch_many(conn, "global", "")).await)
    }

    async fn list_for_project(
        &self,
        project_id: &str,
    ) -> ManualRoleDefaultRepositoryResult<Vec<StoredManualRoleDefault>> {
        let project_id = project_id.to_string();
        boxed(
            self.db
                .run(move |conn| fetch_many(conn, "project", &project_id))
                .await,
        )
    }

    async fn upsert_global(
        &self,
        role: RoutingRole,
        value: &ManualRoleDefault,
    ) -> ManualRoleDefaultRepositoryResult<StoredManualRoleDefault> {
        let value = value.clone();
        boxed(
            self.db
                .run(move |conn| upsert(conn, "global", "", role, &value))
                .await,
        )
    }

    async fn upsert_for_project(
        &self,
        project_id: &str,
        role: RoutingRole,
        value: &ManualRoleDefault,
    ) -> ManualRoleDefaultRepositoryResult<StoredManualRoleDefault> {
        let project_id = project_id.to_string();
        let value = value.clone();
        boxed(
            self.db
                .run(move |conn| upsert(conn, "project", &project_id, role, &value))
                .await,
        )
    }

    async fn clear_global(&self, role: RoutingRole) -> ManualRoleDefaultRepositoryResult<bool> {
        boxed(
            self.db
                .run(move |conn| {
                    conn.execute(
                        "DELETE FROM manual_role_defaults
                         WHERE scope_type = 'global' AND scope_id = '' AND role = ?1",
                        [role.to_string()],
                    )
                    .map(|count| count > 0)
                    .map_err(database_error)
                })
                .await,
        )
    }

    async fn clear_for_project(
        &self,
        project_id: &str,
        role: RoutingRole,
    ) -> ManualRoleDefaultRepositoryResult<bool> {
        let project_id = project_id.to_string();
        boxed(
            self.db
                .run(move |conn| {
                    conn.execute(
                        "DELETE FROM manual_role_defaults
                         WHERE scope_type = 'project' AND scope_id = ?1 AND role = ?2",
                        rusqlite::params![project_id, role.to_string()],
                    )
                    .map(|count| count > 0)
                    .map_err(database_error)
                })
                .await,
        )
    }
}

#[cfg(test)]
#[path = "sqlite_manual_role_default_repo_tests.rs"]
mod tests;

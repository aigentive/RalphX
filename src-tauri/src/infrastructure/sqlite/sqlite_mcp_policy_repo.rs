use std::collections::BTreeMap;
use std::error::Error;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::Mutex;

use crate::domain::agents::{
    validate_mcp_identifier, AgentHarnessKind, McpOverrideState, McpPolicyOverride, McpServerKey,
};
use crate::domain::repositories::{
    mcp_policy_repository::McpPolicyRepositoryResult, McpPolicyRepository,
};
use crate::error::{AppError, AppResult};

use super::DbConnection;

pub struct SqliteMcpPolicyRepository {
    db: DbConnection,
}

impl SqliteMcpPolicyRepository {
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

fn scope(project_id: Option<&str>) -> (&'static str, &str) {
    match project_id {
        Some(project_id) => ("project", project_id),
        None => ("global", ""),
    }
}

fn database_error(error: rusqlite::Error) -> AppError {
    AppError::Database(error.to_string())
}

fn parse_row(row: &rusqlite::Row<'_>) -> AppResult<McpPolicyOverride> {
    let provider = row
        .get::<_, String>("provider")
        .map_err(database_error)?
        .parse::<AgentHarnessKind>()
        .map_err(AppError::Database)?;
    let server_id = row.get::<_, String>("server_id").map_err(database_error)?;
    let tool_states_json = row
        .get::<_, String>("tool_states_json")
        .map_err(database_error)?;
    let tool_states = serde_json::from_str::<BTreeMap<String, McpOverrideState>>(&tool_states_json)
        .map_err(|error| AppError::Database(format!("invalid MCP tool policy JSON: {error}")))?;
    let updated_at =
        DateTime::parse_from_rfc3339(&row.get::<_, String>("updated_at").map_err(database_error)?)
            .map(|value| value.with_timezone(&Utc))
            .map_err(|error| {
                AppError::Database(format!("invalid MCP policy timestamp: {error}"))
            })?;
    let project_id = match row.get::<_, String>("scope_id").map_err(database_error)? {
        value if value.is_empty() => None,
        value => Some(value),
    };
    let policy = McpPolicyOverride {
        project_id,
        key: McpServerKey::new(provider, server_id).map_err(AppError::Database)?,
        server_state: row
            .get::<_, String>("server_state")
            .map_err(database_error)?
            .parse::<McpOverrideState>()
            .map_err(AppError::Database)?,
        tool_states,
        updated_at,
    };
    policy.validate().map_err(AppError::Database)?;
    Ok(policy)
}

fn fetch_optional(
    conn: &Connection,
    project_id: Option<&str>,
    key: &McpServerKey,
) -> AppResult<Option<McpPolicyOverride>> {
    let (scope_type, scope_id) = scope(project_id);
    conn.query_row(
        "SELECT scope_id, provider, server_id, server_state, tool_states_json, updated_at
         FROM mcp_policy_overrides
         WHERE scope_type = ?1 AND scope_id = ?2 AND provider = ?3 AND server_id = ?4",
        rusqlite::params![
            scope_type,
            scope_id,
            key.provider.to_string(),
            key.server_id
        ],
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

fn fetch_many(conn: &Connection, project_id: Option<&str>) -> AppResult<Vec<McpPolicyOverride>> {
    let (scope_type, scope_id) = scope(project_id);
    let mut statement = conn
        .prepare(
            "SELECT scope_id, provider, server_id, server_state, tool_states_json, updated_at
             FROM mcp_policy_overrides
             WHERE scope_type = ?1 AND scope_id = ?2
             ORDER BY provider, server_id",
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

fn upsert(conn: &Connection, policy: &McpPolicyOverride) -> AppResult<McpPolicyOverride> {
    policy.validate().map_err(AppError::Validation)?;
    let (scope_type, scope_id) = scope(policy.project_id.as_deref());
    let tool_states_json = serde_json::to_string(&policy.tool_states)
        .map_err(|error| AppError::Database(format!("serialize MCP tool policy: {error}")))?;
    conn.execute(
        "INSERT INTO mcp_policy_overrides
            (scope_type, scope_id, provider, server_id, server_state, tool_states_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(scope_type, scope_id, provider, server_id) DO UPDATE SET
            server_state = excluded.server_state,
            tool_states_json = excluded.tool_states_json,
            updated_at = strftime('%Y-%m-%dT%H:%M:%f+00:00', 'now')",
        rusqlite::params![
            scope_type,
            scope_id,
            policy.key.provider.to_string(),
            policy.key.server_id,
            policy.server_state.to_string(),
            tool_states_json,
        ],
    )
    .map_err(database_error)?;
    fetch_optional(conn, policy.project_id.as_deref(), &policy.key)?
        .ok_or_else(|| AppError::Database("MCP policy disappeared after upsert".to_string()))
}

fn mutate(
    conn: &Connection,
    project_id: Option<&str>,
    key: &McpServerKey,
    mutation: impl FnOnce(&mut McpPolicyOverride),
) -> AppResult<McpPolicyOverride> {
    let mut policy = fetch_optional(conn, project_id, key)?.unwrap_or_else(|| McpPolicyOverride {
        project_id: project_id.map(str::to_string),
        key: key.clone(),
        server_state: McpOverrideState::Follow,
        tool_states: BTreeMap::new(),
        updated_at: Utc::now(),
    });
    mutation(&mut policy);
    upsert(conn, &policy)
}

fn boxed<T>(result: AppResult<T>) -> McpPolicyRepositoryResult<T> {
    result.map_err(|error| Box::new(error) as Box<dyn Error + Send + Sync>)
}

#[async_trait]
impl McpPolicyRepository for SqliteMcpPolicyRepository {
    async fn list_global(&self) -> McpPolicyRepositoryResult<Vec<McpPolicyOverride>> {
        boxed(self.db.run(|conn| fetch_many(conn, None)).await)
    }

    async fn list_for_project(
        &self,
        project_id: &str,
    ) -> McpPolicyRepositoryResult<Vec<McpPolicyOverride>> {
        let project_id = project_id.to_string();
        boxed(
            self.db
                .run(move |conn| fetch_many(conn, Some(&project_id)))
                .await,
        )
    }

    async fn get_global(
        &self,
        key: &McpServerKey,
    ) -> McpPolicyRepositoryResult<Option<McpPolicyOverride>> {
        let key = key.clone();
        boxed(
            self.db
                .run(move |conn| fetch_optional(conn, None, &key))
                .await,
        )
    }

    async fn get_for_project(
        &self,
        project_id: &str,
        key: &McpServerKey,
    ) -> McpPolicyRepositoryResult<Option<McpPolicyOverride>> {
        let project_id = project_id.to_string();
        let key = key.clone();
        boxed(
            self.db
                .run(move |conn| fetch_optional(conn, Some(&project_id), &key))
                .await,
        )
    }

    async fn set_server_state(
        &self,
        project_id: Option<&str>,
        key: &McpServerKey,
        state: McpOverrideState,
    ) -> McpPolicyRepositoryResult<McpPolicyOverride> {
        let project_id = project_id.map(str::to_string);
        let key = key.clone();
        boxed(
            self.db
                .run(move |conn| {
                    mutate(conn, project_id.as_deref(), &key, |policy| {
                        policy.server_state = state;
                    })
                })
                .await,
        )
    }

    async fn set_tool_state(
        &self,
        project_id: Option<&str>,
        key: &McpServerKey,
        tool_name: &str,
        state: McpOverrideState,
    ) -> McpPolicyRepositoryResult<McpPolicyOverride> {
        validate_mcp_identifier("tool", tool_name)?;
        let project_id = project_id.map(str::to_string);
        let key = key.clone();
        let tool_name = tool_name.to_string();
        boxed(
            self.db
                .run(move |conn| {
                    mutate(conn, project_id.as_deref(), &key, |policy| {
                        policy.tool_states.insert(tool_name, state);
                    })
                })
                .await,
        )
    }

    async fn clear_server(
        &self,
        project_id: Option<&str>,
        key: &McpServerKey,
    ) -> McpPolicyRepositoryResult<bool> {
        let project_id = project_id.map(str::to_string);
        let key = key.clone();
        boxed(
            self.db
                .run(move |conn| {
                    let Some(mut policy) = fetch_optional(conn, project_id.as_deref(), &key)? else {
                        return Ok(false);
                    };
                    if policy.server_state == McpOverrideState::Follow {
                        return Ok(false);
                    }
                    policy.server_state = McpOverrideState::Follow;
                    if policy.tool_states.is_empty() {
                        let (scope_type, scope_id) = scope(project_id.as_deref());
                        conn.execute(
                            "DELETE FROM mcp_policy_overrides
                             WHERE scope_type = ?1 AND scope_id = ?2 AND provider = ?3 AND server_id = ?4",
                            rusqlite::params![scope_type, scope_id, key.provider.to_string(), key.server_id],
                        )
                        .map_err(database_error)?;
                    } else {
                        upsert(conn, &policy)?;
                    }
                    Ok(true)
                })
                .await,
        )
    }

    async fn clear_tool(
        &self,
        project_id: Option<&str>,
        key: &McpServerKey,
        tool_name: &str,
    ) -> McpPolicyRepositoryResult<bool> {
        validate_mcp_identifier("tool", tool_name)?;
        let project_id = project_id.map(str::to_string);
        let key = key.clone();
        let tool_name = tool_name.to_string();
        boxed(
            self.db
                .run(move |conn| {
                    let Some(mut policy) = fetch_optional(conn, project_id.as_deref(), &key)? else {
                        return Ok(false);
                    };
                    let removed = policy.tool_states.remove(&tool_name).is_some();
                    if !removed {
                        return Ok(false);
                    }
                    if policy.server_state == McpOverrideState::Follow
                        && policy.tool_states.is_empty()
                    {
                        let (scope_type, scope_id) = scope(project_id.as_deref());
                        conn.execute(
                            "DELETE FROM mcp_policy_overrides
                             WHERE scope_type = ?1 AND scope_id = ?2 AND provider = ?3 AND server_id = ?4",
                            rusqlite::params![scope_type, scope_id, key.provider.to_string(), key.server_id],
                        )
                        .map_err(database_error)?;
                    } else {
                        upsert(conn, &policy)?;
                    }
                    Ok(true)
                })
                .await,
        )
    }
}

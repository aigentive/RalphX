use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::Connection;
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::agents::{AgentHarnessKind, AgentProviderSettings, LogicalEffort};
use crate::domain::repositories::AgentProviderSettingsRepository;
use crate::error::{AppError, AppResult};

pub struct SqliteAgentProviderSettingsRepository {
    db: DbConnection,
}

impl SqliteAgentProviderSettingsRepository {
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

fn parse_row(row: &rusqlite::Row<'_>) -> AppResult<AgentProviderSettings> {
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

    Ok(AgentProviderSettings {
        provider,
        enabled: row
            .get::<_, i64>("enabled")
            .map_err(|e| AppError::Database(e.to_string()))?
            != 0,
        is_default: row
            .get::<_, i64>("is_default")
            .map_err(|e| AppError::Database(e.to_string()))?
            != 0,
        model: row
            .get("model")
            .map_err(|e| AppError::Database(e.to_string()))?,
        effort,
        approval_policy: row
            .get("approval_policy")
            .map_err(|e| AppError::Database(e.to_string()))?,
        sandbox_mode: row
            .get("sandbox_mode")
            .map_err(|e| AppError::Database(e.to_string()))?,
        claude_permission_mode: row
            .get("claude_permission_mode")
            .map_err(|e| AppError::Database(e.to_string()))?,
        claude_dangerously_skip_permissions: row
            .get::<_, i64>("claude_dangerously_skip_permissions")
            .map_err(|e| AppError::Database(e.to_string()))?
            != 0,
        claude_allow_dangerously_skip_permissions: row
            .get::<_, i64>("claude_allow_dangerously_skip_permissions")
            .map_err(|e| AppError::Database(e.to_string()))?
            != 0,
        updated_at,
    })
}

fn select_columns() -> &'static str {
    "provider, enabled, is_default, model, effort, approval_policy, sandbox_mode,
     claude_permission_mode, claude_dangerously_skip_permissions,
     claude_allow_dangerously_skip_permissions, updated_at"
}

fn fetch_optional<P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> AppResult<Option<AgentProviderSettings>> {
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
) -> AppResult<Vec<AgentProviderSettings>> {
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
impl AgentProviderSettingsRepository for SqliteAgentProviderSettingsRepository {
    async fn get(
        &self,
        provider: AgentHarnessKind,
    ) -> Result<Option<AgentProviderSettings>, Box<dyn std::error::Error>> {
        let provider = provider.to_string();
        self.db
            .run(move |conn| {
                fetch_optional(
                    conn,
                    &format!(
                        "SELECT {} FROM agent_provider_settings WHERE provider = ?1",
                        select_columns()
                    ),
                    rusqlite::params![provider],
                )
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn list(&self) -> Result<Vec<AgentProviderSettings>, Box<dyn std::error::Error>> {
        self.db
            .run(move |conn| {
                fetch_many(
                    conn,
                    &format!(
                        "SELECT {} FROM agent_provider_settings ORDER BY provider",
                        select_columns()
                    ),
                    [],
                )
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn get_default(
        &self,
    ) -> Result<Option<AgentProviderSettings>, Box<dyn std::error::Error>> {
        self.db
            .run(move |conn| {
                fetch_optional(
                    conn,
                    &format!(
                        "SELECT {} FROM agent_provider_settings WHERE is_default = 1",
                        select_columns()
                    ),
                    [],
                )
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn upsert(
        &self,
        settings: &AgentProviderSettings,
    ) -> Result<AgentProviderSettings, Box<dyn std::error::Error>> {
        let settings = settings.clone();
        self.db
            .run_transaction(move |conn| {
                if settings.is_default {
                    conn.execute(
                        "UPDATE agent_provider_settings SET is_default = 0 WHERE is_default = 1",
                        [],
                    )
                    .map_err(|e| AppError::Database(e.to_string()))?;
                }
                conn.execute(
                    "INSERT INTO agent_provider_settings (
                        provider, enabled, is_default, model, effort, approval_policy,
                        sandbox_mode, claude_permission_mode,
                        claude_dangerously_skip_permissions,
                        claude_allow_dangerously_skip_permissions, updated_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                        strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
                     ON CONFLICT(provider) DO UPDATE SET
                        enabled = excluded.enabled,
                        is_default = excluded.is_default,
                        model = excluded.model,
                        effort = excluded.effort,
                        approval_policy = excluded.approval_policy,
                        sandbox_mode = excluded.sandbox_mode,
                        claude_permission_mode = excluded.claude_permission_mode,
                        claude_dangerously_skip_permissions =
                            excluded.claude_dangerously_skip_permissions,
                        claude_allow_dangerously_skip_permissions =
                            excluded.claude_allow_dangerously_skip_permissions,
                        updated_at = excluded.updated_at",
                    rusqlite::params![
                        settings.provider.to_string(),
                        settings.enabled as i64,
                        settings.is_default as i64,
                        settings.model,
                        settings.effort.map(|value| value.to_string()),
                        settings.approval_policy,
                        settings.sandbox_mode,
                        settings.claude_permission_mode,
                        settings.claude_dangerously_skip_permissions as i64,
                        settings.claude_allow_dangerously_skip_permissions as i64,
                    ],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
                let provider = settings.provider.to_string();
                fetch_optional(
                    conn,
                    &format!(
                        "SELECT {} FROM agent_provider_settings WHERE provider = ?1",
                        select_columns()
                    ),
                    rusqlite::params![provider],
                )?
                .ok_or_else(|| {
                    AppError::Database("Provider settings row missing after upsert".to_string())
                })
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }
}

#[cfg(test)]
#[path = "sqlite_agent_provider_settings_repo_tests.rs"]
mod tests;

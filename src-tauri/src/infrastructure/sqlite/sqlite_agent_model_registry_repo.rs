use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::Connection;
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::agents::{
    AgentHarnessKind, AgentModelDefinition, AgentModelSource, LogicalEffort,
};
use crate::domain::repositories::AgentModelRegistryRepository;
use crate::error::{AppError, AppResult};

pub struct SqliteAgentModelRegistryRepository {
    db: DbConnection,
}

impl SqliteAgentModelRegistryRepository {
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

fn parse_datetime(value: &str) -> DateTime<Utc> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return dt.with_timezone(&Utc);
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return Utc.from_utc_datetime(&ndt);
    }
    Utc::now()
}

fn parse_efforts(value: &str) -> AppResult<Vec<LogicalEffort>> {
    let raw: Vec<String> =
        serde_json::from_str(value).map_err(|error| AppError::Database(error.to_string()))?;
    raw.into_iter()
        .map(|effort| effort.parse::<LogicalEffort>().map_err(AppError::Database))
        .collect()
}

fn serialize_efforts(efforts: &[LogicalEffort]) -> AppResult<String> {
    serde_json::to_string(&efforts.iter().map(ToString::to_string).collect::<Vec<_>>())
        .map_err(|error| AppError::Database(error.to_string()))
}

fn parse_model_row(row: &rusqlite::Row<'_>) -> AppResult<AgentModelDefinition> {
    let provider = row
        .get::<_, String>("provider")
        .map_err(|error| AppError::Database(error.to_string()))?
        .parse::<AgentHarnessKind>()
        .map_err(AppError::Database)?;
    let default_effort = row
        .get::<_, String>("default_effort")
        .map_err(|error| AppError::Database(error.to_string()))?
        .parse::<LogicalEffort>()
        .map_err(AppError::Database)?;
    let supported_efforts = parse_efforts(
        &row.get::<_, String>("supported_efforts")
            .map_err(|error| AppError::Database(error.to_string()))?,
    )?;
    let source = match row
        .get::<_, String>("source")
        .map_err(|error| AppError::Database(error.to_string()))?
        .as_str()
    {
        "custom" => AgentModelSource::Custom,
        "built_in" => AgentModelSource::BuiltIn,
        other => {
            return Err(AppError::Database(format!(
                "Invalid agent model source '{other}'"
            )))
        }
    };
    let created_at = row
        .get::<_, String>("created_at")
        .ok()
        .map(|value| parse_datetime(&value));
    let updated_at = row
        .get::<_, String>("updated_at")
        .ok()
        .map(|value| parse_datetime(&value));

    Ok(AgentModelDefinition {
        provider,
        model_id: row
            .get("model_id")
            .map_err(|error| AppError::Database(error.to_string()))?,
        label: row
            .get("label")
            .map_err(|error| AppError::Database(error.to_string()))?,
        menu_label: row
            .get("menu_label")
            .map_err(|error| AppError::Database(error.to_string()))?,
        description: row
            .get("description")
            .map_err(|error| AppError::Database(error.to_string()))?,
        supported_efforts,
        default_effort,
        source,
        enabled: row
            .get::<_, i64>("enabled")
            .map_err(|error| AppError::Database(error.to_string()))?
            != 0,
        created_at,
        updated_at,
    }
    .normalized())
}

fn fetch_custom_models(conn: &Connection) -> AppResult<Vec<AgentModelDefinition>> {
    let mut stmt = conn
        .prepare(
            "SELECT provider, model_id, label, menu_label, description, default_effort,
                    supported_efforts, source, enabled, created_at, updated_at
             FROM agent_model_registry
             WHERE source = 'custom'
             ORDER BY provider, menu_label",
        )
        .map_err(|error| AppError::Database(error.to_string()))?;
    let rows = stmt
        .query_map([], |row| {
            parse_model_row(row).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .map_err(|error| AppError::Database(error.to_string()))?;
    let models = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| AppError::Database(error.to_string()))?;
    Ok(models)
}

#[async_trait]
impl AgentModelRegistryRepository for SqliteAgentModelRegistryRepository {
    async fn list_custom_models(
        &self,
    ) -> Result<Vec<AgentModelDefinition>, Box<dyn std::error::Error>> {
        self.db
            .run(fetch_custom_models)
            .await
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
    }

    async fn upsert_custom_model(
        &self,
        model: &AgentModelDefinition,
    ) -> Result<AgentModelDefinition, Box<dyn std::error::Error>> {
        let mut model = model.clone().normalized();
        model.source = AgentModelSource::Custom;
        let provider = model.provider.to_string();
        let model_id = model.model_id.clone();
        let label = model.label.clone();
        let menu_label = model.menu_label.clone();
        let description = model.description.clone();
        let default_effort = model.default_effort.to_string();
        let enabled = if model.enabled { 1_i64 } else { 0_i64 };
        let supported_efforts = serialize_efforts(&model.supported_efforts)
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)?;
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO agent_model_registry (
                        provider, model_id, label, menu_label, description, default_effort,
                        supported_efforts, source, enabled, created_at, updated_at
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, 'custom', ?8,
                        strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'),
                        strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                    )
                    ON CONFLICT(provider, model_id) DO UPDATE SET
                        label = excluded.label,
                        menu_label = excluded.menu_label,
                        description = excluded.description,
                        default_effort = excluded.default_effort,
                        supported_efforts = excluded.supported_efforts,
                        source = 'custom',
                        enabled = excluded.enabled,
                        updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')",
                    rusqlite::params![
                        provider,
                        model_id,
                        label,
                        menu_label,
                        description,
                        default_effort,
                        supported_efforts,
                        enabled,
                    ],
                )
                .map_err(|error| AppError::Database(error.to_string()))?;
                let mut models = fetch_custom_models(conn)?;
                let saved = models
                    .drain(..)
                    .find(|candidate| {
                        candidate.provider == model.provider && candidate.model_id == model.model_id
                    })
                    .ok_or_else(|| {
                        AppError::Database(format!(
                            "Custom model '{}' was not found after upsert",
                            model.model_id
                        ))
                    })?;
                Ok(saved)
            })
            .await
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
    }

    async fn delete_custom_model(
        &self,
        provider: AgentHarnessKind,
        model_id: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let provider = provider.to_string();
        let model_id = model_id.to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM agent_model_registry
                     WHERE provider = ?1 AND model_id = ?2 AND source = 'custom'",
                    rusqlite::params![provider, model_id],
                )
                .map(|count| count > 0)
                .map_err(|error| AppError::Database(error.to_string()))
            })
            .await
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
    }
}

#[cfg(test)]
#[path = "sqlite_agent_model_registry_repo_tests.rs"]
mod tests;

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::Connection;
use tokio::sync::Mutex;

use super::DbConnection;
use crate::domain::agents::{
    AgentHarnessKind, AgentLane, AgentLaneSettings, LogicalEffort, StoredAgentLaneSettings,
};
use crate::domain::repositories::AgentLaneSettingsRepository;
use crate::error::{AppError, AppResult};

pub struct SqliteAgentLaneSettingsRepository {
    db: DbConnection,
}

impl SqliteAgentLaneSettingsRepository {
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

fn parse_row(row: &rusqlite::Row<'_>) -> AppResult<StoredAgentLaneSettings> {
    let id: i64 = row.get("id").map_err(|e| AppError::Database(e.to_string()))?;
    let project_id: Option<String> = row
        .get("scope_id")
        .map_err(|e| AppError::Database(e.to_string()))?;
    let lane = row
        .get::<_, String>("lane")
        .map_err(|e| AppError::Database(e.to_string()))?
        .parse::<AgentLane>()
        .map_err(AppError::Database)?;
    let harness = row
        .get::<_, String>("harness")
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

    Ok(StoredAgentLaneSettings {
        id,
        project_id,
        lane,
        settings: AgentLaneSettings {
            harness,
            model: row.get("model").map_err(|e| AppError::Database(e.to_string()))?,
            effort,
            approval_policy: row
                .get("approval_policy")
                .map_err(|e| AppError::Database(e.to_string()))?,
            sandbox_mode: row
                .get("sandbox_mode")
                .map_err(|e| AppError::Database(e.to_string()))?,
        },
        updated_at,
    })
}

fn fetch_optional<P: rusqlite::Params>(
    conn: &Connection,
    sql: &str,
    params: P,
) -> AppResult<Option<StoredAgentLaneSettings>> {
    match conn.query_row(sql, params, |row| {
        parse_row(row).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(err),
            )
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
) -> AppResult<Vec<StoredAgentLaneSettings>> {
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
impl AgentLaneSettingsRepository for SqliteAgentLaneSettingsRepository {
    async fn get_global(
        &self,
        lane: AgentLane,
    ) -> Result<Option<StoredAgentLaneSettings>, Box<dyn std::error::Error>> {
        let lane = lane.to_string();
        self.db
            .run(move |conn| {
                fetch_optional(
                    conn,
                    "SELECT id, scope_id, lane, harness, model, effort, approval_policy,
                            sandbox_mode, updated_at
                     FROM agent_lane_settings
                     WHERE scope_type = 'global' AND lane = ?1",
                    rusqlite::params![lane],
                )
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn get_for_project(
        &self,
        project_id: &str,
        lane: AgentLane,
    ) -> Result<Option<StoredAgentLaneSettings>, Box<dyn std::error::Error>> {
        let project_id = project_id.to_string();
        let lane = lane.to_string();
        self.db
            .run(move |conn| {
                fetch_optional(
                    conn,
                    "SELECT id, scope_id, lane, harness, model, effort, approval_policy,
                            sandbox_mode, updated_at
                     FROM agent_lane_settings
                     WHERE scope_type = 'project' AND scope_id = ?1 AND lane = ?2",
                    rusqlite::params![project_id, lane],
                )
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn list_global(&self) -> Result<Vec<StoredAgentLaneSettings>, Box<dyn std::error::Error>> {
        self.db
            .run(move |conn| {
                fetch_many(
                    conn,
                    "SELECT id, scope_id, lane, harness, model, effort, approval_policy,
                            sandbox_mode, updated_at
                     FROM agent_lane_settings
                     WHERE scope_type = 'global'
                     ORDER BY lane",
                    [],
                )
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn list_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<StoredAgentLaneSettings>, Box<dyn std::error::Error>> {
        let project_id = project_id.to_string();
        self.db
            .run(move |conn| {
                fetch_many(
                    conn,
                    "SELECT id, scope_id, lane, harness, model, effort, approval_policy,
                            sandbox_mode, updated_at
                     FROM agent_lane_settings
                     WHERE scope_type = 'project' AND scope_id = ?1
                     ORDER BY lane",
                    rusqlite::params![project_id],
                )
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn upsert_global(
        &self,
        lane: AgentLane,
        settings: &AgentLaneSettings,
    ) -> Result<StoredAgentLaneSettings, Box<dyn std::error::Error>> {
        let lane_key = lane.to_string();
        let harness = settings.harness.to_string();
        let model = settings.model.clone();
        let effort = settings.effort.map(|value| value.to_string());
        let approval_policy = settings.approval_policy.clone();
        let sandbox_mode = settings.sandbox_mode.clone();

        self.db
            .run(move |conn| {
                let exists: bool = conn
                    .query_row(
                        "SELECT COUNT(*) FROM agent_lane_settings
                         WHERE scope_type = 'global' AND lane = ?1",
                        rusqlite::params![lane_key.clone()],
                        |row| row.get::<_, i64>(0),
                    )
                    .map_err(|e| AppError::Database(e.to_string()))?
                    > 0;

                if exists {
                    conn.execute(
                        "UPDATE agent_lane_settings
                         SET harness = ?1,
                             model = ?2,
                             effort = ?3,
                             approval_policy = ?4,
                             sandbox_mode = ?5,
                             updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                         WHERE scope_type = 'global' AND lane = ?6",
                        rusqlite::params![
                            harness,
                            model,
                            effort,
                            approval_policy,
                            sandbox_mode,
                            lane_key.clone(),
                        ],
                    )
                    .map_err(|e| AppError::Database(e.to_string()))?;
                } else {
                    conn.execute(
                        "INSERT INTO agent_lane_settings (
                            scope_type, scope_id, lane, harness, model, effort,
                            approval_policy, sandbox_mode, updated_at
                         ) VALUES (
                            'global', NULL, ?1, ?2, ?3, ?4, ?5, ?6,
                            strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                         )",
                        rusqlite::params![
                            lane_key.clone(),
                            harness,
                            model,
                            effort,
                            approval_policy,
                            sandbox_mode,
                        ],
                    )
                    .map_err(|e| AppError::Database(e.to_string()))?;
                }

                fetch_optional(
                    conn,
                    "SELECT id, scope_id, lane, harness, model, effort, approval_policy,
                            sandbox_mode, updated_at
                     FROM agent_lane_settings
                     WHERE scope_type = 'global' AND lane = ?1",
                    rusqlite::params![lane_key],
                )?
                .ok_or_else(|| AppError::Database("Global lane settings row missing after upsert".to_string()))
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    async fn upsert_for_project(
        &self,
        project_id: &str,
        lane: AgentLane,
        settings: &AgentLaneSettings,
    ) -> Result<StoredAgentLaneSettings, Box<dyn std::error::Error>> {
        let project_id = project_id.to_string();
        let lane_key = lane.to_string();
        let harness = settings.harness.to_string();
        let model = settings.model.clone();
        let effort = settings.effort.map(|value| value.to_string());
        let approval_policy = settings.approval_policy.clone();
        let sandbox_mode = settings.sandbox_mode.clone();

        self.db
            .run(move |conn| {
                let exists: bool = conn
                    .query_row(
                        "SELECT COUNT(*) FROM agent_lane_settings
                         WHERE scope_type = 'project' AND scope_id = ?1 AND lane = ?2",
                        rusqlite::params![project_id.clone(), lane_key.clone()],
                        |row| row.get::<_, i64>(0),
                    )
                    .map_err(|e| AppError::Database(e.to_string()))?
                    > 0;

                if exists {
                    conn.execute(
                        "UPDATE agent_lane_settings
                         SET harness = ?1,
                             model = ?2,
                             effort = ?3,
                             approval_policy = ?4,
                             sandbox_mode = ?5,
                             updated_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                         WHERE scope_type = 'project' AND scope_id = ?6 AND lane = ?7",
                        rusqlite::params![
                            harness,
                            model,
                            effort,
                            approval_policy,
                            sandbox_mode,
                            project_id.clone(),
                            lane_key.clone(),
                        ],
                    )
                    .map_err(|e| AppError::Database(e.to_string()))?;
                } else {
                    conn.execute(
                        "INSERT INTO agent_lane_settings (
                            scope_type, scope_id, lane, harness, model, effort,
                            approval_policy, sandbox_mode, updated_at
                         ) VALUES (
                            'project', ?1, ?2, ?3, ?4, ?5, ?6, ?7,
                            strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                         )",
                        rusqlite::params![
                            project_id.clone(),
                            lane_key.clone(),
                            harness,
                            model,
                            effort,
                            approval_policy,
                            sandbox_mode,
                        ],
                    )
                    .map_err(|e| AppError::Database(e.to_string()))?;
                }

                fetch_optional(
                    conn,
                    "SELECT id, scope_id, lane, harness, model, effort, approval_policy,
                            sandbox_mode, updated_at
                     FROM agent_lane_settings
                     WHERE scope_type = 'project' AND scope_id = ?1 AND lane = ?2",
                    rusqlite::params![project_id, lane_key],
                )?
                .ok_or_else(|| AppError::Database("Project lane settings row missing after upsert".to_string()))
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }
}

#[cfg(test)]
#[path = "sqlite_agent_lane_settings_repo_tests.rs"]
mod tests;

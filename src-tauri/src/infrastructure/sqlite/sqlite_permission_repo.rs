use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use rusqlite::Connection;

use super::DbConnection;
use crate::domain::entities::permission_request::{PendingPermissionInfo, PermissionDecision};
use crate::domain::repositories::permission_repository::PermissionRepository;
use crate::error::{AppError, AppResult};

pub struct SqlitePermissionRepository {
    db: DbConnection,
}

impl SqlitePermissionRepository {
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
impl PermissionRepository for SqlitePermissionRepository {
    async fn create_pending(&self, info: &PendingPermissionInfo) -> AppResult<()> {
        let tool_input_json = serde_json::to_string(&info.tool_input)
            .map_err(|e| AppError::Database(e.to_string()))?;
        let request_id = info.request_id.clone();
        let tool_name = info.tool_name.clone();
        let context = info.context.clone();
        let agent_type = info.agent_type.clone();
        let task_id = info.task_id.clone();
        let context_type = info.context_type.clone();
        let context_id = info.context_id.clone();
        let created_at = info.created_at.clone();

        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO pending_permissions (request_id, tool_name, tool_input, context, status, agent_type, task_id, context_type, context_id, created_at)
                     VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6, ?7, ?8, ?9)",
                    rusqlite::params![request_id, tool_name, tool_input_json, context, agent_type, task_id, context_type, context_id, created_at],
                )?;
                Ok(())
            })
            .await
    }

    async fn resolve(&self, request_id: &str, decision: &PermissionDecision) -> AppResult<bool> {
        let request_id = request_id.to_string();
        let decision_val = decision.decision.clone();
        let decision_message = decision.message.clone();

        self.db
            .run(move |conn| {
                let rows = conn.execute(
                    "UPDATE pending_permissions
                     SET status = 'resolved',
                         decision = ?1,
                         decision_message = ?2,
                         resolved_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                     WHERE request_id = ?3 AND status = 'pending'",
                    rusqlite::params![decision_val, decision_message, request_id],
                )?;
                Ok(rows > 0)
            })
            .await
    }

    async fn get_pending(&self) -> AppResult<Vec<PendingPermissionInfo>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT request_id, tool_name, tool_input, context, agent_type, task_id, context_type, context_id, created_at
                     FROM pending_permissions WHERE status = 'pending'",
                )?;

                let mapped_rows = stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<String>>(7)?,
                        row.get::<_, String>(8)?,
                    ))
                })?;

                let mut results = Vec::new();
                for row_result in mapped_rows {
                    let (request_id, tool_name, tool_input_json, context, agent_type, task_id, context_type, context_id, created_at) = row_result?;
                    let tool_input: serde_json::Value = serde_json::from_str(&tool_input_json)
                        .map_err(|e| AppError::Database(e.to_string()))?;
                    results.push(PendingPermissionInfo {
                        request_id,
                        tool_name,
                        tool_input,
                        context,
                        agent_type,
                        task_id,
                        context_type,
                        context_id,
                        created_at,
                    });
                }

                Ok(results)
            })
            .await
    }

    async fn get_by_request_id(
        &self,
        request_id: &str,
    ) -> AppResult<Option<PendingPermissionInfo>> {
        let request_id = request_id.to_string();
        self.db
            .run(move |conn| {
                let result = conn.query_row(
                    "SELECT request_id, tool_name, tool_input, context, agent_type, task_id, context_type, context_id, created_at
                     FROM pending_permissions WHERE request_id = ?1",
                    rusqlite::params![request_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, Option<String>>(4)?,
                            row.get::<_, Option<String>>(5)?,
                            row.get::<_, Option<String>>(6)?,
                            row.get::<_, Option<String>>(7)?,
                            row.get::<_, String>(8)?,
                        ))
                    },
                );

                match result {
                    Ok((request_id, tool_name, tool_input_json, context, agent_type, task_id, context_type, context_id, created_at)) => {
                        let tool_input: serde_json::Value =
                            serde_json::from_str(&tool_input_json)
                                .map_err(|e| AppError::Database(e.to_string()))?;
                        Ok(Some(PendingPermissionInfo {
                            request_id,
                            tool_name,
                            tool_input,
                            context,
                            agent_type,
                            task_id,
                            context_type,
                            context_id,
                            created_at,
                        }))
                    }
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(AppError::Database(e.to_string())),
                }
            })
            .await
    }

    async fn expire_all_pending(&self) -> AppResult<u64> {
        self.db
            .run(move |conn| {
                let rows = conn.execute(
                    "UPDATE pending_permissions
                     SET status = 'expired',
                         resolved_at = strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now')
                     WHERE status = 'pending'",
                    [],
                )?;
                Ok(rows as u64)
            })
            .await
    }

    async fn remove(&self, request_id: &str) -> AppResult<bool> {
        let request_id = request_id.to_string();
        self.db
            .run(move |conn| {
                let rows = conn.execute(
                    "DELETE FROM pending_permissions WHERE request_id = ?1",
                    rusqlite::params![request_id],
                )?;
                Ok(rows > 0)
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_permission_repo_tests.rs"]
mod tests;

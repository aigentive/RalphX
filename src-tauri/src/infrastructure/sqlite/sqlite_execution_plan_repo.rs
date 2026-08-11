// SQLite-based ExecutionPlanRepository implementation

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use rusqlite::Connection;

use super::DbConnection;
use crate::domain::entities::{
    ExecutionPlan, ExecutionPlanHaltMode, ExecutionPlanId, ExecutionPlanStatus, IdeationSessionId,
};
use crate::domain::repositories::ExecutionPlanRepository;
use crate::error::{AppError, AppResult};

pub struct SqliteExecutionPlanRepository {
    db: DbConnection,
}

impl SqliteExecutionPlanRepository {
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
impl ExecutionPlanRepository for SqliteExecutionPlanRepository {
    async fn create(&self, plan: ExecutionPlan) -> AppResult<ExecutionPlan> {
        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO execution_plans (id, session_id, status, halt_mode, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![
                        plan.id.as_str(),
                        plan.session_id.as_str(),
                        plan.status.to_db_string(),
                        plan.halt_mode.to_db_string(),
                        plan.created_at.to_rfc3339(),
                    ],
                )
                .map_err(|e| {
                    AppError::Database(format!("Failed to create execution plan: {}", e))
                })?;
                Ok(plan)
            })
            .await
    }

    async fn get_by_id(&self, id: &ExecutionPlanId) -> AppResult<Option<ExecutionPlan>> {
        let id = id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT * FROM execution_plans WHERE id = ?1",
                    rusqlite::params![id.as_str()],
                    ExecutionPlan::from_row,
                )
            })
            .await
    }

    async fn get_by_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Vec<ExecutionPlan>> {
        let session_id = session_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn
                    .prepare("SELECT * FROM execution_plans WHERE session_id = ?1 ORDER BY created_at DESC")
                    .map_err(|e| AppError::Database(format!("Failed to prepare query: {}", e)))?;
                let plans = stmt
                    .query_map(rusqlite::params![session_id.as_str()], |row| {
                        ExecutionPlan::from_row(row)
                    })
                    .map_err(|e| AppError::Database(format!("Failed to query execution plans: {}", e)))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| AppError::Database(format!("Failed to collect execution plans: {}", e)))?;
                Ok(plans)
            })
            .await
    }

    async fn get_active_for_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Option<ExecutionPlan>> {
        let session_id = session_id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT * FROM execution_plans WHERE session_id = ?1 AND status = 'active' ORDER BY created_at DESC LIMIT 1",
                    rusqlite::params![session_id.as_str()],
                    ExecutionPlan::from_row,
                )
            })
            .await
    }

    async fn mark_superseded(&self, id: &ExecutionPlanId) -> AppResult<()> {
        let id = id.as_str().to_string();
        let id_display = id.clone();
        self.db
            .run(move |conn| {
                let rows = conn
                    .execute(
                        "UPDATE execution_plans SET status = ?1 WHERE id = ?2",
                        rusqlite::params![
                            ExecutionPlanStatus::Superseded.to_db_string(),
                            id.as_str()
                        ],
                    )
                    .map_err(|e| {
                        AppError::Database(format!(
                            "Failed to mark execution plan as superseded: {}",
                            e
                        ))
                    })?;
                if rows == 0 {
                    return Err(AppError::NotFound(format!(
                        "Execution plan not found: {}",
                        id_display
                    )));
                }
                Ok(())
            })
            .await
    }

    async fn set_halt_mode(
        &self,
        id: &ExecutionPlanId,
        halt_mode: ExecutionPlanHaltMode,
    ) -> AppResult<()> {
        let id = id.as_str().to_string();
        let id_display = id.clone();
        self.db
            .run(move |conn| {
                let rows = conn
                    .execute(
                        "UPDATE execution_plans SET halt_mode = ?1 WHERE id = ?2",
                        rusqlite::params![halt_mode.to_db_string(), id.as_str()],
                    )
                    .map_err(|e| {
                        AppError::Database(format!("Failed to set execution plan halt mode: {}", e))
                    })?;
                if rows == 0 {
                    return Err(AppError::NotFound(format!(
                        "Execution plan not found: {}",
                        id_display
                    )));
                }
                Ok(())
            })
            .await
    }

    async fn delete(&self, id: &ExecutionPlanId) -> AppResult<()> {
        let id = id.as_str().to_string();
        let id_display = id.clone();
        self.db
            .run(move |conn| {
                let rows = conn
                    .execute(
                        "DELETE FROM execution_plans WHERE id = ?1",
                        rusqlite::params![id.as_str()],
                    )
                    .map_err(|e| {
                        AppError::Database(format!("Failed to delete execution plan: {}", e))
                    })?;
                if rows == 0 {
                    return Err(AppError::NotFound(format!(
                        "Execution plan not found: {}",
                        id_display
                    )));
                }
                Ok(())
            })
            .await
    }
}

// SQLite-based WorkflowRepository implementation for production use
// All rusqlite calls go through DbConnection::run() (spawn_blocking + blocking_lock)
// to prevent blocking the tokio async runtime / timer driver.

use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use rusqlite::Connection;

use crate::domain::entities::{WorkflowId, WorkflowSchema};
use crate::domain::repositories::WorkflowRepository;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

/// SQLite implementation of WorkflowRepository for production use
pub struct SqliteWorkflowRepository {
    db: DbConnection,
}

impl SqliteWorkflowRepository {
    /// Create a new SQLite workflow repository with the given connection
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

    /// Parse a WorkflowSchema from a database row
    fn workflow_from_row(row: &rusqlite::Row<'_>) -> Result<WorkflowSchema, rusqlite::Error> {
        let id: String = row.get(0)?;
        let schema_json: String = row.get(3)?;
        let is_default: i32 = row.get(4)?;

        // Parse the JSON schema
        let mut schema: WorkflowSchema = serde_json::from_str(&schema_json)
            .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;

        // Override with database values
        schema.id = WorkflowId::from_string(id);
        schema.is_default = is_default != 0;

        Ok(schema)
    }
}

#[async_trait]
impl WorkflowRepository for SqliteWorkflowRepository {
    async fn create(&self, workflow: WorkflowSchema) -> AppResult<WorkflowSchema> {
        let schema_json = serde_json::to_string(&workflow)
            .map_err(|e| AppError::Database(format!("JSON serialization error: {}", e)))?;
        let id_str = workflow.id.as_str().to_string();
        let name = workflow.name.clone();
        let description = workflow.description.clone();
        let is_default = if workflow.is_default { 1i32 } else { 0 };

        self.db
            .run(move |conn| {
                conn.execute(
                    "INSERT INTO workflows (id, name, description, schema_json, is_default)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![id_str, name, description, schema_json, is_default],
                )?;
                Ok(())
            })
            .await?;
        Ok(workflow)
    }

    async fn get_by_id(&self, id: &WorkflowId) -> AppResult<Option<WorkflowSchema>> {
        let id_str = id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, name, description, schema_json, is_default
                     FROM workflows WHERE id = ?1",
                    rusqlite::params![id_str],
                    SqliteWorkflowRepository::workflow_from_row,
                )
            })
            .await
    }

    async fn get_all(&self) -> AppResult<Vec<WorkflowSchema>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, name, description, schema_json, is_default
                     FROM workflows ORDER BY name ASC",
                )?;
                let workflows = stmt
                    .query_map([], SqliteWorkflowRepository::workflow_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(workflows)
            })
            .await
    }

    async fn get_default(&self) -> AppResult<Option<WorkflowSchema>> {
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, name, description, schema_json, is_default
                     FROM workflows WHERE is_default = 1",
                    [],
                    SqliteWorkflowRepository::workflow_from_row,
                )
            })
            .await
    }

    async fn update(&self, workflow: &WorkflowSchema) -> AppResult<()> {
        let schema_json = serde_json::to_string(workflow)
            .map_err(|e| AppError::Database(format!("JSON serialization error: {}", e)))?;
        let id_str = workflow.id.as_str().to_string();
        let name = workflow.name.clone();
        let description = workflow.description.clone();
        let is_default = if workflow.is_default { 1i32 } else { 0 };

        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE workflows SET name = ?2, description = ?3, schema_json = ?4, is_default = ?5
                     WHERE id = ?1",
                    rusqlite::params![id_str, name, description, schema_json, is_default],
                )?;
                Ok(())
            })
            .await
    }

    async fn delete(&self, id: &WorkflowId) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "DELETE FROM workflows WHERE id = ?1",
                    rusqlite::params![id_str],
                )?;
                Ok(())
            })
            .await
    }

    async fn set_default(&self, id: &WorkflowId) -> AppResult<()> {
        let id_str = id.as_str().to_string();
        self.db
            .run(move |conn| {
                conn.execute(
                    "UPDATE workflows SET is_default = 0 WHERE is_default = 1",
                    [],
                )?;
                conn.execute(
                    "UPDATE workflows SET is_default = 1 WHERE id = ?1",
                    rusqlite::params![id_str],
                )?;
                Ok(())
            })
            .await
    }
}

impl SqliteWorkflowRepository {
    /// Seeds built-in workflows if they don't exist.
    /// Returns the number of workflows seeded.
    pub async fn seed_builtin_workflows(&self) -> AppResult<usize> {
        let builtin_workflows = vec![
            WorkflowSchema::default_ralphx(),
            WorkflowSchema::jira_compatible(),
            WorkflowSchema::linear_compatible(),
        ];
        self.db
            .run(move |conn| {
                let mut seeded_count = 0;
                for workflow in builtin_workflows {
                    let schema_json = serde_json::to_string(&workflow).map_err(|e| {
                        AppError::Database(format!("JSON serialization error: {}", e))
                    })?;
                    let rows = conn.execute(
                        "INSERT OR IGNORE INTO workflows (id, name, description, schema_json, is_default)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![
                            workflow.id.as_str(),
                            workflow.name,
                            workflow.description,
                            schema_json,
                            if workflow.is_default { 1i32 } else { 0 },
                        ],
                    )?;
                    if rows > 0 {
                        seeded_count += 1;
                    }
                }
                Ok(seeded_count)
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_workflow_repo_tests.rs"]
mod tests;

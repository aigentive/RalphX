// SQLite-based TaskStepRepository implementation for production use
// Uses DbConnection (spawn_blocking) for non-blocking rusqlite access

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use rusqlite::Connection;

use crate::domain::entities::{TaskId, TaskStep, TaskStepId, TaskStepStatus};
use crate::domain::ideation::TasksFeatureAction;
use crate::domain::repositories::TaskStepRepository;
use crate::error::{AppError, AppResult};

use super::DbConnection;

/// SQLite implementation of TaskStepRepository for production use
pub struct SqliteTaskStepRepository {
    db: DbConnection,
    enforce_tasks_feature_policy: bool,
}

impl SqliteTaskStepRepository {
    /// Create a new SQLite task step repository with the given connection
    pub fn new(conn: Connection) -> Self {
        Self {
            db: DbConnection::new(conn),
            enforce_tasks_feature_policy: false,
        }
    }

    /// Create from an Arc-wrapped mutex connection (for sharing)
    pub fn from_shared(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            db: DbConnection::from_shared(conn),
            enforce_tasks_feature_policy: false,
        }
    }

    pub(crate) fn with_tasks_feature_policy(mut self) -> Self {
        self.enforce_tasks_feature_policy = true;
        self
    }
}

fn authorize_task_step_write(conn: &Connection, task_id: &TaskId) -> AppResult<()> {
    let exists = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1)",
        [task_id.as_str()],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(AppError::TaskNotFound(task_id.as_str().to_string()));
    }
    crate::infrastructure::sqlite::sqlite_ideation_settings_repo::authorize_tasks_session_sync(
        conn,
        None,
        TasksFeatureAction::HistoryMutation,
    )
}

#[async_trait]
impl TaskStepRepository for SqliteTaskStepRepository {
    async fn create(&self, step: TaskStep) -> AppResult<TaskStep> {
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_task_step_write(conn, &step.task_id)?;
                }
                conn.execute(
                    "INSERT INTO task_steps (id, task_id, title, description, status, sort_order, depends_on, created_by, completion_note, created_at, updated_at, started_at, completed_at, parent_step_id, scope_context)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                    rusqlite::params![
                        step.id.as_str(),
                        step.task_id.as_str(),
                        step.title,
                        step.description,
                        step.status.to_db_string(),
                        step.sort_order,
                        step.depends_on.as_ref().map(|id| id.as_str()),
                        step.created_by,
                        step.completion_note,
                        step.created_at.to_rfc3339(),
                        step.updated_at.to_rfc3339(),
                        step.started_at.map(|dt| dt.to_rfc3339()),
                        step.completed_at.map(|dt| dt.to_rfc3339()),
                        step.parent_step_id.as_ref().map(|id| id.as_str()),
                        step.scope_context,
                    ],
                )?;
                Ok(step)
            })
            .await
    }

    async fn get_by_id(&self, id: &TaskStepId) -> AppResult<Option<TaskStep>> {
        let id = id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT id, task_id, title, description, status, sort_order, depends_on, created_by, completion_note, created_at, updated_at, started_at, completed_at, parent_step_id, scope_context
                     FROM task_steps WHERE id = ?1",
                    [&id],
                    |row| TaskStep::from_row(row),
                )
            })
            .await
    }

    async fn get_by_task(&self, task_id: &TaskId) -> AppResult<Vec<TaskStep>> {
        let task_id = task_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, task_id, title, description, status, sort_order, depends_on, created_by, completion_note, created_at, updated_at, started_at, completed_at, parent_step_id, scope_context
                     FROM task_steps WHERE task_id = ?1
                     ORDER BY sort_order ASC",
                )?;
                let steps = stmt
                    .query_map([&task_id], TaskStep::from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(steps)
            })
            .await
    }

    async fn get_by_task_and_status(
        &self,
        task_id: &TaskId,
        status: TaskStepStatus,
    ) -> AppResult<Vec<TaskStep>> {
        let task_id = task_id.as_str().to_string();
        let status_str = status.to_db_string().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, task_id, title, description, status, sort_order, depends_on, created_by, completion_note, created_at, updated_at, started_at, completed_at, parent_step_id, scope_context
                     FROM task_steps WHERE task_id = ?1 AND status = ?2
                     ORDER BY sort_order ASC",
                )?;
                let steps = stmt
                    .query_map(rusqlite::params![task_id, status_str], TaskStep::from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(steps)
            })
            .await
    }

    async fn update(&self, step: &TaskStep) -> AppResult<()> {
        let id = step.id.as_str().to_string();
        let task_id = step.task_id.as_str().to_string();
        let title = step.title.clone();
        let description = step.description.clone();
        let status = step.status.to_db_string().to_string();
        let sort_order = step.sort_order;
        let depends_on = step.depends_on.as_ref().map(|id| id.as_str().to_string());
        let created_by = step.created_by.clone();
        let completion_note = step.completion_note.clone();
        let updated_at = step.updated_at.to_rfc3339();
        let started_at = step.started_at.map(|dt| dt.to_rfc3339());
        let completed_at = step.completed_at.map(|dt| dt.to_rfc3339());
        let parent_step_id = step
            .parent_step_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let scope_context = step.scope_context.clone();
        let task_id_for_policy = step.task_id.clone();
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;

        self.db
            .run(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_task_step_write(conn, &task_id_for_policy)?;
                }
                conn.execute(
                    "UPDATE task_steps SET task_id = ?2, title = ?3, description = ?4, status = ?5, sort_order = ?6, depends_on = ?7, created_by = ?8, completion_note = ?9, updated_at = ?10, started_at = ?11, completed_at = ?12, parent_step_id = ?13, scope_context = ?14
                     WHERE id = ?1",
                    rusqlite::params![
                        id,
                        task_id,
                        title,
                        description,
                        status,
                        sort_order,
                        depends_on,
                        created_by,
                        completion_note,
                        updated_at,
                        started_at,
                        completed_at,
                        parent_step_id,
                        scope_context,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn delete(&self, id: &TaskStepId) -> AppResult<()> {
        let id = id.as_str().to_string();
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run(move |conn| {
                if enforce_tasks_feature_policy {
                    let task_id = conn
                        .query_row(
                            "SELECT task_id FROM task_steps WHERE id = ?1",
                            [id.as_str()],
                            |row| row.get::<_, String>(0),
                        )
                        .map(TaskId::from_string)?;
                    authorize_task_step_write(conn, &task_id)?;
                }
                conn.execute("DELETE FROM task_steps WHERE id = ?1", [id])?;
                Ok(())
            })
            .await
    }

    async fn delete_by_task(&self, task_id: &TaskId) -> AppResult<()> {
        let task_id = task_id.as_str().to_string();
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_task_step_write(conn, &TaskId::from_string(task_id.clone()))?;
                }
                conn.execute("DELETE FROM task_steps WHERE task_id = ?1", [task_id])?;
                Ok(())
            })
            .await
    }

    async fn count_by_status(&self, task_id: &TaskId) -> AppResult<HashMap<TaskStepStatus, u32>> {
        let task_id = task_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT status, COUNT(*) as count
                     FROM task_steps WHERE task_id = ?1
                     GROUP BY status",
                )?;

                let mut counts = HashMap::new();
                let rows = stmt.query_map([&task_id], |row| {
                    let status_str: String = row.get(0)?;
                    let count: u32 = row.get(1)?;
                    Ok((status_str, count))
                })?;

                for row in rows {
                    let (status_str, count) = row?;
                    if let Ok(status) = TaskStepStatus::from_db_string(&status_str) {
                        counts.insert(status, count);
                    }
                }

                Ok(counts)
            })
            .await
    }

    async fn bulk_create(&self, steps: Vec<TaskStep>) -> AppResult<Vec<TaskStep>> {
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run(move |conn| {
                if enforce_tasks_feature_policy {
                    for task_id in steps.iter().map(|step| &step.task_id) {
                        authorize_task_step_write(conn, task_id)?;
                    }
                }
                let tx = conn.unchecked_transaction()?;
                for step in &steps {
                    tx.execute(
                        "INSERT INTO task_steps (id, task_id, title, description, status, sort_order, depends_on, created_by, completion_note, created_at, updated_at, started_at, completed_at, parent_step_id, scope_context)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                        rusqlite::params![
                            step.id.as_str(),
                            step.task_id.as_str(),
                            step.title,
                            step.description,
                            step.status.to_db_string(),
                            step.sort_order,
                            step.depends_on.as_ref().map(|id| id.as_str()),
                            step.created_by,
                            step.completion_note,
                            step.created_at.to_rfc3339(),
                            step.updated_at.to_rfc3339(),
                            step.started_at.map(|dt| dt.to_rfc3339()),
                            step.completed_at.map(|dt| dt.to_rfc3339()),
                            step.parent_step_id.as_ref().map(|id| id.as_str()),
                            step.scope_context,
                        ],
                    )?;
                }
                tx.commit()?;
                Ok(steps)
            })
            .await
    }

    async fn reorder(&self, task_id: &TaskId, step_ids: Vec<TaskStepId>) -> AppResult<()> {
        let task_id = task_id.as_str().to_string();
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_task_step_write(conn, &TaskId::from_string(task_id.clone()))?;
                }
                let tx = conn.unchecked_transaction()?;
                for (index, step_id) in step_ids.iter().enumerate() {
                    tx.execute(
                        "UPDATE task_steps SET sort_order = ?1 WHERE id = ?2 AND task_id = ?3",
                        rusqlite::params![index as i32, step_id.as_str(), task_id.as_str()],
                    )?;
                }
                tx.commit()?;
                Ok(())
            })
            .await
    }

    async fn reset_all_to_pending(&self, task_id: &TaskId) -> AppResult<u32> {
        let task_id = task_id.as_str().to_string();
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_task_step_write(conn, &TaskId::from_string(task_id.clone()))?;
                }
                let now = chrono::Utc::now().to_rfc3339();
                let count = conn.execute(
                    "UPDATE task_steps SET status = 'pending', started_at = NULL, completed_at = NULL, completion_note = NULL, updated_at = ?1 WHERE task_id = ?2 AND status != 'pending'",
                    rusqlite::params![now, task_id],
                )?;
                Ok(count as u32)
            })
            .await
    }
}

#[cfg(test)]
#[path = "sqlite_task_step_repo_tests.rs"]
mod tests;

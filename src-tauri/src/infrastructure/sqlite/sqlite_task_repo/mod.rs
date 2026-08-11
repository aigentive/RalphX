// SQLite-based TaskRepository implementation for production use
// Uses rusqlite with connection pooling for thread-safe access

mod helpers;
mod queries;
mod query_builder;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;

use async_trait::async_trait;
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension};

use crate::domain::entities::{
    ExecutionPlanId, IdeationSessionId, InternalStatus, ProjectId, Task, TaskCategory, TaskId,
    TaskStepId,
};
use crate::domain::ideation::TasksFeatureAction;
use crate::domain::repositories::{StateHistoryMetadata, StatusTransition, TaskRepository};
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::DbConnection;

/// SQLite implementation of TaskRepository for production use
/// Uses a mutex-protected connection for thread-safe access
pub struct SqliteTaskRepository {
    db: DbConnection,
    enforce_tasks_feature_policy: bool,
}

impl SqliteTaskRepository {
    /// Create a new SQLite task repository with the given connection
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

    /// Enforce the global Tasks policy atomically with task insertion.
    pub(crate) fn with_tasks_feature_policy(mut self) -> Self {
        self.enforce_tasks_feature_policy = true;
        self
    }
}

fn authorize_task_action_sync(
    conn: &Connection,
    task_id: &TaskId,
    action: TasksFeatureAction,
) -> AppResult<()> {
    let session_id = conn
        .query_row(
            "SELECT ideation_session_id FROM tasks WHERE id = ?1",
            [task_id.as_str()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .ok_or_else(|| AppError::TaskNotFound(task_id.as_str().to_string()))?;
    crate::infrastructure::sqlite::sqlite_ideation_settings_repo::authorize_tasks_session_sync(
        conn,
        session_id.as_deref(),
        action,
    )
}

fn update_with_expected_status_sync(
    conn: &Connection,
    task: &Task,
    expected_status: InternalStatus,
) -> AppResult<bool> {
    let rows_affected = conn.execute(
        "UPDATE tasks SET project_id = ?2, category = ?3, title = ?4, description = ?5, priority = ?6, internal_status = ?7, source_proposal_id = ?8, plan_artifact_id = ?9, plan_blueprint_artifact_id = ?10, ideation_session_id = ?11, execution_plan_id = ?12, updated_at = ?13, started_at = ?14, completed_at = ?15, blocked_reason = ?16, task_branch = ?17, task_branch_base_ref = ?18, task_branch_base_sha = ?19, worktree_path = ?20, merge_commit_sha = ?21, metadata = ?22, merge_pipeline_active = ?23
         WHERE id = ?1 AND internal_status = ?24 AND (
            internal_status = ?7 OR NOT EXISTS (
                SELECT 1 FROM branch_update_operations
                WHERE task_id = ?1 AND settled_at IS NULL
            )
         )",
        rusqlite::params![
            task.id.as_str(),
            task.project_id.as_str(),
            task.category.to_string(),
            task.title,
            task.description,
            task.priority,
            task.internal_status.as_str(),
            task.source_proposal_id.as_ref().map(|id| id.as_str()),
            task.plan_artifact_id.as_ref().map(|id| id.as_str()),
            task.plan_blueprint_artifact_id
                .as_ref()
                .map(|id| id.as_str()),
            task.ideation_session_id.as_ref().map(|id| id.as_str()),
            task.execution_plan_id.as_ref().map(|id| id.as_str()),
            task.updated_at.to_rfc3339(),
            task.started_at.map(|dt| dt.to_rfc3339()),
            task.completed_at.map(|dt| dt.to_rfc3339()),
            task.blocked_reason,
            task.task_branch,
            task.task_branch_base_ref,
            task.task_branch_base_sha,
            task.worktree_path,
            task.merge_commit_sha,
            task.metadata,
            task.merge_pipeline_active,
            expected_status.as_str(),
        ],
    )?;
    Ok(rows_affected > 0)
}

#[async_trait]
impl TaskRepository for SqliteTaskRepository {
    async fn create(&self, task: Task) -> AppResult<Task> {
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run(move |conn| {
                if enforce_tasks_feature_policy {
                    crate::infrastructure::sqlite::sqlite_ideation_settings_repo::authorize_tasks_session_sync(
                        conn,
                        task.ideation_session_id.as_ref().map(|id| id.as_str()),
                        TasksFeatureAction::Progress,
                    )?;
                }
                conn.execute(
                    "INSERT INTO tasks (id, project_id, category, title, description, priority, internal_status, needs_review_point, source_proposal_id, plan_artifact_id, plan_blueprint_artifact_id, ideation_session_id, execution_plan_id, created_at, updated_at, started_at, completed_at, archived_at, blocked_reason, task_branch, task_branch_base_ref, task_branch_base_sha, worktree_path, merge_commit_sha, metadata, merge_pipeline_active)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)",
                    rusqlite::params![
                        task.id.as_str(),
                        task.project_id.as_str(),
                        task.category.to_string(),
                        task.title,
                        task.description,
                        task.priority,
                        task.internal_status.as_str(),
                        task.needs_review_point,
                        task.source_proposal_id.as_ref().map(|id| id.as_str()),
                        task.plan_artifact_id.as_ref().map(|id| id.as_str()),
                        task.plan_blueprint_artifact_id
                            .as_ref()
                            .map(|id| id.as_str()),
                        task.ideation_session_id.as_ref().map(|id| id.as_str()),
                        task.execution_plan_id.as_ref().map(|id| id.as_str()),
                        task.created_at.to_rfc3339(),
                        task.updated_at.to_rfc3339(),
                        task.started_at.map(|dt| dt.to_rfc3339()),
                        task.completed_at.map(|dt| dt.to_rfc3339()),
                        task.archived_at.map(|dt| dt.to_rfc3339()),
                        task.blocked_reason,
                        task.task_branch,
                        task.task_branch_base_ref,
                        task.task_branch_base_sha,
                        task.worktree_path,
                        task.merge_commit_sha,
                        task.metadata,
                        task.merge_pipeline_active,
                    ],
                )?;
                Ok(task)
            })
            .await
    }

    async fn get_by_id(&self, id: &TaskId) -> AppResult<Option<Task>> {
        let id = id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(queries::GET_BY_ID, [id.as_str()], |row| Task::from_row(row))
            })
            .await
    }

    async fn get_by_ids(&self, ids: &[TaskId]) -> AppResult<Vec<Task>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<String> = ids.iter().map(|id| id.as_str().to_string()).collect();
        self.db
            .run(move |conn| {
                let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
                let sql = format!(
                    "SELECT {} FROM tasks WHERE id IN ({})",
                    queries::TASK_COLUMNS,
                    placeholders
                );
                let mut stmt = conn.prepare(&sql)?;
                let tasks = stmt
                    .query_map(
                        rusqlite::params_from_iter(ids.iter().map(|id| id.as_str())),
                        Task::from_row,
                    )?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(tasks)
            })
            .await
    }

    async fn get_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<Task>> {
        let project_id = project_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(queries::GET_BY_PROJECT)?;
                let tasks = stmt
                    .query_map([project_id.as_str()], Task::from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(tasks)
            })
            .await
    }

    async fn get_by_ideation_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Vec<Task>> {
        let session_id = session_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(queries::GET_BY_IDEATION_SESSION)?;
                let tasks = stmt
                    .query_map([session_id.as_str()], Task::from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(tasks)
            })
            .await
    }

    async fn update(&self, task: &Task) -> AppResult<()> {
        let task = task.clone();
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_task_action_sync(
                        conn,
                        &task.id,
                        TasksFeatureAction::HistoryMutation,
                    )?;
                }
                let rows_affected = conn.execute(
                    "UPDATE tasks SET project_id = ?2, category = ?3, title = ?4, description = ?5, priority = ?6, internal_status = ?7, source_proposal_id = ?8, plan_artifact_id = ?9, plan_blueprint_artifact_id = ?10, ideation_session_id = ?11, execution_plan_id = ?12, updated_at = ?13, started_at = ?14, completed_at = ?15, blocked_reason = ?16, task_branch = ?17, task_branch_base_ref = ?18, task_branch_base_sha = ?19, worktree_path = ?20, merge_commit_sha = ?21, metadata = ?22, merge_pipeline_active = ?23
                     WHERE id = ?1 AND (
                        internal_status = ?7 OR NOT EXISTS (
                            SELECT 1 FROM branch_update_operations
                            WHERE task_id = ?1 AND settled_at IS NULL
                        )
                     )",
                    rusqlite::params![
                        task.id.as_str(),
                        task.project_id.as_str(),
                        task.category.to_string(),
                        task.title,
                        task.description,
                        task.priority,
                        task.internal_status.as_str(),
                        task.source_proposal_id.as_ref().map(|id| id.as_str()),
                        task.plan_artifact_id.as_ref().map(|id| id.as_str()),
                        task.plan_blueprint_artifact_id
                            .as_ref()
                            .map(|id| id.as_str()),
                        task.ideation_session_id.as_ref().map(|id| id.as_str()),
                        task.execution_plan_id.as_ref().map(|id| id.as_str()),
                        task.updated_at.to_rfc3339(),
                        task.started_at.map(|dt| dt.to_rfc3339()),
                        task.completed_at.map(|dt| dt.to_rfc3339()),
                        task.blocked_reason,
                        task.task_branch,
                        task.task_branch_base_ref,
                        task.task_branch_base_sha,
                        task.worktree_path,
                        task.merge_commit_sha,
                        task.metadata,
                        task.merge_pipeline_active,
                    ],
                )?;
                if rows_affected == 0 {
                    return Err(AppError::Validation(format!(
                        "task {} status is owned by an active branch update",
                        task.id.as_str()
                    )));
                }
                Ok(())
            })
            .await
    }

    async fn update_with_expected_status(
        &self,
        task: &Task,
        expected_status: InternalStatus,
    ) -> AppResult<bool> {
        self.update_with_expected_status_for_action(
            task,
            expected_status,
            TasksFeatureAction::Progress,
        )
        .await
    }

    async fn update_with_expected_status_for_action(
        &self,
        task: &Task,
        expected_status: InternalStatus,
        action: TasksFeatureAction,
    ) -> AppResult<bool> {
        let task = task.clone();
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_task_action_sync(conn, &task.id, action)?;
                }
                update_with_expected_status_sync(conn, &task, expected_status)
            })
            .await
    }

    async fn update_with_expected_status_and_history_for_action(
        &self,
        task: &Task,
        expected_status: InternalStatus,
        trigger: &str,
        action: TasksFeatureAction,
    ) -> AppResult<Option<String>> {
        let task = task.clone();
        let trigger = trigger.to_string();
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run_transaction(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_task_action_sync(conn, &task.id, action)?;
                }
                if !update_with_expected_status_sync(conn, &task, expected_status)? {
                    return Ok(None);
                }
                helpers::insert_status_history(
                    conn,
                    &task.id,
                    expected_status,
                    task.internal_status,
                    &trigger,
                    task.updated_at,
                )
                .map(Some)
            })
            .await
    }

    async fn restart_terminal_task_to_ready_with_history_for_action(
        &self,
        task: &Task,
        expected_status: InternalStatus,
        failed_step_ids: &[TaskStepId],
        trigger: &str,
        action: TasksFeatureAction,
    ) -> AppResult<Option<(String, u32)>> {
        let task = task.clone();
        let failed_step_ids = failed_step_ids.to_vec();
        let trigger = trigger.to_string();
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run_transaction(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_task_action_sync(conn, &task.id, action)?;
                }
                if !update_with_expected_status_sync(conn, &task, expected_status)? {
                    return Ok(None);
                }
                let mut reset_count = 0u32;
                for step_id in failed_step_ids {
                    let changed = conn.execute(
                        "UPDATE task_steps
                         SET status = 'pending', started_at = NULL, completed_at = NULL,
                             completion_note = NULL, updated_at = ?1
                         WHERE id = ?2 AND task_id = ?3 AND status = 'failed'",
                        rusqlite::params![
                            task.updated_at.to_rfc3339(),
                            step_id.as_str(),
                            task.id.as_str(),
                        ],
                    )?;
                    if changed != 1 {
                        return Err(AppError::Validation(format!(
                            "Failed step {} changed during terminal restart for task {}",
                            step_id.as_str(),
                            task.id.as_str()
                        )));
                    }
                    reset_count += 1;
                }
                let history_id = helpers::insert_status_history(
                    conn,
                    &task.id,
                    expected_status,
                    task.internal_status,
                    &trigger,
                    task.updated_at,
                )?;
                Ok(Some((history_id, reset_count)))
            })
            .await
    }

    async fn update_metadata(&self, id: &TaskId, metadata: Option<String>) -> AppResult<()> {
        let id = id.clone();
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_task_action_sync(conn, &id, TasksFeatureAction::HistoryMutation)?;
                }
                let now = Utc::now();
                conn.execute(
                    "UPDATE tasks SET metadata = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![metadata, now.to_rfc3339(), id.as_str()],
                )?;
                Ok(())
            })
            .await
    }

    async fn delete(&self, id: &TaskId) -> AppResult<()> {
        let id = id.clone();
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_task_action_sync(conn, &id, TasksFeatureAction::HistoryMutation)?;
                }
                conn.execute(queries::DELETE_TASK, [id.as_str()])?;
                Ok(())
            })
            .await
    }

    async fn get_by_status(
        &self,
        project_id: &ProjectId,
        status: InternalStatus,
    ) -> AppResult<Vec<Task>> {
        let project_id = project_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let sql = format!(
                    "SELECT {} FROM tasks WHERE project_id = ?1 AND internal_status = ?2 AND archived_at IS NULL
                     ORDER BY priority DESC, created_at ASC",
                    queries::TASK_COLUMNS
                );
                let mut stmt = conn.prepare(&sql)?;
                let tasks = stmt
                    .query_map(
                        rusqlite::params![project_id.as_str(), status.as_str()],
                        |row| Task::from_row(row),
                    )?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(tasks)
            })
            .await
    }

    async fn get_by_status_with_metadata_bool(
        &self,
        project_id: &ProjectId,
        status: InternalStatus,
        metadata_key: &str,
    ) -> AppResult<Vec<Task>> {
        let project_id = project_id.as_str().to_string();
        let metadata_path = format!("$.{}", metadata_key);
        self.db
            .run(move |conn| {
                let sql = format!(
                    "SELECT {} FROM tasks
                     WHERE project_id = ?1
                       AND internal_status = ?2
                       AND archived_at IS NULL
                       AND metadata IS NOT NULL
                       AND json_valid(metadata)
                       AND json_extract(metadata, ?3) = 1
                     ORDER BY priority DESC, created_at ASC",
                    queries::TASK_COLUMNS
                );
                let mut stmt = conn.prepare(&sql)?;
                let tasks = stmt
                    .query_map(
                        rusqlite::params![project_id.as_str(), status.as_str(), metadata_path],
                        Task::from_row,
                    )?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(tasks)
            })
            .await
    }

    async fn find_merged_regular_plan_keys(
        &self,
        project_id: &ProjectId,
        plan_keys: &[(IdeationSessionId, ExecutionPlanId)],
    ) -> AppResult<HashSet<(IdeationSessionId, ExecutionPlanId)>> {
        if plan_keys.is_empty() {
            return Ok(HashSet::new());
        }

        let project_id = project_id.as_str().to_string();
        let plan_keys: Vec<(String, String)> = plan_keys
            .iter()
            .map(|(session_id, execution_plan_id)| {
                (
                    session_id.as_str().to_string(),
                    execution_plan_id.as_str().to_string(),
                )
            })
            .collect();
        self.db
            .run(move |conn| {
                let pair_filters = plan_keys
                    .iter()
                    .map(|_| "(ideation_session_id = ? AND execution_plan_id = ?)")
                    .collect::<Vec<_>>()
                    .join(" OR ");
                let sql = format!(
                    "SELECT DISTINCT ideation_session_id, execution_plan_id
                     FROM tasks
                     WHERE project_id = ?
                       AND internal_status = ?
                       AND category = ?
                       AND archived_at IS NULL
                       AND ideation_session_id IS NOT NULL
                       AND execution_plan_id IS NOT NULL
                       AND ({pair_filters})"
                );

                let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
                    Box::new(project_id),
                    Box::new(InternalStatus::Merged.as_str().to_string()),
                    Box::new(TaskCategory::Regular.to_string()),
                ];
                for (session_id, execution_plan_id) in plan_keys {
                    params.push(Box::new(session_id));
                    params.push(Box::new(execution_plan_id));
                }

                let params_ref: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|param| param.as_ref()).collect();
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params_ref.as_slice(), |row| {
                    let session_id: String = row.get(0)?;
                    let execution_plan_id: String = row.get(1)?;
                    Ok((
                        IdeationSessionId::from_string(session_id),
                        ExecutionPlanId::from_string(execution_plan_id),
                    ))
                })?;

                let mut result = HashSet::new();
                for row in rows {
                    result.insert(row?);
                }
                Ok(result)
            })
            .await
    }

    async fn persist_status_change(
        &self,
        id: &TaskId,
        from: InternalStatus,
        to: InternalStatus,
        trigger: &str,
    ) -> AppResult<String> {
        self.persist_status_change_for_action(id, from, to, trigger, TasksFeatureAction::Progress)
            .await
    }

    async fn persist_status_change_for_action(
        &self,
        id: &TaskId,
        from: InternalStatus,
        to: InternalStatus,
        trigger: &str,
        action: TasksFeatureAction,
    ) -> AppResult<String> {
        let id = id.clone();
        let trigger = trigger.to_string();
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run_transaction(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_task_action_sync(conn, &id, action)?;
                }
                let now = Utc::now();
                helpers::persist_status_change(conn, &id, from, to, &trigger, now)
            })
            .await
    }

    async fn get_status_history(&self, id: &TaskId) -> AppResult<Vec<StatusTransition>> {
        let id = id.as_str().to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT from_status, to_status, changed_by, created_at, metadata
                     FROM task_state_history WHERE task_id = ?1
                     ORDER BY created_at ASC",
                )?;
                let transitions = stmt
                    .query_map([id.as_str()], |row| {
                        let from_str: String = row.get(0)?;
                        let to_str: String = row.get(1)?;
                        let trigger: String = row.get(2)?;
                        let created_at_str: String = row.get(3)?;
                        let metadata_json: Option<String> = row.get(4)?;

                        let from = from_str.parse().unwrap_or(InternalStatus::Backlog);
                        let to = to_str.parse().unwrap_or(InternalStatus::Backlog);
                        let timestamp = Task::parse_datetime(created_at_str);

                        let (conversation_id, agent_run_id) = metadata_json
                            .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
                            .map(|v| {
                                let conv_id = v
                                    .get("conversation_id")
                                    .and_then(|v| v.as_str())
                                    .map(String::from);
                                let run_id = v
                                    .get("agent_run_id")
                                    .and_then(|v| v.as_str())
                                    .map(String::from);
                                (conv_id, run_id)
                            })
                            .unwrap_or((None, None));

                        Ok(StatusTransition::with_metadata(
                            from,
                            to,
                            trigger,
                            timestamp,
                            conversation_id,
                            agent_run_id,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(transitions)
            })
            .await
    }

    async fn get_status_history_batch(
        &self,
        task_ids: &[TaskId],
    ) -> AppResult<HashMap<TaskId, Vec<StatusTransition>>> {
        if task_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let ids_str: Vec<String> = task_ids.iter().map(|id| id.as_str().to_string()).collect();
        self.db
            .run(move |conn| {
                let placeholders = ids_str.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
                let sql = format!(
                    "SELECT task_id, from_status, to_status, changed_by, created_at, metadata \
                     FROM task_state_history WHERE task_id IN ({}) ORDER BY created_at ASC",
                    placeholders
                );
                let mut stmt = conn.prepare(&sql)?;
                let mut result: HashMap<TaskId, Vec<StatusTransition>> = HashMap::new();
                let rows = stmt.query_map(
                    rusqlite::params_from_iter(ids_str.iter().map(|s| s.as_str())),
                    |row| {
                        let task_id_str: String = row.get(0)?;
                        let from_str: String = row.get(1)?;
                        let to_str: String = row.get(2)?;
                        let trigger: String = row.get(3)?;
                        let created_at_str: String = row.get(4)?;
                        let metadata_json: Option<String> = row.get(5)?;
                        Ok((
                            task_id_str,
                            from_str,
                            to_str,
                            trigger,
                            created_at_str,
                            metadata_json,
                        ))
                    },
                )?;
                for row in rows {
                    let (task_id_str, from_str, to_str, trigger, created_at_str, metadata_json) =
                        row?;
                    let from = from_str.parse().unwrap_or(InternalStatus::Backlog);
                    let to = to_str.parse().unwrap_or(InternalStatus::Backlog);
                    let timestamp = Task::parse_datetime(created_at_str);
                    let (conversation_id, agent_run_id) = metadata_json
                        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
                        .map(|v| {
                            let conv_id = v
                                .get("conversation_id")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                            let run_id = v
                                .get("agent_run_id")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                            (conv_id, run_id)
                        })
                        .unwrap_or((None, None));
                    let transition = StatusTransition::with_metadata(
                        from,
                        to,
                        trigger,
                        timestamp,
                        conversation_id,
                        agent_run_id,
                    );
                    result
                        .entry(TaskId(task_id_str))
                        .or_default()
                        .push(transition);
                }
                Ok(result)
            })
            .await
    }

    async fn get_status_entered_at(
        &self,
        task_id: &TaskId,
        status: InternalStatus,
    ) -> AppResult<Option<chrono::DateTime<Utc>>> {
        let task_id = task_id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT created_at
                     FROM task_state_history
                     WHERE task_id = ?1 AND to_status = ?2
                     ORDER BY created_at ASC
                     LIMIT 1",
                    rusqlite::params![task_id.as_str(), status.as_str()],
                    |row| {
                        let created_at_str: String = row.get(0)?;
                        Ok(Task::parse_datetime(created_at_str))
                    },
                )
            })
            .await
    }

    async fn get_status_last_entered_at(
        &self,
        task_id: &TaskId,
        status: InternalStatus,
    ) -> AppResult<Option<chrono::DateTime<Utc>>> {
        let task_id = task_id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                conn.query_row(
                    "SELECT created_at
                     FROM task_state_history
                     WHERE task_id = ?1 AND to_status = ?2
                     ORDER BY created_at DESC, rowid DESC
                     LIMIT 1",
                    rusqlite::params![task_id.as_str(), status.as_str()],
                    |row| {
                        let created_at_str: String = row.get(0)?;
                        Ok(Task::parse_datetime(created_at_str))
                    },
                )
            })
            .await
    }

    async fn get_next_executable(&self, project_id: &ProjectId) -> AppResult<Option<Task>> {
        let project_id = project_id.as_str().to_string();
        self.db
            .query_optional(move |conn| {
                let task_columns = queries::TASK_COLUMNS
                    .split(", ")
                    .map(|column| format!("t.{column}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let sql = format!(
                    "SELECT {task_columns}
                     FROM tasks t
                     WHERE t.project_id = ?1
                       AND t.internal_status = 'ready'
                       AND NOT EXISTS (
                           SELECT 1 FROM task_dependencies td
                           JOIN tasks blocker ON blocker.id = td.depends_on_task_id
                           WHERE td.task_id = t.id
                           AND blocker.internal_status NOT IN ('merged', 'cancelled', 'merge_incomplete')
                       )
                     ORDER BY t.priority DESC, t.created_at ASC
                     LIMIT 1"
                );
                conn.query_row(
                    &sql,
                    [project_id.as_str()],
                    |row| Task::from_row(row),
                )
            })
            .await
    }

    async fn get_by_project_filtered(
        &self,
        project_id: &ProjectId,
        include_archived: bool,
    ) -> AppResult<Vec<Task>> {
        let project_id = project_id.as_str().to_string();
        self.db
            .run(move |conn| {
                let query = query_builder::build_filtered_query(include_archived);
                let mut stmt = conn.prepare(&query)?;
                let tasks = stmt
                    .query_map([project_id.as_str()], Task::from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(tasks)
            })
            .await
    }

    async fn archive(&self, task_id: &TaskId) -> AppResult<Task> {
        let task_id = task_id.clone();
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_task_action_sync(
                        conn,
                        &task_id,
                        TasksFeatureAction::HistoryMutation,
                    )?;
                }
                let now = Utc::now();
                conn.execute(
                    "UPDATE tasks SET archived_at = ?2, updated_at = ?3 WHERE id = ?1",
                    rusqlite::params![task_id.as_str(), now.to_rfc3339(), now.to_rfc3339()],
                )?;
                let sql = format!("SELECT {} FROM tasks WHERE id = ?1", queries::TASK_COLUMNS);
                let task = conn.query_row(&sql, [task_id.as_str()], |row| Task::from_row(row))?;
                Ok(task)
            })
            .await
    }

    async fn restore(&self, task_id: &TaskId) -> AppResult<Task> {
        let task_id = task_id.clone();
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_task_action_sync(
                        conn,
                        &task_id,
                        TasksFeatureAction::HistoryMutation,
                    )?;
                }
                let now = Utc::now();
                conn.execute(
                    "UPDATE tasks SET archived_at = NULL, updated_at = ?2 WHERE id = ?1",
                    rusqlite::params![task_id.as_str(), now.to_rfc3339()],
                )?;
                let sql = format!("SELECT {} FROM tasks WHERE id = ?1", queries::TASK_COLUMNS);
                let task = conn.query_row(&sql, [task_id.as_str()], |row| Task::from_row(row))?;
                Ok(task)
            })
            .await
    }

    async fn get_archived_count(
        &self,
        project_id: &ProjectId,
        ideation_session_id: Option<&str>,
    ) -> AppResult<u32> {
        let project_id = project_id.as_str().to_string();
        let ideation_session_id = ideation_session_id.map(|s| s.to_string());
        self.db
            .run(move |conn| {
                let (query, params): (String, Vec<Box<dyn rusqlite::ToSql>>) =
                    if let Some(ref sid) = ideation_session_id {
                        (
                            "SELECT COUNT(*) FROM tasks WHERE project_id = ?1 AND archived_at IS NOT NULL AND ideation_session_id = ?2".to_string(),
                            vec![Box::new(project_id.clone()), Box::new(sid.clone())],
                        )
                    } else {
                        (
                            "SELECT COUNT(*) FROM tasks WHERE project_id = ?1 AND archived_at IS NOT NULL".to_string(),
                            vec![Box::new(project_id.clone())],
                        )
                    };
                let params_ref: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();
                let count: i64 =
                    conn.query_row(&query, params_ref.as_slice(), |row| row.get(0))?;
                Ok(count as u32)
            })
            .await
    }

    async fn list_paginated(
        &self,
        project_id: &ProjectId,
        statuses: Option<Vec<InternalStatus>>,
        offset: u32,
        limit: u32,
        include_archived: bool,
        ideation_session_id: Option<&str>,
        execution_plan_id: Option<&str>,
        categories: Option<&[String]>,
    ) -> AppResult<Vec<Task>> {
        let project_id = project_id.as_str().to_string();
        let ideation_session_id = ideation_session_id.map(|s| s.to_string());
        let execution_plan_id = execution_plan_id.map(|s| s.to_string());
        let categories: Option<Vec<String>> = categories.map(|c| c.to_vec());
        let status_count = statuses.as_ref().map_or(0, |s| s.len());
        let has_session_filter = ideation_session_id.is_some();
        let has_execution_plan_filter = execution_plan_id.is_some();
        let category_count = categories.as_ref().map_or(0, |c| c.len());
        self.db
            .run(move |conn| {
                let query = query_builder::build_paginated_query(
                    status_count,
                    include_archived,
                    has_session_filter,
                    has_execution_plan_filter,
                    category_count,
                );
                let mut stmt = conn.prepare(&query)?;

                let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
                params.push(Box::new(project_id.clone()));

                if let Some(ref status_vec) = statuses {
                    for s in status_vec {
                        params.push(Box::new(s.as_str().to_string()));
                    }
                }

                if let Some(ref sid) = ideation_session_id {
                    params.push(Box::new(sid.clone()));
                }

                if let Some(ref epid) = execution_plan_id {
                    params.push(Box::new(epid.clone()));
                }

                if let Some(ref cats) = categories {
                    for cat in cats {
                        params.push(Box::new(cat.clone()));
                    }
                }

                params.push(Box::new(limit as i64));
                params.push(Box::new(offset as i64));

                let params_ref: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();
                let tasks = stmt
                    .query_map(params_ref.as_slice(), Task::from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(tasks)
            })
            .await
    }

    async fn count_tasks(
        &self,
        project_id: &ProjectId,
        include_archived: bool,
        ideation_session_id: Option<&str>,
        execution_plan_id: Option<&str>,
    ) -> AppResult<u32> {
        let project_id = project_id.as_str().to_string();
        let ideation_session_id = ideation_session_id.map(|s| s.to_string());
        let execution_plan_id = execution_plan_id.map(|s| s.to_string());
        self.db
            .run(move |conn| {
                let mut conditions = vec!["project_id = ?1".to_string()];
                let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(project_id.clone())];
                let mut param_idx = 2;

                if !include_archived {
                    conditions.push("archived_at IS NULL".to_string());
                }

                if let Some(ref sid) = ideation_session_id {
                    conditions.push(format!("ideation_session_id = ?{}", param_idx));
                    params.push(Box::new(sid.clone()));
                    param_idx += 1;
                }

                if let Some(ref epid) = execution_plan_id {
                    conditions.push(format!("execution_plan_id = ?{}", param_idx));
                    params.push(Box::new(epid.clone()));
                    let _ = param_idx; // suppress unused warning
                }

                let query = format!(
                    "SELECT COUNT(*) FROM tasks WHERE {}",
                    conditions.join(" AND ")
                );
                let params_ref: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();
                let count: i64 = conn.query_row(&query, params_ref.as_slice(), |row| row.get(0))?;
                Ok(count as u32)
            })
            .await
    }

    async fn search(
        &self,
        project_id: &ProjectId,
        query: &str,
        include_archived: bool,
    ) -> AppResult<Vec<Task>> {
        let project_id = project_id.as_str().to_string();
        let query_str = query.to_string();
        self.db
            .run(move |conn| {
                let sql_query = query_builder::build_search_query(include_archived);
                let search_pattern = format!("%{}%", query_str);
                let mut stmt = conn.prepare(&sql_query)?;
                let tasks = stmt
                    .query_map(
                        rusqlite::params![project_id.as_str(), &search_pattern],
                        Task::from_row,
                    )?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(tasks)
            })
            .await
    }

    async fn get_oldest_ready_task(&self) -> AppResult<Option<Task>> {
        self.db
            .query_optional(|conn| {
                conn.query_row(queries::GET_OLDEST_READY_TASK, [], |row| {
                    Task::from_row(row)
                })
            })
            .await
    }

    async fn get_oldest_ready_tasks(&self, limit: u32) -> AppResult<Vec<Task>> {
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(queries::GET_OLDEST_READY_TASKS)?;
                let tasks = stmt
                    .query_map([limit as i64], Task::from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(tasks)
            })
            .await
    }

    async fn get_stale_ready_tasks(&self, threshold_secs: u64) -> AppResult<Vec<Task>> {
        use chrono::Duration;
        let cutoff = Utc::now() - Duration::seconds(threshold_secs as i64);
        let cutoff_str = cutoff.format("%Y-%m-%dT%H:%M:%S%.fZ").to_string();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(queries::GET_STALE_READY_TASKS)?;
                let tasks = stmt
                    .query_map([cutoff_str], Task::from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(tasks)
            })
            .await
    }

    async fn update_latest_state_history_metadata(
        &self,
        task_id: &TaskId,
        metadata: &StateHistoryMetadata,
    ) -> AppResult<()> {
        let task_id = task_id.clone();
        let metadata = metadata.clone();
        let enforce_tasks_feature_policy = self.enforce_tasks_feature_policy;
        self.db
            .run(move |conn| {
                if enforce_tasks_feature_policy {
                    authorize_task_action_sync(
                        conn,
                        &task_id,
                        TasksFeatureAction::HistoryMutation,
                    )?;
                }
                helpers::update_latest_state_history_metadata_sync(conn, &task_id, &metadata)
            })
            .await
    }

    async fn has_task_in_states(
        &self,
        project_id: &ProjectId,
        statuses: &[InternalStatus],
    ) -> AppResult<bool> {
        if statuses.is_empty() {
            return Ok(false);
        }

        let project_id = project_id.as_str().to_string();
        let statuses: Vec<String> = statuses.iter().map(|s| s.as_str().to_string()).collect();
        self.db
            .run(move |conn| {
                let placeholders: Vec<String> = (2..=statuses.len() + 1)
                    .map(|i| format!("?{}", i))
                    .collect();
                let placeholders_str = placeholders.join(", ");
                let query = format!(
                    "SELECT 1 FROM tasks
                     WHERE project_id = ?1
                       AND internal_status IN ({})
                       AND archived_at IS NULL
                     LIMIT 1",
                    placeholders_str
                );

                let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
                params.push(Box::new(project_id));
                for s in statuses {
                    params.push(Box::new(s));
                }

                let params_ref: Vec<&dyn rusqlite::ToSql> =
                    params.iter().map(|p| p.as_ref()).collect();
                let result: rusqlite::Result<i32> =
                    conn.query_row(&query, params_ref.as_slice(), |row| row.get(0));

                match result {
                    Ok(_) => Ok(true),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
                    Err(e) => Err(AppError::from(e)),
                }
            })
            .await
    }
}

#[cfg(test)]
mod tests;

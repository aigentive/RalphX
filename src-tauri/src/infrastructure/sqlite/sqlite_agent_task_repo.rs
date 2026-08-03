use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use rusqlite::{params, types::Type, Connection, OptionalExtension};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use uuid::Uuid;

use super::DbConnection;
use crate::domain::entities::{
    merge_agent_task_metadata, AgentRunId, AgentTaskAssignmentId, AgentTaskAssignmentReservation,
    AgentTaskAssignmentSettlement, AgentTaskAssignmentTerminalStatus, AgentTaskAssignmentView,
    AgentTaskCreate, AgentTaskDetail, AgentTaskId, AgentTaskList, AgentTaskListId,
    AgentTaskListSummary, AgentTaskMutationResult, AgentTaskPatch, AgentTaskScope, AgentTaskState,
    AgentTaskStateChange, AgentTaskSummary, DelegatedSessionId, ProjectId,
};
use crate::domain::repositories::{AgentTaskListOptions, AgentTaskRepository};
use crate::error::{AppError, AppResult};

mod assignments;

#[derive(Clone)]
struct AgentTaskRow {
    task_id: AgentTaskId,
    task_list_id: AgentTaskListId,
    task_number: i64,
    title: String,
    details: String,
    active_label: Option<String>,
    owner_agent: Option<String>,
    state: AgentTaskState,
    metadata: Option<Value>,
    version: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

pub struct SqliteAgentTaskRepository {
    db: DbConnection,
}

impl SqliteAgentTaskRepository {
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
impl AgentTaskRepository for SqliteAgentTaskRepository {
    async fn create_task(
        &self,
        scope: &AgentTaskScope,
        input: AgentTaskCreate,
    ) -> AppResult<AgentTaskMutationResult> {
        validate_title_and_details(&input.title, &input.details)?;
        let scope = scope.clone();
        self.db
            .run_transaction(move |conn| {
                let list = ensure_list(conn, &scope)?;
                let now = Utc::now();
                let task_id = AgentTaskId::new();
                let task_number = list.next_task_number;
                conn.execute(
                    "UPDATE agent_task_lists
                     SET next_task_number = next_task_number + 1, updated_at = ?1
                     WHERE id = ?2",
                    params![now.to_rfc3339(), list.id.as_str()],
                )?;
                conn.execute(
                    "INSERT INTO agent_tasks (
                        id, task_list_id, task_number, title, details, active_label,
                        owner_agent, state, metadata_json, version,
                        created_at, updated_at, completed_at
                    ) VALUES (
                        ?1, ?2, ?3, ?4, ?5, ?6,
                        ?7, ?8, ?9, 1,
                        ?10, ?11, NULL
                    )",
                    params![
                        task_id.as_str(),
                        list.id.as_str(),
                        task_number,
                        input.title,
                        input.details,
                        input.active_label,
                        input.owner_agent,
                        AgentTaskState::Open.as_str(),
                        value_to_json_text(&input.metadata)?,
                        now.to_rfc3339(),
                        now.to_rfc3339(),
                    ],
                )?;

                for blocker_ref in input.blocked_by {
                    let blocker = resolve_task_id(conn, &list.id, &blocker_ref)?;
                    add_dependency(conn, &list.id, &blocker, &task_id)?;
                }
                for blocked_ref in input.blocks {
                    let blocked = resolve_task_id(conn, &list.id, &blocked_ref)?;
                    add_dependency(conn, &list.id, &task_id, &blocked)?;
                }
                append_event(
                    conn,
                    &list.id,
                    "agent_task.created",
                    scope.actor_agent.as_deref(),
                    Some(&task_id),
                    json!({"task_number": task_number}),
                )?;

                let detail = detail_for_task_id(conn, &list.id, &task_id)?;
                Ok(AgentTaskMutationResult {
                    task: detail,
                    changed_fields: vec!["created".to_string()],
                    state_change: None,
                })
            })
            .await
    }

    async fn get_task(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
    ) -> AppResult<Option<AgentTaskDetail>> {
        let scope = scope.clone();
        let task_ref = task_ref.to_string();
        self.db
            .run(move |conn| {
                let Some(list) = find_list(conn, &scope)? else {
                    return Ok(None);
                };
                let Some(row) = find_task(conn, &list.id, &task_ref)? else {
                    return Ok(None);
                };
                Ok(Some(detail_for_row(conn, &row)?))
            })
            .await
    }

    async fn list_tasks(
        &self,
        scope: &AgentTaskScope,
        options: AgentTaskListOptions,
    ) -> AppResult<Vec<AgentTaskSummary>> {
        let scope = scope.clone();
        self.db
            .run(move |conn| {
                let Some(list) = find_list(conn, &scope)? else {
                    return Ok(Vec::new());
                };
                list_tasks_for_list_id(conn, &list.id, options)
            })
            .await
    }

    async fn list_task_lists(
        &self,
        scope: &AgentTaskScope,
    ) -> AppResult<Vec<AgentTaskListSummary>> {
        let scope = scope.clone();
        self.db
            .run(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT
                        l.id,
                        l.list_sequence,
                        COUNT(t.id) AS task_count,
                        COALESCE(SUM(CASE WHEN t.state = 'open' THEN 1 ELSE 0 END), 0) AS open_count,
                        COALESCE(SUM(CASE WHEN t.state = 'active' THEN 1 ELSE 0 END), 0) AS active_count,
                        COALESCE(SUM(CASE WHEN t.state = 'done' THEN 1 ELSE 0 END), 0) AS done_count,
                        COALESCE(SUM(CASE WHEN t.state = 'dropped' THEN 1 ELSE 0 END), 0) AS dropped_count,
                        l.created_at,
                        l.updated_at
                     FROM agent_task_lists l
                     LEFT JOIN agent_tasks t ON t.task_list_id = l.id
                     WHERE l.scope_type = ?1 AND l.scope_id = ?2
                     GROUP BY l.id, l.list_sequence, l.created_at, l.updated_at
                     ORDER BY l.list_sequence DESC",
                )?;
                let rows = stmt.query_map(
                    params![scope.scope_type.as_str(), scope.scope_id.as_str()],
                    row_to_list_summary,
                )?
                .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }

    async fn list_tasks_for_list(
        &self,
        scope: &AgentTaskScope,
        list_id: &AgentTaskListId,
        options: AgentTaskListOptions,
    ) -> AppResult<Vec<AgentTaskSummary>> {
        let scope = scope.clone();
        let list_id = list_id.clone();
        self.db
            .run(move |conn| {
                let Some(list) = find_list_by_id(conn, &scope, &list_id)? else {
                    return Ok(Vec::new());
                };
                list_tasks_for_list_id(conn, &list.id, options)
            })
            .await
    }

    async fn update_task(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
        patch: AgentTaskPatch,
    ) -> AppResult<Option<AgentTaskMutationResult>> {
        let scope = scope.clone();
        let task_ref = task_ref.to_string();
        self.db
            .run_transaction(move |conn| {
                let Some(list) = find_list(conn, &scope)? else {
                    return Ok(None);
                };
                let Some(mut row) = find_task(conn, &list.id, &task_ref)? else {
                    return Ok(None);
                };
                assignments::ensure_mutation_allowed(conn, &list.id, &row.task_id, &patch)?;

                let mut changed_fields = Vec::new();
                let mut state_change = None;

                if let Some(title) = patch.title {
                    if title.trim().is_empty() {
                        return Err(AppError::Validation(
                            "agent task title is required".to_string(),
                        ));
                    }
                    if row.title != title {
                        row.title = title;
                        changed_fields.push("title".to_string());
                    }
                }
                if let Some(details) = patch.details {
                    if details.trim().is_empty() {
                        return Err(AppError::Validation(
                            "agent task details are required".to_string(),
                        ));
                    }
                    if row.details != details {
                        row.details = details;
                        changed_fields.push("details".to_string());
                    }
                }
                if let Some(active_label) = patch.active_label {
                    if row.active_label != active_label {
                        row.active_label = active_label;
                        changed_fields.push("active_label".to_string());
                    }
                }
                if let Some(owner_agent) = patch.owner_agent {
                    if row.owner_agent != owner_agent {
                        row.owner_agent = owner_agent;
                        changed_fields.push("owner_agent".to_string());
                    }
                }
                if let Some(next_state) = patch.state {
                    if row.state != next_state {
                        let previous = row.state;
                        row.state = next_state;
                        row.completed_at = if next_state == AgentTaskState::Done {
                            Some(Utc::now())
                        } else {
                            None
                        };
                        state_change = Some(AgentTaskStateChange {
                            from: previous,
                            to: next_state,
                        });
                        changed_fields.push("state".to_string());
                    }
                }
                if let Some(metadata_patch) = patch.metadata_patch {
                    row.metadata = merge_agent_task_metadata(row.metadata, metadata_patch);
                    changed_fields.push("metadata".to_string());
                }

                let mut dependencies_changed = false;
                for blocker_ref in patch.add_blocked_by {
                    let blocker = resolve_task_id(conn, &list.id, &blocker_ref)?;
                    dependencies_changed |= add_dependency(conn, &list.id, &blocker, &row.task_id)?;
                }
                for blocked_ref in patch.add_blocks {
                    let blocked = resolve_task_id(conn, &list.id, &blocked_ref)?;
                    dependencies_changed |= add_dependency(conn, &list.id, &row.task_id, &blocked)?;
                }
                for blocker_ref in patch.remove_blocked_by {
                    let blocker = resolve_task_id(conn, &list.id, &blocker_ref)?;
                    dependencies_changed |=
                        remove_dependency(conn, &list.id, &blocker, &row.task_id)?;
                }
                for blocked_ref in patch.remove_blocks {
                    let blocked = resolve_task_id(conn, &list.id, &blocked_ref)?;
                    dependencies_changed |=
                        remove_dependency(conn, &list.id, &row.task_id, &blocked)?;
                }
                if dependencies_changed {
                    changed_fields.push("dependencies".to_string());
                }

                changed_fields.sort();
                changed_fields.dedup();
                if !changed_fields.is_empty() {
                    let now = Utc::now();
                    conn.execute(
                        "UPDATE agent_tasks
                         SET title = ?1, details = ?2, active_label = ?3,
                             owner_agent = ?4, state = ?5, metadata_json = ?6,
                             version = version + 1, updated_at = ?7, completed_at = ?8
                         WHERE task_list_id = ?9 AND id = ?10",
                        params![
                            row.title,
                            row.details,
                            row.active_label,
                            row.owner_agent,
                            row.state.as_str(),
                            value_to_json_text(&row.metadata)?,
                            now.to_rfc3339(),
                            row.completed_at.map(|dt| dt.to_rfc3339()),
                            list.id.as_str(),
                            row.task_id.as_str(),
                        ],
                    )?;
                    append_update_events(
                        conn,
                        &list.id,
                        scope.actor_agent.as_deref(),
                        &row.task_id,
                        &changed_fields,
                        state_change.as_ref(),
                    )?;
                }

                let detail = detail_for_task_id(conn, &list.id, &row.task_id)?;
                Ok(Some(AgentTaskMutationResult {
                    task: detail,
                    changed_fields,
                    state_change,
                }))
            })
            .await
    }

    async fn reserve_assignment(
        &self,
        scope: &AgentTaskScope,
        task_ref: &str,
        delegated_session_id: &DelegatedSessionId,
        caller_agent_run_id: &AgentRunId,
        delegate_agent_name: &str,
    ) -> AppResult<Option<AgentTaskAssignmentReservation>> {
        assignments::reserve(
            &self.db,
            scope,
            task_ref,
            delegated_session_id,
            caller_agent_run_id,
            delegate_agent_name,
        )
        .await
    }

    async fn bind_assignment_run(
        &self,
        assignment_id: &AgentTaskAssignmentId,
        delegated_session_id: &DelegatedSessionId,
        delegated_agent_run_id: &AgentRunId,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        assignments::bind_run(
            &self.db,
            assignment_id,
            delegated_session_id,
            delegated_agent_run_id,
        )
        .await
    }

    async fn plan_assignment_run(
        &self,
        assignment_id: &AgentTaskAssignmentId,
        delegated_session_id: &DelegatedSessionId,
        delegated_agent_run_id: &AgentRunId,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        assignments::plan_run(
            &self.db,
            assignment_id,
            delegated_session_id,
            delegated_agent_run_id,
        )
        .await
    }

    async fn set_assignment_team_identity(
        &self,
        assignment_id: &AgentTaskAssignmentId,
        delegated_session_id: &DelegatedSessionId,
        team_id: &crate::domain::entities::TeamSessionId,
        team_member_id: &crate::domain::entities::TeamMemberId,
        team_member_generation: i64,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        assignments::set_team_identity(
            &self.db,
            assignment_id,
            delegated_session_id,
            team_id,
            team_member_id,
            team_member_generation,
        )
        .await
    }

    async fn get_unresolved_assignment(
        &self,
        delegated_session_id: &DelegatedSessionId,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        assignments::get_unresolved(&self.db, delegated_session_id).await
    }

    async fn request_assignment_completion(
        &self,
        delegated_session_id: &DelegatedSessionId,
        delegated_agent_run_id: &AgentRunId,
        local_scope: &AgentTaskScope,
        completion_metadata: Option<Value>,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        assignments::request_completion(
            &self.db,
            delegated_session_id,
            delegated_agent_run_id,
            local_scope,
            completion_metadata,
        )
        .await
    }

    async fn request_assignment_release(
        &self,
        delegated_session_id: &DelegatedSessionId,
        delegated_agent_run_id: &AgentRunId,
        reason: &str,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        assignments::request_release(
            &self.db,
            delegated_session_id,
            delegated_agent_run_id,
            reason,
        )
        .await
    }

    async fn settle_assignment_for_run(
        &self,
        delegated_agent_run_id: &AgentRunId,
        terminal_status: AgentTaskAssignmentTerminalStatus,
        reason: Option<&str>,
    ) -> AppResult<Option<AgentTaskAssignmentSettlement>> {
        assignments::settle(&self.db, delegated_agent_run_id, terminal_status, reason).await
    }

    async fn get_assignment_for_run(
        &self,
        delegated_agent_run_id: &AgentRunId,
    ) -> AppResult<Option<AgentTaskAssignmentView>> {
        assignments::get_for_run(&self.db, delegated_agent_run_id).await
    }

    async fn fail_reserved_assignment(
        &self,
        delegated_session_id: &DelegatedSessionId,
        reason: &str,
    ) -> AppResult<Option<AgentTaskAssignmentSettlement>> {
        assignments::fail_reserved(&self.db, delegated_session_id, reason).await
    }

    async fn list_unresolved_assignments(&self) -> AppResult<Vec<AgentTaskAssignmentView>> {
        assignments::list_unresolved(&self.db).await
    }
}

fn validate_title_and_details(title: &str, details: &str) -> AppResult<()> {
    if title.trim().is_empty() {
        return Err(AppError::Validation(
            "agent task title is required".to_string(),
        ));
    }
    if details.trim().is_empty() {
        return Err(AppError::Validation(
            "agent task details are required".to_string(),
        ));
    }
    Ok(())
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

fn parse_state(value: &str) -> rusqlite::Result<AgentTaskState> {
    value.parse::<AgentTaskState>().map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            Type::Text,
            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, error)),
        )
    })
}

fn parse_metadata(value: Option<String>) -> rusqlite::Result<Option<Value>> {
    value
        .map(|raw| {
            serde_json::from_str::<Value>(&raw).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error))
            })
        })
        .transpose()
}

fn value_to_json_text(value: &Option<Value>) -> AppResult<Option<String>> {
    value
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| AppError::Database(error.to_string()))
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentTaskRow> {
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    let completed_at: Option<String> = row.get("completed_at")?;
    let state: String = row.get("state")?;
    Ok(AgentTaskRow {
        task_id: AgentTaskId::from_string(row.get::<_, String>("id")?),
        task_list_id: AgentTaskListId::from_string(row.get::<_, String>("task_list_id")?),
        task_number: row.get("task_number")?,
        title: row.get("title")?,
        details: row.get("details")?,
        active_label: row.get("active_label")?,
        owner_agent: row.get("owner_agent")?,
        state: parse_state(&state)?,
        metadata: parse_metadata(row.get("metadata_json")?)?,
        version: row.get("version")?,
        created_at: parse_datetime(&created_at),
        updated_at: parse_datetime(&updated_at),
        completed_at: completed_at.as_deref().map(parse_datetime),
    })
}

fn row_to_list_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentTaskListSummary> {
    let created_at: String = row.get("created_at")?;
    let updated_at: String = row.get("updated_at")?;
    Ok(AgentTaskListSummary {
        list_id: AgentTaskListId::from_string(row.get::<_, String>("id")?),
        list_sequence: row.get("list_sequence")?,
        task_count: row.get("task_count")?,
        open_count: row.get("open_count")?,
        active_count: row.get("active_count")?,
        done_count: row.get("done_count")?,
        dropped_count: row.get("dropped_count")?,
        created_at: parse_datetime(&created_at),
        updated_at: parse_datetime(&updated_at),
    })
}

fn find_list(conn: &Connection, scope: &AgentTaskScope) -> AppResult<Option<AgentTaskList>> {
    let result = conn
        .query_row(
            "SELECT id, project_id, scope_type, scope_id, name, created_by_agent,
                    list_sequence, next_task_number, created_at, updated_at
             FROM agent_task_lists
             WHERE scope_type = ?1 AND scope_id = ?2
             ORDER BY list_sequence DESC
             LIMIT 1",
            params![scope.scope_type.as_str(), scope.scope_id.as_str()],
            |row| {
                let created_at: String = row.get("created_at")?;
                let updated_at: String = row.get("updated_at")?;
                Ok(AgentTaskList {
                    id: AgentTaskListId::from_string(row.get::<_, String>("id")?),
                    project_id: row
                        .get::<_, Option<String>>("project_id")?
                        .map(ProjectId::from_string),
                    scope_type: row.get("scope_type")?,
                    scope_id: row.get("scope_id")?,
                    list_sequence: row.get("list_sequence")?,
                    name: row.get("name")?,
                    created_by_agent: row.get("created_by_agent")?,
                    next_task_number: row.get("next_task_number")?,
                    created_at: parse_datetime(&created_at),
                    updated_at: parse_datetime(&updated_at),
                })
            },
        )
        .optional()?;
    Ok(result)
}

fn find_list_by_id(
    conn: &Connection,
    scope: &AgentTaskScope,
    list_id: &AgentTaskListId,
) -> AppResult<Option<AgentTaskList>> {
    let result = conn
        .query_row(
            "SELECT id, project_id, scope_type, scope_id, name, created_by_agent,
                    list_sequence, next_task_number, created_at, updated_at
             FROM agent_task_lists
             WHERE id = ?1 AND scope_type = ?2 AND scope_id = ?3",
            params![
                list_id.as_str(),
                scope.scope_type.as_str(),
                scope.scope_id.as_str()
            ],
            |row| {
                let created_at: String = row.get("created_at")?;
                let updated_at: String = row.get("updated_at")?;
                Ok(AgentTaskList {
                    id: AgentTaskListId::from_string(row.get::<_, String>("id")?),
                    project_id: row
                        .get::<_, Option<String>>("project_id")?
                        .map(ProjectId::from_string),
                    scope_type: row.get("scope_type")?,
                    scope_id: row.get("scope_id")?,
                    list_sequence: row.get("list_sequence")?,
                    name: row.get("name")?,
                    created_by_agent: row.get("created_by_agent")?,
                    next_task_number: row.get("next_task_number")?,
                    created_at: parse_datetime(&created_at),
                    updated_at: parse_datetime(&updated_at),
                })
            },
        )
        .optional()?;
    Ok(result)
}

fn ensure_list(conn: &Connection, scope: &AgentTaskScope) -> AppResult<AgentTaskList> {
    if let Some(list) = find_list(conn, scope)? {
        if list_should_roll_over(conn, &list.id)? {
            return create_list(conn, scope, list.list_sequence + 1);
        }
        return Ok(list);
    }
    create_list(conn, scope, 1)
}

fn create_list(
    conn: &Connection,
    scope: &AgentTaskScope,
    list_sequence: i64,
) -> AppResult<AgentTaskList> {
    let now = Utc::now();
    let list = AgentTaskList {
        id: AgentTaskListId::new(),
        project_id: scope.project_id.clone(),
        scope_type: scope.scope_type.clone(),
        scope_id: scope.scope_id.clone(),
        list_sequence,
        name: None,
        created_by_agent: scope.actor_agent.clone(),
        next_task_number: 1,
        created_at: now,
        updated_at: now,
    };
    conn.execute(
        "INSERT INTO agent_task_lists (
            id, project_id, scope_type, scope_id, list_sequence, name, created_by_agent,
            next_task_number, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            list.id.as_str(),
            list.project_id.as_ref().map(|id| id.as_str()),
            list.scope_type.as_str(),
            list.scope_id.as_str(),
            list.list_sequence,
            list.name.as_deref(),
            list.created_by_agent.as_deref(),
            list.next_task_number,
            list.created_at.to_rfc3339(),
            list.updated_at.to_rfc3339(),
        ],
    )?;
    append_event(
        conn,
        &list.id,
        "agent_task_list.created",
        scope.actor_agent.as_deref(),
        None,
        json!({
            "scope_type": scope.scope_type.as_str(),
            "scope_id": scope.scope_id.as_str(),
            "list_sequence": list.list_sequence
        }),
    )?;
    Ok(list)
}

fn list_should_roll_over(conn: &Connection, list_id: &AgentTaskListId) -> AppResult<bool> {
    let (total, actionable): (i64, i64) = conn.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(CASE WHEN state NOT IN ('done', 'dropped') THEN 1 ELSE 0 END), 0)
         FROM agent_tasks
         WHERE task_list_id = ?1",
        [list_id.as_str()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(total > 0 && actionable == 0)
}

fn list_tasks_for_list_id(
    conn: &Connection,
    list_id: &AgentTaskListId,
    options: AgentTaskListOptions,
) -> AppResult<Vec<AgentTaskSummary>> {
    let sql = if options.include_done {
        "SELECT id, task_list_id, task_number, title, details, active_label,
                owner_agent, state, metadata_json, version,
                created_at, updated_at, completed_at
         FROM agent_tasks
         WHERE task_list_id = ?1
         ORDER BY task_number ASC"
    } else {
        "SELECT id, task_list_id, task_number, title, details, active_label,
                owner_agent, state, metadata_json, version,
                created_at, updated_at, completed_at
         FROM agent_tasks
         WHERE task_list_id = ?1 AND state NOT IN ('done', 'dropped')
         ORDER BY task_number ASC"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt
        .query_map([list_id.as_str()], row_to_task)?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|row| summary_for_row(conn, &row))
        .collect()
}

fn find_task(
    conn: &Connection,
    list_id: &AgentTaskListId,
    task_ref: &str,
) -> AppResult<Option<AgentTaskRow>> {
    let result = conn
        .query_row(
            "SELECT id, task_list_id, task_number, title, details, active_label,
                    owner_agent, state, metadata_json, version,
                    created_at, updated_at, completed_at
             FROM agent_tasks
             WHERE task_list_id = ?1 AND (id = ?2 OR CAST(task_number AS TEXT) = ?2)",
            params![list_id.as_str(), task_ref],
            row_to_task,
        )
        .optional()?;
    Ok(result)
}

fn resolve_task_id(
    conn: &Connection,
    list_id: &AgentTaskListId,
    task_ref: &str,
) -> AppResult<AgentTaskId> {
    find_task(conn, list_id, task_ref)?
        .map(|row| row.task_id)
        .ok_or_else(|| AppError::Validation(format!("agent task dependency not found: {task_ref}")))
}

fn detail_for_task_id(
    conn: &Connection,
    list_id: &AgentTaskListId,
    task_id: &AgentTaskId,
) -> AppResult<AgentTaskDetail> {
    let row = conn.query_row(
        "SELECT id, task_list_id, task_number, title, details, active_label,
                owner_agent, state, metadata_json, version,
                created_at, updated_at, completed_at
         FROM agent_tasks
         WHERE task_list_id = ?1 AND id = ?2",
        params![list_id.as_str(), task_id.as_str()],
        row_to_task,
    )?;
    detail_for_row(conn, &row)
}

fn detail_for_row(conn: &Connection, row: &AgentTaskRow) -> AppResult<AgentTaskDetail> {
    let blocked_by = blocker_ids(conn, &row.task_list_id, &row.task_id)?
        .iter()
        .map(|task_id| task_ref_for_id(conn, &row.task_list_id, task_id))
        .collect::<AppResult<Vec<_>>>()?;
    let unresolved_blocked_by = unresolved_blocker_ids(conn, &row.task_list_id, &row.task_id)?
        .iter()
        .map(|task_id| task_ref_for_id(conn, &row.task_list_id, task_id))
        .collect::<AppResult<Vec<_>>>()?;
    let blocks = blocked_ids(conn, &row.task_list_id, &row.task_id)?
        .iter()
        .map(|task_id| task_ref_for_id(conn, &row.task_list_id, task_id))
        .collect::<AppResult<Vec<_>>>()?;
    Ok(AgentTaskDetail {
        task_id: row.task_id.clone(),
        task_number: row.task_number,
        title: row.title.clone(),
        details: row.details.clone(),
        active_label: row.active_label.clone(),
        owner_agent: row.owner_agent.clone(),
        state: row.state,
        metadata: row.metadata.clone(),
        blocked_by,
        unresolved_blocked_by,
        blocks,
        version: row.version,
        created_at: row.created_at,
        updated_at: row.updated_at,
        completed_at: row.completed_at,
    })
}

fn summary_for_row(conn: &Connection, row: &AgentTaskRow) -> AppResult<AgentTaskSummary> {
    let detail = detail_for_row(conn, row)?;
    let availability = detail.availability().to_string();
    Ok(AgentTaskSummary {
        task_id: detail.task_id,
        task_number: detail.task_number,
        title: detail.title,
        state: detail.state,
        owner_agent: detail.owner_agent,
        blocked_by: detail.unresolved_blocked_by,
        blocks: detail.blocks,
        availability,
        updated_at: detail.updated_at,
    })
}

fn task_ref_for_id(
    conn: &Connection,
    list_id: &AgentTaskListId,
    task_id: &AgentTaskId,
) -> AppResult<String> {
    let result = conn
        .query_row(
            "SELECT task_number FROM agent_tasks WHERE task_list_id = ?1 AND id = ?2",
            params![list_id.as_str(), task_id.as_str()],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    Ok(result
        .map(|number| number.to_string())
        .unwrap_or_else(|| task_id.to_string()))
}

fn blocker_ids(
    conn: &Connection,
    list_id: &AgentTaskListId,
    task_id: &AgentTaskId,
) -> AppResult<Vec<AgentTaskId>> {
    query_dependency_ids(
        conn,
        "SELECT blocker_task_id FROM agent_task_dependencies
         WHERE task_list_id = ?1 AND blocked_task_id = ?2
         ORDER BY created_at ASC, blocker_task_id ASC",
        list_id,
        task_id,
    )
}

fn unresolved_blocker_ids(
    conn: &Connection,
    list_id: &AgentTaskListId,
    task_id: &AgentTaskId,
) -> AppResult<Vec<AgentTaskId>> {
    let mut stmt = conn.prepare(
        "SELECT dep.blocker_task_id
         FROM agent_task_dependencies dep
         JOIN agent_tasks blocker
           ON blocker.task_list_id = dep.task_list_id
          AND blocker.id = dep.blocker_task_id
         WHERE dep.task_list_id = ?1
           AND dep.blocked_task_id = ?2
           AND blocker.state NOT IN ('done', 'dropped')
         ORDER BY dep.created_at ASC, dep.blocker_task_id ASC",
    )?;
    let ids = stmt
        .query_map(params![list_id.as_str(), task_id.as_str()], |row| {
            Ok(AgentTaskId::from_string(row.get::<_, String>(0)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

fn blocked_ids(
    conn: &Connection,
    list_id: &AgentTaskListId,
    task_id: &AgentTaskId,
) -> AppResult<Vec<AgentTaskId>> {
    query_dependency_ids(
        conn,
        "SELECT blocked_task_id FROM agent_task_dependencies
         WHERE task_list_id = ?1 AND blocker_task_id = ?2
         ORDER BY created_at ASC, blocked_task_id ASC",
        list_id,
        task_id,
    )
}

fn query_dependency_ids(
    conn: &Connection,
    sql: &str,
    list_id: &AgentTaskListId,
    task_id: &AgentTaskId,
) -> AppResult<Vec<AgentTaskId>> {
    let mut stmt = conn.prepare(sql)?;
    let ids = stmt
        .query_map(params![list_id.as_str(), task_id.as_str()], |row| {
            Ok(AgentTaskId::from_string(row.get::<_, String>(0)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

fn add_dependency(
    conn: &Connection,
    list_id: &AgentTaskListId,
    blocker_id: &AgentTaskId,
    blocked_id: &AgentTaskId,
) -> AppResult<bool> {
    if blocker_id == blocked_id {
        return Err(AppError::Validation(
            "agent task dependency cannot reference itself".to_string(),
        ));
    }
    let exists = conn.query_row(
        "SELECT 1 FROM agent_task_dependencies
         WHERE task_list_id = ?1 AND blocker_task_id = ?2 AND blocked_task_id = ?3",
        params![list_id.as_str(), blocker_id.as_str(), blocked_id.as_str()],
        |_| Ok(()),
    );
    if exists.optional()?.is_some() {
        return Ok(false);
    }
    if has_path(conn, list_id, blocked_id, blocker_id)? {
        return Err(AppError::Validation(
            "agent task dependency would create a cycle".to_string(),
        ));
    }
    let inserted = conn.execute(
        "INSERT INTO agent_task_dependencies (
            task_list_id, blocker_task_id, blocked_task_id, created_at
        ) VALUES (?1, ?2, ?3, ?4)",
        params![
            list_id.as_str(),
            blocker_id.as_str(),
            blocked_id.as_str(),
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(inserted > 0)
}

fn remove_dependency(
    conn: &Connection,
    list_id: &AgentTaskListId,
    blocker_id: &AgentTaskId,
    blocked_id: &AgentTaskId,
) -> AppResult<bool> {
    let deleted = conn.execute(
        "DELETE FROM agent_task_dependencies
         WHERE task_list_id = ?1 AND blocker_task_id = ?2 AND blocked_task_id = ?3",
        params![list_id.as_str(), blocker_id.as_str(), blocked_id.as_str()],
    )?;
    Ok(deleted > 0)
}

fn has_path(
    conn: &Connection,
    list_id: &AgentTaskListId,
    start: &AgentTaskId,
    target: &AgentTaskId,
) -> AppResult<bool> {
    let mut visited = std::collections::HashSet::new();
    let mut stack = vec![start.clone()];
    while let Some(current) = stack.pop() {
        if current == *target {
            return Ok(true);
        }
        if !visited.insert(current.clone()) {
            continue;
        }
        for next in blocked_ids(conn, list_id, &current)? {
            stack.push(next);
        }
    }
    Ok(false)
}

fn append_update_events(
    conn: &Connection,
    list_id: &AgentTaskListId,
    actor_agent: Option<&str>,
    task_id: &AgentTaskId,
    changed_fields: &[String],
    state_change: Option<&AgentTaskStateChange>,
) -> AppResult<()> {
    append_event(
        conn,
        list_id,
        "agent_task.updated",
        actor_agent,
        Some(task_id),
        json!({"changed_fields": changed_fields}),
    )?;
    if let Some(change) = state_change {
        append_event(
            conn,
            list_id,
            "agent_task.state_changed",
            actor_agent,
            Some(task_id),
            json!({"from": change.from.as_str(), "to": change.to.as_str()}),
        )?;
    }
    if changed_fields.iter().any(|field| field == "owner_agent") {
        append_event(
            conn,
            list_id,
            "agent_task.owner_changed",
            actor_agent,
            Some(task_id),
            json!({}),
        )?;
    }
    if changed_fields.iter().any(|field| field == "metadata") {
        append_event(
            conn,
            list_id,
            "agent_task.metadata_changed",
            actor_agent,
            Some(task_id),
            json!({}),
        )?;
    }
    if changed_fields.iter().any(|field| field == "dependencies") {
        append_event(
            conn,
            list_id,
            "agent_task.dependencies_changed",
            actor_agent,
            Some(task_id),
            json!({}),
        )?;
    }
    Ok(())
}

fn append_event(
    conn: &Connection,
    list_id: &AgentTaskListId,
    event_type: &str,
    actor_agent: Option<&str>,
    task_id: Option<&AgentTaskId>,
    payload: Value,
) -> AppResult<()> {
    let seq = conn
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM agent_task_events WHERE task_list_id = ?1",
            params![list_id.as_str()],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(1);
    conn.execute(
        "INSERT INTO agent_task_events (
            event_id, task_list_id, seq, event_type, actor_agent,
            task_id, payload_json, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            Uuid::new_v4().to_string(),
            list_id.as_str(),
            seq,
            event_type,
            actor_agent,
            task_id.map(|id| id.as_str()),
            serde_json::to_string(&payload)
                .map_err(|error| AppError::Database(error.to_string()))?,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ChatConversationId, ChatMessageId, ExecutionPlan,
    IdeationSessionId, IdeationSessionStatus, InternalStatus, MessageRole, PlanBranch,
    PlanBranchStatus, Task, TaskCategory, TaskStep,
};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendIdeationPlanTaskInput {
    pub project_id: Option<String>,
    pub session_id: String,
    pub title: String,
    pub description: Option<String>,
    #[serde(default)]
    pub steps: Vec<String>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub depends_on_task_ids: Vec<String>,
    pub priority: Option<i32>,
    pub source_conversation_id: Option<String>,
    pub source_message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppendIdeationPlanTaskResult {
    pub project_id: String,
    pub session_id: String,
    pub task_id: String,
    pub execution_plan_id: String,
    pub plan_branch_id: String,
    pub merge_task_id: String,
    pub task_status: String,
    pub dependencies_created: usize,
    pub any_ready_tasks: bool,
}

pub async fn append_ideation_plan_task_core(
    app_state: &AppState,
    input: AppendIdeationPlanTaskInput,
) -> AppResult<AppendIdeationPlanTaskResult> {
    let title = input.title.trim().to_string();
    if title.is_empty() {
        return Err(AppError::Validation(
            "Task title is required when appending to an ideation plan".to_string(),
        ));
    }

    let session_id = IdeationSessionId::from_string(input.session_id.clone());
    crate::application::tasks_feature_policy::TasksFeaturePolicy::from_state(app_state)
        .authorize_session(
            Some(&session_id),
            crate::domain::ideation::TasksFeatureAction::Progress,
        )
        .await?;
    let tasks_owner = app_state
        .agent_conversation_workspace_repo
        .get_by_task_pipeline_session_id(&session_id)
        .await?
        .filter(|workspace| workspace.mode == AgentConversationWorkspaceMode::Tasks);
    let tasks_source_identity = if let Some(workspace) = tasks_owner {
        let conversation_id = input
            .source_conversation_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                AppError::Validation(
                    "Tasks follow-ups require an explicit source user message".to_string(),
                )
            })?;
        let message_id = input
            .source_message_id
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                AppError::Validation(
                    "Tasks follow-ups require an explicit source user message".to_string(),
                )
            })?;
        if conversation_id != workspace.conversation_id.as_str() {
            return Err(AppError::Validation(
                "Tasks follow-ups must come from the owning conversation".to_string(),
            ));
        }
        if workspace
            .publication_pr_status
            .as_deref()
            .is_some_and(|status| matches!(status, "closed" | "merged"))
        {
            return Err(AppError::Validation(
                "Cannot append after the attached pull request is closed or merged".to_string(),
            ));
        }
        let source_message = app_state
            .chat_message_repo
            .get_by_id(&ChatMessageId::from_string(message_id.to_string()))
            .await?
            .ok_or_else(|| {
                AppError::Validation("Tasks follow-up source message was not found".to_string())
            })?;
        if source_message.role != MessageRole::User
            || source_message.conversation_id.as_ref() != Some(&workspace.conversation_id)
        {
            return Err(AppError::Validation(
                "Tasks follow-ups must reference a user message from the owning conversation"
                    .to_string(),
            ));
        }
        Some((conversation_id.to_string(), message_id.to_string()))
    } else if let Some(conversation_id) = input.source_conversation_id.as_deref() {
        let workspace = app_state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&ChatConversationId::from_string(
                conversation_id.to_string(),
            ))
            .await?;
        if workspace
            .is_some_and(|workspace| workspace.mode == AgentConversationWorkspaceMode::Tasks)
        {
            return Err(AppError::Validation(
                "Tasks conversation is not attached to this pipeline".to_string(),
            ));
        }
        None
    } else {
        None
    };
    let _attempt_mutation_guard =
        super::ideation_commands_restart::RestartInFlightGuard::acquire(&session_id)?;
    let session = app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|e| AppError::Database(format!("Failed to load ideation session: {}", e)))?
        .ok_or_else(|| AppError::NotFound(format!("Ideation session {} not found", session_id)))?;

    if session.status != IdeationSessionStatus::Accepted {
        return Err(AppError::Validation(
            "Can only append tasks to an accepted ideation plan".to_string(),
        ));
    }

    if let Some(project_id) = input.project_id.as_deref() {
        if project_id != session.project_id.as_str() {
            return Err(AppError::Validation(format!(
                "Session {} does not belong to project {}",
                session_id, project_id
            )));
        }
    }

    let execution_plan = app_state
        .execution_plan_repo
        .get_active_for_session(&session_id)
        .await
        .map_err(|e| AppError::Database(format!("Failed to load execution plan: {}", e)))?
        .ok_or_else(|| {
            AppError::Validation(format!(
                "Accepted session {} has no active execution plan",
                session_id
            ))
        })?;

    let branch = app_state
        .plan_branch_repo
        .get_by_execution_plan_id(&execution_plan.id)
        .await
        .map_err(|e| AppError::Database(format!("Failed to load plan branch: {}", e)))?
        .ok_or_else(|| {
            AppError::Validation(format!(
                "Execution plan {} has no plan branch",
                execution_plan.id
            ))
        })?;

    let merge_task_id = branch.merge_task_id.clone().ok_or_else(|| {
        AppError::Validation(format!("Plan branch {} has no merge task", branch.id))
    })?;

    let requested_blocker_ids = normalize_dependency_ids(&input.depends_on_task_ids);

    let mut task = Task::new(session.project_id.clone(), title.clone());
    task.description = input
        .description
        .as_ref()
        .map(|value| value.trim().to_string());
    task.priority = input.priority.unwrap_or(0);
    task.internal_status = InternalStatus::Ready;
    task.plan_artifact_id = session.plan_artifact_id.clone();
    task.ideation_session_id = Some(session_id.clone());
    task.execution_plan_id = Some(execution_plan.id.clone());
    task.blocked_reason = None;
    task.metadata = Some(
        serde_json::json!({
            "created_via": "ideation_plan_append",
            "source": {
                "tool": "append_task_to_ideation_plan",
                "conversation_id": input.source_conversation_id,
                "message_id": input.source_message_id,
            },
            "acceptance_criteria": input.acceptance_criteria,
        })
        .to_string(),
    );

    let steps = input
        .steps
        .into_iter()
        .map(|step| step.trim().to_string())
        .filter(|step| !step.is_empty())
        .collect::<Vec<_>>();

    let tx_task = task.clone();
    let tx_steps = steps.clone();
    let tx_requested_blocker_ids = requested_blocker_ids.clone();
    let tx_session_id = session_id.as_str().to_string();
    let tx_execution_plan_id = execution_plan.id.as_str().to_string();
    let tx_branch_id = branch.id.as_str().to_string();
    let tx_merge_task_id = merge_task_id.as_str().to_string();
    let tx_title = title.clone();
    let tx_tasks_source_identity = tasks_source_identity.clone();

    let (dependencies_created, was_waiting_on_pr, inserted_status) = app_state
        .db
        .run_transaction(move |conn| {
            crate::application::tasks_feature_policy::authorize_tasks_session_sync(
                conn,
                Some(&tx_session_id),
                crate::domain::ideation::TasksFeatureAction::Progress,
            )?;
            if let Some((source_conversation_id, source_message_id)) =
                tx_tasks_source_identity.as_ref()
            {
                conn.execute(
                    "INSERT INTO agent_task_pipeline_append_replays (
                        session_id, source_conversation_id, source_message_id, task_id
                     ) VALUES (?1, ?2, ?3, ?4)",
                    rusqlite::params![
                        tx_session_id.as_str(),
                        source_conversation_id,
                        source_message_id,
                        tx_task.id.as_str(),
                    ],
                )
                .map_err(|error| match error {
                    rusqlite::Error::SqliteFailure(ref failure, _)
                        if failure.extended_code
                            == rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY =>
                    {
                        AppError::Conflict(
                            "This Tasks follow-up message was already appended".to_string(),
                        )
                    }
                    _ => AppError::Database(format!(
                        "Failed to reserve Tasks follow-up replay key: {}",
                        error
                    )),
                })?;
            }
            let current_plan = conn
                .query_row(
                    "SELECT * FROM execution_plans WHERE id = ?1 AND session_id = ?2 AND status = 'active'",
                    rusqlite::params![tx_execution_plan_id.as_str(), tx_session_id.as_str()],
                    ExecutionPlan::from_row,
                )
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => AppError::Validation(format!(
                        "Accepted session {} no longer has an active execution plan",
                        tx_session_id
                    )),
                    _ => AppError::Database(format!("Failed to verify execution plan: {}", e)),
                })?;

            let current_branch = conn
                .query_row(
                    "SELECT * FROM plan_branches WHERE id = ?1 AND execution_plan_id = ?2",
                    rusqlite::params![tx_branch_id.as_str(), current_plan.id.as_str()],
                    PlanBranch::from_row,
                )
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => AppError::Validation(format!(
                        "Execution plan {} no longer has an active plan branch",
                        current_plan.id
                    )),
                    _ => AppError::Database(format!("Failed to verify plan branch: {}", e)),
                })?;

            if current_branch.status != PlanBranchStatus::Active {
                return Err(AppError::Validation(
                    "Cannot append a task to a merged or abandoned plan branch".to_string(),
                ));
            }

            if current_branch
                .merge_task_id
                .as_ref()
                .map(|id| id.as_str())
                != Some(tx_merge_task_id.as_str())
            {
                return Err(AppError::Validation(format!(
                    "Plan branch {} merge task changed while appending task",
                    current_branch.id
                )));
            }

            let mut merge_task = conn
                .query_row(
                    "SELECT * FROM tasks WHERE id = ?1",
                    rusqlite::params![tx_merge_task_id.as_str()],
                    Task::from_row,
                )
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => AppError::Validation(format!(
                        "Plan merge task {} no longer exists",
                        tx_merge_task_id
                    )),
                    _ => AppError::Database(format!("Failed to verify merge task: {}", e)),
                })?;

            if merge_task.category != TaskCategory::PlanMerge
                || merge_task.archived_at.is_some()
                || merge_task.ideation_session_id.as_ref().map(|id| id.as_str())
                    != Some(tx_session_id.as_str())
                || merge_task.execution_plan_id.as_ref().map(|id| id.as_str())
                    != Some(tx_execution_plan_id.as_str())
            {
                return Err(AppError::Validation(
                    "Plan merge task is not linked to the accepted ideation plan".to_string(),
                ));
            }

            if !matches!(
                merge_task.internal_status,
                InternalStatus::Blocked | InternalStatus::Ready | InternalStatus::WaitingOnPr
            ) {
                return Err(AppError::Validation(
                    "Cannot append a task to a closed or actively merging plan".to_string(),
                ));
            }
            let was_waiting_on_pr = merge_task.internal_status == InternalStatus::WaitingOnPr;

            let blocker_tasks = resolve_append_blocker_tasks(
                conn,
                &tx_requested_blocker_ids,
                tx_task.project_id.as_str(),
                tx_session_id.as_str(),
                tx_execution_plan_id.as_str(),
                tx_merge_task_id.as_str(),
            )?;
            let has_unsatisfied_blocker = blocker_tasks
                .iter()
                .any(|task| task.internal_status.is_active_dependency_blocker());
            let mut task_to_insert = tx_task.clone();
            task_to_insert.internal_status = if has_unsatisfied_blocker {
                InternalStatus::Blocked
            } else {
                InternalStatus::Ready
            };
            task_to_insert.blocked_reason = append_blocked_reason(&blocker_tasks);

            insert_task_row(conn, &task_to_insert)?;

            for (idx, title) in tx_steps.iter().enumerate() {
                let step = TaskStep::new(
                    task_to_insert.id.clone(),
                    title.clone(),
                    idx as i32,
                    "ideation_plan_append".to_string(),
                );
                insert_task_step_row(conn, &step)?;
            }

            let mut dependencies_created = 0usize;
            for blocker in &blocker_tasks {
                insert_task_dependency_row(conn, task_to_insert.id.as_str(), blocker.id.as_str())?;
                dependencies_created += 1;
            }
            insert_task_dependency_row(
                conn,
                tx_merge_task_id.as_str(),
                task_to_insert.id.as_str(),
            )?;
            dependencies_created += 1;

            merge_task.internal_status = InternalStatus::Blocked;
            merge_task.blocked_reason = Some(format!("Waiting for appended task: {}", tx_title));
            merge_task.updated_at = chrono::Utc::now();
            let rows = conn
                .execute(
                    "UPDATE tasks
                     SET internal_status = ?2, blocked_reason = ?3, updated_at = ?4
                     WHERE id = ?1 AND internal_status IN ('blocked', 'ready', 'waiting_on_pr')",
                    rusqlite::params![
                        merge_task.id.as_str(),
                        merge_task.internal_status.as_str(),
                        merge_task.blocked_reason,
                        merge_task.updated_at.to_rfc3339(),
                    ],
                )
                .map_err(|e| AppError::Database(format!("Failed to block merge task: {}", e)))?;
            if rows == 0 {
                return Err(AppError::Validation(
                    "Cannot append a task to a closed or actively merging plan".to_string(),
                ));
            }

            if was_waiting_on_pr {
                conn.execute(
                    "UPDATE plan_branches SET pr_polling_active = 0 WHERE id = ?1",
                    rusqlite::params![tx_branch_id.as_str()],
                )
                .map_err(|e| {
                    AppError::Database(format!("Failed to stop PR polling for plan branch: {}", e))
                })?;
            }

            Ok((
                dependencies_created,
                was_waiting_on_pr,
                task_to_insert.internal_status,
            ))
        })
        .await?;

    if was_waiting_on_pr && app_state.pr_poller_registry.is_polling(&merge_task_id) {
        app_state.pr_poller_registry.stop_polling(&merge_task_id);
    }

    Ok(AppendIdeationPlanTaskResult {
        project_id: session.project_id.as_str().to_string(),
        session_id: session_id.as_str().to_string(),
        task_id: task.id.as_str().to_string(),
        execution_plan_id: execution_plan.id.as_str().to_string(),
        plan_branch_id: branch.id.as_str().to_string(),
        merge_task_id: merge_task_id.as_str().to_string(),
        task_status: inserted_status.as_str().to_string(),
        dependencies_created,
        any_ready_tasks: inserted_status == InternalStatus::Ready,
    })
}

fn normalize_dependency_ids(ids: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for id in ids {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            continue;
        }
        let trimmed = trimmed.to_string();
        if seen.insert(trimmed.clone()) {
            normalized.push(trimmed);
        }
    }
    normalized
}

fn resolve_append_blocker_tasks(
    conn: &rusqlite::Connection,
    requested_blocker_ids: &[String],
    project_id: &str,
    session_id: &str,
    execution_plan_id: &str,
    merge_task_id: &str,
) -> AppResult<Vec<Task>> {
    if requested_blocker_ids.is_empty() {
        infer_plan_leaf_blockers(conn, project_id, session_id, execution_plan_id)
    } else {
        validate_requested_blockers(
            conn,
            requested_blocker_ids,
            project_id,
            session_id,
            execution_plan_id,
            merge_task_id,
        )
    }
}

fn validate_requested_blockers(
    conn: &rusqlite::Connection,
    requested_blocker_ids: &[String],
    project_id: &str,
    session_id: &str,
    execution_plan_id: &str,
    merge_task_id: &str,
) -> AppResult<Vec<Task>> {
    let mut blockers = Vec::new();
    for blocker_id in requested_blocker_ids {
        let blocker = query_task_by_id(conn, blocker_id)?
            .ok_or_else(|| AppError::NotFound(format!("Task {} not found", blocker_id)))?;
        validate_append_blocker(
            &blocker,
            project_id,
            session_id,
            execution_plan_id,
            merge_task_id,
        )?;
        blockers.push(blocker);
    }
    Ok(blockers)
}

fn infer_plan_leaf_blockers(
    conn: &rusqlite::Connection,
    project_id: &str,
    session_id: &str,
    execution_plan_id: &str,
) -> AppResult<Vec<Task>> {
    let plan_merge_category = TaskCategory::PlanMerge.to_string();
    let mut stmt = conn
        .prepare(
            "SELECT t.*
             FROM tasks t
             WHERE t.project_id = ?1
               AND t.ideation_session_id = ?2
               AND t.execution_plan_id = ?3
               AND t.category != ?4
               AND t.archived_at IS NULL
               AND NOT EXISTS (
                   SELECT 1
                   FROM task_dependencies td
                   JOIN tasks dependent ON dependent.id = td.task_id
                   WHERE td.depends_on_task_id = t.id
                     AND dependent.ideation_session_id = ?2
                     AND dependent.execution_plan_id = ?3
                     AND dependent.category != ?4
                     AND dependent.archived_at IS NULL
               )
             ORDER BY t.created_at ASC, t.id ASC",
        )
        .map_err(|e| AppError::Database(format!("Failed to prepare blocker inference: {}", e)))?;

    let rows = stmt
        .query_map(
            rusqlite::params![
                project_id,
                session_id,
                execution_plan_id,
                plan_merge_category
            ],
            Task::from_row,
        )
        .map_err(|e| AppError::Database(format!("Failed to infer plan leaf blockers: {}", e)))?;

    let mut blockers = Vec::new();
    for row in rows {
        blockers.push(
            row.map_err(|e| AppError::Database(format!("Failed to read inferred blocker: {}", e)))?,
        );
    }
    Ok(blockers)
}

fn query_task_by_id(conn: &rusqlite::Connection, task_id: &str) -> AppResult<Option<Task>> {
    match conn.query_row(
        "SELECT * FROM tasks WHERE id = ?1",
        rusqlite::params![task_id],
        Task::from_row,
    ) {
        Ok(task) => Ok(Some(task)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::Database(format!(
            "Failed to load blocker task: {}",
            e
        ))),
    }
}

fn validate_append_blocker(
    blocker: &Task,
    project_id: &str,
    session_id: &str,
    execution_plan_id: &str,
    merge_task_id: &str,
) -> AppResult<()> {
    if blocker.id.as_str() == merge_task_id || blocker.category == TaskCategory::PlanMerge {
        return Err(AppError::Validation(
            "Cannot use the plan merge task as an appended task blocker".to_string(),
        ));
    }
    if blocker.archived_at.is_some() {
        return Err(AppError::Validation(format!(
            "Blocker task {} is archived",
            blocker.id
        )));
    }
    if blocker.project_id.as_str() != project_id {
        return Err(AppError::Validation(format!(
            "Blocker task {} belongs to a different project",
            blocker.id
        )));
    }
    if blocker.ideation_session_id.as_ref().map(|id| id.as_str()) != Some(session_id)
        || blocker.execution_plan_id.as_ref().map(|id| id.as_str()) != Some(execution_plan_id)
    {
        return Err(AppError::Validation(format!(
            "Blocker task {} is not part of the accepted ideation plan",
            blocker.id
        )));
    }
    Ok(())
}

fn append_blocked_reason(blocker_tasks: &[Task]) -> Option<String> {
    let blocker_titles = blocker_tasks
        .iter()
        .filter(|task| task.internal_status.is_active_dependency_blocker())
        .map(|task| task.title.as_str())
        .collect::<Vec<_>>();
    if blocker_titles.is_empty() {
        None
    } else {
        Some(format!("Waiting for: {}", blocker_titles.join(", ")))
    }
}

fn insert_task_row(conn: &rusqlite::Connection, task: &Task) -> AppResult<()> {
    conn.execute(
        "INSERT INTO tasks (id, project_id, category, title, description, priority, internal_status, needs_review_point, source_proposal_id, plan_artifact_id, ideation_session_id, execution_plan_id, created_at, updated_at, started_at, completed_at, archived_at, blocked_reason, task_branch, worktree_path, merge_commit_sha, metadata, merge_pipeline_active)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
        rusqlite::params![
            task.id.as_str(),
            task.project_id.as_str(),
            task.category.to_string(),
            task.title.clone(),
            task.description.clone(),
            task.priority,
            task.internal_status.as_str(),
            task.needs_review_point,
            task.source_proposal_id.as_ref().map(|id| id.as_str()),
            task.plan_artifact_id.as_ref().map(|id| id.as_str()),
            task.ideation_session_id.as_ref().map(|id| id.as_str()),
            task.execution_plan_id.as_ref().map(|id| id.as_str()),
            task.created_at.to_rfc3339(),
            task.updated_at.to_rfc3339(),
            task.started_at.map(|dt| dt.to_rfc3339()),
            task.completed_at.map(|dt| dt.to_rfc3339()),
            task.archived_at.map(|dt| dt.to_rfc3339()),
            task.blocked_reason.clone(),
            task.task_branch.clone(),
            task.worktree_path.clone(),
            task.merge_commit_sha.clone(),
            task.metadata.clone(),
            task.merge_pipeline_active.clone(),
        ],
    )
    .map_err(|e| AppError::Database(format!("Failed to create appended task: {}", e)))?;
    Ok(())
}

fn insert_task_step_row(conn: &rusqlite::Connection, step: &TaskStep) -> AppResult<()> {
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
    )
    .map_err(|e| AppError::Database(format!("Failed to create appended task step: {}", e)))?;
    Ok(())
}

fn insert_task_dependency_row(
    conn: &rusqlite::Connection,
    task_id: &str,
    depends_on_task_id: &str,
) -> AppResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO task_dependencies (id, task_id, depends_on_task_id)
         VALUES (?1, ?2, ?3)",
        rusqlite::params![
            uuid::Uuid::new_v4().to_string(),
            task_id,
            depends_on_task_id
        ],
    )
    .map_err(|e| AppError::Database(format!("Failed to create task dependency: {}", e)))?;
    Ok(())
}

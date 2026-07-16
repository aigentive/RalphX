use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::application::AppState;
use crate::domain::entities::{
    ExecutionPlan, IdeationSessionId, IdeationSessionStatus, InternalStatus, PlanBranch,
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

    let (dependencies_created, was_waiting_on_pr, inserted_status) = app_state
        .db
        .run_transaction(move |conn| {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::{
        ArtifactId, ExecutionPlan, ExecutionPlanId, IdeationSession, IdeationSessionStatus,
        InternalStatus, PlanBranch, PlanBranchId, PlanBranchStatus, Project, ProjectId, Task,
        TaskCategory, TaskId,
    };

    struct AcceptedPlanFixture {
        state: AppState,
        project_id: ProjectId,
        session_id: IdeationSessionId,
        execution_plan_id: String,
        plan_branch_id: String,
        merge_task_id: TaskId,
    }

    async fn accepted_plan_fixture(merge_status: InternalStatus) -> AcceptedPlanFixture {
        let state = AppState::new_sqlite_for_apply_test();
        let mut project = Project::new("RalphX".to_string(), "/tmp/ralphx-append-test".to_string());
        project.base_branch = Some("main".to_string());
        let project = state.project_repo.create(project).await.unwrap();

        let mut session = IdeationSession::new_with_title(
            project.id.clone(),
            "Accepted append target".to_string(),
        );
        session.status = IdeationSessionStatus::Accepted;
        session.plan_artifact_id = Some(ArtifactId::from_string("plan-artifact-append-test"));
        let session = state.ideation_session_repo.create(session).await.unwrap();

        let execution_plan = state
            .execution_plan_repo
            .create(ExecutionPlan::new(session.id.clone()))
            .await
            .unwrap();

        let mut branch = PlanBranch::new(
            session.plan_artifact_id.clone().unwrap(),
            session.id.clone(),
            project.id.clone(),
            "ralphx/ralphx/plan-append-test".to_string(),
            "main".to_string(),
        );
        branch.status = PlanBranchStatus::Active;
        branch.execution_plan_id = Some(execution_plan.id.clone());
        let branch = state.plan_branch_repo.create(branch).await.unwrap();

        let mut merge_task = Task::new_with_category(
            project.id.clone(),
            "Merge plan into main".to_string(),
            TaskCategory::PlanMerge,
        );
        merge_task.internal_status = merge_status;
        merge_task.plan_artifact_id = session.plan_artifact_id.clone();
        merge_task.ideation_session_id = Some(session.id.clone());
        merge_task.execution_plan_id = Some(execution_plan.id.clone());
        merge_task.blocked_reason = match merge_status {
            InternalStatus::Blocked => Some("Waiting for all plan tasks to complete".to_string()),
            _ => None,
        };
        let merge_task = state.task_repo.create(merge_task).await.unwrap();
        state
            .plan_branch_repo
            .set_merge_task_id(&branch.id, &merge_task.id)
            .await
            .unwrap();

        AcceptedPlanFixture {
            state,
            project_id: project.id,
            session_id: session.id,
            execution_plan_id: execution_plan.id.as_str().to_string(),
            plan_branch_id: branch.id.as_str().to_string(),
            merge_task_id: merge_task.id,
        }
    }

    async fn create_plan_task(
        fixture: &AcceptedPlanFixture,
        title: &str,
        status: InternalStatus,
    ) -> Task {
        let mut task = Task::new(fixture.project_id.clone(), title.to_string());
        task.internal_status = status;
        task.ideation_session_id = Some(fixture.session_id.clone());
        task.execution_plan_id = Some(ExecutionPlanId::from_string(
            fixture.execution_plan_id.clone(),
        ));
        fixture.state.task_repo.create(task).await.unwrap()
    }

    async fn append_follow_up(
        fixture: &AcceptedPlanFixture,
        title: &str,
        depends_on_task_ids: Vec<String>,
    ) -> AppendIdeationPlanTaskResult {
        append_ideation_plan_task_core(
            &fixture.state,
            AppendIdeationPlanTaskInput {
                project_id: Some(fixture.project_id.as_str().to_string()),
                session_id: fixture.session_id.as_str().to_string(),
                title: title.to_string(),
                description: Some("Append follow-up work.".to_string()),
                steps: vec!["Implement the follow-up".to_string()],
                acceptance_criteria: vec!["Follow-up is complete".to_string()],
                depends_on_task_ids,
                priority: None,
                source_conversation_id: None,
                source_message_id: None,
            },
        )
        .await
        .unwrap()
    }

    async fn appended_task(fixture: &AcceptedPlanFixture, task_id: &str) -> Task {
        fixture
            .state
            .task_repo
            .get_by_id(&TaskId::from_string(task_id.to_string()))
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn append_infers_leaf_blockers_before_merge_when_no_explicit_dependencies() {
        let fixture = accepted_plan_fixture(InternalStatus::Ready).await;
        let foundation =
            create_plan_task(&fixture, "Foundation plan task", InternalStatus::Merged).await;
        let leaf = create_plan_task(&fixture, "Leaf plan task", InternalStatus::Merged).await;
        fixture
            .state
            .task_dependency_repo
            .add_dependency(&leaf.id, &foundation.id)
            .await
            .unwrap();
        fixture
            .state
            .task_dependency_repo
            .add_dependency(&fixture.merge_task_id, &foundation.id)
            .await
            .unwrap();
        fixture
            .state
            .task_dependency_repo
            .add_dependency(&fixture.merge_task_id, &leaf.id)
            .await
            .unwrap();

        let result = append_follow_up(&fixture, "Append after leaf", vec![]).await;

        assert_eq!(result.task_status, "ready");
        assert_eq!(result.dependencies_created, 2);
        let appended = appended_task(&fixture, &result.task_id).await;
        let appended_blockers = fixture
            .state
            .task_dependency_repo
            .get_blockers(&appended.id)
            .await
            .unwrap();
        assert_eq!(appended_blockers.len(), 1);
        assert!(appended_blockers.contains(&leaf.id));
        assert!(!appended_blockers.contains(&foundation.id));

        let merge_blockers = fixture
            .state
            .task_dependency_repo
            .get_blockers(&fixture.merge_task_id)
            .await
            .unwrap();
        assert!(merge_blockers.contains(&appended.id));
    }

    #[tokio::test]
    async fn append_inferred_active_leaf_blocks_appended_task() {
        let fixture = accepted_plan_fixture(InternalStatus::Ready).await;
        let active_leaf =
            create_plan_task(&fixture, "Active leaf task", InternalStatus::Executing).await;

        let result = append_follow_up(&fixture, "Append behind active leaf", vec![]).await;

        assert_eq!(result.task_status, "blocked");
        assert!(!result.any_ready_tasks);
        let appended = appended_task(&fixture, &result.task_id).await;
        assert_eq!(appended.internal_status, InternalStatus::Blocked);
        assert!(appended
            .blocked_reason
            .as_deref()
            .unwrap()
            .contains("Active leaf task"));
        let appended_blockers = fixture
            .state
            .task_dependency_repo
            .get_blockers(&appended.id)
            .await
            .unwrap();
        assert_eq!(appended_blockers, vec![active_leaf.id]);
    }

    #[tokio::test]
    async fn append_explicit_dependencies_are_validated_and_respected() {
        let fixture = accepted_plan_fixture(InternalStatus::Ready).await;
        let inferred_leaf =
            create_plan_task(&fixture, "Inferred leaf", InternalStatus::Merged).await;
        let explicit_blocker =
            create_plan_task(&fixture, "Explicit blocker", InternalStatus::Merged).await;

        let result = append_follow_up(
            &fixture,
            "Append with explicit blocker",
            vec![explicit_blocker.id.as_str().to_string()],
        )
        .await;

        assert_eq!(result.task_status, "ready");
        assert_eq!(result.dependencies_created, 2);
        let appended = appended_task(&fixture, &result.task_id).await;
        let appended_blockers = fixture
            .state
            .task_dependency_repo
            .get_blockers(&appended.id)
            .await
            .unwrap();
        assert_eq!(appended_blockers.len(), 1);
        assert!(appended_blockers.contains(&explicit_blocker.id));
        assert!(!appended_blockers.contains(&inferred_leaf.id));

        let merge_blockers = fixture
            .state
            .task_dependency_repo
            .get_blockers(&fixture.merge_task_id)
            .await
            .unwrap();
        assert!(merge_blockers.contains(&appended.id));
    }

    #[tokio::test]
    async fn append_rejects_invalid_explicit_dependency_without_partial_insert() {
        let fixture = accepted_plan_fixture(InternalStatus::Ready).await;

        let error = append_ideation_plan_task_core(
            &fixture.state,
            AppendIdeationPlanTaskInput {
                project_id: Some(fixture.project_id.as_str().to_string()),
                session_id: fixture.session_id.as_str().to_string(),
                title: "Invalid dependency append".to_string(),
                description: None,
                steps: vec![],
                acceptance_criteria: vec![],
                depends_on_task_ids: vec![fixture.merge_task_id.as_str().to_string()],
                priority: None,
                source_conversation_id: None,
                source_message_id: None,
            },
        )
        .await
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("Cannot use the plan merge task as an appended task blocker"));
        let tasks = fixture
            .state
            .task_repo
            .get_by_project(&fixture.project_id)
            .await
            .unwrap();
        assert!(!tasks
            .iter()
            .any(|task| task.title == "Invalid dependency append"));
    }

    #[tokio::test]
    async fn append_creates_linked_task_steps_and_merge_dependency() {
        let fixture = accepted_plan_fixture(InternalStatus::Ready).await;

        let result = append_ideation_plan_task_core(
            &fixture.state,
            AppendIdeationPlanTaskInput {
                project_id: Some(fixture.project_id.as_str().to_string()),
                session_id: fixture.session_id.as_str().to_string(),
                title: "Polish publish CTA".to_string(),
                description: Some("Tune the managed publish action copy.".to_string()),
                steps: vec![
                    "Find the publish panel action".to_string(),
                    "Adjust the CTA treatment".to_string(),
                ],
                acceptance_criteria: vec!["CTA copy is clear".to_string()],
                depends_on_task_ids: vec![],
                priority: Some(8),
                source_conversation_id: Some("conversation-1".to_string()),
                source_message_id: Some("message-1".to_string()),
            },
        )
        .await
        .unwrap();

        assert_eq!(result.project_id, fixture.project_id.as_str());
        assert_eq!(result.session_id, fixture.session_id.as_str());
        assert_eq!(result.execution_plan_id, fixture.execution_plan_id);
        assert_eq!(result.plan_branch_id, fixture.plan_branch_id);
        assert_eq!(result.merge_task_id, fixture.merge_task_id.as_str());
        assert_eq!(result.task_status, "ready");
        assert_eq!(result.dependencies_created, 1);
        assert!(result.any_ready_tasks);

        let appended_task = fixture
            .state
            .task_repo
            .get_by_id(&TaskId::from_string(result.task_id.clone()))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(appended_task.category, TaskCategory::Regular);
        assert_eq!(appended_task.internal_status, InternalStatus::Ready);
        assert_eq!(
            appended_task.ideation_session_id,
            Some(fixture.session_id.clone())
        );
        assert_eq!(
            appended_task.execution_plan_id.as_ref().unwrap().as_str(),
            fixture.execution_plan_id
        );
        assert!(appended_task.source_proposal_id.is_none());
        assert!(appended_task
            .metadata
            .as_deref()
            .unwrap()
            .contains("ideation_plan_append"));
        assert!(appended_task
            .metadata
            .as_deref()
            .unwrap()
            .contains("CTA copy is clear"));

        let steps = fixture
            .state
            .task_step_repo
            .get_by_task(&appended_task.id)
            .await
            .unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].title, "Find the publish panel action");
        assert_eq!(steps[0].created_by, "ideation_plan_append");

        let merge_blockers = fixture
            .state
            .task_dependency_repo
            .get_blockers(&fixture.merge_task_id)
            .await
            .unwrap();
        assert!(merge_blockers.contains(&appended_task.id));

        let merge_task = fixture
            .state
            .task_repo
            .get_by_id(&fixture.merge_task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(merge_task.internal_status, InternalStatus::Blocked);
        assert!(merge_task
            .blocked_reason
            .as_deref()
            .unwrap()
            .contains("Polish publish CTA"));
    }

    #[tokio::test]
    async fn append_allows_waiting_on_pr_plan_and_blocks_merge_again() {
        let fixture = accepted_plan_fixture(InternalStatus::WaitingOnPr).await;
        let plan_branch_id = PlanBranchId::from_string(fixture.plan_branch_id.clone());
        fixture
            .state
            .plan_branch_repo
            .update_last_polled_at(&plan_branch_id, chrono::Utc::now())
            .await
            .unwrap();

        let result = append_ideation_plan_task_core(
            &fixture.state,
            AppendIdeationPlanTaskInput {
                project_id: Some(fixture.project_id.as_str().to_string()),
                session_id: fixture.session_id.as_str().to_string(),
                title: "Apply requested PR adjustment".to_string(),
                description: Some(
                    "Handle a follow-up while the plan PR is still open.".to_string(),
                ),
                steps: vec!["Make the requested adjustment".to_string()],
                acceptance_criteria: vec!["The existing PR includes the follow-up".to_string()],
                depends_on_task_ids: vec![],
                priority: None,
                source_conversation_id: None,
                source_message_id: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(result.task_status, "ready");
        assert!(result.any_ready_tasks);

        let appended_task = fixture
            .state
            .task_repo
            .get_by_id(&TaskId::from_string(result.task_id))
            .await
            .unwrap()
            .unwrap();
        let merge_blockers = fixture
            .state
            .task_dependency_repo
            .get_blockers(&fixture.merge_task_id)
            .await
            .unwrap();
        assert!(merge_blockers.contains(&appended_task.id));

        let merge_task = fixture
            .state
            .task_repo
            .get_by_id(&fixture.merge_task_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(merge_task.internal_status, InternalStatus::Blocked);
        assert!(merge_task
            .blocked_reason
            .as_deref()
            .unwrap()
            .contains("Apply requested PR adjustment"));

        let branch = fixture
            .state
            .plan_branch_repo
            .get_by_id(&plan_branch_id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            !branch.pr_polling_active,
            "appending to a waiting-on-PR plan must stop the stale PR poller"
        );
    }

    #[tokio::test]
    async fn append_rejects_after_merge_has_started() {
        let fixture = accepted_plan_fixture(InternalStatus::PendingMerge).await;

        let error = append_ideation_plan_task_core(
            &fixture.state,
            AppendIdeationPlanTaskInput {
                project_id: Some(fixture.project_id.as_str().to_string()),
                session_id: fixture.session_id.as_str().to_string(),
                title: "Too late".to_string(),
                description: None,
                steps: vec![],
                acceptance_criteria: vec![],
                depends_on_task_ids: vec![],
                priority: None,
                source_conversation_id: None,
                source_message_id: None,
            },
        )
        .await
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("Cannot append a task to a closed or actively merging plan"));
    }
}

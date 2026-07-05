use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use tauri::{Emitter, Manager, State};

use crate::application::task_cleanup_service::{StopMode, TaskCleanupService};
use crate::application::{spawn_ready_task_scheduler_if_needed, AppState};
use crate::commands::branch_helpers::ensure_base_branch_exists;
use crate::commands::task_commands::emit_queue_changed;
use crate::commands::ExecutionState;
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ArtifactId, ExecutionPlanId, ExecutionPlanStatus,
    IdeationSessionId, IdeationSessionStatus, ProjectId, SessionOrigin, Task, TaskProposal,
};
use crate::domain::services::validate_project_path;
use crate::error::{AppError, AppResult};

use super::ideation_commands_apply::{
    load_linked_agent_conversation_workspace, phase_insert_dependencies,
    phase_insert_execution_plan, phase_insert_merge_task, phase_insert_tasks_and_steps,
    phase_update_proposals, phase_upsert_plan_branch,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestartImplementationResult {
    pub session_id: String,
    pub project_id: String,
    pub old_execution_plan_id: String,
    pub new_execution_plan_id: String,
    pub archived_task_count: usize,
    pub stopped_agent_count: usize,
    pub tasks_created: usize,
    pub dependencies_created: usize,
    pub created_task_ids: Vec<String>,
    pub any_ready_tasks: bool,
    pub warnings: Vec<String>,
}

struct RestartTxOutput {
    execution_plan_id: ExecutionPlanId,
    plan_branch_id: crate::domain::entities::PlanBranchId,
    created_tasks: Vec<Task>,
    archived_task_count: usize,
    dependencies_created: usize,
    warnings: Vec<String>,
    any_ready_tasks: bool,
}

fn is_local_restart_proposal(proposal: &TaskProposal, project_dir: &std::path::Path) -> bool {
    let Some(target_project) = proposal.target_project.as_deref() else {
        return true;
    };

    match validate_project_path(target_project) {
        Ok(target_dir) => target_dir == project_dir,
        Err(_) => false,
    }
}

async fn load_active_plan_tasks(
    app_state: &AppState,
    project_id: &ProjectId,
    execution_plan_id: &ExecutionPlanId,
) -> AppResult<Vec<Task>> {
    let count = app_state
        .task_repo
        .count_tasks(project_id, false, None, Some(execution_plan_id.as_str()))
        .await
        .map_err(|error| {
            AppError::Database(format!(
                "Failed to count active execution plan tasks: {}",
                error
            ))
        })?;

    if count == 0 {
        return Ok(Vec::new());
    }

    app_state
        .task_repo
        .list_paginated(
            project_id,
            None,
            0,
            count,
            false,
            None,
            Some(execution_plan_id.as_str()),
            None,
        )
        .await
        .map_err(|error| {
            AppError::Database(format!(
                "Failed to load active execution plan tasks: {}",
                error
            ))
        })
}

async fn load_proposal_dependencies(
    app_state: &AppState,
    proposals: &[TaskProposal],
) -> AppResult<HashMap<String, Vec<String>>> {
    let mut proposal_deps: HashMap<String, Vec<String>> = HashMap::new();
    for proposal in proposals {
        let deps = app_state
            .proposal_dependency_repo
            .get_dependencies(&proposal.id)
            .await
            .map_err(|error| AppError::Database(error.to_string()))?;
        proposal_deps.insert(
            proposal.id.as_str().to_string(),
            deps.into_iter()
                .map(|dep| dep.as_str().to_string())
                .collect(),
        );
    }
    Ok(proposal_deps)
}

fn clear_restart_proposal_links(
    conn: &rusqlite::Connection,
    session_id: &IdeationSessionId,
    proposals: &[TaskProposal],
    now_str: &str,
) -> AppResult<()> {
    for proposal in proposals {
        conn.execute(
            "UPDATE task_proposals
             SET created_task_id = NULL, updated_at = ?3
             WHERE id = ?1 AND session_id = ?2",
            rusqlite::params![proposal.id.as_str(), session_id.as_str(), now_str],
        )
        .map_err(|error| {
            AppError::Database(format!(
                "Failed to clear proposal task link for restart: {}",
                error
            ))
        })?;
    }
    Ok(())
}

fn clear_restart_plan_branch_runtime_fields(
    conn: &rusqlite::Connection,
    branch_id: &crate::domain::entities::PlanBranchId,
) -> AppResult<()> {
    conn.execute(
        "UPDATE plan_branches
         SET pr_number = NULL,
             pr_url = NULL,
             pr_status = NULL,
             pr_draft = NULL,
             pr_push_status = 'pending',
             pr_polling_active = 0,
             last_polled_at = NULL,
             merge_commit_sha = NULL
         WHERE id = ?1",
        rusqlite::params![branch_id.as_str()],
    )
    .map_err(|error| {
        AppError::Database(format!(
            "Failed to reset plan branch runtime fields for restart: {}",
            error
        ))
    })?;
    Ok(())
}

fn update_active_plan_pointer(
    conn: &rusqlite::Connection,
    project_id: &ProjectId,
    session_id: &IdeationSessionId,
    execution_plan_id: &ExecutionPlanId,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO project_active_plan
             (project_id, ideation_session_id, execution_plan_id, updated_at)
         VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
         ON CONFLICT(project_id) DO UPDATE SET
             ideation_session_id = excluded.ideation_session_id,
             execution_plan_id = excluded.execution_plan_id,
             updated_at = excluded.updated_at",
        rusqlite::params![
            project_id.as_str(),
            session_id.as_str(),
            execution_plan_id.as_str(),
        ],
    )
    .map_err(|error| {
        AppError::Database(format!(
            "Failed to update active execution plan pointer: {}",
            error
        ))
    })?;
    Ok(())
}

fn archive_restart_active_plan_tasks(
    conn: &rusqlite::Connection,
    project_id: &ProjectId,
    execution_plan_id: &ExecutionPlanId,
    now_str: &str,
) -> AppResult<usize> {
    conn.execute(
        "UPDATE tasks
         SET archived_at = ?3,
             updated_at = ?3
         WHERE project_id = ?1
           AND execution_plan_id = ?2
           AND archived_at IS NULL",
        rusqlite::params![project_id.as_str(), execution_plan_id.as_str(), now_str],
    )
    .map_err(|error| {
        AppError::Database(format!(
            "Failed to archive active execution plan tasks for restart: {}",
            error
        ))
    })
}

#[doc(hidden)]
pub async fn restart_implementation_core(
    app_state: &AppState,
    session_id: &IdeationSessionId,
) -> AppResult<RestartImplementationResult> {
    let session = app_state
        .ideation_session_repo
        .get_by_id(session_id)
        .await
        .map_err(|error| AppError::Database(error.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("Session {} not found", session_id)))?;

    if session.status != IdeationSessionStatus::Accepted {
        return Err(AppError::Validation(
            "Restart implementation requires an Accepted ideation session".to_string(),
        ));
    }

    let project = app_state
        .project_repo
        .get_by_id(&session.project_id)
        .await
        .map_err(|error| AppError::Database(error.to_string()))?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Project not found: {}",
                session.project_id.as_str()
            ))
        })?;
    let project_dir = validate_project_path(&project.working_directory)?;

    let all_proposals = app_state
        .task_proposal_repo
        .get_by_session(session_id)
        .await
        .map_err(|error| AppError::Database(error.to_string()))?;
    let proposals_to_recreate: Vec<TaskProposal> = all_proposals
        .into_iter()
        .filter(|proposal| is_local_restart_proposal(proposal, &project_dir))
        .collect();

    if proposals_to_recreate.is_empty() {
        return Err(AppError::Validation(
            "No local proposals available to restart implementation".to_string(),
        ));
    }

    let active_plan = app_state
        .execution_plan_repo
        .get_active_for_session(session_id)
        .await
        .map_err(|error| {
            AppError::Database(format!("Failed to load active execution plan: {}", error))
        })?
        .ok_or_else(|| {
            AppError::Validation(
                "No active execution plan exists for this accepted session".to_string(),
            )
        })?;

    let active_session = app_state
        .active_plan_repo
        .get(&session.project_id)
        .await
        .map_err(|error| {
            AppError::Database(format!("Failed to load active project plan: {}", error))
        })?;
    if active_session.as_ref() != Some(session_id) {
        return Err(AppError::Validation(
            "Session is not the active implementation plan for its project".to_string(),
        ));
    }

    let active_execution_plan = app_state
        .active_plan_repo
        .get_execution_plan_id(&session.project_id)
        .await
        .map_err(|error| {
            AppError::Database(format!(
                "Failed to load active project execution plan: {}",
                error
            ))
        })?;
    if active_execution_plan
        .as_ref()
        .is_some_and(|execution_plan_id| execution_plan_id != &active_plan.id)
    {
        return Err(AppError::Validation(
            "Active project execution plan does not match the session execution plan".to_string(),
        ));
    }

    let old_tasks = load_active_plan_tasks(app_state, &session.project_id, &active_plan.id).await?;

    let linked_agent_workspace =
        load_linked_agent_conversation_workspace(app_state, session_id, &session.project_id)
            .await?;
    if let Some(workspace) = linked_agent_workspace.as_ref() {
        if workspace.mode != AgentConversationWorkspaceMode::Ideation {
            return Err(AppError::Validation(
                "Linked agent conversation workspace is not in ideation mode".to_string(),
            ));
        }
        crate::application::agent_conversation_workspace::resolve_valid_agent_conversation_workspace_path(
            &project,
            workspace,
        )
        .await?;
    }

    let session_base_ref = session
        .analysis
        .base_ref
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .cloned();
    let effective_base_branch_override = if session.origin == SessionOrigin::Internal {
        session_base_ref.clone()
    } else {
        session_base_ref
    };

    if let Some(base_branch) = effective_base_branch_override.as_deref() {
        let was_created =
            ensure_base_branch_exists(&project_dir, base_branch, project.base_branch.as_deref())
                .await
                .map_err(AppError::Validation)?;
        if was_created {
            tracing::info!(
                "restart_implementation_core: auto-created base branch '{}' from project default",
                base_branch
            );
        }
    }

    let proposal_deps_tx = load_proposal_dependencies(app_state, &proposals_to_recreate).await?;

    let cleanup_service = TaskCleanupService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.running_agent_registry),
        None,
    )
    .with_interactive_process_registry(Arc::clone(&app_state.interactive_process_registry));

    let old_execution_plan_id = active_plan.id.clone();
    let old_execution_plan_id_tx = active_plan.id.clone();
    let session_id_tx = session_id.clone();
    let project_id_tx = session.project_id.clone();
    let session_id_str = session_id.as_str().to_string();
    let project_id_str = session.project_id.as_str().to_string();
    let plan_artifact_id_tx: Option<ArtifactId> = session.plan_artifact_id.clone();
    let base_branch_override_tx = effective_base_branch_override.clone();
    let agent_workspace_branch_name_tx = linked_agent_workspace
        .as_ref()
        .map(|workspace| workspace.branch_name.clone());
    let project_base_branch_tx = project.base_branch.clone();
    let project_name_tx = project.name.clone();
    let project_pr_eligible_tx = project.github_pr_enabled;
    let proposals_tx = proposals_to_recreate.clone();

    let tx_output = app_state
        .db
        .run_transaction(move |conn| {
            let updated = conn
                .execute(
                    "UPDATE execution_plans SET status = ?1 WHERE id = ?2 AND status = 'active'",
                    rusqlite::params![
                        ExecutionPlanStatus::Superseded.to_db_string(),
                        old_execution_plan_id.as_str(),
                    ],
                )
                .map_err(|error| {
                    AppError::Database(format!("Failed to supersede old execution plan: {}", error))
                })?;
            if updated == 0 {
                return Err(AppError::Validation(
                    "Active execution plan changed before restart could be committed".to_string(),
                ));
            }

            let now_str = chrono::Utc::now().to_rfc3339();
            let archived_task_count = archive_restart_active_plan_tasks(
                conn,
                &project_id_tx,
                &old_execution_plan_id_tx,
                &now_str,
            )?;
            clear_restart_proposal_links(conn, &session_id_tx, &proposals_tx, &now_str)?;

            let exec_plan = phase_insert_execution_plan(conn, &session_id_str)?;
            let execution_plan_id = exec_plan.id.clone();

            let (branch_id, base_branch_name) = phase_upsert_plan_branch(
                conn,
                &plan_artifact_id_tx,
                &session_id_str,
                &project_id_str,
                &base_branch_override_tx,
                &project_base_branch_tx,
                &project_name_tx,
                project_pr_eligible_tx,
                &execution_plan_id,
                &agent_workspace_branch_name_tx,
            )?;
            clear_restart_plan_branch_runtime_fields(conn, &branch_id)?;

            let (created_tasks, proposal_to_task, any_ready_tasks) = phase_insert_tasks_and_steps(
                conn,
                &proposals_tx,
                &project_id_str,
                &session_id_str,
                &plan_artifact_id_tx,
                true,
                &proposal_deps_tx,
                &execution_plan_id,
            )?;

            let (dependencies_created, warnings) = phase_insert_dependencies(
                conn,
                &proposals_tx,
                &proposal_deps_tx,
                &proposal_to_task,
            )?;
            phase_update_proposals(conn, &proposals_tx, &proposal_to_task, &now_str)?;
            phase_insert_merge_task(
                conn,
                &branch_id,
                &base_branch_name,
                &project_id_str,
                &plan_artifact_id_tx,
                &session_id_str,
                &execution_plan_id,
                &created_tasks,
            )?;
            update_active_plan_pointer(conn, &project_id_tx, &session_id_tx, &execution_plan_id)?;

            Ok(RestartTxOutput {
                execution_plan_id,
                plan_branch_id: branch_id,
                created_tasks,
                archived_task_count,
                dependencies_created,
                warnings,
                any_ready_tasks,
            })
        })
        .await?;

    let runtime_cleanup_report = cleanup_service
        .cleanup_task_runtime_resources(&old_tasks, StopMode::DirectStop)
        .await;
    let mut warnings = tx_output.warnings;

    if let Some(workspace) = linked_agent_workspace.as_ref() {
        if let Err(error) = app_state
            .agent_conversation_workspace_repo
            .update_links(
                &workspace.conversation_id,
                Some(session_id),
                Some(&tx_output.plan_branch_id),
            )
            .await
        {
            let warning = format!(
                "Failed to link agent conversation workspace to restarted plan branch: {}",
                error
            );
            tracing::warn!(
                conversation_id = %workspace.conversation_id,
                session_id = %session_id,
                error = %error,
                "restart_implementation_core: committed restart before workspace relink failure"
            );
            warnings.push(warning);
        }
    }

    Ok(RestartImplementationResult {
        session_id: session_id.as_str().to_string(),
        project_id: session.project_id.as_str().to_string(),
        old_execution_plan_id: active_plan.id.as_str().to_string(),
        new_execution_plan_id: tx_output.execution_plan_id.as_str().to_string(),
        archived_task_count: tx_output.archived_task_count,
        stopped_agent_count: runtime_cleanup_report.tasks_stopped,
        tasks_created: tx_output.created_tasks.len(),
        dependencies_created: tx_output.dependencies_created,
        created_task_ids: tx_output
            .created_tasks
            .into_iter()
            .map(|task| task.id.as_str().to_string())
            .collect(),
        any_ready_tasks: tx_output.any_ready_tasks,
        warnings,
    })
}

#[tauri::command]
pub async fn restart_ideation_implementation(
    session_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<RestartImplementationResult, String> {
    let session_id = IdeationSessionId::from_string(session_id);
    let result = restart_implementation_core(&state, &session_id)
        .await
        .map_err(|error| error.to_string())?;

    let _ = app.emit(
        "task:list_changed",
        serde_json::json!({
            "projectId": result.project_id,
        }),
    );
    let _ = app.emit(
        "ideation:implementation_restarted",
        serde_json::json!({
            "sessionId": result.session_id,
            "projectId": result.project_id,
            "oldExecutionPlanId": result.old_execution_plan_id,
            "newExecutionPlanId": result.new_execution_plan_id,
            "archivedTaskCount": result.archived_task_count,
            "tasksCreated": result.tasks_created,
        }),
    );

    if result.any_ready_tasks {
        let project_id = ProjectId::from_string(result.project_id.clone());
        emit_queue_changed(&state, &project_id, &app).await;

        let execution_state = app.state::<Arc<ExecutionState>>();
        spawn_ready_task_scheduler_if_needed(
            &state,
            Arc::clone(&*execution_state),
            Some(app.clone()),
            true,
        );
    }

    Ok(result)
}

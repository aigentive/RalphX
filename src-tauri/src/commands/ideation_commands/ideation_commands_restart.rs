// Restart an accepted implementation attempt from the accepted plan/proposals.

use std::collections::HashMap;
use std::sync::Arc;

use tauri::{Emitter, Manager, State};

use crate::application::task_cleanup_service::StopMode;
use crate::application::{
    agent_conversation_archive::close_agent_workspace_pr_for_restart,
    agent_conversation_workspace_restart::{
        prepare_linked_plan_branch_agent_worktree_for_restart,
        resolve_restart_workspace_cleanup_proof,
    },
    spawn_ready_task_scheduler_if_needed, AppState, GitService, TaskCleanupService,
};
use crate::commands::{emit_queue_changed, ExecutionState};
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ArtifactId, ExecutionPlanId, IdeationSessionId,
    IdeationSessionStatus, ProjectId, Task, TaskProposal, TaskProposalId,
};
use crate::error::{AppError, AppResult};

use super::ideation_commands_apply::{
    load_linked_agent_conversation_workspace, phase_insert_dependencies,
    phase_insert_execution_plan, phase_insert_merge_task, phase_insert_tasks_and_steps,
    phase_update_proposals, phase_upsert_plan_branch,
};
use super::ideation_commands_types::{
    RestartImplementationResult, RestartImplementationResultResponse,
};
use super::is_local_proposal;

struct RestartTxOutput {
    execution_plan_id: ExecutionPlanId,
    created_tasks: Vec<Task>,
    archived_task_count: usize,
    any_ready_tasks: bool,
}

fn clear_proposal_task_links(
    conn: &rusqlite::Connection,
    proposals: &[TaskProposal],
    now_str: &str,
) -> AppResult<()> {
    for proposal in proposals {
        conn.execute(
            "UPDATE task_proposals SET created_task_id = NULL, updated_at = ?2 WHERE id = ?1",
            rusqlite::params![proposal.id.as_str(), now_str],
        )
        .map_err(|error| {
            AppError::Database(format!("Failed to clear proposal task link: {}", error))
        })?;
    }
    Ok(())
}

fn archive_execution_plan_tasks(
    conn: &rusqlite::Connection,
    execution_plan_id: &ExecutionPlanId,
    now_str: &str,
) -> AppResult<usize> {
    conn.execute(
        "UPDATE tasks
         SET archived_at = ?2, updated_at = ?2
         WHERE execution_plan_id = ?1 AND archived_at IS NULL",
        rusqlite::params![execution_plan_id.as_str(), now_str],
    )
    .map_err(|error| AppError::Database(format!("Failed to archive old tasks: {}", error)))
}

fn mark_execution_plan_superseded(
    conn: &rusqlite::Connection,
    session_id_str: &str,
    execution_plan_id: &ExecutionPlanId,
) -> AppResult<()> {
    let rows = conn
        .execute(
            "UPDATE execution_plans
             SET status = 'superseded'
             WHERE id = ?1 AND session_id = ?2 AND status = 'active'",
            rusqlite::params![execution_plan_id.as_str(), session_id_str],
        )
        .map_err(|error| {
            AppError::Database(format!("Failed to supersede execution plan: {}", error))
        })?;
    if rows == 0 {
        return Err(AppError::Validation(
            "Current implementation attempt is no longer active".to_string(),
        ));
    }
    Ok(())
}

fn upsert_active_plan_pointer(
    conn: &rusqlite::Connection,
    project_id_str: &str,
    session_id_str: &str,
    execution_plan_id: &ExecutionPlanId,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO project_active_plan (
             project_id,
             ideation_session_id,
             execution_plan_id,
             updated_at
         )
         VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))
         ON CONFLICT(project_id) DO UPDATE SET
             ideation_session_id = excluded.ideation_session_id,
             execution_plan_id = excluded.execution_plan_id,
             updated_at = excluded.updated_at",
        rusqlite::params![project_id_str, session_id_str, execution_plan_id.as_str()],
    )
    .map_err(|error| {
        AppError::Database(format!(
            "Failed to update active implementation plan: {}",
            error
        ))
    })?;
    Ok(())
}

/// Core restart logic without Tauri transport side effects.
pub async fn restart_ideation_implementation_core(
    app_state: &AppState,
    session_id: String,
) -> AppResult<RestartImplementationResult> {
    let session_id = IdeationSessionId::from_string(session_id);
    let session = app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|error| AppError::Database(error.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("Session {} not found", session_id)))?;

    if session.status != IdeationSessionStatus::Accepted {
        return Err(AppError::Validation(
            "Can only restart implementation for an accepted ideation session".to_string(),
        ));
    }

    let old_execution_plan = app_state
        .execution_plan_repo
        .get_active_for_session(&session_id)
        .await
        .map_err(|error| {
            AppError::Database(format!("Failed to load active execution plan: {}", error))
        })?
        .ok_or_else(|| {
            AppError::Validation(
                "Accepted ideation session has no active implementation attempt".to_string(),
            )
        })?;

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
    let project_root = crate::utils::path_safety::validate_absolute_non_root_path(
        std::path::Path::new(&project.working_directory),
        "project checkout",
    )?;

    let session_base_ref = session
        .analysis
        .base_ref
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .cloned();
    let effective_base_branch_override = session_base_ref;
    let restart_base_branch = effective_base_branch_override
        .as_deref()
        .or(project.base_branch.as_deref())
        .unwrap_or("main");

    let linked_agent_workspace =
        load_linked_agent_conversation_workspace(app_state, &session_id, &session.project_id)
            .await?;
    let linked_plan_branch_worktree = if let Some(workspace) = linked_agent_workspace.as_ref() {
        if workspace.mode != AgentConversationWorkspaceMode::Ideation {
            return Err(AppError::Validation(
                "Linked agent conversation workspace is not in ideation mode".to_string(),
            ));
        }
        let plan_branch_id = workspace.linked_plan_branch_id.as_ref().ok_or_else(|| {
            AppError::Validation(
                "Linked ideation workspace has no linked plan branch for restart".to_string(),
            )
        })?;
        let plan_branch = app_state
            .plan_branch_repo
            .get_by_id(plan_branch_id)
            .await
            .map_err(|error| {
                AppError::Database(format!(
                    "Failed to load linked plan branch for restart: {}",
                    error
                ))
            })?
            .ok_or_else(|| {
                AppError::Validation(format!(
                    "Linked plan branch not found for restart: {}",
                    plan_branch_id
                ))
            })?;
        let origin_base_ref =
            GitService::fetch_origin_branch_strict(&project_root, restart_base_branch).await?;
        let workspace_cleanup_status = app_state
            .agent_conversation_workspace_repo
            .get_local_cleanup_status(&workspace.conversation_id)
            .await
            .map_err(|error| {
                AppError::Database(format!(
                    "Failed to load workspace cleanup provenance: {error}"
                ))
            })?;
        let plan_branch_cleanup_status = app_state
            .plan_branch_repo
            .get_local_cleanup_status(&plan_branch.id)
            .await
            .map_err(|error| {
                AppError::Database(format!(
                    "Failed to load plan branch cleanup provenance: {error}"
                ))
            })?;
        let cleanup_proof = resolve_restart_workspace_cleanup_proof(
            workspace,
            workspace_cleanup_status.as_deref(),
            &plan_branch,
            plan_branch_cleanup_status.as_deref(),
        );
        let preparation = prepare_linked_plan_branch_agent_worktree_for_restart(
            &project,
            workspace,
            &plan_branch,
            &origin_base_ref,
            cleanup_proof,
        )
        .await
        .map_err(|error| error.into_app_error())?;
        tracing::info!(
            conversation_id = workspace.conversation_id.as_str(),
            source = ?preparation.source,
            "Prepared linked implementation workspace for restart"
        );
        Some((plan_branch, preparation.path, origin_base_ref))
    } else {
        None
    };

    let current_task_count = app_state
        .task_repo
        .count_tasks(
            &session.project_id,
            false,
            None,
            Some(old_execution_plan.id.as_str()),
        )
        .await
        .map_err(|error| {
            AppError::Database(format!(
                "Failed to count current implementation tasks: {}",
                error
            ))
        })?;
    let current_tasks = if current_task_count == 0 {
        Vec::new()
    } else {
        app_state
            .task_repo
            .list_paginated(
                &session.project_id,
                None,
                0,
                current_task_count,
                false,
                None,
                Some(old_execution_plan.id.as_str()),
                None,
            )
            .await
            .map_err(|error| {
                AppError::Database(format!(
                    "Failed to load current implementation tasks: {}",
                    error
                ))
            })?
    };

    let all_proposals = app_state
        .task_proposal_repo
        .get_by_session(&session_id)
        .await
        .map_err(|error| AppError::Database(error.to_string()))?;
    let project_dir = std::fs::canonicalize(&project.working_directory)
        .unwrap_or_else(|_| std::path::PathBuf::from(&project.working_directory));
    let proposals_to_apply: Vec<TaskProposal> = all_proposals
        .into_iter()
        .filter(|proposal| is_local_proposal(proposal, &project_dir))
        .collect();
    if proposals_to_apply.is_empty() {
        return Err(AppError::Validation(
            "Accepted ideation session has no local proposals to restart".to_string(),
        ));
    }

    let current_execution_plan = app_state
        .execution_plan_repo
        .get_active_for_session(&session_id)
        .await
        .map_err(|error| {
            AppError::Database(format!(
                "Failed to verify current implementation attempt: {error}"
            ))
        })?;
    if current_execution_plan.as_ref().map(|plan| &plan.id) != Some(&old_execution_plan.id) {
        return Err(AppError::Validation(
            "Current implementation attempt is no longer active".to_string(),
        ));
    }

    if let (Some(workspace), Some((plan_branch, _, _))) = (
        linked_agent_workspace.as_ref(),
        linked_plan_branch_worktree.as_ref(),
    ) {
        close_agent_workspace_pr_for_restart(workspace, plan_branch, app_state).await?;
    }

    let mut proposal_deps: HashMap<TaskProposalId, Vec<TaskProposalId>> = HashMap::new();
    for proposal in &proposals_to_apply {
        let deps = app_state
            .proposal_dependency_repo
            .get_dependencies(&proposal.id)
            .await
            .map_err(|error| AppError::Database(error.to_string()))?;
        proposal_deps.insert(proposal.id.clone(), deps);
    }

    let task_cleanup = TaskCleanupService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.running_agent_registry),
        None,
    )
    .with_interactive_process_registry(Arc::clone(&app_state.interactive_process_registry));
    let cleanup_report = task_cleanup
        .prepare_tasks_for_replacement(&current_tasks, StopMode::DirectStop)
        .await;
    if !cleanup_report.errors.is_empty() {
        return Err(AppError::Database(format!(
            "Failed to prepare current implementation tasks for restart: {}",
            cleanup_report.errors.join("; ")
        )));
    }

    if let Some((_, worktree_path, origin_base_ref)) = linked_plan_branch_worktree.as_ref() {
        GitService::reset_hard(worktree_path, origin_base_ref).await?;
        GitService::clean_working_tree(worktree_path).await?;
    }

    let session_id_str = session_id.as_str().to_string();
    let project_id_str = session.project_id.as_str().to_string();
    let plan_artifact_id_tx: Option<ArtifactId> = session.plan_artifact_id.clone();
    let old_execution_plan_id = old_execution_plan.id.clone();
    let base_branch_override_tx = effective_base_branch_override.clone();
    let agent_workspace_branch_name_tx = linked_agent_workspace
        .as_ref()
        .map(|workspace| workspace.branch_name.clone());
    let project_base_branch_tx = project.base_branch.clone();
    let project_name_tx = project.name.clone();
    let project_pr_eligible_tx = project.github_pr_enabled;
    let proposals_tx = proposals_to_apply.clone();
    let proposal_deps_tx: HashMap<String, Vec<String>> = proposal_deps
        .iter()
        .map(|(proposal_id, dependency_ids)| {
            (
                proposal_id.as_str().to_string(),
                dependency_ids
                    .iter()
                    .map(|dependency_id| dependency_id.as_str().to_string())
                    .collect(),
            )
        })
        .collect();

    let tx_output = app_state
        .db
        .run_transaction(move |conn| {
            let now_str = chrono::Utc::now().to_rfc3339();
            let archived_task_count =
                archive_execution_plan_tasks(conn, &old_execution_plan_id, &now_str)?;
            mark_execution_plan_superseded(conn, &session_id_str, &old_execution_plan_id)?;
            clear_proposal_task_links(conn, &proposals_tx, &now_str)?;

            let execution_plan = phase_insert_execution_plan(conn, &session_id_str)?;
            let execution_plan_id = execution_plan.id.clone();

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

            let (_dependencies_created, warnings) = phase_insert_dependencies(
                conn,
                &proposals_tx,
                &proposal_deps_tx,
                &proposal_to_task,
            )?;
            if !warnings.is_empty() {
                tracing::warn!(
                    warnings = ?warnings,
                    "restart_ideation_implementation_core: some proposal dependencies were not preserved"
                );
            }

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
            upsert_active_plan_pointer(
                conn,
                &project_id_str,
                &session_id_str,
                &execution_plan_id,
            )?;

            Ok(RestartTxOutput {
                execution_plan_id,
                created_tasks,
                archived_task_count,
                any_ready_tasks,
            })
        })
        .await?;

    let active_execution_plan_id = app_state
        .active_plan_repo
        .get_execution_plan_id(&session.project_id)
        .await
        .map_err(|error| {
            AppError::Database(format!("Failed to verify active execution plan: {}", error))
        })?;
    if active_execution_plan_id.as_ref() != Some(&tx_output.execution_plan_id) {
        app_state
            .active_plan_repo
            .set(&session.project_id, &session_id)
            .await
            .map_err(|error| {
                AppError::Database(format!("Failed to select restarted active plan: {}", error))
            })?;
        app_state
            .active_plan_repo
            .set_execution_plan_id(&session.project_id, &tx_output.execution_plan_id)
            .await
            .map_err(|error| {
                AppError::Database(format!(
                    "Failed to select restarted execution plan: {}",
                    error
                ))
            })?;
    }

    if let Some(workspace) = linked_agent_workspace.as_ref() {
        if let Some(plan_branch) = app_state
            .plan_branch_repo
            .get_by_execution_plan_id(&tx_output.execution_plan_id)
            .await
            .map_err(|error| {
                AppError::Database(format!(
                    "Failed to load restarted implementation branch: {}",
                    error
                ))
            })?
        {
            app_state
                .agent_conversation_workspace_repo
                .restore_after_restart(&workspace.conversation_id, &session_id, &plan_branch.id)
                .await
                .map_err(|error| {
                    AppError::Database(format!(
                        "Failed to link agent conversation workspace to restarted branch: {}",
                        error
                    ))
                })?;
            app_state
                .plan_branch_repo
                .clear_local_cleanup_status(&plan_branch.id)
                .await
                .map_err(|error| {
                    AppError::Database(format!(
                        "Failed to clear restarted branch cleanup provenance: {error}"
                    ))
                })?;
        }
    }

    Ok(RestartImplementationResult {
        session_id: session_id.as_str().to_string(),
        project_id: session.project_id.as_str().to_string(),
        old_execution_plan_id: old_execution_plan.id.as_str().to_string(),
        execution_plan_id: tx_output.execution_plan_id.as_str().to_string(),
        archived_task_count: tx_output.archived_task_count,
        created_task_ids: tx_output
            .created_tasks
            .into_iter()
            .map(|task| task.id.as_str().to_string())
            .collect(),
        any_ready_tasks: tx_output.any_ready_tasks,
    })
}

#[tauri::command]
pub async fn restart_ideation_implementation(
    session_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<RestartImplementationResultResponse, String> {
    let result = restart_ideation_implementation_core(&state, session_id)
        .await
        .map_err(|error| error.to_string())?;

    let project_id = ProjectId::from_string(result.project_id.clone());
    let _ = app.emit(
        "ideation:session_accepted",
        serde_json::json!({
            "sessionId": result.session_id,
            "projectId": result.project_id,
        }),
    );
    let _ = app.emit(
        "task:list_changed",
        serde_json::json!({
            "projectId": project_id.as_str(),
        }),
    );

    if result.any_ready_tasks {
        emit_queue_changed(&state, &project_id, &app).await;
        let execution_state = app.state::<Arc<ExecutionState>>();
        spawn_ready_task_scheduler_if_needed(
            &state,
            Arc::clone(&*execution_state),
            Some(app.clone()),
            true,
        );
    }

    Ok(result.into())
}

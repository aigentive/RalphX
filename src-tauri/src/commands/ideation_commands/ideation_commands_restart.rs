// Restart an accepted implementation attempt from the accepted plan/proposals.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use tauri::{Emitter, Manager, State};

use crate::application::interactive_process_registry::InteractiveProcessKey;
use crate::application::task_cleanup_service::StopMode;
use crate::application::{
    agent_conversation_archive::close_agent_workspace_pr_for_restart,
    agent_conversation_workspace_restart::{
        inspect_linked_plan_branch_owner_for_restart,
        prepare_linked_plan_branch_agent_worktree_for_restart,
        resolve_restart_workspace_cleanup_proof,
    },
    spawn_ready_task_scheduler_if_needed, AppState, GitService, TaskCleanupService,
};
use crate::commands::{emit_queue_changed, ExecutionState};
use crate::domain::entities::{
    AgentConversationWorkspaceMode, ArtifactId, BranchUpdateDirection, BranchUpdateOperation,
    ChatContextType, ExecutionPlanId, GitTargetLease, GitTargetLeaseOwner, IdeationSessionId,
    IdeationSessionStatus, InternalStatus, Project, ProjectId, Task, TaskId, TaskProposal,
    TaskProposalId,
};
use crate::domain::repositories::{BranchUpdateCasOutcome, StopBranchUpdate};
use crate::domain::services::{QueueKey, RunningAgentKey};
use crate::domain::state_machine::transition_handler::{
    compute_plan_update_worktree_path, compute_source_update_worktree_path,
};
use crate::error::{AppError, AppResult};

use super::ideation_commands_apply::{
    inspect_plan_branch_pr_eligibility, load_linked_agent_conversation_workspace,
    phase_insert_dependencies, phase_insert_execution_plan, phase_insert_merge_task,
    phase_insert_tasks_and_steps, phase_update_proposals, phase_upsert_plan_branch,
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

#[derive(Debug)]
pub(super) struct RestartBranchUpdate {
    operation: BranchUpdateOperation,
    lease: GitTargetLease,
    workspace_path: std::path::PathBuf,
}

pub(super) async fn preflight_branch_updates_for_restart(
    app_state: &AppState,
    project: &Project,
    tasks: &[Task],
) -> AppResult<Vec<RestartBranchUpdate>> {
    let mut registered_worktrees = None;
    let mut updates = Vec::new();
    for task_snapshot in tasks {
        let task = app_state
            .task_repo
            .get_by_id(&task_snapshot.id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "Task {} disappeared during branch-update restart preflight",
                    task_snapshot.id
                ))
            })?;
        let operation = app_state
            .branch_update_repo
            .get_active_operation(&task.id)
            .await?;
        let Some(operation) = operation else {
            if matches!(
                task.internal_status,
                InternalStatus::UpdatingPlanBranch | InternalStatus::UpdatingTaskBranch
            ) {
                return Err(AppError::Validation(format!(
                    "Task {} is updating a branch without active durable authority",
                    task.id
                )));
            }
            continue;
        };
        let expected_status = match operation.direction {
            BranchUpdateDirection::PlanBranch => InternalStatus::UpdatingPlanBranch,
            BranchUpdateDirection::TaskBranch => InternalStatus::UpdatingTaskBranch,
        };
        if task.internal_status != expected_status {
            return Err(AppError::Validation(format!(
                "Task {} branch-update status does not match its active operation",
                task.id
            )));
        }
        let lease = app_state
            .branch_update_repo
            .get_target_lease(&operation.target_identity)
            .await?
            .ok_or_else(|| {
                AppError::Validation(format!(
                    "Task {} branch-update target authority is missing",
                    task.id
                ))
            })?;
        let expected_owner =
            GitTargetLeaseOwner::branch_update(task.id.as_str(), operation.id.as_str());
        if lease.owner() != &expected_owner
            || lease.fencing_epoch() != operation.target_lease_epoch
            || lease.is_released()
            || lease.active_mutation().is_some()
        {
            return Err(AppError::Validation(format!(
                "Task {} branch-update authority is busy or stale",
                task.id
            )));
        }
        let expected_workspace = match operation.direction {
            BranchUpdateDirection::PlanBranch => {
                compute_plan_update_worktree_path(project, task.id.as_str())
            }
            BranchUpdateDirection::TaskBranch => {
                compute_source_update_worktree_path(project, task.id.as_str())
            }
        };
        let workspace_path = operation.workspace_path.as_ref().ok_or_else(|| {
            AppError::Validation(format!(
                "Task {} branch-update operation has no workspace path",
                task.id
            ))
        })?;
        let workspace_path = crate::utils::path_safety::validate_absolute_non_root_path(
            workspace_path,
            "restart branch-update workspace",
        )?;
        let expected_workspace = crate::utils::path_safety::validate_absolute_non_root_path(
            std::path::Path::new(&expected_workspace),
            "derived restart branch-update workspace",
        )?;
        if workspace_path != expected_workspace {
            return Err(AppError::Validation(format!(
                "Task {} branch-update workspace is not process-owned",
                task.id
            )));
        }
        if registered_worktrees.is_none() {
            let project_root = crate::utils::path_safety::validate_absolute_non_root_path(
                std::path::Path::new(&project.working_directory),
                "restart branch-update project checkout",
            )?;
            registered_worktrees = Some(GitService::list_worktrees(&project_root).await?);
        }
        let registered_worktrees = registered_worktrees
            .as_ref()
            .expect("registered worktrees should be loaded for active branch updates");
        let registered = registered_worktrees.iter().any(|worktree| {
            crate::utils::path_safety::validate_absolute_non_root_path(
                std::path::Path::new(&worktree.path),
                "registered branch-update worktree",
            )
            .is_ok_and(|path| restart_paths_match(&path, &workspace_path))
        });
        if workspace_path.exists() && !registered {
            return Err(AppError::Validation(format!(
                "Task {} branch-update workspace exists without Git registration",
                task.id
            )));
        }
        updates.push(RestartBranchUpdate {
            operation,
            lease,
            workspace_path,
        });
    }
    Ok(updates)
}

pub(super) async fn stop_branch_updates_for_restart(
    app_state: &AppState,
    updates: &[RestartBranchUpdate],
) -> AppResult<()> {
    for update in updates {
        let operation = &update.operation;
        let expected_status = match operation.direction {
            BranchUpdateDirection::PlanBranch => InternalStatus::UpdatingPlanBranch,
            BranchUpdateDirection::TaskBranch => InternalStatus::UpdatingTaskBranch,
        };
        let outcome = app_state
            .branch_update_repo
            .stop_operation(StopBranchUpdate {
                operation_id: operation.id.clone(),
                task_id: operation.task_id.clone(),
                originating_history_id: operation.originating_history_id.clone(),
                update_status: expected_status,
                owner: update.lease.owner().clone(),
                fencing_epoch: update.lease.fencing_epoch(),
                history_id: uuid::Uuid::new_v4().to_string(),
                reason: Some("implementation_attempt_restarted".to_string()),
            })
            .await?;
        if outcome != BranchUpdateCasOutcome::Applied {
            return Err(AppError::Conflict(format!(
                "Task {} branch-update authority changed during restart: {outcome:?}",
                operation.task_id
            )));
        }
        let context_type = ChatContextType::BranchUpdate.to_string();
        app_state
            .interactive_process_registry
            .remove(&InteractiveProcessKey::new(
                context_type.clone(),
                operation.task_id.as_str(),
            ))
            .await;
        let _ = app_state
            .running_agent_registry
            .stop(&RunningAgentKey::new(
                context_type,
                operation.task_id.as_str(),
            ))
            .await;
        let queue_key = QueueKey::new(ChatContextType::BranchUpdate, operation.task_id.as_str());
        app_state.message_queue.clear_with_key(&queue_key);
        app_state.queued_message_repo.clear(&queue_key).await?;
    }
    Ok(())
}

pub(super) async fn cleanup_branch_update_worktrees_for_restart(
    project: &Project,
    updates: &[RestartBranchUpdate],
) -> AppResult<()> {
    let project_root = crate::utils::path_safety::validate_absolute_non_root_path(
        std::path::Path::new(&project.working_directory),
        "restart branch-update project checkout",
    )?;
    for update in updates {
        if update.workspace_path.exists() {
            GitService::delete_worktree(&project_root, &update.workspace_path).await?;
        }
    }
    Ok(())
}

fn restart_paths_match(left: &std::path::Path, right: &std::path::Path) -> bool {
    left == right
        || left
            .canonicalize()
            .ok()
            .zip(right.canonicalize().ok())
            .is_some_and(|(left, right)| left == right)
}

#[derive(Debug)]
pub(super) struct RestartInFlightGuard {
    session_id: String,
}

impl RestartInFlightGuard {
    pub(super) fn acquire(session_id: &IdeationSessionId) -> AppResult<Self> {
        let session_id = session_id.as_str().to_string();
        match restart_in_flight().entry(session_id.clone()) {
            Entry::Occupied(_) => Err(AppError::Validation(
                "Restart Implementation is already in progress for this plan".to_string(),
            )),
            Entry::Vacant(entry) => {
                entry.insert(());
                Ok(Self { session_id })
            }
        }
    }
}

impl Drop for RestartInFlightGuard {
    fn drop(&mut self) {
        restart_in_flight().remove(&self.session_id);
    }
}

fn restart_in_flight() -> &'static DashMap<String, ()> {
    static RESTARTS: OnceLock<DashMap<String, ()>> = OnceLock::new();
    RESTARTS.get_or_init(DashMap::new)
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

pub(super) fn archive_execution_plan_tasks(
    conn: &rusqlite::Connection,
    execution_plan_id: &ExecutionPlanId,
    expected_task_ids: &[TaskId],
    now_str: &str,
) -> AppResult<usize> {
    let mut statement = conn
        .prepare(
            "SELECT id FROM tasks
             WHERE execution_plan_id = ?1 AND archived_at IS NULL",
        )
        .map_err(|error| {
            AppError::Database(format!("Failed to inspect current attempt tasks: {error}"))
        })?;
    let actual_task_ids = statement
        .query_map(rusqlite::params![execution_plan_id.as_str()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| {
            AppError::Database(format!("Failed to query current attempt tasks: {error}"))
        })?
        .collect::<Result<HashSet<_>, _>>()
        .map_err(|error| {
            AppError::Database(format!("Failed to read current attempt tasks: {error}"))
        })?;
    let expected_task_ids = expected_task_ids
        .iter()
        .map(|task_id| task_id.as_str().to_string())
        .collect::<HashSet<_>>();
    if actual_task_ids != expected_task_ids {
        return Err(AppError::Validation(
            "Current implementation tasks changed while restart was preparing; retry restart"
                .to_string(),
        ));
    }

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

/// Resets workspace state for a restart inside the restart transaction.
///
/// The inline SQL is required because this runs on the transaction's connection.
/// `AgentConversationWorkspaceRepository::restore_after_restart` is the contract
/// counterpart and must clear the same column set; keep the two in sync.
fn restore_restart_workspace_state(
    conn: &rusqlite::Connection,
    workspace_conversation_id: Option<&str>,
    session_id: &str,
    plan_branch_id: &str,
    now_str: &str,
) -> AppResult<()> {
    conn.execute(
        "UPDATE plan_branches
         SET local_cleanup_status = NULL,
             local_cleanup_checked_at = NULL
         WHERE id = ?1",
        rusqlite::params![plan_branch_id],
    )
    .map_err(|error| AppError::Database(format!("Failed to reset plan branch state: {error}")))?;

    if let Some(conversation_id) = workspace_conversation_id {
        let rows = conn
            .execute(
                "UPDATE agent_conversation_workspaces
                 SET linked_ideation_session_id = ?2,
                     linked_plan_branch_id = ?3,
                     status = 'active',
                     publication_pushed_sha = NULL,
                     local_cleanup_status = NULL,
                     local_cleanup_checked_at = NULL,
                     publication_pr_number = NULL,
                     publication_pr_url = NULL,
                     publication_pr_status = NULL,
                     publication_push_status = NULL,
                     pr_supervision_status = NULL,
                     pr_supervision_summary = NULL,
                     pr_supervision_updated_at = ?4,
                     updated_at = ?4
                 WHERE conversation_id = ?1",
                rusqlite::params![conversation_id, session_id, plan_branch_id, now_str],
            )
            .map_err(|error| {
                AppError::Database(format!(
                    "Failed to restore restart workspace state: {error}"
                ))
            })?;
        if rows == 0 {
            return Err(AppError::NotFound(format!(
                "Workspace not found during restart transaction: {conversation_id}"
            )));
        }
    }
    Ok(())
}

/// Core restart logic without Tauri transport side effects.
pub async fn restart_ideation_implementation_core(
    app_state: &AppState,
    session_id: String,
) -> AppResult<RestartImplementationResult> {
    let session_id = IdeationSessionId::from_string(session_id);
    let _restart_guard = RestartInFlightGuard::acquire(&session_id)?;
    let session = app_state
        .ideation_session_repo
        .get_by_id(&session_id)
        .await
        .map_err(|error| AppError::Database(error.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("Session {} not found", session_id)))?;

    crate::application::tasks_feature_policy::TasksFeaturePolicy::from_state(app_state)
        .authorize_session(
            Some(&session_id),
            crate::domain::ideation::TasksFeatureAction::Progress,
        )
        .await?;

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
    // Read the current origin topology before any restart cleanup or transaction
    // effects. Local and non-GitHub repositories deliberately select local
    // routing; an unreadable topology fails closed.
    let effective_plan_pr_eligible = inspect_plan_branch_pr_eligibility(&project).await?;

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

    let linked_agent_workspace =
        load_linked_agent_conversation_workspace(app_state, &session_id, &session.project_id)
            .await?;
    let linked_plan_branch_context = if let Some(workspace) = linked_agent_workspace.as_ref() {
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
        let owner = inspect_linked_plan_branch_owner_for_restart(
            &project,
            workspace,
            &plan_branch,
            &session_id,
            &old_execution_plan.id,
            &current_tasks,
            cleanup_proof,
        )
        .await
        .map_err(|error| error.into_app_error())?;
        tracing::info!(
            conversation_id = workspace.conversation_id.as_str(),
            owner = ?owner,
            "Verified linked implementation workspace ownership for restart"
        );
        Some((plan_branch, origin_base_ref, cleanup_proof, owner))
    } else {
        None
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

    let mut proposal_deps: HashMap<TaskProposalId, Vec<TaskProposalId>> = HashMap::new();
    for proposal in &proposals_to_apply {
        let deps = app_state
            .proposal_dependency_repo
            .get_dependencies(&proposal.id)
            .await
            .map_err(|error| AppError::Database(error.to_string()))?;
        proposal_deps.insert(proposal.id.clone(), deps);
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

    let task_cleanup = TaskCleanupService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.running_agent_registry),
        None,
    )
    .with_interactive_process_registry(Arc::clone(&app_state.interactive_process_registry));
    let preserved_branch = linked_plan_branch_context
        .as_ref()
        .map(|(plan_branch, _, _, _)| plan_branch.branch_name.as_str());
    task_cleanup
        .preflight_tasks_for_replacement(&current_tasks, preserved_branch)
        .await?;
    let branch_updates =
        preflight_branch_updates_for_restart(app_state, &project, &current_tasks).await?;
    crate::application::tasks_feature_policy::TasksFeaturePolicy::from_state(app_state)
        .authorize_session(
            Some(&session_id),
            crate::domain::ideation::TasksFeatureAction::Progress,
        )
        .await?;
    stop_branch_updates_for_restart(app_state, &branch_updates).await?;
    task_cleanup
        .stop_tasks_for_replacement(&current_tasks, StopMode::DirectStop)
        .await?;

    if let (Some(workspace), Some((plan_branch, _, _, _))) = (
        linked_agent_workspace.as_ref(),
        linked_plan_branch_context.as_ref(),
    ) {
        close_agent_workspace_pr_for_restart(workspace, plan_branch, app_state).await?;
    }
    cleanup_branch_update_worktrees_for_restart(&project, &branch_updates).await?;

    let cleanup_report = task_cleanup
        .prepare_tasks_for_replacement(&current_tasks, StopMode::DirectStop, preserved_branch)
        .await;
    if !cleanup_report.errors.is_empty() {
        return Err(AppError::Database(format!(
            "Failed to prepare current implementation tasks for restart: {}",
            cleanup_report.errors.join("; ")
        )));
    }

    if let (Some(workspace), Some((plan_branch, origin_base_ref, cleanup_proof, owner))) = (
        linked_agent_workspace.as_ref(),
        linked_plan_branch_context.as_ref(),
    ) {
        let preparation = prepare_linked_plan_branch_agent_worktree_for_restart(
            &project,
            workspace,
            plan_branch,
            origin_base_ref,
            *cleanup_proof,
        )
        .await
        .map_err(|error| error.into_app_error())?;
        tracing::info!(
            conversation_id = workspace.conversation_id.as_str(),
            preflight_owner = ?owner,
            source = ?preparation.source,
            "Prepared linked implementation workspace after attempt cleanup"
        );
        GitService::reset_hard(&preparation.path, origin_base_ref).await?;
        GitService::clean_working_tree(&preparation.path).await?;
    }

    let session_id_str = session_id.as_str().to_string();
    let project_id_str = session.project_id.as_str().to_string();
    let plan_artifact_id_tx: Option<ArtifactId> = session.plan_artifact_id.clone();
    let old_execution_plan_id = old_execution_plan.id.clone();
    let old_task_ids_tx = current_tasks
        .iter()
        .map(|task| task.id.clone())
        .collect::<Vec<_>>();
    let base_branch_override_tx = effective_base_branch_override.clone();
    let agent_workspace_branch_name_tx = linked_agent_workspace
        .as_ref()
        .map(|workspace| workspace.branch_name.clone());
    let workspace_conversation_id_tx = linked_agent_workspace
        .as_ref()
        .map(|workspace| workspace.conversation_id.as_str().to_string());
    let project_base_branch_tx = project.base_branch.clone();
    let project_name_tx = project.name.clone();
    let effective_plan_pr_eligible_tx = effective_plan_pr_eligible;
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
            crate::application::tasks_feature_policy::authorize_tasks_session_sync(
                conn,
                Some(&session_id_str),
                crate::domain::ideation::TasksFeatureAction::Progress,
            )?;
            let now_str = chrono::Utc::now().to_rfc3339();
            let archived_task_count = archive_execution_plan_tasks(
                conn,
                &old_execution_plan_id,
                &old_task_ids_tx,
                &now_str,
            )?;
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
                effective_plan_pr_eligible_tx,
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
            restore_restart_workspace_state(
                conn,
                workspace_conversation_id_tx.as_deref(),
                &session_id_str,
                branch_id.as_str(),
                &now_str,
            )?;

            Ok(RestartTxOutput {
                execution_plan_id,
                created_tasks,
                archived_task_count,
                any_ready_tasks,
            })
        })
        .await?;

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

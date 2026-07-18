use std::path::Path;
use std::sync::Arc;

use crate::application::chat_service::ChatService;
use crate::application::task_cleanup_service::{StopMode, TaskCleanupService};
use crate::application::AppState;
use crate::domain::entities::plan_branch::PrStatus;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    ChatContextType, ChatConversationId, ExecutionPlan, ExecutionPlanStatus, PlanBranch,
    PlanBranchStatus,
};
use crate::domain::services::github_service::PrStatus as RemotePrStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectivePrSource {
    PlanBranch,
    Workspace,
}

#[derive(Debug, Clone)]
struct EffectivePrTarget {
    number: i64,
    url: Option<String>,
    source: EffectivePrSource,
    is_open: bool,
}

#[derive(Debug, Clone)]
struct RestartPrTarget {
    number: i64,
    locally_terminal: bool,
}

/// Archive an agent conversation and clean up linked ideation execution state.
pub async fn archive_agent_conversation_for_state(
    conversation_id: &ChatConversationId,
    state: &AppState,
    close_pull_request: bool,
) -> Result<(), String> {
    let _conversation = state
        .chat_conversation_repo
        .get_by_id(conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Conversation not found".to_string())?;

    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|e| e.to_string())?;

    stop_project_agent(conversation_id, state).await?;

    if let Some(workspace) = workspace.as_ref() {
        cleanup_ideation_execution_workspace(workspace, state).await?;
        if close_pull_request && workspace_allows_pr_closure(workspace) {
            close_effective_pr_if_open(conversation_id, workspace, state).await?;
        }
    }

    state
        .chat_conversation_repo
        .archive(conversation_id)
        .await
        .map_err(|e| e.to_string())?;

    if workspace.is_some() {
        state
            .agent_conversation_workspace_repo
            .update_status(conversation_id, AgentConversationWorkspaceStatus::Archived)
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Close the effective PR for an agent workspace, using linked PlanBranch PRs first.
pub async fn close_agent_workspace_pr_for_state(
    conversation_id: &ChatConversationId,
    state: &AppState,
) -> Result<(), String> {
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            format!(
                "Agent conversation workspace not found for conversation {}",
                conversation_id
            )
        })?;
    if !workspace_allows_pr_closure(&workspace) {
        return Err("Cannot close a pull request for a Review PR conversation".to_string());
    }
    let linked_plan_branch = load_linked_plan_branch_for_pr(&workspace, state).await?;
    let target = resolve_effective_pr(&workspace, linked_plan_branch.as_ref())
        .ok_or_else(|| "No PR associated with this workspace".to_string())?;

    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project not found: {}", workspace.project_id))?;

    close_remote_pr(
        state,
        Path::new(&project.working_directory),
        target.number,
        "close_agent_workspace_pr",
    )
    .await;
    mark_effective_pr_closed(
        conversation_id,
        &workspace,
        linked_plan_branch.as_ref(),
        &target,
        state,
    )
    .await
}

/// Strictly reconcile the effective remote PR before replacing an implementation attempt.
/// Local pointers remain intact until the replacement transaction commits.
pub(crate) async fn close_agent_workspace_pr_for_restart(
    workspace: &AgentConversationWorkspace,
    linked_plan_branch: &PlanBranch,
    state: &AppState,
) -> crate::error::AppResult<()> {
    let mut pr_targets = Vec::with_capacity(2);
    if let Some(number) = linked_plan_branch.pr_number {
        add_restart_pr_target(
            &mut pr_targets,
            number,
            is_plan_branch_pr_terminal(linked_plan_branch),
        );
    }
    if let Some(number) = workspace.publication_pr_number {
        add_restart_pr_target(
            &mut pr_targets,
            number,
            !is_workspace_pr_open(workspace.publication_pr_status.as_deref()),
        );
    }
    let pr_numbers_to_reconcile: Vec<i64> = pr_targets
        .into_iter()
        .filter_map(|target| (!target.locally_terminal).then_some(target.number))
        .collect();
    if !pr_numbers_to_reconcile.is_empty() {
        let project = state
            .project_repo
            .get_by_id(&workspace.project_id)
            .await
            .map_err(|error| crate::error::AppError::Database(error.to_string()))?
            .ok_or_else(|| {
                crate::error::AppError::NotFound(format!(
                    "Project not found while closing PRs for restart: {}",
                    workspace.project_id
                ))
            })?;
        let project_path = crate::utils::path_safety::validate_absolute_non_root_path(
            Path::new(&project.working_directory),
            "project checkout",
        )?;
        let github_svc = state.github_service.as_ref().ok_or_else(|| {
            crate::error::AppError::Validation(format!(
                "Restart cannot reconcile existing PRs {:?} because GitHub integration is unavailable",
                pr_numbers_to_reconcile
            ))
        })?;
        for number in pr_numbers_to_reconcile {
            let remote_status = github_svc
                .check_pr_status(&project_path, number)
                .await
                .map_err(|error| {
                    crate::error::AppError::Infrastructure(format!(
                        "Restart could not check existing PR {}: {}",
                        number, error
                    ))
                })?;
            if remote_status == RemotePrStatus::Open {
                github_svc
                    .close_pr(&project_path, number)
                    .await
                    .map_err(|error| {
                        crate::error::AppError::Infrastructure(format!(
                            "Restart could not close existing PR {}: {}",
                            number, error
                        ))
                    })?;
            }
        }
    }
    Ok(())
}

fn add_restart_pr_target(
    pr_targets: &mut Vec<RestartPrTarget>,
    number: i64,
    locally_terminal: bool,
) {
    if let Some(existing) = pr_targets.iter_mut().find(|target| target.number == number) {
        existing.locally_terminal &= locally_terminal;
        return;
    }

    pr_targets.push(RestartPrTarget {
        number,
        locally_terminal,
    });
}

fn workspace_allows_pr_closure(workspace: &AgentConversationWorkspace) -> bool {
    workspace.mode != AgentConversationWorkspaceMode::ReviewPr
}

async fn stop_project_agent(
    conversation_id: &ChatConversationId,
    state: &AppState,
) -> Result<(), String> {
    let chat_service = state.build_chat_service();
    let conversation_id_str = conversation_id.as_str();
    chat_service
        .stop_agent(ChatContextType::Project, &conversation_id_str)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn cleanup_ideation_execution_workspace(
    workspace: &AgentConversationWorkspace,
    state: &AppState,
) -> Result<(), String> {
    if workspace.mode != AgentConversationWorkspaceMode::Ideation {
        return Ok(());
    }

    let Some(plan_branch_id) = workspace.linked_plan_branch_id.as_ref() else {
        return Ok(());
    };

    let plan_branch = state
        .plan_branch_repo
        .get_by_id(plan_branch_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Linked plan branch not found: {}", plan_branch_id))?;

    if let Some(execution_plan) = resolve_current_execution_plan(&plan_branch, state).await? {
        archive_current_execution_tasks(workspace, &execution_plan, state).await?;
        if execution_plan.status == ExecutionPlanStatus::Active {
            state
                .execution_plan_repo
                .mark_superseded(&execution_plan.id)
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    if plan_branch.status != PlanBranchStatus::Abandoned {
        state
            .plan_branch_repo
            .update_status(&plan_branch.id, PlanBranchStatus::Abandoned)
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

async fn resolve_current_execution_plan(
    plan_branch: &PlanBranch,
    state: &AppState,
) -> Result<Option<ExecutionPlan>, String> {
    if let Some(execution_plan_id) = plan_branch.execution_plan_id.as_ref() {
        return state
            .execution_plan_repo
            .get_by_id(execution_plan_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| {
                format!(
                    "Linked execution plan not found: {}",
                    execution_plan_id.as_str()
                )
            })
            .map(Some);
    }

    state
        .execution_plan_repo
        .get_active_for_session(&plan_branch.session_id)
        .await
        .map_err(|e| e.to_string())
}

async fn archive_current_execution_tasks(
    workspace: &AgentConversationWorkspace,
    execution_plan: &ExecutionPlan,
    state: &AppState,
) -> Result<(), String> {
    let tasks = state
        .task_repo
        .list_paginated(
            &workspace.project_id,
            None,
            0,
            10_000,
            false,
            None,
            Some(execution_plan.id.as_str()),
            None,
        )
        .await
        .map_err(|e| e.to_string())?;

    if tasks.is_empty() {
        return Ok(());
    }

    let cleanup_service = TaskCleanupService::new(
        Arc::clone(&state.task_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.running_agent_registry),
        state.app_handle.clone(),
    )
    .with_interactive_process_registry(Arc::clone(&state.interactive_process_registry));

    let report = cleanup_service
        .cleanup_tasks(&tasks, StopMode::DirectStop, true)
        .await;

    if !report.errors.is_empty() {
        return Err(format!(
            "Failed to archive current execution tasks: {}",
            report.errors.join("; ")
        ));
    }

    Ok(())
}

async fn close_effective_pr_if_open(
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    state: &AppState,
) -> Result<(), String> {
    let linked_plan_branch = load_linked_plan_branch_for_pr(workspace, state).await?;
    let Some(target) = resolve_effective_pr(workspace, linked_plan_branch.as_ref()) else {
        return Ok(());
    };

    if !target.is_open {
        return Ok(());
    }

    match state.project_repo.get_by_id(&workspace.project_id).await {
        Ok(Some(project)) => {
            close_remote_pr(
                state,
                Path::new(&project.working_directory),
                target.number,
                "archive_agent_conversation",
            )
            .await;
        }
        Ok(None) => {
            tracing::warn!(
                project_id = workspace.project_id.as_str(),
                pr_number = target.number,
                "archive_agent_conversation: project not found while closing PR; continuing with local status update"
            );
        }
        Err(error) => {
            tracing::warn!(
                project_id = workspace.project_id.as_str(),
                pr_number = target.number,
                error = %error,
                "archive_agent_conversation: failed to load project while closing PR; continuing with local status update"
            );
        }
    }

    mark_effective_pr_closed(
        conversation_id,
        workspace,
        linked_plan_branch.as_ref(),
        &target,
        state,
    )
    .await
}

async fn close_remote_pr(
    state: &AppState,
    working_dir: &Path,
    pr_number: i64,
    operation: &'static str,
) {
    if let Some(github_svc) = &state.github_service {
        if let Err(error) = github_svc.close_pr(working_dir, pr_number).await {
            tracing::warn!(
                pr_number,
                operation,
                error = %error,
                "failed to close PR on remote (continuing with local status update)"
            );
        }
    }
}

async fn load_linked_plan_branch_for_pr(
    workspace: &AgentConversationWorkspace,
    state: &AppState,
) -> Result<Option<PlanBranch>, String> {
    if workspace.mode != AgentConversationWorkspaceMode::Ideation {
        return Ok(None);
    }

    let Some(plan_branch_id) = workspace.linked_plan_branch_id.as_ref() else {
        return Ok(None);
    };

    state
        .plan_branch_repo
        .get_by_id(plan_branch_id)
        .await
        .map_err(|e| e.to_string())
}

fn resolve_effective_pr(
    workspace: &AgentConversationWorkspace,
    linked_plan_branch: Option<&PlanBranch>,
) -> Option<EffectivePrTarget> {
    if let Some(plan_branch) = linked_plan_branch {
        if let Some(number) = plan_branch.pr_number {
            return Some(EffectivePrTarget {
                number,
                url: plan_branch.pr_url.clone(),
                source: EffectivePrSource::PlanBranch,
                is_open: is_plan_branch_pr_open(plan_branch),
            });
        }
    }

    workspace
        .publication_pr_number
        .map(|number| EffectivePrTarget {
            number,
            url: workspace.publication_pr_url.clone(),
            source: EffectivePrSource::Workspace,
            is_open: is_workspace_pr_open(workspace.publication_pr_status.as_deref()),
        })
}

fn is_plan_branch_pr_open(plan_branch: &PlanBranch) -> bool {
    !is_plan_branch_pr_terminal(plan_branch)
}

fn is_plan_branch_pr_terminal(plan_branch: &PlanBranch) -> bool {
    matches!(
        plan_branch.pr_status.as_ref().map(PrStatus::to_db_string),
        Some("Closed" | "Merged")
    )
}

fn is_workspace_pr_open(status: Option<&str>) -> bool {
    !matches!(
        status,
        Some(value) if value.eq_ignore_ascii_case("closed") || value.eq_ignore_ascii_case("merged")
    )
}

async fn mark_effective_pr_closed(
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    linked_plan_branch: Option<&PlanBranch>,
    target: &EffectivePrTarget,
    state: &AppState,
) -> Result<(), String> {
    if target.source == EffectivePrSource::PlanBranch {
        if let Some(plan_branch) = linked_plan_branch {
            state
                .plan_branch_repo
                .update_pr_status(&plan_branch.id, PrStatus::Closed)
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    state
        .agent_conversation_workspace_repo
        .update_publication(
            conversation_id,
            Some(target.number),
            target.url.as_deref(),
            Some("closed"),
            workspace.publication_push_status.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())
}

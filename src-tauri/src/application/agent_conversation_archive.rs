use std::path::Path;
use std::sync::Arc;

use crate::application::agent_workspace_terminal_cleanup::{
    record_no_pr_terminal_observation_best_effort, record_terminal_pr_observation_best_effort,
    terminalize_agent_workspace_after_pr_with_observation, TerminalAgentWorkspaceCause,
    TerminalAgentWorkspaceOutcome, TerminalCleanupClaimState, TerminalLocalCleanupResult,
    TerminalPrObservation,
};
use crate::application::agent_workspace_terminal_observation::resolve_merge_cleanliness_best_effort;
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
use crate::domain::services::{
    WORKSPACE_TERMINAL_REASON_ARCHIVE_ABANDONED, WORKSPACE_TERMINAL_REASON_ARCHIVE_CLOSED,
    WORKSPACE_TERMINAL_REASON_PUBLISH_FAILED, WORKSPACE_TERMINAL_REASON_RESTART_SUPERSEDED,
    WORKSPACE_TERMINAL_REASON_USER_CLOSED,
};

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
    status: Option<String>,
}

/// Archive an agent conversation and clean up linked ideation execution state.
pub async fn archive_agent_conversation_for_state(
    conversation_id: &ChatConversationId,
    state: &AppState,
    close_pull_request: bool,
) -> Result<TerminalAgentWorkspaceOutcome, String> {
    let conversation = state
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
    if workspace.is_none() {
        stop_conversation_agent(conversation.context_type, conversation_id, state).await?;
    }
    let project = match workspace.as_ref() {
        Some(workspace) => Some(
            state
                .project_repo
                .get_by_id(&workspace.project_id)
                .await
                .map_err(|error| error.to_string())?
                .ok_or_else(|| format!("Project not found: {}", workspace.project_id))?,
        ),
        None => None,
    };

    let mut archive_closed_pr_number = None;
    let had_effective_pr = if let Some(workspace) = workspace.as_ref() {
        let had_effective_pr = workspace_has_effective_pr(workspace, state).await?;
        cleanup_ideation_execution_workspace(workspace, state).await?;
        if close_pull_request && workspace_allows_pr_closure(workspace) {
            archive_closed_pr_number =
                close_effective_pr_if_open(conversation_id, workspace, state).await?;
        }
        had_effective_pr
    } else {
        false
    };

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

    let Some(workspace) = workspace else {
        return Ok(TerminalAgentWorkspaceOutcome {
            runtime_shutdown_succeeded: true,
            cleanup_claim: TerminalCleanupClaimState::NotClaimed,
            local_cleanup: TerminalLocalCleanupResult::Cleaned,
            message: None,
        });
    };
    let project = project.ok_or_else(|| {
        format!(
            "Project not found after workspace archive: {}",
            workspace.project_id
        )
    })?;
    let chat_service: Arc<dyn ChatService> = Arc::new(state.build_chat_service());
    let persisted_workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|error| error.to_string())?;
    let observation = persisted_workspace
        .as_ref()
        .and_then(TerminalPrObservation::from_persisted_workspace)
        .map(|observation| {
            if observation.status == "closed"
                && archive_closed_pr_number == Some(observation.pr_number)
            {
                observation.with_reason(WORKSPACE_TERMINAL_REASON_ARCHIVE_CLOSED)
            } else {
                observation
            }
        });
    let observation = match (persisted_workspace.as_ref(), observation) {
        (Some(persisted_workspace), Some(observation)) => Some(
            resolve_merge_cleanliness_best_effort(
                state.github_service.as_ref(),
                Path::new(&project.working_directory),
                persisted_workspace,
                observation,
            )
            .await,
        ),
        _ => None,
    };
    if !had_effective_pr {
        let reason = if matches!(
            workspace.publication_push_status.as_deref(),
            Some("failed" | "description_failed")
        ) {
            WORKSPACE_TERMINAL_REASON_PUBLISH_FAILED
        } else {
            WORKSPACE_TERMINAL_REASON_ARCHIVE_ABANDONED
        };
        record_no_pr_terminal_observation_best_effort(
            &state.agent_conversation_workspace_repo,
            &state.task_outcome_repo,
            conversation_id,
            None,
            reason,
            "Workspace session ended without a pull request",
        )
        .await;
    }
    let outcome = terminalize_agent_workspace_after_pr_with_observation(
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
        Some(Arc::clone(&state.plan_branch_repo)),
        Some(chat_service),
        Some(Arc::clone(&state.task_outcome_repo)),
        observation,
        conversation_id,
        &project,
        TerminalAgentWorkspaceCause::ArchivedConversation,
    )
    .await;
    outcome.require_runtime_shutdown()?;

    Ok(outcome)
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

    if target.is_open {
        close_remote_pr(
            state,
            Path::new(&project.working_directory),
            target.number,
            "close_agent_workspace_pr",
        )
        .await?;
        mark_effective_pr_closed(
            conversation_id,
            &workspace,
            linked_plan_branch.as_ref(),
            &target,
            state,
        )
        .await?;
    }

    let chat_service: Arc<dyn ChatService> = Arc::new(state.build_chat_service());
    let terminal_status = if target.is_open {
        "closed"
    } else {
        target.status.as_deref().unwrap_or("closed")
    };
    let summary = if terminal_status == "merged" {
        "Pull request already merged"
    } else {
        "Pull request closed without merging"
    };
    let observation = TerminalPrObservation::new(target.number, terminal_status, summary);
    let observation = if terminal_status == "closed" {
        observation.with_reason(WORKSPACE_TERMINAL_REASON_USER_CLOSED)
    } else {
        observation
    };
    let outcome = terminalize_agent_workspace_after_pr_with_observation(
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
        Some(Arc::clone(&state.plan_branch_repo)),
        Some(chat_service),
        Some(Arc::clone(&state.task_outcome_repo)),
        Some(observation),
        conversation_id,
        &project,
        TerminalAgentWorkspaceCause::ClosedPr,
    )
    .await;
    outcome.require_runtime_shutdown()
}

/// Strictly reconcile the effective remote PR before replacing an implementation attempt.
/// Local pointers remain intact until the replacement transaction commits.
pub(crate) async fn close_agent_workspace_pr_for_restart(
    workspace: &AgentConversationWorkspace,
    linked_plan_branch: &PlanBranch,
    state: &AppState,
) -> crate::error::AppResult<()> {
    let mut pr_numbers = Vec::with_capacity(2);
    if let Some(number) = linked_plan_branch.pr_number {
        pr_numbers.push(number);
    }
    if let Some(number) = workspace.publication_pr_number {
        if !pr_numbers.contains(&number) {
            pr_numbers.push(number);
        }
    }
    if !pr_numbers.is_empty() {
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
                pr_numbers
            ))
        })?;
        for number in pr_numbers {
            let remote_status = github_svc
                .check_pr_status(&project_path, number)
                .await
                .map_err(|error| {
                    crate::error::AppError::Infrastructure(format!(
                        "Restart could not check existing PR {}: {}",
                        number, error
                    ))
                })?;
            let status = match remote_status {
                RemotePrStatus::Open => {
                    github_svc
                        .close_pr(&project_path, number)
                        .await
                        .map_err(|error| {
                            crate::error::AppError::Infrastructure(format!(
                                "Restart could not close existing PR {}: {}",
                                number, error
                            ))
                        })?;
                    "closed"
                }
                RemotePrStatus::Merged { .. } => "merged",
                RemotePrStatus::Closed => "closed",
            };
            let summary = if status == "merged" {
                "Pull request already merged before restart"
            } else {
                "Pull request closed before restart"
            };
            let event = crate::domain::entities::AgentConversationWorkspacePublicationEvent::new(
                workspace.conversation_id.clone(),
                format!("pr_{status}"),
                "succeeded",
                summary,
                None,
            );
            state
                .agent_conversation_workspace_repo
                .append_publication_event(event.clone())
                .await?;
            let observation = TerminalPrObservation::new(number, status, summary);
            let observation = if status == "closed" {
                observation.with_reason(WORKSPACE_TERMINAL_REASON_RESTART_SUPERSEDED)
            } else {
                observation
            };
            record_terminal_pr_observation_best_effort(
                &state.agent_conversation_workspace_repo,
                Some(&state.task_outcome_repo),
                &workspace.conversation_id,
                Some(&observation.with_publication_event(event)),
            )
            .await;
        }
    } else {
        record_no_pr_terminal_observation_best_effort(
            &state.agent_conversation_workspace_repo,
            &state.task_outcome_repo,
            &workspace.conversation_id,
            None,
            WORKSPACE_TERMINAL_REASON_RESTART_SUPERSEDED,
            "Workspace session was superseded before opening a pull request",
        )
        .await;
    }
    Ok(())
}

fn workspace_allows_pr_closure(workspace: &AgentConversationWorkspace) -> bool {
    workspace.mode != AgentConversationWorkspaceMode::ReviewPr
}

async fn stop_conversation_agent(
    context_type: ChatContextType,
    conversation_id: &ChatConversationId,
    state: &AppState,
) -> Result<(), String> {
    let conversation_id = conversation_id.as_str();
    state
        .build_chat_service()
        .stop_agent(context_type, &conversation_id)
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
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
) -> Result<Option<i64>, String> {
    let linked_plan_branch = load_linked_plan_branch_for_pr(workspace, state).await?;
    let Some(target) = resolve_effective_pr(workspace, linked_plan_branch.as_ref()) else {
        return Ok(None);
    };

    if !target.is_open {
        return Ok(None);
    }

    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|error| {
            format!(
                "Failed to load project {} while closing PR: {error}",
                workspace.project_id
            )
        })?
        .ok_or_else(|| {
            format!(
                "Project {} was not found while closing PR",
                workspace.project_id
            )
        })?;
    close_remote_pr(
        state,
        Path::new(&project.working_directory),
        target.number,
        "archive_agent_conversation",
    )
    .await?;

    mark_effective_pr_closed(
        conversation_id,
        workspace,
        linked_plan_branch.as_ref(),
        &target,
        state,
    )
    .await?;
    Ok(Some(target.number))
}

async fn workspace_has_effective_pr(
    workspace: &AgentConversationWorkspace,
    state: &AppState,
) -> Result<bool, String> {
    if workspace.source_pull_request.is_some() {
        return Ok(true);
    }
    let linked_plan_branch = load_linked_plan_branch_for_pr(workspace, state).await?;
    Ok(resolve_effective_pr(workspace, linked_plan_branch.as_ref()).is_some())
}

async fn close_remote_pr(
    state: &AppState,
    working_dir: &Path,
    pr_number: i64,
    operation: &'static str,
) -> Result<(), String> {
    let github_svc = state.github_service.as_ref().ok_or_else(|| {
        format!("{operation}: GitHub integration is unavailable while closing PR #{pr_number}")
    })?;
    github_svc
        .close_pr(working_dir, pr_number)
        .await
        .map_err(|error| format!("{operation}: failed to close remote PR #{pr_number}: {error}"))
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
                status: plan_branch.pr_status.map(|status| match status {
                    PrStatus::Draft => "draft".to_string(),
                    PrStatus::Open => "open".to_string(),
                    PrStatus::Merged => "merged".to_string(),
                    PrStatus::Closed => "closed".to_string(),
                }),
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
            status: workspace.publication_pr_status.clone(),
        })
}

fn is_plan_branch_pr_open(plan_branch: &PlanBranch) -> bool {
    !matches!(
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

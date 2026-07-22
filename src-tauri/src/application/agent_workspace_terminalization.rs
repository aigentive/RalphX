use std::sync::Arc;

use super::agent_workspace_terminal_cleanup::{
    cleanup_terminal_agent_workspace_after_pr, TerminalAgentWorkspaceCause,
    TerminalAgentWorkspaceOutcome,
};
use super::agent_workspace_terminal_observation::{
    record_terminal_pr_observation_best_effort, TerminalPrObservation,
};
use crate::application::chat_service::ChatService;
use crate::application::interactive_notification_producer::pr_review_notification_key;
use crate::application::NotificationService;
use crate::domain::entities::{ChatContextType, ChatConversationId, Project};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, PlanBranchRepository,
    TaskOutcomeRepository,
};

#[cfg(test)]
pub(crate) async fn terminalize_agent_workspace_after_pr(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    chat_service: Option<Arc<dyn ChatService>>,
    conversation_id: &ChatConversationId,
    project: &Project,
    cause: TerminalAgentWorkspaceCause,
) -> TerminalAgentWorkspaceOutcome {
    terminalize_agent_workspace_after_pr_with_observation(
        workspace_repo,
        agent_run_repo,
        plan_branch_repo,
        chat_service,
        None,
        None,
        conversation_id,
        project,
        cause,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn terminalize_agent_workspace_after_pr_with_observation(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    chat_service: Option<Arc<dyn ChatService>>,
    task_outcome_repo: Option<Arc<dyn TaskOutcomeRepository>>,
    observation: Option<TerminalPrObservation>,
    conversation_id: &ChatConversationId,
    project: &Project,
    cause: TerminalAgentWorkspaceCause,
) -> TerminalAgentWorkspaceOutcome {
    record_terminal_pr_observation_best_effort(
        &workspace_repo,
        task_outcome_repo.as_ref(),
        conversation_id,
        observation.as_ref(),
    )
    .await;

    let active_run = match agent_run_repo
        .get_active_for_conversation(conversation_id)
        .await
    {
        Ok(run) => run,
        Err(error) => {
            return TerminalAgentWorkspaceOutcome::runtime_blocked(format!(
                "Failed to inspect the active workspace run: {error}"
            ));
        }
    };

    if active_run.is_some() || chat_service.is_some() {
        let Some(chat_service) = chat_service.as_ref() else {
            return TerminalAgentWorkspaceOutcome::runtime_blocked(
                "An active workspace run could not be stopped because no chat runtime was available"
                    .to_string(),
            );
        };
        if let Err(error) = chat_service
            .stop_agent(ChatContextType::Project, &conversation_id.as_str())
            .await
        {
            return TerminalAgentWorkspaceOutcome::runtime_blocked(format!(
                "Failed to stop the workspace runtime: {error}"
            ));
        }
    }

    if let Some(run) = active_run {
        if let Err(error) = agent_run_repo.fail(&run.id, &cause.stop_reason()).await {
            return TerminalAgentWorkspaceOutcome::runtime_blocked(format!(
                "Failed to persist the terminal workspace run result: {error}"
            ));
        }
    }

    match agent_run_repo
        .get_active_for_conversation(conversation_id)
        .await
    {
        Ok(None) => {}
        Ok(Some(run)) => {
            return TerminalAgentWorkspaceOutcome::runtime_blocked(format!(
                "Workspace run {} remained active after stop",
                run.id.as_str()
            ));
        }
        Err(error) => {
            return TerminalAgentWorkspaceOutcome::runtime_blocked(format!(
                "Failed to verify the stopped workspace runtime: {error}"
            ));
        }
    }

    cleanup_terminal_agent_workspace_after_pr(
        workspace_repo,
        plan_branch_repo,
        conversation_id,
        project,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn settle_review_pr_terminal_observation(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    chat_service: Option<Arc<dyn ChatService>>,
    notification_service: Option<Arc<NotificationService>>,
    task_outcome_repo: Arc<dyn TaskOutcomeRepository>,
    conversation_id: &ChatConversationId,
    project: &Project,
    pr_number: i64,
    status: &str,
    summary: &str,
) -> crate::AppResult<TerminalAgentWorkspaceOutcome> {
    let settlement = workspace_repo
        .settle_pr_review_terminal(conversation_id, pr_number, status, summary)
        .await?;

    if let Some(notification_service) = notification_service {
        for action_id in settlement.superseded_action_ids {
            notification_service
                .resolve_workflow_notification(&pr_review_notification_key(
                    conversation_id.as_str(),
                    &action_id,
                ))
                .await;
        }
    }

    Ok(terminalize_agent_workspace_after_pr_with_observation(
        workspace_repo,
        agent_run_repo,
        plan_branch_repo,
        chat_service,
        Some(task_outcome_repo),
        Some(TerminalPrObservation::new(pr_number, status, summary)),
        conversation_id,
        project,
        TerminalAgentWorkspaceCause::from_pr_status(status),
    )
    .await)
}

pub(crate) async fn cleanup_terminal_agent_workspace_after_pr_with_observation(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    task_outcome_repo: Arc<dyn TaskOutcomeRepository>,
    observation: Option<TerminalPrObservation>,
    conversation_id: &ChatConversationId,
    project: &Project,
) -> TerminalAgentWorkspaceOutcome {
    record_terminal_pr_observation_best_effort(
        &workspace_repo,
        Some(&task_outcome_repo),
        conversation_id,
        observation.as_ref(),
    )
    .await;
    cleanup_terminal_agent_workspace_after_pr(
        workspace_repo,
        plan_branch_repo,
        conversation_id,
        project,
    )
    .await
}

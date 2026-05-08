use std::sync::Arc;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspacePublicationEvent,
};
use crate::domain::repositories::{AgentConversationWorkspaceRepository, AgentRunRepository};
use crate::error::AppResult;

const STALE_REPAIR_RECOVERED_STEP: &str = "stale_repair_recovered";
const STALE_NEEDS_AGENT_CLASSIFICATION: &str = "stale_needs_agent";

pub async fn recover_stale_agent_workspace_publish_repairs(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
) -> AppResult<u32> {
    let workspaces = workspace_repo.list_active_needs_agent_workspaces().await?;
    let mut recovered = 0u32;

    for workspace in workspaces {
        if recover_stale_publish_repair_for_workspace(
            Arc::clone(&workspace_repo),
            Arc::clone(&agent_run_repo),
            workspace,
        )
        .await?
        {
            recovered += 1;
        }
    }

    Ok(recovered)
}

pub async fn recover_stale_publish_repair_for_workspace(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    workspace: AgentConversationWorkspace,
) -> AppResult<bool> {
    if workspace.publication_push_status.as_deref() != Some("needs_agent") {
        return Ok(false);
    }

    if agent_run_repo
        .get_active_for_conversation(&workspace.conversation_id)
        .await?
        .is_some()
    {
        return Ok(false);
    }

    let Some(latest_run) = agent_run_repo
        .get_latest_for_conversation(&workspace.conversation_id)
        .await?
    else {
        return Ok(false);
    };

    if !latest_run.status.is_terminal() {
        return Ok(false);
    }

    let Some(completed_at) = latest_run.completed_at else {
        return Ok(false);
    };
    if completed_at < workspace.updated_at {
        return Ok(false);
    }

    workspace_repo
        .update_publication(
            &workspace.conversation_id,
            workspace.publication_pr_number,
            workspace.publication_pr_url.as_deref(),
            workspace.publication_pr_status.as_deref(),
            Some("failed"),
        )
        .await?;
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            workspace.conversation_id,
            STALE_REPAIR_RECOVERED_STEP,
            "succeeded",
            "Recovered stale publish repair state; no active workspace agent repair is running.",
            Some(STALE_NEEDS_AGENT_CLASSIFICATION.to_string()),
        ))
        .await?;

    tracing::info!(
        conversation_id = latest_run.conversation_id.as_str(),
        agent_run_id = %latest_run.id,
        agent_run_status = %latest_run.status,
        "Recovered stale agent workspace publish repair state"
    );

    Ok(true)
}

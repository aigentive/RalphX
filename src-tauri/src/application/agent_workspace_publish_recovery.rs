use std::sync::Arc;

use crate::application::agent_workspace_publish_repair_state::reconcile_active_agent_workspace_repair;
use crate::application::agent_workspace_review::{
    resolve_review_target, AgentWorkspaceReviewTarget,
};
use crate::application::agent_workspace_review_publish_handoff::{
    has_open_pr_fix_workspace_review_publish_handoff,
    has_pending_pr_fix_workspace_review_publish_handoff,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspacePublicationEvent,
};
use crate::domain::repositories::{AgentConversationWorkspaceRepository, AgentRunRepository};
use crate::error::AppResult;

pub(super) const STALE_REPAIR_RECOVERED_STEP: &str = "stale_repair_recovered";
pub(super) const STALE_NEEDS_AGENT_CLASSIFICATION: &str = "stale_needs_agent";
pub(super) const STALE_PR_AUTOFIX_SUMMARY: &str =
    "Recovered stale PR autofix state; no active fixer run is running.";
pub(super) const STALE_TRANSIENT_RECOVERED_STEP: &str = "stale_transient_recovered";
pub(super) const STALE_TRANSIENT_CLASSIFICATION: &str = "stale_transient_status";
pub const STALE_TRANSIENT_STATUS_STALE_SECS: u64 = 300;

pub async fn recover_stale_agent_workspace_publish_repairs_on_startup(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
) {
    match recover_stale_agent_workspace_publish_repairs(workspace_repo, agent_run_repo).await {
        Ok(count) if count > 0 => {
            tracing::info!(
                count,
                "Recovered stale agent workspace publish repair states on startup"
            );
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(
                error = %error,
                "Failed to recover stale agent workspace publish repair states on startup"
            );
        }
    }
}

pub async fn recover_stale_agent_workspace_publish_repairs_on_startup_for_state(state: &AppState) {
    match recover_stale_agent_workspace_publish_repairs_for_state(state).await {
        Ok(count) if count > 0 => {
            tracing::info!(
                count,
                "Recovered stale agent workspace publish repair states on startup"
            );
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(
                error = %error,
                "Failed to recover stale agent workspace publish repair states on startup"
            );
        }
    }
}

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

pub async fn recover_stale_agent_workspace_publish_repairs_for_state(
    state: &AppState,
) -> AppResult<u32> {
    let workspaces = state
        .agent_conversation_workspace_repo
        .list_active_needs_agent_workspaces()
        .await?;
    let mut recovered = 0u32;

    for workspace in workspaces {
        let (_workspace, was_recovered) =
            recover_stale_publish_repair_for_workspace_in_state_result(state, workspace).await?;
        if was_recovered {
            recovered += 1;
        }
    }

    Ok(recovered)
}

pub async fn recover_stale_publish_repair_for_workspace_in_state(
    state: &crate::application::AppState,
    workspace: AgentConversationWorkspace,
) -> AppResult<AgentConversationWorkspace> {
    recover_stale_publish_repair_for_workspace_in_state_result(state, workspace)
        .await
        .map(|(workspace, _)| workspace)
}

async fn recover_stale_publish_repair_for_workspace_in_state_result(
    state: &AppState,
    workspace: AgentConversationWorkspace,
) -> AppResult<(AgentConversationWorkspace, bool)> {
    let current_review_target =
        current_pr_fix_review_handoff_target_for_state(state, &workspace).await?;
    recover_stale_publish_repair_for_workspace_and_reload_with_review_target(
        Arc::clone(&state.agent_conversation_workspace_repo),
        Arc::clone(&state.agent_run_repo),
        workspace,
        current_review_target.as_ref(),
    )
    .await
}

pub async fn recover_stale_publish_repair_for_workspace_and_reload(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    workspace: AgentConversationWorkspace,
) -> AppResult<AgentConversationWorkspace> {
    let (workspace, _recovered) =
        recover_stale_publish_repair_for_workspace_and_reload_with_review_target(
            workspace_repo,
            agent_run_repo,
            workspace,
            None,
        )
        .await?;
    Ok(workspace)
}

pub(crate) async fn recover_stale_publish_repair_for_workspace_and_reload_with_review_target(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    workspace: AgentConversationWorkspace,
    current_review_target: Option<&AgentWorkspaceReviewTarget>,
) -> AppResult<(AgentConversationWorkspace, bool)> {
    let conversation_id = workspace.conversation_id;
    let recovered = recover_stale_publish_repair_for_workspace_with_review_target(
        Arc::clone(&workspace_repo),
        agent_run_repo,
        workspace.clone(),
        current_review_target,
    )
    .await?;
    if recovered {
        return Ok((
            workspace_repo
                .get_by_conversation_id(&conversation_id)
                .await?
                .unwrap_or(workspace),
            true,
        ));
    }

    Ok((workspace, false))
}

async fn current_pr_fix_review_handoff_target_for_state(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
) -> AppResult<Option<AgentWorkspaceReviewTarget>> {
    let publication_events = state
        .agent_conversation_workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await?;
    if !has_pending_pr_fix_workspace_review_publish_handoff(&publication_events) {
        return Ok(None);
    }

    let Some(project) = state.project_repo.get_by_id(&workspace.project_id).await? else {
        return Ok(None);
    };
    resolve_review_target(workspace, &project).await
}

pub async fn recover_stale_publish_repair_for_workspace(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    workspace: AgentConversationWorkspace,
) -> AppResult<bool> {
    recover_stale_publish_repair_for_workspace_with_review_target(
        workspace_repo,
        agent_run_repo,
        workspace,
        None,
    )
    .await
}

async fn recover_stale_publish_repair_for_workspace_with_review_target(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    workspace: AgentConversationWorkspace,
    current_review_target: Option<&AgentWorkspaceReviewTarget>,
) -> AppResult<bool> {
    if workspace.publication_push_status.as_deref() != Some("needs_agent") {
        return Ok(false);
    }

    if agent_run_repo
        .get_active_for_conversation(&workspace.conversation_id)
        .await?
        .is_some()
    {
        return reconcile_active_agent_workspace_repair(workspace_repo, agent_run_repo, &workspace)
            .await;
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

    let publication_events = workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await?;
    let review_monitor = workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await?;
    if has_open_pr_fix_workspace_review_publish_handoff(
        &publication_events,
        review_monitor.as_ref(),
        current_review_target,
    ) {
        tracing::info!(
            conversation_id = workspace.conversation_id.as_str(),
            agent_run_id = %latest_run.id,
            agent_run_status = %latest_run.status,
            "Skipped stale publish repair recovery while PR fix waits on current Workspace Review"
        );
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
    if workspace.pr_autofix_enabled {
        workspace_repo
            .update_pr_auto_merge_state(
                &workspace.conversation_id,
                workspace.pr_auto_merge_current,
                Some("blocked"),
                Some(STALE_PR_AUTOFIX_SUMMARY),
            )
            .await?;
    }
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

pub async fn recover_stale_transient_publish_statuses(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    stale_older_than_secs: u64,
) -> AppResult<u32> {
    let workspaces = workspace_repo
        .list_active_transient_publish_status_workspaces(stale_older_than_secs)
        .await?;
    let mut recovered = 0u32;

    for workspace in workspaces {
        let stuck_status = workspace
            .publication_push_status
            .clone()
            .unwrap_or_default();
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
                workspace.conversation_id.clone(),
                STALE_TRANSIENT_RECOVERED_STEP,
                "succeeded",
                format!(
                    "Recovered stale transient publish status '{stuck_status}'; no agent is actively progressing it."
                ),
                Some(STALE_TRANSIENT_CLASSIFICATION.to_string()),
            ))
            .await?;
        tracing::info!(
            conversation_id = workspace.conversation_id.as_str(),
            stuck_status = %stuck_status,
            "Recovered stale transient publish status workspace"
        );
        recovered += 1;
    }

    Ok(recovered)
}

pub async fn run_periodic_workspace_publish_recovery(state: AppState) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(120)).await;

        if let Err(err) = recover_stale_agent_workspace_publish_repairs_for_state(&state).await {
            tracing::warn!(
                error = %err,
                "Periodic recovery: failed to recover stale needs_agent workspace repairs"
            );
        }

        if let Err(err) = recover_stale_transient_publish_statuses(
            Arc::clone(&state.agent_conversation_workspace_repo),
            STALE_TRANSIENT_STATUS_STALE_SECS,
        )
        .await
        {
            tracing::warn!(
                error = %err,
                "Periodic recovery: failed to recover stale transient publish statuses"
            );
        }

        if let Err(err) = crate::application::agent_workspace_review_auto_merge::
            reconcile_workspace_review_auto_merge_guards(&state)
            .await
        {
            tracing::warn!(
                error = %err,
                "Periodic recovery: failed to reconcile workspace Review auto-merge guards"
            );
        }
    }
}

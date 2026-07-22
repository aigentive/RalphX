use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspacePublicationEvent, AgentRun,
    ChatConversationId,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, AgentWorkspaceRepairStateGuard,
    AgentWorkspaceRepairStateTransition,
};
use crate::error::AppResult;

pub(crate) const REPAIR_REQUESTED_STEP: &str = "repair_requested";
pub(crate) const REPAIR_DEFERRED_STEP: &str = "repair_deferred";
pub(crate) const REPAIR_SENT_STEP: &str = "repair_sent";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentWorkspaceRepairClaim {
    pub conversation_id: ChatConversationId,
    pub guard: AgentWorkspaceRepairStateGuard,
}

fn next_transition_at(previous: Option<DateTime<Utc>>) -> DateTime<Utc> {
    let now = Utc::now();
    match previous {
        Some(previous) if now <= previous => previous + Duration::nanoseconds(1),
        _ => now,
    }
}

fn transition_guard(
    transition: &AgentWorkspaceRepairStateTransition,
) -> AgentWorkspaceRepairStateGuard {
    AgentWorkspaceRepairStateGuard {
        publication_push_status: transition.publication_push_status.clone(),
        pr_supervision_status: transition.pr_supervision_status.clone(),
        pr_supervision_updated_at: Some(transition.pr_supervision_updated_at),
    }
}

pub(crate) async fn claim_agent_workspace_repair(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: &ChatConversationId,
    summary: &str,
    auto_merge_current: Option<bool>,
) -> AppResult<Option<AgentWorkspaceRepairClaim>> {
    let Some(workspace) = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
    else {
        return Ok(None);
    };
    if workspace.publication_push_status.as_deref() == Some("needs_agent")
        && workspace.pr_supervision_status.as_deref() == Some("fixing")
    {
        return Ok(None);
    }

    let expected = AgentWorkspaceRepairStateGuard::from_workspace(&workspace);
    let transition = AgentWorkspaceRepairStateTransition {
        publication_push_status: Some("needs_agent".to_string()),
        pr_supervision_status: Some("fixing".to_string()),
        pr_supervision_summary: Some(summary.to_string()),
        pr_supervision_updated_at: next_transition_at(workspace.pr_supervision_updated_at),
        pr_auto_merge_current: auto_merge_current,
        base_commit: None,
    };
    if !workspace_repo
        .compare_and_set_repair_state(conversation_id, &expected, &transition)
        .await?
    {
        return Ok(None);
    }

    Ok(Some(AgentWorkspaceRepairClaim {
        conversation_id: conversation_id.clone(),
        guard: transition_guard(&transition),
    }))
}

pub(crate) async fn settle_agent_workspace_repair_failure(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    claim: &AgentWorkspaceRepairClaim,
    summary: &str,
) -> AppResult<bool> {
    let transition = AgentWorkspaceRepairStateTransition {
        publication_push_status: Some("needs_agent".to_string()),
        pr_supervision_status: Some("blocked".to_string()),
        pr_supervision_summary: Some(summary.to_string()),
        pr_supervision_updated_at: next_transition_at(claim.guard.pr_supervision_updated_at),
        pr_auto_merge_current: None,
        base_commit: None,
    };
    workspace_repo
        .compare_and_set_repair_state(&claim.conversation_id, &claim.guard, &transition)
        .await
}

pub(crate) async fn settle_agent_workspace_failure_without_repair(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: &ChatConversationId,
    expected_push_status: &str,
    summary: &str,
) -> AppResult<bool> {
    let Some(workspace) = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
    else {
        return Ok(false);
    };
    if workspace.publication_push_status.as_deref() != Some(expected_push_status)
        || workspace.pr_supervision_status.as_deref() == Some("fixing")
    {
        return Ok(false);
    }
    let transition = AgentWorkspaceRepairStateTransition {
        publication_push_status: Some(expected_push_status.to_string()),
        pr_supervision_status: Some("blocked".to_string()),
        pr_supervision_summary: Some(summary.to_string()),
        pr_supervision_updated_at: next_transition_at(workspace.pr_supervision_updated_at),
        pr_auto_merge_current: None,
        base_commit: None,
    };
    workspace_repo
        .compare_and_set_repair_state(
            conversation_id,
            &AgentWorkspaceRepairStateGuard::from_workspace(&workspace),
            &transition,
        )
        .await
}

fn latest_repair_event(
    events: &[AgentConversationWorkspacePublicationEvent],
) -> Option<&AgentConversationWorkspacePublicationEvent> {
    events
        .iter()
        .filter(|event| {
            matches!(
                event.step.as_str(),
                REPAIR_REQUESTED_STEP | REPAIR_DEFERRED_STEP | REPAIR_SENT_STEP
            )
        })
        .max_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        })
}

pub(crate) fn repair_event_authorizes_active_run(
    events: &[AgentConversationWorkspacePublicationEvent],
    active_run: &AgentRun,
) -> bool {
    let Some(event) = latest_repair_event(events) else {
        return false;
    };
    if event.created_at < active_run.started_at {
        return false;
    }
    match event.step.as_str() {
        REPAIR_REQUESTED_STEP | REPAIR_DEFERRED_STEP => {
            matches!(event.status.as_str(), "started" | "succeeded")
        }
        REPAIR_SENT_STEP => event.status == "succeeded",
        _ => false,
    }
}

fn successful_send_authorizes_completion(
    events: &[AgentConversationWorkspacePublicationEvent],
    active_run: &AgentRun,
) -> bool {
    latest_repair_event(events).is_some_and(|event| {
        event.step == REPAIR_SENT_STEP
            && event.status == "succeeded"
            && event.created_at >= active_run.started_at
    })
}

pub(crate) async fn reconcile_active_agent_workspace_repair(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    workspace: &AgentConversationWorkspace,
) -> AppResult<bool> {
    if workspace.publication_push_status.as_deref() != Some("needs_agent")
        || workspace.pr_supervision_status.as_deref() != Some("blocked")
    {
        return Ok(false);
    }
    let Some(active_run) = agent_run_repo
        .get_active_for_conversation(&workspace.conversation_id)
        .await?
    else {
        return Ok(false);
    };
    let events = workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await?;
    if !repair_event_authorizes_active_run(&events, &active_run) {
        return Ok(false);
    }

    let transition = AgentWorkspaceRepairStateTransition {
        publication_push_status: Some("needs_agent".to_string()),
        pr_supervision_status: Some("fixing".to_string()),
        pr_supervision_summary: Some("Agent workspace repair is in progress.".to_string()),
        pr_supervision_updated_at: next_transition_at(workspace.pr_supervision_updated_at),
        pr_auto_merge_current: None,
        base_commit: None,
    };
    workspace_repo
        .compare_and_set_repair_state(
            &workspace.conversation_id,
            &AgentWorkspaceRepairStateGuard::from_workspace(workspace),
            &transition,
        )
        .await
}

pub(crate) async fn current_agent_workspace_repair_claim_for_completion(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    workspace: &AgentConversationWorkspace,
) -> AppResult<Option<AgentWorkspaceRepairClaim>> {
    if workspace.publication_push_status.as_deref() != Some("needs_agent")
        || workspace.pr_supervision_status.as_deref() != Some("fixing")
    {
        return Ok(None);
    }
    let Some(active_run) = agent_run_repo
        .get_active_for_conversation(&workspace.conversation_id)
        .await?
    else {
        return Ok(None);
    };
    let events = workspace_repo
        .list_publication_events(&workspace.conversation_id)
        .await?;
    if !successful_send_authorizes_completion(&events, &active_run) {
        return Ok(None);
    }
    Ok(Some(AgentWorkspaceRepairClaim {
        conversation_id: workspace.conversation_id.clone(),
        guard: AgentWorkspaceRepairStateGuard::from_workspace(workspace),
    }))
}

pub(crate) async fn complete_agent_workspace_repair_claim(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    claim: &AgentWorkspaceRepairClaim,
    base_commit: &str,
    supervision_status: Option<&str>,
    supervision_summary: Option<&str>,
) -> AppResult<bool> {
    let transition = AgentWorkspaceRepairStateTransition {
        publication_push_status: Some("refreshed".to_string()),
        pr_supervision_status: supervision_status.map(str::to_string),
        pr_supervision_summary: supervision_summary.map(str::to_string),
        pr_supervision_updated_at: next_transition_at(claim.guard.pr_supervision_updated_at),
        pr_auto_merge_current: None,
        base_commit: Some(base_commit.to_string()),
    };
    workspace_repo
        .compare_and_set_repair_state(&claim.conversation_id, &claim.guard, &transition)
        .await
}

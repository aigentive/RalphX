use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspacePublicationEvent, AgentRun, AgentRunId,
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
pub(crate) const PR_AUTOFIX_COMPLETED_STEP: &str = "pr_autofix_completed";
pub(crate) const PR_AUTOFIX_BLOCKED_STEP: &str = "pr_autofix_blocked";
pub(crate) const PR_AUTOFIX_WORKSPACE_REVIEW_STEP: &str = "pr_autofix_workspace_review";
pub(crate) const PR_AUTOFIX_WORKSPACE_REVIEW_ABORTED_STEP: &str =
    "pr_autofix_workspace_review_aborted";
pub(crate) const PR_AUTOFIX_WORKSPACE_REVIEW_PASSED_STEP: &str =
    "pr_autofix_workspace_review_passed";
pub(crate) const DEFERRED_REPAIR_WAIT_TIMEOUT_SECS: u64 = 300;
const REPAIR_RUN_CLASSIFICATION_PREFIX: &str = "agent_fixable:run:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentWorkspaceRepairClaim {
    pub conversation_id: ChatConversationId,
    pub guard: AgentWorkspaceRepairStateGuard,
}

pub(crate) fn repair_run_event_classification(run_id: &AgentRunId) -> String {
    format!("{REPAIR_RUN_CLASSIFICATION_PREFIX}{}", run_id.as_str())
}

fn repair_event_run_id(event: &AgentConversationWorkspacePublicationEvent) -> Option<&str> {
    event
        .classification
        .as_deref()?
        .strip_prefix(REPAIR_RUN_CLASSIFICATION_PREFIX)
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

pub(crate) async fn restore_refreshed_agent_workspace_pr_fix_claim(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    workspace: &AgentConversationWorkspace,
) -> AppResult<Option<AgentWorkspaceRepairClaim>> {
    if workspace.publication_push_status.as_deref() != Some("refreshed")
        || workspace.pr_supervision_status.as_deref() != Some("fixing")
    {
        return Ok(None);
    }
    let transition = AgentWorkspaceRepairStateTransition {
        publication_push_status: Some("needs_agent".to_string()),
        pr_supervision_status: Some("fixing".to_string()),
        pr_supervision_summary: workspace.pr_supervision_summary.clone(),
        pr_supervision_updated_at: next_transition_at(workspace.pr_supervision_updated_at),
        pr_auto_merge_current: None,
        base_commit: None,
    };
    if !workspace_repo
        .compare_and_set_repair_state(
            &workspace.conversation_id,
            &AgentWorkspaceRepairStateGuard::from_workspace(workspace),
            &transition,
        )
        .await?
    {
        return Ok(None);
    }
    Ok(Some(AgentWorkspaceRepairClaim {
        conversation_id: workspace.conversation_id.clone(),
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
    match event.step.as_str() {
        REPAIR_REQUESTED_STEP | REPAIR_DEFERRED_STEP => {
            event.created_at >= active_run.started_at
                && matches!(event.status.as_str(), "started" | "succeeded")
        }
        REPAIR_SENT_STEP => {
            if let Some(run_id) = repair_event_run_id(event) {
                run_id == active_run.id.as_str()
                    && matches!(event.status.as_str(), "started" | "succeeded")
            } else {
                event.status == "succeeded" && event.created_at >= active_run.started_at
            }
        }
        _ => false,
    }
}

fn repair_sent_event_authorizes_run(
    event: &AgentConversationWorkspacePublicationEvent,
    active_run: &AgentRun,
    claim_started_at: DateTime<Utc>,
) -> bool {
    if event.step != REPAIR_SENT_STEP
        || !matches!(event.status.as_str(), "started" | "succeeded")
        || event.created_at < claim_started_at
    {
        return false;
    }
    if let Some(run_id) = repair_event_run_id(event) {
        return run_id == active_run.id.as_str();
    }

    event.status == "succeeded" && event.created_at >= active_run.started_at
}

fn successful_send_authorizes_completion(
    events: &[AgentConversationWorkspacePublicationEvent],
    active_run: &AgentRun,
    claim_started_at: DateTime<Utc>,
) -> bool {
    latest_repair_event(events)
        .is_some_and(|event| repair_sent_event_authorizes_run(event, active_run, claim_started_at))
}

pub(crate) fn terminal_run_authorizes_repair_recovery(
    workspace: &AgentConversationWorkspace,
    events: &[AgentConversationWorkspacePublicationEvent],
    terminal_run: &AgentRun,
) -> bool {
    let claim_started_at = workspace
        .pr_supervision_updated_at
        .unwrap_or(workspace.updated_at);
    let Some(event) = latest_repair_event(events) else {
        return terminal_run.started_at >= claim_started_at;
    };
    if event.created_at < claim_started_at {
        return false;
    }

    match event.step.as_str() {
        REPAIR_SENT_STEP => repair_sent_event_authorizes_run(event, terminal_run, claim_started_at),
        REPAIR_REQUESTED_STEP => terminal_run.started_at >= event.created_at,
        REPAIR_DEFERRED_STEP => {
            Utc::now().signed_duration_since(event.created_at)
                >= Duration::seconds(DEFERRED_REPAIR_WAIT_TIMEOUT_SECS as i64)
                && terminal_run
                    .completed_at
                    .is_some_and(|completed_at| event.created_at <= completed_at)
        }
        _ => false,
    }
}

pub(crate) async fn settle_terminal_agent_workspace_repair(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    workspace: &AgentConversationWorkspace,
    summary: &str,
) -> AppResult<bool> {
    if workspace.publication_push_status.as_deref() != Some("needs_agent") {
        return Ok(false);
    }
    let transition = AgentWorkspaceRepairStateTransition {
        publication_push_status: Some("failed".to_string()),
        pr_supervision_status: Some("blocked".to_string()),
        pr_supervision_summary: Some(summary.to_string()),
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
    let Some(claim_started_at) = workspace.pr_supervision_updated_at else {
        return Ok(None);
    };
    if !successful_send_authorizes_completion(&events, &active_run, claim_started_at) {
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

async fn transition_agent_workspace_repair_claim_with_events(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    claim: &AgentWorkspaceRepairClaim,
    publication_push_status: &str,
    pr_supervision_status: &str,
    pr_supervision_summary: &str,
    events: Vec<AgentConversationWorkspacePublicationEvent>,
) -> AppResult<Option<AgentWorkspaceRepairClaim>> {
    let transition = AgentWorkspaceRepairStateTransition {
        publication_push_status: Some(publication_push_status.to_string()),
        pr_supervision_status: Some(pr_supervision_status.to_string()),
        pr_supervision_summary: Some(pr_supervision_summary.to_string()),
        pr_supervision_updated_at: next_transition_at(claim.guard.pr_supervision_updated_at),
        pr_auto_merge_current: None,
        base_commit: None,
    };
    if !workspace_repo
        .compare_and_set_repair_state_with_events(
            &claim.conversation_id,
            &claim.guard,
            &transition,
            events,
        )
        .await?
    {
        return Ok(None);
    }
    Ok(Some(AgentWorkspaceRepairClaim {
        conversation_id: claim.conversation_id.clone(),
        guard: transition_guard(&transition),
    }))
}

pub(crate) async fn complete_agent_workspace_pr_fix_claim(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    claim: &AgentWorkspaceRepairClaim,
    summary: &str,
    workspace_review_required: bool,
    auto_publish_enabled: bool,
) -> AppResult<Option<AgentWorkspaceRepairClaim>> {
    let (supervision_status, supervision_summary) = if workspace_review_required {
        (
            "reviewing",
            "PR fix verified; Workspace Review must finish before publishing resumes.",
        )
    } else if auto_publish_enabled {
        ("publishing", "PR fix verified; publishing updates.")
    } else {
        ("paused", "PR fix verified; Auto Publish is paused.")
    };
    let mut events = vec![AgentConversationWorkspacePublicationEvent::new(
        claim.conversation_id.clone(),
        PR_AUTOFIX_COMPLETED_STEP,
        "succeeded",
        summary,
        Some(PR_AUTOFIX_COMPLETED_STEP.to_string()),
    )];
    if workspace_review_required {
        events.push(AgentConversationWorkspacePublicationEvent::new(
            claim.conversation_id.clone(),
            PR_AUTOFIX_WORKSPACE_REVIEW_STEP,
            "pending",
            format!("PR fix verified; Workspace Review handoff is pending. Fix summary: {summary}"),
            Some("workspace_review_pending".to_string()),
        ));
    } else if !auto_publish_enabled {
        events.push(AgentConversationWorkspacePublicationEvent::new(
            claim.conversation_id.clone(),
            "pr_autofix_publish_skipped",
            "skipped",
            format!("PR fix completed, but Auto Publish is paused. Fix summary: {summary}"),
            Some("auto_publish_paused".to_string()),
        ));
    }
    transition_agent_workspace_repair_claim_with_events(
        workspace_repo,
        claim,
        "refreshed",
        supervision_status,
        supervision_summary,
        events,
    )
    .await
}

pub(crate) async fn block_agent_workspace_pr_fix_claim(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    claim: &AgentWorkspaceRepairClaim,
    blocker: &str,
) -> AppResult<Option<AgentWorkspaceRepairClaim>> {
    transition_agent_workspace_repair_claim_with_events(
        workspace_repo,
        claim,
        "failed",
        "blocked",
        blocker,
        vec![AgentConversationWorkspacePublicationEvent::new(
            claim.conversation_id.clone(),
            PR_AUTOFIX_BLOCKED_STEP,
            "blocked",
            blocker,
            Some("pr_autofix_blocker".to_string()),
        )],
    )
    .await
}

pub(crate) async fn abort_agent_workspace_pr_fix_review_handoff(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    claim: &AgentWorkspaceRepairClaim,
    blocker: &str,
) -> AppResult<Option<AgentWorkspaceRepairClaim>> {
    transition_agent_workspace_repair_claim_with_events(
        workspace_repo,
        claim,
        "failed",
        "blocked",
        blocker,
        vec![AgentConversationWorkspacePublicationEvent::new(
            claim.conversation_id.clone(),
            PR_AUTOFIX_WORKSPACE_REVIEW_ABORTED_STEP,
            "failed",
            blocker,
            Some("workspace_review_aborted".to_string()),
        )],
    )
    .await
}

pub(crate) async fn continue_agent_workspace_pr_fix_after_review_handoff(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    claim: &AgentWorkspaceRepairClaim,
    summary: &str,
) -> AppResult<Option<AgentWorkspaceRepairClaim>> {
    transition_agent_workspace_repair_claim_with_events(
        workspace_repo,
        claim,
        "refreshed",
        "publishing",
        "Workspace Review handoff settled; publishing PR fix updates.",
        vec![AgentConversationWorkspacePublicationEvent::new(
            claim.conversation_id.clone(),
            PR_AUTOFIX_WORKSPACE_REVIEW_PASSED_STEP,
            "publishing",
            summary,
            Some("workspace_review_not_required".to_string()),
        )],
    )
    .await
}

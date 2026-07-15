use std::future::Future;
use std::sync::Arc;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspacePublicationEvent,
    AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitor, AgentWorkspaceReviewMonitorStatus,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::error::AppResult;

use super::agent_workspace_review::AgentWorkspaceReviewTarget;

const PR_AUTOFIX_WORKSPACE_REVIEW_STEP: &str = "pr_autofix_workspace_review";
const PR_AUTOFIX_WORKSPACE_REVIEW_PASSED_STEP: &str = "pr_autofix_workspace_review_passed";
const PR_AUTOFIX_PUBLISH_FAILED_STEP: &str = "pr_autofix_publish_failed";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PrFixReviewPublishResumeOutcome {
    Skipped,
    Published,
    Failed { error: String },
}

pub(crate) fn has_pending_pr_fix_workspace_review_publish_handoff(
    events: &[AgentConversationWorkspacePublicationEvent],
) -> bool {
    let mut pending = false;
    for event in events {
        match event.step.as_str() {
            PR_AUTOFIX_WORKSPACE_REVIEW_STEP => {
                pending = event.status == "reviewing"
                    && matches!(
                        event.classification.as_deref(),
                        Some("workspace_review_started" | "workspace_reviewing")
                    );
            }
            PR_AUTOFIX_WORKSPACE_REVIEW_PASSED_STEP | PR_AUTOFIX_PUBLISH_FAILED_STEP => {
                pending = false;
            }
            "published" if event.status == "succeeded" => {
                pending = false;
            }
            "failed" if event.status == "failed" => {
                pending = false;
            }
            _ => {}
        }
    }
    pending
}

fn workspace_review_monitor_current_target_matches(
    monitor: &AgentWorkspaceReviewMonitor,
    target: &AgentWorkspaceReviewTarget,
) -> bool {
    if monitor.current_target_scope != Some(target.scope)
        || monitor.current_diff_fingerprint.as_deref() != Some(target.diff_fingerprint.as_str())
    {
        return false;
    }

    match target.scope {
        crate::domain::entities::AgentWorkspaceReviewTargetScope::SelectedSource => {
            monitor.selected_source_head_sha.as_deref() == target.head_sha.as_deref()
        }
        crate::domain::entities::AgentWorkspaceReviewTargetScope::WorkspaceDelta => target
            .head_sha
            .as_deref()
            .is_none_or(|head_sha| monitor.workspace_head_sha.as_deref() == Some(head_sha)),
    }
}

fn workspace_review_monitor_has_current_passing_review(
    monitor: &AgentWorkspaceReviewMonitor,
    target: &AgentWorkspaceReviewTarget,
) -> bool {
    monitor.review_gate_status == AgentWorkspaceReviewGateStatus::Passed
        && workspace_review_monitor_current_target_matches(monitor, target)
        && monitor.has_current_passing_review_for_target(
            target.scope,
            target.head_sha.as_deref(),
            &target.diff_fingerprint,
        )
}

pub(crate) fn workspace_review_monitor_keeps_pr_fix_publish_handoff(
    monitor: Option<&AgentWorkspaceReviewMonitor>,
    current_target: Option<&AgentWorkspaceReviewTarget>,
) -> bool {
    let (Some(monitor), Some(target)) = (monitor, current_target) else {
        return false;
    };

    match monitor.review_gate_status {
        AgentWorkspaceReviewGateStatus::Reviewing => {
            monitor.status == AgentWorkspaceReviewMonitorStatus::Reviewing
                && workspace_review_monitor_current_target_matches(monitor, target)
        }
        AgentWorkspaceReviewGateStatus::Passed => {
            workspace_review_monitor_has_current_passing_review(monitor, target)
        }
        _ => false,
    }
}

pub(crate) fn has_open_pr_fix_workspace_review_publish_handoff(
    events: &[AgentConversationWorkspacePublicationEvent],
    monitor: Option<&AgentWorkspaceReviewMonitor>,
    current_target: Option<&AgentWorkspaceReviewTarget>,
) -> bool {
    has_pending_pr_fix_workspace_review_publish_handoff(events)
        && workspace_review_monitor_keeps_pr_fix_publish_handoff(monitor, current_target)
}

pub(crate) fn pr_fix_publish_can_resume_after_workspace_review(
    workspace: &AgentConversationWorkspace,
    monitor: &AgentWorkspaceReviewMonitor,
    current_target: Option<&AgentWorkspaceReviewTarget>,
    publication_events: &[AgentConversationWorkspacePublicationEvent],
) -> bool {
    let Some(current_target) = current_target else {
        return false;
    };

    workspace_review_monitor_has_current_passing_review(monitor, current_target)
        && workspace.auto_publish_enabled
        && workspace.publication_pr_number.is_some()
        && !workspace.has_terminal_publication_pr_status()
        && (workspace.pr_autofix_enabled
            || workspace.pr_auto_merge_desired
            || workspace.pr_auto_merge_current.is_some())
        && match workspace.pr_supervision_status.as_deref() {
            Some("reviewing") => true,
            Some("blocked") => {
                pr_supervision_block_is_workspace_review_gate(workspace)
                    || has_pending_pr_fix_workspace_review_publish_handoff(publication_events)
            }
            _ => false,
        }
}

pub(crate) fn pr_supervision_block_is_workspace_review_gate(
    workspace: &AgentConversationWorkspace,
) -> bool {
    let Some(summary) = workspace.pr_supervision_summary.as_deref() else {
        return false;
    };
    let summary = summary.trim();
    let summary = summary
        .strip_prefix("PR fix publish failed: ")
        .unwrap_or(summary);
    summary == "Workspace Review is required before publishing"
        || summary == "Workspace Review is still running"
        || summary == "Workspace Review failed"
        || summary == "Workspace Review failed; retry before publishing"
        || summary == "Workspace reviewer completed without writing a current Review"
}

pub(crate) async fn resume_pr_fix_publish_after_passed_workspace_review<P, F>(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    conversation_id: &crate::domain::entities::ChatConversationId,
    workspace: &AgentConversationWorkspace,
    monitor: &AgentWorkspaceReviewMonitor,
    current_target: Option<&AgentWorkspaceReviewTarget>,
    publish_workspace: P,
) -> AppResult<PrFixReviewPublishResumeOutcome>
where
    P: FnOnce(crate::domain::entities::ChatConversationId) -> F,
    F: Future<Output = Result<Option<bool>, String>>,
{
    let publication_events = workspace_repo
        .list_publication_events(conversation_id)
        .await?;
    if !pr_fix_publish_can_resume_after_workspace_review(
        workspace,
        monitor,
        current_target,
        &publication_events,
    ) {
        return Ok(PrFixReviewPublishResumeOutcome::Skipped);
    }

    let publishing_message = "Workspace Review passed; publishing PR fix updates.";
    workspace_repo
        .update_pr_auto_merge_state(
            conversation_id,
            workspace.pr_auto_merge_current,
            Some("publishing"),
            Some(publishing_message),
        )
        .await?;
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            PR_AUTOFIX_WORKSPACE_REVIEW_PASSED_STEP,
            "publishing",
            publishing_message,
            Some("workspace_review_passed".to_string()),
        ))
        .await?;

    match publish_workspace(conversation_id.clone()).await {
        Ok(pr_auto_merge_current) => {
            workspace_repo
                .update_pr_auto_merge_state(
                    conversation_id,
                    pr_auto_merge_current,
                    Some("monitoring"),
                    Some(
                        "Workspace Review passed and PR fix published; RalphX is monitoring the pull request.",
                    ),
                )
                .await?;
            Ok(PrFixReviewPublishResumeOutcome::Published)
        }
        Err(error) => {
            workspace_repo
                .update_pr_auto_merge_state(
                    conversation_id,
                    workspace.pr_auto_merge_current,
                    Some("blocked"),
                    Some(&format!(
                        "Workspace Review passed, but PR fix publish failed: {error}"
                    )),
                )
                .await?;
            workspace_repo
                .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
                    conversation_id.clone(),
                    PR_AUTOFIX_PUBLISH_FAILED_STEP,
                    "failed",
                    error.clone(),
                    Some(PR_AUTOFIX_PUBLISH_FAILED_STEP.to_string()),
                ))
                .await?;
            Ok(PrFixReviewPublishResumeOutcome::Failed { error })
        }
    }
}

use std::future::Future;
use std::sync::Arc;

#[cfg(any(test, feature = "test-utils"))]
use crate::domain::entities::AgentWorkspaceReviewMonitorStatus;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspacePublicationEvent,
    AgentWorkspaceReviewGateStatus, AgentWorkspaceReviewMonitor,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::error::AppResult;

use super::agent_workspace_review::{
    workspace_review_mode_is_eligible, AgentWorkspaceReviewTarget,
};

const PR_AUTOFIX_WORKSPACE_REVIEW_STEP: &str = "pr_autofix_workspace_review";
const PR_AUTOFIX_WORKSPACE_REVIEW_PASSED_STEP: &str = "pr_autofix_workspace_review_passed";
const PR_AUTOFIX_WORKSPACE_REVIEW_ABORTED_STEP: &str = "pr_autofix_workspace_review_aborted";
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
                pending = matches!(event.status.as_str(), "pending" | "reviewing")
                    && matches!(
                        event.classification.as_deref(),
                        Some(
                            "workspace_review_pending"
                                | "workspace_review_started"
                                | "workspace_reviewing"
                        )
                    );
            }
            PR_AUTOFIX_WORKSPACE_REVIEW_PASSED_STEP
            | PR_AUTOFIX_WORKSPACE_REVIEW_ABORTED_STEP
            | PR_AUTOFIX_PUBLISH_FAILED_STEP => {
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

fn workspace_review_monitor_has_current_publish_authorization(
    monitor: &AgentWorkspaceReviewMonitor,
    target: &AgentWorkspaceReviewTarget,
) -> bool {
    workspace_review_authorization_kind(monitor, target).is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkspaceReviewAuthorizationKind {
    ReviewerPassed,
    HumanBypass,
}

pub(crate) fn workspace_review_authorization_kind(
    monitor: &AgentWorkspaceReviewMonitor,
    target: &AgentWorkspaceReviewTarget,
) -> Option<WorkspaceReviewAuthorizationKind> {
    if monitor.review_gate_status != AgentWorkspaceReviewGateStatus::Passed
        || !workspace_review_monitor_current_target_matches(monitor, target)
    {
        return None;
    }
    if monitor.has_current_review_bypass_for_target(
        target.scope,
        target.head_sha.as_deref(),
        &target.diff_fingerprint,
    ) {
        return Some(WorkspaceReviewAuthorizationKind::HumanBypass);
    }
    monitor
        .has_current_passing_review_for_target(
            target.scope,
            target.head_sha.as_deref(),
            &target.diff_fingerprint,
        )
        .then_some(WorkspaceReviewAuthorizationKind::ReviewerPassed)
}

impl WorkspaceReviewAuthorizationKind {
    pub(crate) fn publishing_message(self, subject: &str) -> String {
        match self {
            Self::ReviewerPassed => format!("Workspace Review passed; publishing {subject}."),
            Self::HumanBypass => {
                format!("Workspace Review approved anyway; publishing {subject}.")
            }
        }
    }

    pub(crate) fn classification(self) -> String {
        match self {
            Self::ReviewerPassed => "workspace_review_passed",
            Self::HumanBypass => "workspace_review_approved_anyway",
        }
        .to_string()
    }
}

#[cfg(any(test, feature = "test-utils"))]
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
            workspace_review_monitor_has_current_publish_authorization(monitor, target)
        }
        _ => false,
    }
}

#[cfg(any(test, feature = "test-utils"))]
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
    if !workspace_review_mode_is_eligible(workspace.mode) {
        return false;
    }
    let Some(current_target) = current_target else {
        return false;
    };

    workspace_review_monitor_has_current_publish_authorization(monitor, current_target)
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
            Some("review_paused") => {
                workspace.pr_supervision_summary.as_deref()
                    == Some(
                        crate::application::agent_workspace_review_auto_merge::REVIEW_AUTO_MERGE_PAUSED_SUMMARY,
                    )
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
    _workspace: &AgentConversationWorkspace,
    monitor: &AgentWorkspaceReviewMonitor,
    current_target: Option<&AgentWorkspaceReviewTarget>,
    publish_workspace: P,
) -> AppResult<PrFixReviewPublishResumeOutcome>
where
    P: FnOnce(crate::domain::entities::ChatConversationId) -> F,
    F: Future<Output = Result<Option<bool>, String>>,
{
    let Some(current_workspace) = workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
    else {
        return Ok(PrFixReviewPublishResumeOutcome::Skipped);
    };
    if !workspace_review_mode_is_eligible(current_workspace.mode) {
        return Ok(PrFixReviewPublishResumeOutcome::Skipped);
    }
    let publication_events = workspace_repo
        .list_publication_events(conversation_id)
        .await?;
    if !pr_fix_publish_can_resume_after_workspace_review(
        &current_workspace,
        monitor,
        current_target,
        &publication_events,
    ) {
        return Ok(PrFixReviewPublishResumeOutcome::Skipped);
    }

    let Some(authorization_kind) =
        current_target.and_then(|target| workspace_review_authorization_kind(monitor, target))
    else {
        return Ok(PrFixReviewPublishResumeOutcome::Skipped);
    };
    let publishing_message = authorization_kind.publishing_message("PR fix updates");
    workspace_repo
        .update_pr_auto_merge_state(
            conversation_id,
            current_workspace.pr_auto_merge_current,
            Some("publishing"),
            Some(publishing_message.as_str()),
        )
        .await?;
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            PR_AUTOFIX_WORKSPACE_REVIEW_PASSED_STEP,
            "publishing",
            publishing_message,
            Some(authorization_kind.classification()),
        ))
        .await?;

    match publish_workspace(conversation_id.clone()).await {
        Ok(pr_auto_merge_current) => {
            let monitoring_message = format!(
                "{} and PR fix published; RalphX is monitoring the pull request.",
                match authorization_kind {
                    WorkspaceReviewAuthorizationKind::ReviewerPassed => "Workspace Review passed",
                    WorkspaceReviewAuthorizationKind::HumanBypass => {
                        "Workspace Review was approved anyway"
                    }
                }
            );
            workspace_repo
                .update_pr_auto_merge_state(
                    conversation_id,
                    pr_auto_merge_current,
                    Some("monitoring"),
                    Some(monitoring_message.as_str()),
                )
                .await?;
            Ok(PrFixReviewPublishResumeOutcome::Published)
        }
        Err(error) => {
            let failure_message = format!(
                "{}, but PR fix publish failed: {error}",
                match authorization_kind {
                    WorkspaceReviewAuthorizationKind::ReviewerPassed => "Workspace Review passed",
                    WorkspaceReviewAuthorizationKind::HumanBypass => {
                        "Workspace Review was approved anyway"
                    }
                }
            );
            workspace_repo
                .update_pr_auto_merge_state(
                    conversation_id,
                    current_workspace.pr_auto_merge_current,
                    Some("blocked"),
                    Some(failure_message.as_str()),
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

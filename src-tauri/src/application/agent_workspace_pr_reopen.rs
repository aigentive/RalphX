use std::path::Path;
use std::sync::Arc;

use serde::Serialize;

use crate::application::agent_conversation_archive::{
    load_linked_plan_branch_for_pr, resolve_effective_pr, EffectivePrSource, EffectivePrTarget,
};
use crate::application::agent_workspace_pr_reopen_restore::{
    restore_agent_workspace_local_artifacts, ReopenLocalWorkspaceState, WorkspaceLocalRestore,
};
use crate::application::chat_service::ChatService;
use crate::application::AppState;
use crate::domain::entities::plan_branch::PrStatus as PlanBranchPrStatus;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus,
    ChatConversationId, PlanBranch, Project,
};
use crate::domain::services::github_service::PrStatus as RemotePrStatus;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReopenAgentWorkspacePrOutcome {
    pub outcome: ReopenAgentWorkspacePrResult,
    pub pr_number: i64,
    /// State of the local checkout after the reopen attempt. `None` on paths
    /// that never touch local artifacts (`ConfirmationRequired`, `AlreadyMerged`).
    pub local_workspace: Option<ReopenLocalWorkspaceState>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReopenAgentWorkspacePrResult {
    /// GitHub already open; only the stale local latch was cleared.
    LatchCleared,
    /// `gh pr reopen` ran, GitHub confirmed open, latch cleared.
    ReopenedOnGithub,
    /// GitHub still closed and caller did not confirm a remote mutation.
    ConfirmationRequired,
    /// GitHub reports merged; local latch corrected instead.
    AlreadyMerged,
}

/// Reopen a terminal-closed PR for an agent conversation workspace.
///
/// `ConfirmationRequired` is an Ok variant, not an error: the caller must
/// re-invoke with `reopen_on_github = true` to authorize the remote mutation.
pub(crate) async fn reopen_agent_workspace_pr_for_state(
    conversation_id: &ChatConversationId,
    reopen_on_github: bool,
    state: &AppState,
) -> Result<ReopenAgentWorkspacePrOutcome, String> {
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

    let linked_plan_branch = load_linked_plan_branch_for_pr(&workspace, state).await?;
    let target = resolve_effective_pr(&workspace, linked_plan_branch.as_ref())
        .ok_or_else(|| "No PR associated with this workspace".to_string())?;

    let normalized_status = workspace
        .publication_pr_status
        .as_deref()
        .map(|status| status.trim().to_lowercase());
    match normalized_status.as_deref() {
        Some("merged") => return Ok(already_merged_outcome(&target)),
        Some("closed") => {}
        _ => return Err("This pull request is not closed".to_string()),
    }

    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Project not found: {}", workspace.project_id))?;

    let github = state
        .github_service
        .as_ref()
        .ok_or_else(|| "GitHub integration is unavailable".to_string())?;
    let working_dir = Path::new(&project.working_directory);

    let live = github
        .check_pr_status(working_dir, target.number)
        .await
        .map_err(|e| format!("Could not read PR #{} from GitHub: {e}", target.number))?;

    let marker = match live {
        RemotePrStatus::Merged { .. } => {
            return mark_live_merged(
                conversation_id,
                &workspace,
                linked_plan_branch.as_ref(),
                &target,
                state,
            )
            .await;
        }
        RemotePrStatus::Closed if !reopen_on_github => {
            return Ok(ReopenAgentWorkspacePrOutcome {
                outcome: ReopenAgentWorkspacePrResult::ConfirmationRequired,
                pr_number: target.number,
                local_workspace: None,
                message: format!(
                    "Pull request #{} is still closed on GitHub. Confirm to reopen it.",
                    target.number
                ),
            });
        }
        RemotePrStatus::Closed => {
            github
                .reopen_pr(working_dir, target.number)
                .await
                .map_err(|e| format!("Could not reopen PR #{} on GitHub: {e}", target.number))?;
            let confirmed = github
                .check_pr_status(working_dir, target.number)
                .await
                .map_err(|e| format!("Could not confirm PR #{} reopened: {e}", target.number))?;
            if confirmed != RemotePrStatus::Open {
                return Err(format!(
                    "PR #{} did not reopen on GitHub (status: {confirmed:?})",
                    target.number
                ));
            }
            UnlatchedMarker::ReopenedOnGithub
        }
        RemotePrStatus::Open => UnlatchedMarker::LatchCleared,
    };

    unlatch_and_restart(
        conversation_id,
        &workspace,
        linked_plan_branch.as_ref(),
        &target,
        marker,
        &project,
        state,
    )
    .await
}

/// Outcome markers reachable only after a live GitHub read confirmed the PR is no longer
/// closed. Kept distinct from `ReopenAgentWorkspacePrResult` so the un-latch path cannot be
/// called with `ConfirmationRequired`/`AlreadyMerged`, which return before it is reached.
#[derive(Debug, Clone, Copy)]
enum UnlatchedMarker {
    LatchCleared,
    ReopenedOnGithub,
}

impl From<UnlatchedMarker> for ReopenAgentWorkspacePrResult {
    fn from(marker: UnlatchedMarker) -> Self {
        match marker {
            UnlatchedMarker::LatchCleared => ReopenAgentWorkspacePrResult::LatchCleared,
            UnlatchedMarker::ReopenedOnGithub => ReopenAgentWorkspacePrResult::ReopenedOnGithub,
        }
    }
}

fn already_merged_outcome(target: &EffectivePrTarget) -> ReopenAgentWorkspacePrOutcome {
    ReopenAgentWorkspacePrOutcome {
        outcome: ReopenAgentWorkspacePrResult::AlreadyMerged,
        pr_number: target.number,
        local_workspace: None,
        message: format!(
            "Pull request #{} was already merged and cannot be reopened.",
            target.number
        ),
    }
}

async fn mark_live_merged(
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    linked_plan_branch: Option<&PlanBranch>,
    target: &EffectivePrTarget,
    state: &AppState,
) -> Result<ReopenAgentWorkspacePrOutcome, String> {
    if target.source == EffectivePrSource::PlanBranch {
        if let Some(plan_branch) = linked_plan_branch {
            state
                .plan_branch_repo
                .update_pr_status(&plan_branch.id, PlanBranchPrStatus::Merged)
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
            Some("merged"),
            workspace.publication_push_status.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())?;
    state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_reopen_attempted",
            "succeeded",
            format!(
                "Pull request #{} was already merged on GitHub",
                target.number
            ),
            None,
        ))
        .await
        .map_err(|e| e.to_string())?;

    Ok(already_merged_outcome(target))
}

async fn unlatch_and_restart(
    conversation_id: &ChatConversationId,
    workspace: &AgentConversationWorkspace,
    linked_plan_branch: Option<&PlanBranch>,
    target: &EffectivePrTarget,
    marker: UnlatchedMarker,
    project: &Project,
    state: &AppState,
) -> Result<ReopenAgentWorkspacePrOutcome, String> {
    // (a) Plan-branch PR status returns to open before any workspace write.
    if target.source == EffectivePrSource::PlanBranch {
        if let Some(plan_branch) = linked_plan_branch {
            state
                .plan_branch_repo
                .update_pr_status(&plan_branch.id, PlanBranchPrStatus::Open)
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    // (b) Un-latch the workspace publication status before any re-arm can observe it.
    state
        .agent_conversation_workspace_repo
        .update_publication(
            conversation_id,
            Some(target.number),
            target.url.as_deref(),
            Some("open"),
            workspace.publication_push_status.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())?;

    // (c) Review PR monitors may only re-arm once the workspace is no longer terminal.
    if workspace.mode == AgentConversationWorkspaceMode::ReviewPr {
        state
            .agent_conversation_workspace_repo
            .rearm_terminal_pr_review_monitor_after_live_open(conversation_id, target.number)
            .await
            .map_err(|e| e.to_string())?;
    }

    // (d) Clear any stale local-cleanup marker now that the PR is live again.
    state
        .agent_conversation_workspace_repo
        .clear_local_cleanup_status(conversation_id)
        .await
        .map_err(|e| e.to_string())?;

    // (e) Record the reopen as a durable publication event. A failed append must not abort a
    // reopen whose remote mutation and durable un-latch already committed; the poller restart
    // and status reconciliation below still need to run.
    if let Err(error) = state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            "pr_reopened",
            "succeeded",
            format!("Pull request #{} was reopened", target.number),
            None,
        ))
        .await
    {
        tracing::warn!(
            conversation_id = %conversation_id,
            pr_number = target.number,
            error = %error,
            "Could not append pr_reopened publication event after a successful reopen"
        );
    }

    // (f) Rebuild the local branch and worktree from origin before anything reads
    // the checkout; terminal cleanup force-deletes both.
    let restore =
        restore_agent_workspace_local_artifacts(project, workspace, linked_plan_branch).await;
    if let Err(error) = record_restore_outcome(conversation_id, &restore, state).await {
        tracing::warn!(
            conversation_id = %conversation_id,
            pr_number = target.number,
            error = %error,
            "Could not append local-restore publication event after a successful reopen"
        );
    }

    // (g) Restart the poller last, after the workspace is confirmed nonterminal. Prefer
    // the restored worktree; fall back to the project root only when restore failed.
    let poller_working_dir = restore
        .worktree_path
        .clone()
        .unwrap_or_else(|| Path::new(&project.working_directory).to_path_buf());
    let chat_service: Arc<dyn ChatService> = Arc::new(state.build_chat_service());
    state
        .pr_poller_registry
        .start_agent_workspace_polling_with_repair_repo_and_recovery_state(
            conversation_id.clone(),
            target.number,
            project.clone(),
            poller_working_dir,
            Arc::clone(&state.agent_conversation_workspace_repo),
            Arc::clone(&state.agent_run_repo),
            Arc::clone(&state.agent_workspace_repair_repo),
            chat_service,
            Some(Arc::new(state.clone())),
        );

    // (h) Workspace status follows the restored checkout, not the pre-reopen record.
    let next_status = if restore.is_restore_failure() {
        AgentConversationWorkspaceStatus::Missing
    } else {
        AgentConversationWorkspaceStatus::Active
    };
    if next_status != workspace.status {
        state
            .agent_conversation_workspace_repo
            .update_status(conversation_id, next_status)
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(ReopenAgentWorkspacePrOutcome {
        outcome: marker.into(),
        pr_number: target.number,
        local_workspace: Some(restore.state),
        message: reopen_message(marker, target.number, &restore),
    })
}

async fn record_restore_outcome(
    conversation_id: &ChatConversationId,
    restore: &WorkspaceLocalRestore,
    state: &AppState,
) -> Result<(), String> {
    let (event, status, detail) = match restore.state {
        // Nothing was rebuilt, so there is no durable state change worth an event.
        ReopenLocalWorkspaceState::AlreadyPresent => return Ok(()),
        ReopenLocalWorkspaceState::Restored => (
            "workspace_local_restored",
            "succeeded",
            "Local branch and worktree were restored from origin".to_string(),
        ),
        ReopenLocalWorkspaceState::RestoreFailed => (
            "workspace_local_restore_failed",
            "failed",
            restore
                .failure_reason
                .clone()
                .unwrap_or_else(|| "Local checkout could not be restored".to_string()),
        ),
    };

    state
        .agent_conversation_workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            conversation_id.clone(),
            event,
            status,
            detail,
            None,
        ))
        .await
        .map_err(|e| e.to_string())
}

fn reopen_message(
    marker: UnlatchedMarker,
    pr_number: i64,
    restore: &WorkspaceLocalRestore,
) -> String {
    let mut message = match marker {
        UnlatchedMarker::LatchCleared => format!(
            "Pull request #{pr_number} is already open on GitHub. The stale local status has been cleared."
        ),
        UnlatchedMarker::ReopenedOnGithub => {
            format!("Pull request #{pr_number} has been reopened on GitHub.")
        }
    };
    match restore.state {
        ReopenLocalWorkspaceState::AlreadyPresent => {}
        ReopenLocalWorkspaceState::Restored => {
            message.push_str(" The local branch and workspace were restored from origin.");
        }
        ReopenLocalWorkspaceState::RestoreFailed => {
            let reason = restore
                .failure_reason
                .as_deref()
                .unwrap_or("the local checkout could not be restored");
            message.push_str(&format!(" The workspace could not be restored: {reason}"));
        }
    }
    message
}

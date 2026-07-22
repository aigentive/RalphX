use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use chrono::{Duration, Utc};
use serde::Serialize;

use crate::application::agent_conversation_workspace::{
    expand_worktree_parent_public, resolve_agent_conversation_project_workspace_dir,
    resolve_agent_conversation_workspace_path_from_record_identity,
    resolve_linked_plan_branch_agent_worktree_path, validate_workspace_linked_plan_branch,
};
use crate::application::git_artifact_cleanup::{
    cleanup_terminal_agent_workspace_local_artifacts,
    cleanup_terminal_linked_plan_branch_local_artifacts, LocalGitArtifactCleanupReport,
    LOCAL_CLEANUP_STATUS_CLEANED, LOCAL_CLEANUP_STATUS_FAILED_OPERATIONAL,
    LOCAL_CLEANUP_STATUS_FAILED_UNSAFE,
};
use crate::application::git_service::git_cmd::{self, GitCommandLane};
use crate::domain::entities::{
    AgentConversationWorkspaceStatus, ChatConversationId, PlanBranch, Project,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentWorkspaceLocalCleanupClaim, PlanBranchRepository,
};
use crate::domain::services::kill_worktree_processes_async;
use crate::infrastructure::agents::claude::git_runtime_config;

pub(crate) use super::agent_workspace_terminal_observation::{
    record_terminal_pr_observation_best_effort, TerminalPrObservation,
};
#[cfg(test)]
pub(crate) use super::agent_workspace_terminalization::terminalize_agent_workspace_after_pr;
pub(crate) use super::agent_workspace_terminalization::{
    cleanup_terminal_agent_workspace_after_pr_with_observation,
    settle_review_pr_terminal_observation, terminalize_agent_workspace_after_pr_with_observation,
};

const TERMINAL_WORKSPACE_STOP_PREFIX: &str = "Agent stopped because the workspace pull request was";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalAgentWorkspaceCause {
    MergedPr,
    ClosedPr,
    ArchivedConversation,
}

impl TerminalAgentWorkspaceCause {
    pub(super) fn stop_reason(self) -> String {
        match self {
            Self::MergedPr => format!("{TERMINAL_WORKSPACE_STOP_PREFIX} merged"),
            Self::ClosedPr => format!("{TERMINAL_WORKSPACE_STOP_PREFIX} closed"),
            Self::ArchivedConversation => {
                "Agent stopped because the workspace conversation was archived".to_string()
            }
        }
    }

    pub(crate) fn from_pr_status(status: &str) -> Self {
        match status {
            "merged" => Self::MergedPr,
            _ => Self::ClosedPr,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalCleanupClaimState {
    Claimed,
    AlreadyInProgress,
    AlreadyCleaned,
    NotClaimed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalLocalCleanupResult {
    Cleaned,
    Pending,
    FailedUnsafe,
    FailedOperational,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerminalAgentWorkspaceOutcome {
    pub runtime_shutdown_succeeded: bool,
    pub cleanup_claim: TerminalCleanupClaimState,
    pub local_cleanup: TerminalLocalCleanupResult,
    pub message: Option<String>,
}

impl TerminalAgentWorkspaceOutcome {
    pub(super) fn runtime_blocked(message: String) -> Self {
        Self {
            runtime_shutdown_succeeded: false,
            cleanup_claim: TerminalCleanupClaimState::NotClaimed,
            local_cleanup: TerminalLocalCleanupResult::Pending,
            message: Some(message),
        }
    }

    pub(crate) fn require_runtime_shutdown(&self) -> Result<(), String> {
        if self.runtime_shutdown_succeeded {
            Ok(())
        } else {
            Err(self
                .message
                .clone()
                .unwrap_or_else(|| "Workspace runtime could not be stopped".to_string()))
        }
    }
}

enum TerminalCleanupTarget {
    Direct {
        worktree_path: PathBuf,
    },
    LinkedPlan {
        plan_branch: Box<PlanBranch>,
        worktree_path: PathBuf,
    },
}

pub(crate) async fn cleanup_terminal_agent_workspace_after_pr(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    plan_branch_repo: Option<Arc<dyn PlanBranchRepository>>,
    conversation_id: &ChatConversationId,
    project: &Project,
) -> TerminalAgentWorkspaceOutcome {
    let cleanup_started = Instant::now();
    let workspace = match workspace_repo.get_by_conversation_id(conversation_id).await {
        Ok(Some(workspace)) => workspace,
        Ok(None) => {
            return TerminalAgentWorkspaceOutcome {
                runtime_shutdown_succeeded: true,
                cleanup_claim: TerminalCleanupClaimState::NotClaimed,
                local_cleanup: TerminalLocalCleanupResult::FailedOperational,
                message: Some("Workspace row disappeared before local cleanup".to_string()),
            };
        }
        Err(error) => {
            return TerminalAgentWorkspaceOutcome {
                runtime_shutdown_succeeded: true,
                cleanup_claim: TerminalCleanupClaimState::NotClaimed,
                local_cleanup: TerminalLocalCleanupResult::FailedOperational,
                message: Some(format!("Failed to load workspace cleanup state: {error}")),
            };
        }
    };
    if workspace.status != AgentConversationWorkspaceStatus::Archived
        && !workspace
            .publication_pr_status
            .as_deref()
            .is_some_and(|status| matches!(status, "merged" | "closed"))
    {
        return TerminalAgentWorkspaceOutcome {
            runtime_shutdown_succeeded: true,
            cleanup_claim: TerminalCleanupClaimState::NotClaimed,
            local_cleanup: TerminalLocalCleanupResult::FailedUnsafe,
            message: Some(
                "Local cleanup requires persisted merged, closed, or archived authority"
                    .to_string(),
            ),
        };
    }

    let now = Utc::now();
    let retry_secs = git_runtime_config().terminal_pr_local_cleanup_retry_secs;
    let retry_secs = i64::try_from(retry_secs).unwrap_or(i64::MAX);
    let stale_before = now - Duration::seconds(retry_secs);
    let claim = match workspace_repo
        .claim_local_cleanup(conversation_id, now, stale_before)
        .await
    {
        Ok(AgentWorkspaceLocalCleanupClaim::Claimed) => TerminalCleanupClaimState::Claimed,
        Ok(AgentWorkspaceLocalCleanupClaim::AlreadyInProgress) => {
            return TerminalAgentWorkspaceOutcome {
                runtime_shutdown_succeeded: true,
                cleanup_claim: TerminalCleanupClaimState::AlreadyInProgress,
                local_cleanup: TerminalLocalCleanupResult::Pending,
                message: None,
            };
        }
        Ok(AgentWorkspaceLocalCleanupClaim::AlreadyCleaned) => {
            return TerminalAgentWorkspaceOutcome {
                runtime_shutdown_succeeded: true,
                cleanup_claim: TerminalCleanupClaimState::AlreadyCleaned,
                local_cleanup: TerminalLocalCleanupResult::Cleaned,
                message: None,
            };
        }
        Err(error) => {
            return TerminalAgentWorkspaceOutcome {
                runtime_shutdown_succeeded: true,
                cleanup_claim: TerminalCleanupClaimState::NotClaimed,
                local_cleanup: TerminalLocalCleanupResult::FailedOperational,
                message: Some(format!("Failed to claim local cleanup: {error}")),
            };
        }
    };

    let target = resolve_cleanup_target(&workspace, project, plan_branch_repo.as_deref()).await;
    let (cleanup_status, cleanup_result, message) = match target {
        Ok(target) => {
            let worktree_path = match &target {
                TerminalCleanupTarget::Direct { worktree_path } => worktree_path.clone(),
                TerminalCleanupTarget::LinkedPlan { worktree_path, .. } => worktree_path.clone(),
            };
            if worktree_path.exists() {
                kill_worktree_processes_async(
                    &worktree_path,
                    git_runtime_config().worktree_lsof_timeout_secs,
                    true,
                )
                .await;
            }
            match run_local_cleanup(project, &workspace, target).await {
                Ok(report) => classify_report(report),
                Err(error) => (
                    LOCAL_CLEANUP_STATUS_FAILED_OPERATIONAL,
                    TerminalLocalCleanupResult::FailedOperational,
                    Some(error.to_string()),
                ),
            }
        }
        Err(message) => (
            LOCAL_CLEANUP_STATUS_FAILED_UNSAFE,
            TerminalLocalCleanupResult::FailedUnsafe,
            Some(message),
        ),
    };

    let finalized = match workspace_repo
        .finalize_local_cleanup(conversation_id, now, cleanup_status, Utc::now())
        .await
    {
        Ok(finalized) => finalized,
        Err(error) => {
            return TerminalAgentWorkspaceOutcome {
                runtime_shutdown_succeeded: true,
                cleanup_claim: claim,
                local_cleanup: TerminalLocalCleanupResult::FailedOperational,
                message: Some(format!("Failed to persist local cleanup outcome: {error}")),
            };
        }
    };
    if !finalized {
        let current_status = workspace_repo
            .get_local_cleanup_status(conversation_id)
            .await
            .ok()
            .flatten();
        if current_status.as_deref() == Some(LOCAL_CLEANUP_STATUS_CLEANED) {
            return TerminalAgentWorkspaceOutcome {
                runtime_shutdown_succeeded: true,
                cleanup_claim: TerminalCleanupClaimState::AlreadyCleaned,
                local_cleanup: TerminalLocalCleanupResult::Cleaned,
                message: None,
            };
        }
        return TerminalAgentWorkspaceOutcome {
            runtime_shutdown_succeeded: true,
            cleanup_claim: claim,
            local_cleanup: TerminalLocalCleanupResult::FailedOperational,
            message: Some(
                "Local cleanup completed, but its claim was no longer current; automatic retry remains enabled"
                    .to_string(),
            ),
        };
    }

    tracing::info!(
        conversation_id = conversation_id.as_str(),
        cleanup_status,
        elapsed_ms = cleanup_started.elapsed().as_millis() as u64,
        "Terminal agent workspace local cleanup settled"
    );
    TerminalAgentWorkspaceOutcome {
        runtime_shutdown_succeeded: true,
        cleanup_claim: claim,
        local_cleanup: cleanup_result,
        message,
    }
}

async fn resolve_cleanup_target(
    workspace: &crate::domain::entities::AgentConversationWorkspace,
    project: &Project,
    plan_branch_repo: Option<&dyn PlanBranchRepository>,
) -> Result<TerminalCleanupTarget, String> {
    let Some(plan_branch_id) = workspace.linked_plan_branch_id.as_ref() else {
        let worktree_path =
            resolve_agent_conversation_workspace_path_from_record_identity(project, workspace)
                .map_err(|error| error.to_string())?;
        let worktree_path = validate_process_cleanup_target_path(project, &worktree_path)?;
        return Ok(TerminalCleanupTarget::Direct { worktree_path });
    };
    let repo = plan_branch_repo.ok_or_else(|| {
        format!(
            "Linked plan branch {} cannot be verified because its repository is unavailable",
            plan_branch_id.as_str()
        )
    })?;
    let plan_branch = repo
        .get_by_id(plan_branch_id)
        .await
        .map_err(|error| format!("Failed to load linked plan branch: {error}"))?
        .ok_or_else(|| {
            format!(
                "Linked plan branch {} was not found",
                plan_branch_id.as_str()
            )
        })?;
    validate_workspace_linked_plan_branch(project, workspace, &plan_branch)
        .map_err(|error| error.to_string())?;
    let worktree_path = resolve_linked_plan_branch_agent_worktree_path(project, &plan_branch)
        .map_err(|error| error.to_string())?;
    let worktree_path = validate_process_cleanup_target_path(project, &worktree_path)?;
    Ok(TerminalCleanupTarget::LinkedPlan {
        plan_branch: Box::new(plan_branch),
        worktree_path,
    })
}

fn validate_process_cleanup_target_path(
    project: &Project,
    worktree_path: &std::path::Path,
) -> Result<PathBuf, String> {
    let project_workspace_dir = resolve_agent_conversation_project_workspace_dir(project)
        .map_err(|error| error.to_string())?;
    let worktree_parent = expand_worktree_parent_public(project.worktree_parent_or_default())
        .map_err(|error| error.to_string())?;
    if worktree_path == project_workspace_dir
        || worktree_path == worktree_parent
        || !worktree_path.starts_with(&project_workspace_dir)
    {
        return Err("Workspace cleanup target is outside the RalphX project directory".to_string());
    }

    let metadata = match std::fs::symlink_metadata(worktree_path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(format!(
                "Failed to inspect workspace cleanup target: {error}"
            ));
        }
    };
    if let Some(metadata) = metadata {
        if metadata.file_type().is_symlink() {
            return Err("Workspace cleanup target is a symlink".to_string());
        }
        let canonical_project_dir = project_workspace_dir.canonicalize().map_err(|error| {
            format!("Failed to canonicalize RalphX project workspace directory: {error}")
        })?;
        let canonical_target = worktree_path
            .canonicalize()
            .map_err(|error| format!("Failed to canonicalize workspace cleanup target: {error}"))?;
        if canonical_target == canonical_project_dir
            || !canonical_target.starts_with(&canonical_project_dir)
        {
            return Err(
                "Workspace cleanup target escapes the canonical RalphX project directory"
                    .to_string(),
            );
        }
    }

    Ok(worktree_path.to_path_buf())
}

pub(crate) async fn terminal_cleanup_target_path(
    workspace: &crate::domain::entities::AgentConversationWorkspace,
    project: &Project,
    plan_branch_repo: &dyn PlanBranchRepository,
) -> Result<PathBuf, String> {
    match resolve_cleanup_target(workspace, project, Some(plan_branch_repo)).await? {
        TerminalCleanupTarget::Direct { worktree_path } => Ok(worktree_path),
        TerminalCleanupTarget::LinkedPlan { worktree_path, .. } => Ok(worktree_path),
    }
}

async fn run_local_cleanup(
    project: &Project,
    workspace: &crate::domain::entities::AgentConversationWorkspace,
    target: TerminalCleanupTarget,
) -> crate::error::AppResult<LocalGitArtifactCleanupReport> {
    git_cmd::with_git_command_lane(GitCommandLane::Background, async move {
        match target {
            TerminalCleanupTarget::Direct { .. } => {
                cleanup_terminal_agent_workspace_local_artifacts(project, workspace, true).await
            }
            TerminalCleanupTarget::LinkedPlan { plan_branch, .. } => {
                cleanup_terminal_linked_plan_branch_local_artifacts(
                    project,
                    workspace,
                    &plan_branch,
                )
                .await
            }
        }
    })
    .await
}

fn classify_report(
    report: LocalGitArtifactCleanupReport,
) -> (&'static str, TerminalLocalCleanupResult, Option<String>) {
    match report.skipped_reason {
        Some(reason) => (
            LOCAL_CLEANUP_STATUS_FAILED_UNSAFE,
            TerminalLocalCleanupResult::FailedUnsafe,
            Some(reason),
        ),
        None => (
            LOCAL_CLEANUP_STATUS_CLEANED,
            TerminalLocalCleanupResult::Cleaned,
            None,
        ),
    }
}

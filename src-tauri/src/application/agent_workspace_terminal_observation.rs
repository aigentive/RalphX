use std::path::Path;
use std::sync::Arc;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentRun, ChatConversationId,
};
use crate::domain::repositories::{AgentConversationWorkspaceRepository, TaskOutcomeRepository};
use crate::domain::services::{
    AgentWorkspaceOutcomeAdapter, GithubServiceTrait, PrStatus, PrSyncState,
};

pub(crate) const MERGED_CLEAN_STATUS: &str = "merged_clean";
pub(crate) const MERGED_WITH_FOLLOWUPS_STATUS: &str = "merged_with_followups";

fn normalized_branch_name(value: &str) -> &str {
    value.trim().strip_prefix("origin/").unwrap_or(value.trim())
}

fn is_full_git_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn classify_merged_workspace_observation(
    workspace: &AgentConversationWorkspace,
    pr_number: i64,
    sync_state: &PrSyncState,
) -> &'static str {
    if workspace.publication_pr_number != Some(pr_number)
        || !matches!(sync_state.status, PrStatus::Merged { .. })
        || sync_state.head_ref_name != workspace.branch_name
        || normalized_branch_name(&sync_state.base_ref_name)
            != normalized_branch_name(&workspace.base_ref)
    {
        return "merged";
    }
    let (Some(pushed_sha), Some(remote_head_sha)) = (
        workspace.publication_pushed_sha.as_deref(),
        sync_state.head_ref_oid.as_deref(),
    ) else {
        return "merged";
    };
    if !is_full_git_object_id(pushed_sha) || !is_full_git_object_id(remote_head_sha) {
        return "merged";
    }
    if pushed_sha.eq_ignore_ascii_case(remote_head_sha) {
        MERGED_CLEAN_STATUS
    } else {
        MERGED_WITH_FOLLOWUPS_STATUS
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TerminalPrObservation {
    pub pr_number: i64,
    pub status: String,
    pub summary: String,
    pub reason: Option<&'static str>,
    pub publication_event: Option<AgentConversationWorkspacePublicationEvent>,
}

impl TerminalPrObservation {
    pub(crate) fn new(pr_number: i64, status: &str, summary: impl Into<String>) -> Self {
        Self {
            pr_number,
            status: status.to_string(),
            summary: summary.into(),
            reason: None,
            publication_event: None,
        }
    }

    pub(crate) fn with_reason(mut self, reason: &'static str) -> Self {
        self.reason = Some(reason);
        self
    }

    pub(crate) fn with_publication_event(
        mut self,
        publication_event: AgentConversationWorkspacePublicationEvent,
    ) -> Self {
        self.publication_event = Some(publication_event);
        self
    }

    pub(crate) fn with_merge_cleanliness(
        mut self,
        workspace: &AgentConversationWorkspace,
        sync_state: &PrSyncState,
    ) -> Self {
        if self.status == "merged" {
            self.status =
                classify_merged_workspace_observation(workspace, self.pr_number, sync_state)
                    .to_string();
        }
        self
    }

    pub(crate) fn from_persisted_workspace(workspace: &AgentConversationWorkspace) -> Option<Self> {
        let status = workspace.publication_pr_status.as_deref()?;
        if !matches!(status, "merged" | "closed" | "failed") {
            return None;
        }
        let pr_number = if workspace.mode == AgentConversationWorkspaceMode::ReviewPr {
            workspace
                .source_pull_request
                .as_ref()
                .map(|pull_request| pull_request.number)
                .or(workspace.publication_pr_number)
        } else {
            workspace.publication_pr_number
        }?;
        let summary = match status {
            "merged" => "Pull request merged",
            "closed" => "Pull request closed without merging",
            _ => "Pull request terminalization failed",
        };
        Some(Self::new(pr_number, status, summary))
    }
}

pub(crate) async fn resolve_merge_cleanliness_best_effort(
    github: Option<&Arc<dyn GithubServiceTrait>>,
    working_dir: &Path,
    workspace: &AgentConversationWorkspace,
    observation: TerminalPrObservation,
) -> TerminalPrObservation {
    if observation.status != "merged" {
        return observation;
    }
    let Some(github) = github else {
        return observation;
    };
    match github
        .check_pr_sync_state(working_dir, observation.pr_number)
        .await
    {
        Ok(sync_state) => observation.with_merge_cleanliness(workspace, &sync_state),
        Err(error) => {
            tracing::warn!(
                conversation_id = workspace.conversation_id.as_str(),
                pr_number = observation.pr_number,
                error = %error,
                "Could not prove merged workspace cleanliness; keeping plain merged outcome"
            );
            observation
        }
    }
}

pub(crate) async fn record_terminal_pr_observation_best_effort(
    workspace_repo: &Arc<dyn AgentConversationWorkspaceRepository>,
    task_outcome_repo: Option<&Arc<dyn TaskOutcomeRepository>>,
    conversation_id: &ChatConversationId,
    observation: Option<&TerminalPrObservation>,
) {
    let (Some(task_outcome_repo), Some(observation)) = (task_outcome_repo, observation) else {
        return;
    };
    let workspace = match workspace_repo.get_by_conversation_id(conversation_id).await {
        Ok(Some(workspace)) => workspace,
        Ok(None) => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                pr_number = observation.pr_number,
                "Skipped terminal PR outcome because the workspace row was missing"
            );
            return;
        }
        Err(error) => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                pr_number = observation.pr_number,
                error = %error,
                "Failed to load workspace for terminal PR outcome"
            );
            return;
        }
    };
    let adapter = AgentWorkspaceOutcomeAdapter::new(Arc::clone(task_outcome_repo));
    if let Err(error) = adapter
        .record_pr_terminal(
            &workspace,
            observation.publication_event.as_ref(),
            observation.pr_number,
            &observation.status,
            observation.reason,
            &observation.summary,
        )
        .await
    {
        tracing::warn!(
            conversation_id = conversation_id.as_str(),
            pr_number = observation.pr_number,
            status = observation.status,
            error = %error,
            "Failed to record best-effort terminal PR outcome"
        );
    }
}

pub(crate) async fn record_no_pr_terminal_observation_best_effort(
    workspace_repo: &Arc<dyn AgentConversationWorkspaceRepository>,
    task_outcome_repo: &Arc<dyn TaskOutcomeRepository>,
    conversation_id: &ChatConversationId,
    agent_run: Option<&AgentRun>,
    reason: &str,
    summary: &str,
) {
    let workspace = match workspace_repo.get_by_conversation_id(conversation_id).await {
        Ok(Some(workspace)) => workspace,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                error = %error,
                "Failed to load workspace for no-PR terminal outcome"
            );
            return;
        }
    };
    if workspace.publication_pr_number.is_some() || workspace.source_pull_request.is_some() {
        return;
    }

    let adapter = AgentWorkspaceOutcomeAdapter::new(Arc::clone(task_outcome_repo));
    if let Err(error) = adapter
        .record_no_pr_terminal(&workspace, agent_run, reason, summary)
        .await
    {
        tracing::warn!(
            conversation_id = conversation_id.as_str(),
            reason,
            error = %error,
            "Failed to record best-effort no-PR terminal outcome"
        );
    }
}

use std::sync::Arc;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, ChatConversationId,
};
use crate::domain::repositories::{AgentConversationWorkspaceRepository, TaskOutcomeRepository};
use crate::domain::services::AgentWorkspaceOutcomeAdapter;

#[derive(Debug, Clone)]
pub(crate) struct TerminalPrObservation {
    pub pr_number: i64,
    pub status: String,
    pub summary: String,
    pub publication_event: Option<AgentConversationWorkspacePublicationEvent>,
}

impl TerminalPrObservation {
    pub(crate) fn new(pr_number: i64, status: &str, summary: impl Into<String>) -> Self {
        Self {
            pr_number,
            status: status.to_string(),
            summary: summary.into(),
            publication_event: None,
        }
    }

    pub(crate) fn with_publication_event(
        mut self,
        publication_event: AgentConversationWorkspacePublicationEvent,
    ) -> Self {
        self.publication_event = Some(publication_event);
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

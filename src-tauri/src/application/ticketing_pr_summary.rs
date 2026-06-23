//! Shared rollup that joins a ticket's linked agent conversations to their
//! workspace git branch / pull-request state. A single batched project query
//! avoids the N+1 that a per-conversation workspace lookup would create.

use std::collections::HashSet;

use crate::domain::entities::{is_open_pr, ChatConversationId, ProjectId};
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::error::AppResult;

/// Branch / PR state for one conversation linked to a ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketPrBranchSummary {
    pub conversation_id: String,
    pub branch_name: String,
    pub base_ref: String,
    pub pr_number: Option<i64>,
    pub pr_url: Option<String>,
    pub pr_status: Option<String>,
    /// PR exists and is not terminal (merged/closed). Draft counts as open.
    pub is_open: bool,
}

impl TicketPrBranchSummary {
    pub fn has_pr(&self) -> bool {
        self.pr_number.is_some()
    }
}

/// Branch / PR state for every linked conversation that has a workspace, built
/// from one batched `get_by_project_id`. Preserves the project's workspace order.
pub async fn ticket_pr_branch_summary(
    workspace_repo: &dyn AgentConversationWorkspaceRepository,
    project_id: &ProjectId,
    conversation_ids: &[ChatConversationId],
) -> AppResult<Vec<TicketPrBranchSummary>> {
    if conversation_ids.is_empty() {
        return Ok(Vec::new());
    }
    let wanted: HashSet<&ChatConversationId> = conversation_ids.iter().collect();
    let workspaces = workspace_repo.get_by_project_id(project_id).await?;
    Ok(workspaces
        .into_iter()
        .filter(|workspace| wanted.contains(&workspace.conversation_id))
        .map(|workspace| {
            let is_open = is_open_pr(
                workspace.publication_pr_number,
                workspace.publication_pr_status.as_deref(),
            );
            TicketPrBranchSummary {
                conversation_id: workspace.conversation_id.to_string(),
                branch_name: workspace.branch_name,
                base_ref: workspace.base_ref,
                pr_number: workspace.publication_pr_number,
                pr_url: workspace.publication_pr_url,
                pr_status: workspace.publication_pr_status,
                is_open,
            }
        })
        .collect())
}

/// Count of linked conversations that have an open (non-terminal) PR.
pub fn open_pr_count(summaries: &[TicketPrBranchSummary]) -> usize {
    summaries.iter().filter(|summary| summary.is_open).count()
}

/// Whether the ticket (via any linked conversation) has an open PR.
pub fn has_open_pr(summaries: &[TicketPrBranchSummary]) -> bool {
    summaries.iter().any(|summary| summary.is_open)
}

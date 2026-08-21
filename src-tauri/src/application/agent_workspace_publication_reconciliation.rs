use std::path::Path;
use std::sync::Arc;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode,
    AgentConversationWorkspacePublicationEvent, AgentConversationWorkspaceStatus, Project,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, ChatConversationRepository,
};
use crate::domain::services::GithubServiceTrait;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationCorrectionOutcome {
    Corrected,
    Skipped,
    Unverified,
    /// PR detail was unreadable specifically because GitHub's rate limit is exhausted. Callers
    /// treat this like `Unverified` for correction purposes but also feed the shared budget, so
    /// the rest of the pass stops spending calls that cannot succeed.
    RateLimited,
    NotApplicable,
}

pub async fn correct_foreign_agent_workspace_publication(
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    chat_conversation_repo: Arc<dyn ChatConversationRepository>,
    github: Arc<dyn GithubServiceTrait>,
    project: &Project,
    workspace: &AgentConversationWorkspace,
) -> AppResult<PublicationCorrectionOutcome> {
    if workspace.mode != AgentConversationWorkspaceMode::Edit
        || workspace.linked_plan_branch_id.is_some()
    {
        return Ok(PublicationCorrectionOutcome::NotApplicable);
    }
    let Some(pr_number) = workspace.publication_pr_number else {
        return Ok(PublicationCorrectionOutcome::NotApplicable);
    };

    let detail = match github
        .fetch_pr_detail(Path::new(&project.working_directory), pr_number)
        .await
    {
        Ok(detail) => detail,
        Err(error) => {
            if matches!(error, AppError::GithubRateLimited { .. }) {
                tracing::debug!(
                    conversation_id = workspace.conversation_id.as_str(),
                    pr_number,
                    error = %error,
                    "Foreign publication correction deferred by the GitHub rate limit"
                );
                return Ok(PublicationCorrectionOutcome::RateLimited);
            }
            tracing::warn!(
                conversation_id = workspace.conversation_id.as_str(),
                pr_number,
                error = %error,
                "Foreign publication correction skipped because PR detail could not be read"
            );
            return Ok(PublicationCorrectionOutcome::Unverified);
        }
    };
    if detail.head_ref_name == workspace.branch_name {
        // The PR provably belongs to this workspace's own branch. Record that once so the
        // reconciliation gate can stop paying for this same proof on every future trigger.
        // Best effort: a failed marker write only means the next pass re-verifies, which is
        // exactly today's behavior, so it must never fail the correction.
        if workspace.publication_association_verified_at.is_none() {
            if let Err(error) = workspace_repo
                .mark_publication_association_verified(&workspace.conversation_id)
                .await
            {
                tracing::warn!(
                    conversation_id = workspace.conversation_id.as_str(),
                    pr_number,
                    error = %error,
                    "Could not record the verified publication association; reconciliation will re-verify"
                );
            }
        }
        return Ok(PublicationCorrectionOutcome::Skipped);
    }

    let conversation = match chat_conversation_repo
        .get_by_id(&workspace.conversation_id)
        .await
    {
        Ok(conversation) => conversation,
        Err(error) => {
            tracing::warn!(
                conversation_id = workspace.conversation_id.as_str(),
                pr_number,
                error = %error,
                "Foreign publication correction skipped because conversation could not be read"
            );
            return Ok(PublicationCorrectionOutcome::Unverified);
        }
    };

    workspace_repo
        .update_publication(&workspace.conversation_id, None, None, None, None)
        .await?;
    workspace_repo
        .append_publication_event(AgentConversationWorkspacePublicationEvent::new(
            workspace.conversation_id.clone(),
            "publication_association_corrected",
            "succeeded",
            format!(
                "Cleared foreign pull request #{pr_number} from branch {}",
                detail.head_ref_name
            ),
            None,
        ))
        .await?;

    if workspace.status == AgentConversationWorkspaceStatus::Archived
        && workspace.has_terminal_publication_pr_status()
        && conversation.is_some_and(|conversation| !conversation.is_archived())
    {
        workspace_repo
            .update_status(
                &workspace.conversation_id,
                AgentConversationWorkspaceStatus::Active,
            )
            .await?;
    }

    Ok(PublicationCorrectionOutcome::Corrected)
}

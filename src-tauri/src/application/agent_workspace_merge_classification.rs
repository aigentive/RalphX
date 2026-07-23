use std::path::Path;
use std::sync::Arc;

use crate::domain::entities::ChatConversationId;
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::domain::services::{GithubServiceTrait, PrStatus, PrSyncState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MergedWorkspaceOutcome {
    Merged,
    Clean,
    WithFollowups,
}

impl MergedWorkspaceOutcome {
    pub(crate) const fn observation_status(self) -> &'static str {
        match self {
            Self::Merged => "merged",
            Self::Clean => "merged_clean",
            Self::WithFollowups => "merged_with_followups",
        }
    }
}

pub(crate) fn classify_merged_workspace_outcome(
    publication_pushed_sha: Option<&str>,
    sync_state: Option<&PrSyncState>,
) -> MergedWorkspaceOutcome {
    let Some(publication_pushed_sha) =
        publication_pushed_sha.filter(|sha| is_canonical_commit_oid(sha))
    else {
        return MergedWorkspaceOutcome::Merged;
    };
    let Some(sync_state) = sync_state else {
        return MergedWorkspaceOutcome::Merged;
    };
    let PrStatus::Merged {
        merge_commit_sha, ..
    } = &sync_state.status
    else {
        return MergedWorkspaceOutcome::Merged;
    };
    let Some(head_ref_oid) = sync_state
        .head_ref_oid
        .as_deref()
        .filter(|sha| is_canonical_commit_oid(sha))
    else {
        return MergedWorkspaceOutcome::Merged;
    };
    if head_ref_oid == publication_pushed_sha {
        MergedWorkspaceOutcome::Clean
    } else if merge_commit_sha
        .as_deref()
        .filter(|sha| is_canonical_commit_oid(sha))
        == Some(head_ref_oid)
    {
        MergedWorkspaceOutcome::WithFollowups
    } else {
        MergedWorkspaceOutcome::Merged
    }
}

pub(crate) async fn classify_merged_workspace_outcome_from_github(
    workspace_repo: &Arc<dyn AgentConversationWorkspaceRepository>,
    github: &Arc<dyn GithubServiceTrait>,
    conversation_id: &ChatConversationId,
    working_dir: &Path,
    pr_number: i64,
) -> MergedWorkspaceOutcome {
    let workspace = match workspace_repo.get_by_conversation_id(conversation_id).await {
        Ok(Some(workspace)) => workspace,
        Ok(None) => return MergedWorkspaceOutcome::Merged,
        Err(error) => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                pr_number,
                error = %error,
                "Merge refinement retained coarse outcome because workspace evidence was unavailable"
            );
            return MergedWorkspaceOutcome::Merged;
        }
    };
    let sync_state = match github.check_pr_sync_state(working_dir, pr_number).await {
        Ok(sync_state) => sync_state,
        Err(error) => {
            tracing::warn!(
                conversation_id = conversation_id.as_str(),
                pr_number,
                error = %error,
                "Merge refinement retained coarse outcome because GitHub evidence was unavailable"
            );
            return MergedWorkspaceOutcome::Merged;
        }
    };
    classify_merged_workspace_outcome(
        workspace.publication_pushed_sha.as_deref(),
        Some(&sync_state),
    )
}

fn is_canonical_commit_oid(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

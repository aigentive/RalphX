use std::path::Path;

use chrono::Utc;

use super::{update_strict_branch_from_target, validate_branch_reuse_evidence};
use crate::application::git_artifact_cleanup::cleanup_terminal_agent_workspace_local_artifacts;
use crate::application::git_service::GitService;
use crate::application::ticket_git_strict_start::{
    StrictTicketGitBlocker, StrictTicketGitBlockerCode,
};
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, TicketCanonicalBranch, TicketCanonicalBranchCycle,
    TicketCanonicalBranchCycleState,
};

pub async fn prepare_merged_strict_ticket_cycle_for_start(
    state: &AppState,
    binding: &TicketCanonicalBranch,
) -> Result<TicketCanonicalBranch, StrictTicketGitBlocker> {
    match binding.cycle.state {
        TicketCanonicalBranchCycleState::Preparing | TicketCanonicalBranchCycleState::Active => {
            return Ok(binding.clone())
        }
        TicketCanonicalBranchCycleState::ClosedUnmerged => {
            return Err(blocker(
                binding,
                "The previous pull request closed without merge; the frozen branch is preserved and cannot be reused automatically",
            ))
        }
        TicketCanonicalBranchCycleState::Merged => {}
        state => {
            return Err(blocker(
                binding,
                format!("Strict ticket branch cannot prepare a new cycle from state '{state}'"),
            ))
        }
    }

    let project = state
        .project_repo
        .get_by_id(&binding.project_id)
        .await
        .map_err(|error| blocker(binding, error.to_string()))?
        .ok_or_else(|| blocker(binding, "Strict ticket project no longer exists"))?;
    let prior = released_terminal_workspace(state, binding).await?;
    let repo_path = Path::new(&project.working_directory);
    GitService::fetch_origin(repo_path)
        .await
        .map_err(|error| blocker(binding, error.to_string()))?;
    let remote_target = format!("origin/{}", binding.base_branch);
    let target_ref = if GitService::ref_exists(repo_path, &remote_target)
        .await
        .map_err(|error| blocker(binding, error.to_string()))?
    {
        remote_target
    } else {
        binding.base_branch.clone()
    };
    validate_branch_reuse_evidence(repo_path, &binding.branch_name, &target_ref)
        .await
        .map_err(|error| blocker(binding, error.to_string()))?;
    let cleanup = cleanup_terminal_agent_workspace_local_artifacts(&project, &prior, false)
        .await
        .map_err(|error| blocker(binding, error.to_string()))?;
    if let Some(reason) = cleanup.skipped_reason {
        return Err(blocker(
            binding,
            format!("Strict ticket terminal workspace is not safely releasable: {reason}"),
        ));
    }
    state
        .agent_conversation_workspace_repo
        .mark_local_cleanup_status(&prior.conversation_id, "cleaned", Utc::now())
        .await
        .map_err(|error| blocker(binding, error.to_string()))?;

    update_strict_branch_from_target(&project, &prior, &target_ref)
        .await
        .map_err(|error| blocker(binding, error.to_string()))?;
    let cycle_base = GitService::get_branch_sha(repo_path, &binding.branch_name)
        .await
        .map_err(|error| blocker(binding, error.to_string()))?;
    persist_preparing_cycle(state, binding, &cycle_base).await
}

async fn released_terminal_workspace(
    state: &AppState,
    binding: &TicketCanonicalBranch,
) -> Result<AgentConversationWorkspace, StrictTicketGitBlocker> {
    let workspaces = state
        .agent_conversation_workspace_repo
        .find_by_head_ref(&binding.project_id, &binding.branch_name)
        .await
        .map_err(|error| blocker(binding, error.to_string()))?;
    workspaces
        .into_iter()
        .filter(|workspace| workspace.publication_pr_status.as_deref() == Some("merged"))
        .max_by_key(|workspace| workspace.updated_at)
        .ok_or_else(|| {
            blocker(
                binding,
                "Strict ticket branch has no merged terminal workspace proof for reuse",
            )
        })
}

async fn persist_preparing_cycle(
    state: &AppState,
    binding: &TicketCanonicalBranch,
    cycle_base: &str,
) -> Result<TicketCanonicalBranch, StrictTicketGitBlocker> {
    let generation = binding
        .cycle
        .generation
        .checked_add(1)
        .ok_or_else(|| blocker(binding, "Strict ticket cycle generation overflow"))?;
    let replacement = TicketCanonicalBranchCycle {
        generation,
        state: TicketCanonicalBranchCycleState::Preparing,
        base_commit: Some(cycle_base.to_string()),
        effective_merge_base: None,
        started_at: Some(Utc::now()),
        terminal_at: None,
    };
    let swapped = state
        .ticket_canonical_branch_repo
        .compare_and_swap_cycle(
            &binding.project_id,
            &binding.provider,
            &binding.issue_key,
            binding.cycle.generation,
            TicketCanonicalBranchCycleState::Merged,
            replacement,
        )
        .await
        .map_err(|error| blocker(binding, error.to_string()))?;
    let current = state
        .ticket_canonical_branch_repo
        .get_by_branch_name(&binding.project_id, &binding.branch_name)
        .await
        .map_err(|error| blocker(binding, error.to_string()))?
        .ok_or_else(|| blocker(binding, "Strict ticket binding disappeared during rollover"))?;
    if swapped
        || (current.cycle.generation == generation
            && current.cycle.state == TicketCanonicalBranchCycleState::Preparing
            && current.cycle.base_commit.as_deref() == Some(cycle_base))
    {
        Ok(current)
    } else {
        Err(blocker(
            binding,
            "Strict ticket cycle changed concurrently before the next workspace started",
        ))
    }
}

fn blocker(binding: &TicketCanonicalBranch, message: impl Into<String>) -> StrictTicketGitBlocker {
    StrictTicketGitBlocker::new(StrictTicketGitBlockerCode::InvalidCycleState, message)
        .for_task(&binding.issue_key)
        .for_branch(&binding.branch_name)
}

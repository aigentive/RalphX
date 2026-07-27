use std::path::{Path, PathBuf};

use chrono::Utc;

use crate::application::agent_conversation_workspace::{
    ensure_agent_conversation_worktree,
    resolve_agent_conversation_workspace_path_from_record_identity,
    run_or_defer_agent_conversation_workspace_setup, AgentConversationWorkspaceSetupMode,
};
use crate::application::agent_conversation_workspace_base::{
    apply_workspace_base_resolution, resolve_workspace_base_from_local_snapshot, BaseStatus,
};
use crate::application::git_service::GitService;
use crate::application::ticket_git_publish_policy::install_ticket_git_commit_hook;
use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceStatus, Project, TicketCanonicalBranch,
    TicketCanonicalBranchCycle, TicketCanonicalBranchCycleState, TicketCanonicalBranchPolicyKind,
};
use crate::domain::repositories::TicketCanonicalBranchRepository;
use crate::domain::state_machine::transition_handler::{
    update_source_from_target, SourceUpdateResult,
};
use crate::error::{AppError, AppResult};

#[path = "ticket_git_cycle_start.rs"]
mod start;
pub use start::prepare_merged_strict_ticket_cycle_for_start;
#[path = "ticket_git_cycle_workspace.rs"]
mod workspace_state;
pub(crate) use workspace_state::active_cycle_is_partial_rollover;
use workspace_state::{validate_binding_workspace_identity, validate_clean_workspace};

pub async fn mark_strict_ticket_cycle_terminal(
    repository: &dyn TicketCanonicalBranchRepository,
    workspace: &AgentConversationWorkspace,
    pr_status: &str,
) -> AppResult<Option<TicketCanonicalBranch>> {
    let Some(binding) = repository
        .get_by_branch_name(&workspace.project_id, &workspace.branch_name)
        .await?
    else {
        return Ok(None);
    };
    if binding.policy_kind != TicketCanonicalBranchPolicyKind::StrictGitConvention {
        return Ok(None);
    }
    validate_binding_workspace_identity(&binding, workspace)?;

    let terminal_state = match pr_status {
        "merged" => TicketCanonicalBranchCycleState::Merged,
        "closed" => TicketCanonicalBranchCycleState::ClosedUnmerged,
        other => {
            return Err(AppError::Validation(format!(
                "Strict ticket cycle cannot terminalize from PR status '{other}'"
            )))
        }
    };
    if binding.cycle.state == terminal_state {
        return Ok(Some(binding));
    }
    if binding.cycle.state != TicketCanonicalBranchCycleState::Active {
        return Err(AppError::Validation(format!(
            "Strict ticket cycle is already terminal or unavailable in state '{}' and cannot become '{}'",
            binding.cycle.state, terminal_state
        )));
    }

    let replacement = TicketCanonicalBranchCycle {
        generation: binding.cycle.generation,
        state: terminal_state,
        base_commit: binding.cycle.base_commit.clone(),
        effective_merge_base: binding.cycle.effective_merge_base.clone(),
        started_at: binding.cycle.started_at,
        terminal_at: Some(Utc::now()),
    };
    let swapped = repository
        .compare_and_swap_cycle(
            &binding.project_id,
            &binding.provider,
            &binding.issue_key,
            binding.cycle.generation,
            TicketCanonicalBranchCycleState::Active,
            replacement,
        )
        .await?;
    let current = repository
        .get_by_branch_name(&workspace.project_id, &workspace.branch_name)
        .await?
        .ok_or_else(|| {
            AppError::Validation(
                "Strict ticket binding disappeared during terminal reconciliation".to_string(),
            )
        })?;
    if swapped || current.cycle.state == terminal_state {
        return Ok(Some(current));
    }
    Err(AppError::Validation(
        "Strict ticket cycle changed concurrently during terminal reconciliation".to_string(),
    ))
}

pub async fn rollover_strict_ticket_workspace(
    state: &AppState,
    project: &Project,
    workspace: &AgentConversationWorkspace,
    setup_mode: AgentConversationWorkspaceSetupMode,
) -> AppResult<Option<AgentConversationWorkspace>> {
    let Some(mut binding) = state
        .ticket_canonical_branch_repo
        .get_by_branch_name(&workspace.project_id, &workspace.branch_name)
        .await?
    else {
        return Ok(None);
    };
    if binding.policy_kind != TicketCanonicalBranchPolicyKind::StrictGitConvention {
        return Ok(None);
    }
    validate_binding_workspace_identity(&binding, workspace)?;
    if workspace.publication_pr_status.as_deref() != Some("merged") {
        return Err(AppError::Validation(
            "Strict ticket branch cannot start another cycle because its pull request closed without merge"
                .to_string(),
        ));
    }
    let repo_path = PathBuf::from(&project.working_directory);
    let partial_active_rollover =
        active_cycle_is_partial_rollover(&repo_path, workspace, &binding).await?;
    if binding.cycle.state == TicketCanonicalBranchCycleState::Active && !partial_active_rollover {
        binding = mark_strict_ticket_cycle_terminal(
            state.ticket_canonical_branch_repo.as_ref(),
            workspace,
            "merged",
        )
        .await?
        .ok_or_else(|| {
            AppError::Validation("Strict ticket binding disappeared before rollover".to_string())
        })?;
    }
    if binding.cycle.state == TicketCanonicalBranchCycleState::ClosedUnmerged {
        return Err(AppError::Validation(
            "Strict ticket branch cannot start another cycle because its pull request closed without merge"
                .to_string(),
        ));
    }
    if !matches!(
        binding.cycle.state,
        TicketCanonicalBranchCycleState::Merged
            | TicketCanonicalBranchCycleState::Preparing
            | TicketCanonicalBranchCycleState::Active
    ) {
        return Err(AppError::Validation(format!(
            "Strict ticket branch cannot start another cycle from state '{}'",
            binding.cycle.state
        )));
    }

    let expected_path =
        resolve_agent_conversation_workspace_path_from_record_identity(project, workspace)?;
    validate_clean_workspace(&expected_path).await?;
    GitService::fetch_origin(&repo_path)
        .await
        .map_err(|error| {
            AppError::GitOperation(format!(
                "Failed to refresh origin before strict ticket branch reuse: {error}"
            ))
        })?;
    let base_resolution = resolve_workspace_base_from_local_snapshot(project, workspace).await?;
    if base_resolution.status == BaseStatus::Blocked {
        return Err(AppError::Validation(
            base_resolution
                .block_reason
                .clone()
                .unwrap_or_else(|| "Strict ticket workspace base is blocked".to_string()),
        ));
    }
    let target_ref = base_resolution.effective_checkout_ref()?.to_string();

    validate_branch_reuse_evidence(&repo_path, &binding.branch_name, &target_ref).await?;
    update_strict_branch_from_target(project, workspace, &target_ref).await?;
    validate_clean_workspace(&expected_path).await?;
    if expected_path.exists() {
        GitService::delete_worktree(&repo_path, &expected_path).await?;
    }
    ensure_agent_conversation_worktree(
        &repo_path,
        &expected_path,
        &binding.branch_name,
        &target_ref,
    )
    .await?;
    run_or_defer_agent_conversation_workspace_setup(
        project,
        &workspace.conversation_id,
        &expected_path,
        &binding.branch_name,
        setup_mode,
    )
    .await;
    let frozen = binding.strict_policy.as_ref().ok_or_else(|| {
        AppError::Validation("Strict ticket binding has no frozen convention".to_string())
    })?;
    install_ticket_git_commit_hook(&expected_path, frozen)
        .await
        .map_err(|error| AppError::GitOperation(error.to_string()))?;
    let cycle_base = GitService::get_branch_sha(&repo_path, &binding.branch_name).await?;
    binding = persist_next_active_cycle(state, &binding, &cycle_base).await?;

    let mut updated = workspace.clone();
    apply_workspace_base_resolution(&mut updated, &base_resolution)?;
    updated.base_commit = Some(cycle_base);
    updated.branch_name = binding.branch_name;
    updated.worktree_path = expected_path.to_string_lossy().to_string();
    updated.publication_pr_number = None;
    updated.publication_pr_url = None;
    updated.publication_pr_status = None;
    updated.publication_push_status = None;
    updated.pr_auto_merge_current = None;
    updated.pr_supervision_status = None;
    updated.pr_supervision_summary = None;
    updated.pr_supervision_updated_at = None;
    updated.status = AgentConversationWorkspaceStatus::Active;
    updated.updated_at = Utc::now();
    Ok(Some(updated))
}

pub(super) async fn validate_branch_reuse_evidence(
    repo_path: &Path,
    branch: &str,
    target_ref: &str,
) -> AppResult<()> {
    if !GitService::branch_exists_strict(repo_path, branch).await? {
        return Err(AppError::Validation(format!(
            "Strict ticket branch '{branch}' is missing locally; automatic reuse is blocked"
        )));
    }
    let (landed, reason) =
        GitService::is_branch_merged_or_content_equivalent(repo_path, branch, target_ref).await;
    if !landed {
        return Err(AppError::Validation(format!(
            "Strict ticket branch '{branch}' is not contained or content-equivalent to '{target_ref}': {reason}"
        )));
    }
    let remote_ref = format!("origin/{branch}");
    if GitService::ref_exists(repo_path, &remote_ref).await? {
        let local_sha = GitService::get_branch_sha(repo_path, branch).await?;
        let remote_sha = GitService::get_branch_sha(repo_path, &remote_ref).await?;
        let safe_rollover_advance = local_sha != remote_sha
            && GitService::is_commit_on_branch(repo_path, &remote_sha, branch).await?;
        if local_sha != remote_sha && !safe_rollover_advance {
            return Err(AppError::Validation(format!(
                "Strict ticket branch '{branch}' has unpublished local or remote-only commits; automatic reuse is blocked"
            )));
        }
    }
    Ok(())
}

pub(super) async fn update_strict_branch_from_target(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    target_ref: &str,
) -> AppResult<()> {
    match update_source_from_target(
        Path::new(&project.working_directory),
        &workspace.branch_name,
        target_ref,
        project,
        &workspace.conversation_id.as_str(),
        None,
    )
    .await
    {
        SourceUpdateResult::AlreadyUpToDate | SourceUpdateResult::Updated => Ok(()),
        SourceUpdateResult::Conflicts { conflict_files } => Err(AppError::Validation(format!(
            "Strict ticket branch reuse conflicts with its target in: {}",
            conflict_files
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
        SourceUpdateResult::BranchMissing { branch } => Err(AppError::Validation(format!(
            "Strict ticket branch reuse requires missing branch '{branch}'"
        ))),
        SourceUpdateResult::Error(error) => Err(AppError::GitOperation(format!(
            "Strict ticket branch reuse failed while updating from target: {error}"
        ))),
    }
}

async fn persist_next_active_cycle(
    state: &AppState,
    binding: &TicketCanonicalBranch,
    cycle_base: &str,
) -> AppResult<TicketCanonicalBranch> {
    if binding.cycle.state == TicketCanonicalBranchCycleState::Active
        && binding.cycle.base_commit.as_deref() == Some(cycle_base)
    {
        return Ok(binding.clone());
    }
    let next_generation = if binding.cycle.state == TicketCanonicalBranchCycleState::Preparing {
        binding.cycle.generation
    } else {
        binding.cycle.generation.checked_add(1).ok_or_else(|| {
            AppError::Validation("Strict ticket cycle generation overflow".to_string())
        })?
    };
    let preparing = TicketCanonicalBranchCycle {
        generation: next_generation,
        state: TicketCanonicalBranchCycleState::Preparing,
        base_commit: Some(cycle_base.to_string()),
        effective_merge_base: None,
        started_at: Some(Utc::now()),
        terminal_at: None,
    };
    let prepared_binding = if binding.cycle.state == TicketCanonicalBranchCycleState::Preparing {
        if binding.cycle.base_commit.as_deref() != Some(cycle_base) {
            return Err(AppError::Validation(
                "Strict ticket rollover retry resolved a different cycle base".to_string(),
            ));
        }
        binding.clone()
    } else {
        let prepared = state
            .ticket_canonical_branch_repo
            .compare_and_swap_cycle(
                &binding.project_id,
                &binding.provider,
                &binding.issue_key,
                binding.cycle.generation,
                TicketCanonicalBranchCycleState::Merged,
                preparing.clone(),
            )
            .await?;
        let current = state
            .ticket_canonical_branch_repo
            .get_by_branch_name(&binding.project_id, &binding.branch_name)
            .await?
            .ok_or_else(|| AppError::Validation("Strict ticket binding disappeared".to_string()))?;
        if prepared
            || (current.cycle.generation == next_generation
                && matches!(
                    current.cycle.state,
                    TicketCanonicalBranchCycleState::Preparing
                        | TicketCanonicalBranchCycleState::Active
                )
                && current.cycle.base_commit.as_deref() == Some(cycle_base))
        {
            current
        } else {
            return Err(AppError::Validation(
                "Strict ticket cycle changed concurrently before rollover persistence".to_string(),
            ));
        }
    };
    if prepared_binding.cycle.state == TicketCanonicalBranchCycleState::Active {
        return Ok(prepared_binding);
    }
    let active = TicketCanonicalBranchCycle {
        state: TicketCanonicalBranchCycleState::Active,
        ..preparing
    };
    let activated = state
        .ticket_canonical_branch_repo
        .compare_and_swap_cycle(
            &prepared_binding.project_id,
            &prepared_binding.provider,
            &prepared_binding.issue_key,
            next_generation,
            TicketCanonicalBranchCycleState::Preparing,
            active,
        )
        .await?;
    let current = state
        .ticket_canonical_branch_repo
        .get_by_branch_name(&binding.project_id, &binding.branch_name)
        .await?
        .ok_or_else(|| AppError::Validation("Strict ticket binding disappeared".to_string()))?;
    if activated || current.cycle.state == TicketCanonicalBranchCycleState::Active {
        Ok(current)
    } else {
        Err(AppError::Validation(
            "Strict ticket cycle changed concurrently before rollover activation".to_string(),
        ))
    }
}

use crate::application::git_service::git_cmd;
use crate::domain::entities::{
    AgentRunId, AgentRunStatus, BranchUpdateContinuation, BranchUpdateDirection, BranchUpdatePhase,
    GitMutationClaim, GitTargetLeaseOwnerKind, InternalStatus, TaskId,
};
use crate::domain::repositories::{
    AgentRunRepository, BranchUpdateCasOutcome, BranchUpdateRepository, CompleteGitMutation,
    GitAuthorityCasOutcome, ProjectRepository, TaskRepository, UnbindBranchUpdateRun,
};
use crate::error::{AppError, AppResult};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitMutationRecoveryOutcome {
    Cleared { claim_id: String },
    NeedsRepair { claim_id: String, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchUpdateContinuationRecoveryOutcome {
    Completed {
        operation_id: String,
    },
    NeedsRepair {
        operation_id: String,
        reason: String,
    },
}

/// Resume crash-interrupted continuation CASes. Publication continuations re-run
/// the fenced idempotent push and remote-head receipt check; ordinary
/// continuations reuse an existing durable claim or create one if settlement
/// completed immediately before the crash.
pub async fn recover_branch_update_continuations(
    repository: Arc<dyn BranchUpdateRepository>,
    task_repository: Arc<dyn TaskRepository>,
    project_repository: Arc<dyn ProjectRepository>,
) -> AppResult<Vec<BranchUpdateContinuationRecoveryOutcome>> {
    let mut outcomes = Vec::new();
    for operation in repository.list_active_operations().await? {
        if !matches!(
            operation.phase,
            BranchUpdatePhase::ContinuationPending | BranchUpdatePhase::ContinuationInProgress
        ) {
            continue;
        }
        let operation_id = operation.id.as_str().to_string();
        let result = async {
            let task = task_repository
                .get_by_id(&operation.task_id)
                .await?
                .ok_or_else(|| AppError::TaskNotFound(operation.task_id.as_str().to_string()))?;
            let update_status = match operation.direction {
                BranchUpdateDirection::PlanBranch => InternalStatus::UpdatingPlanBranch,
                BranchUpdateDirection::TaskBranch => InternalStatus::UpdatingTaskBranch,
            };
            if task.internal_status != update_status {
                return Err(AppError::Conflict(format!(
                    "task status {} does not match continuation owner {}",
                    task.internal_status.as_str(),
                    update_status.as_str()
                )));
            }
            if operation.continuation == BranchUpdateContinuation::FinalizePostMergePrPublication {
                let project = project_repository
                    .get_by_id(&task.project_id)
                    .await?
                    .ok_or_else(|| {
                        AppError::ProjectNotFound(task.project_id.as_str().to_string())
                    })?;
                crate::application::branch_update_executor::publish_post_merge_branch_update(
                    Arc::clone(&repository),
                    std::path::Path::new(&project.working_directory),
                    &operation,
                    update_status,
                )
                .await
            } else {
                crate::application::branch_update_executor::resume_branch_update_continuation(
                    Arc::clone(&repository),
                    &operation,
                    update_status,
                )
                .await
            }
        }
        .await;
        match result {
            Ok(_) => {
                outcomes.push(BranchUpdateContinuationRecoveryOutcome::Completed { operation_id })
            }
            Err(error) => outcomes.push(BranchUpdateContinuationRecoveryOutcome::NeedsRepair {
                operation_id,
                reason: error.to_string(),
            }),
        }
    }
    Ok(outcomes)
}

/// Clear only terminal/missing pre-spawn run bindings so restart can launch a
/// replacement updater. Running bindings remain authoritative until normal
/// process reconciliation proves them interrupted.
pub async fn recover_terminal_branch_update_run_bindings(
    repository: Arc<dyn BranchUpdateRepository>,
    agent_runs: Arc<dyn AgentRunRepository>,
) -> AppResult<usize> {
    let mut recovered = 0usize;
    for operation in repository.list_active_operations().await? {
        let (Some(run_id), Some(conversation_id)) = (
            operation.agent_run_id.clone(),
            operation.conversation_id.clone(),
        ) else {
            continue;
        };
        let run = agent_runs
            .get_by_id(&AgentRunId::from_string(run_id.clone()))
            .await?;
        if matches!(
            run.as_ref().map(|run| run.status),
            Some(AgentRunStatus::Running)
        ) {
            continue;
        }
        let update_status = match operation.direction {
            BranchUpdateDirection::PlanBranch => InternalStatus::UpdatingPlanBranch,
            BranchUpdateDirection::TaskBranch => InternalStatus::UpdatingTaskBranch,
        };
        if repository
            .unbind_agent_run(UnbindBranchUpdateRun {
                operation_id: operation.id,
                task_id: operation.task_id,
                originating_history_id: operation.originating_history_id,
                update_status,
                conversation_id,
                agent_run_id: run_id,
            })
            .await?
            == BranchUpdateCasOutcome::Applied
        {
            recovered += 1;
        }
    }
    Ok(recovered)
}

/// Reconcile durable mutation claims before target authority can be reused.
///
/// A surviving process group is terminated and awaited. The claim is cleared
/// only after owner-specific Git state proof. Branch-update workspaces must be
/// clean; merge attempts may retain stable dirty state because the exact target
/// lease remains owned while the normal merge retry performs cleanup.
pub async fn recover_in_flight_git_mutations(
    repository: Arc<dyn BranchUpdateRepository>,
    task_repository: Arc<dyn TaskRepository>,
    project_repository: Arc<dyn ProjectRepository>,
) -> AppResult<Vec<GitMutationRecoveryOutcome>> {
    let claims = repository.list_in_flight_mutations().await?;
    let mut outcomes = Vec::with_capacity(claims.len());
    for claim in claims {
        if let Err(reason) = terminate_process_group(&claim).await {
            outcomes.push(GitMutationRecoveryOutcome::NeedsRepair {
                claim_id: claim.claim_id,
                reason,
            });
            continue;
        }
        let safe = match claim.owner.kind {
            GitTargetLeaseOwnerKind::MergeAttempt => {
                inspect_merge_attempt_workspaces(
                    task_repository.as_ref(),
                    project_repository.as_ref(),
                    &claim,
                )
                .await?
            }
            GitTargetLeaseOwnerKind::BranchUpdateOperation
            | GitTargetLeaseOwnerKind::PublicationRecovery => {
                inspect_operation_workspace(repository.as_ref(), &claim).await?
            }
            GitTargetLeaseOwnerKind::Manual => {
                Err("manual mutation claims require explicit repair".into())
            }
        };
        if let Err(reason) = safe {
            outcomes.push(GitMutationRecoveryOutcome::NeedsRepair {
                claim_id: claim.claim_id,
                reason,
            });
            continue;
        }
        let completion = repository
            .complete_git_mutation(CompleteGitMutation {
                identity: claim.identity,
                owner: claim.owner,
                fencing_epoch: claim.fencing_epoch,
                claim_id: claim.claim_id.clone(),
            })
            .await?;
        if matches!(completion, GitAuthorityCasOutcome::Applied { .. }) {
            outcomes.push(GitMutationRecoveryOutcome::Cleared {
                claim_id: claim.claim_id,
            });
        } else {
            outcomes.push(GitMutationRecoveryOutcome::NeedsRepair {
                claim_id: claim.claim_id,
                reason: format!("authority changed during recovery: {completion:?}"),
            });
        }
    }
    Ok(outcomes)
}

async fn inspect_merge_attempt_workspaces(
    task_repository: &dyn TaskRepository,
    project_repository: &dyn ProjectRepository,
    claim: &GitMutationClaim,
) -> AppResult<Result<(), String>> {
    let Some(task_id) = claim.owner.task_id.as_ref() else {
        return Ok(Err("merge mutation owner has no task id".into()));
    };
    let task_id = TaskId::from_string(task_id.clone());
    let Some(task) = task_repository.get_by_id(&task_id).await? else {
        return Ok(Err("merge mutation owner task is missing".into()));
    };
    let Some(project) = project_repository.get_by_id(&task.project_id).await? else {
        return Ok(Err("merge mutation owner project is missing".into()));
    };
    let repo_path = match crate::utils::path_safety::validate_absolute_non_root_path(
        std::path::Path::new(&project.working_directory),
        "merge mutation project repository",
    ) {
        Ok(path) if path.is_dir() => path,
        Ok(path) => {
            return Ok(Err(format!(
                "merge mutation project repository is missing: {}",
                path.display()
            )))
        }
        Err(error) => return Ok(Err(error.to_string())),
    };
    let Some(target_branch) = claim.identity.full_ref().strip_prefix("refs/heads/") else {
        return Ok(Err("merge mutation target is not a local branch ref".into()));
    };
    let expected_owner_id = format!("pending-merge:{}:{target_branch}", task.id.as_str());
    if claim.owner.owner_id != expected_owner_id {
        return Ok(Err(
            "merge mutation owner id does not match its task and target".into(),
        ));
    }
    let observed_identity =
        match crate::application::GitService::canonical_target_identity(&repo_path, target_branch)
            .await
        {
            Ok(identity) => identity,
            Err(error) => return Ok(Err(error.to_string())),
        };
    if observed_identity != claim.identity {
        return Ok(Err(
            "merge mutation project repository does not match target identity".into(),
        ));
    }

    let first = inspect_merge_attempt_snapshot(&project, task.id.as_str(), &repo_path).await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let second = inspect_merge_attempt_snapshot(&project, task.id.as_str(), &repo_path).await?;
    match (first, second) {
        (Ok(first), Ok(second)) if first == second => Ok(Ok(())),
        (Ok(_), Ok(_)) => Ok(Err(
            "merge mutation Git state changed during recovery inspection".into(),
        )),
        (Err(reason), _) | (_, Err(reason)) => Ok(Err(reason)),
    }
}

async fn inspect_merge_attempt_snapshot(
    project: &crate::domain::entities::Project,
    task_id: &str,
    repo_path: &std::path::Path,
) -> AppResult<Result<Vec<String>, String>> {
    let worktrees = crate::application::GitService::list_worktrees(repo_path).await?;
    let mut paths = vec![repo_path.to_path_buf()];
    for candidate in [
        crate::domain::state_machine::transition_handler::compute_merge_worktree_path(
            project, task_id,
        ),
        crate::domain::state_machine::transition_handler::compute_rebase_worktree_path(
            project, task_id,
        ),
    ] {
        let candidate = match crate::utils::path_safety::validate_absolute_non_root_path(
            std::path::Path::new(&candidate),
            "merge mutation recovery worktree",
        ) {
            Ok(path) => path.to_path_buf(),
            Err(error) => return Ok(Err(error.to_string())),
        };
        if !candidate.exists() {
            continue;
        }
        let canonical = std::fs::canonicalize(&candidate).map_err(|error| {
            AppError::Infrastructure(format!(
                "failed to resolve merge recovery worktree {}: {error}",
                candidate.display()
            ))
        })?;
        let registered = worktrees.iter().any(|worktree| {
            std::fs::canonicalize(&worktree.path)
                .map(|path| path == canonical)
                .unwrap_or(false)
        });
        if !registered {
            return Ok(Err(format!(
                "merge recovery path is not a registered project worktree: {}",
                candidate.display()
            )));
        }
        paths.push(candidate);
    }

    let mut snapshot = Vec::with_capacity(paths.len());
    for path in paths {
        match inspect_git_path_snapshot(&path).await? {
            Ok(state) => snapshot.push(state),
            Err(reason) => return Ok(Err(reason)),
        }
    }
    Ok(Ok(snapshot))
}

async fn inspect_git_path_snapshot(path: &std::path::Path) -> AppResult<Result<String, String>> {
    let merge_head =
        git_cmd::run_status(&["rev-parse", "-q", "--verify", "MERGE_HEAD"], path).await?;
    let rebase_head =
        git_cmd::run_status(&["rev-parse", "-q", "--verify", "REBASE_HEAD"], path).await?;
    let status = git_cmd::run(&["status", "--porcelain"], path).await?;
    if !status.status.success() {
        return Err(AppError::GitOperation(format!(
            "failed to inspect mutation workspace {}: {}",
            path.display(),
            String::from_utf8_lossy(&status.stderr).trim()
        )));
    }
    let head = git_cmd::run(&["rev-parse", "HEAD"], path).await?;
    if !head.status.success() {
        return Err(AppError::GitOperation(format!(
            "failed to resolve mutation workspace HEAD {}: {}",
            path.display(),
            String::from_utf8_lossy(&head.stderr).trim()
        )));
    }
    Ok(Ok(format!(
        "{}:{}:merge={merge_head}:rebase={rebase_head}:status={}",
        path.display(),
        String::from_utf8_lossy(&head.stdout).trim(),
        String::from_utf8_lossy(&status.stdout)
    )))
}

async fn inspect_operation_workspace(
    repository: &dyn BranchUpdateRepository,
    claim: &GitMutationClaim,
) -> AppResult<Result<(), String>> {
    let operation_id =
        crate::domain::entities::BranchUpdateOperationId::from_string(claim.owner.owner_id.clone());
    let Some(operation) = repository.get_operation(&operation_id).await? else {
        return Ok(Err(
            "mutation owner has no recoverable branch-update operation".into(),
        ));
    };
    let Some(workspace) = operation.workspace_path else {
        return Ok(Err(
            "mutation operation has no persisted workspace path".into()
        ));
    };
    if !workspace.is_dir() {
        return Ok(Err(format!(
            "persisted mutation workspace is missing: {}",
            workspace.display()
        )));
    }
    let merge_head =
        git_cmd::run_status(&["rev-parse", "-q", "--verify", "MERGE_HEAD"], &workspace).await?;
    let rebase_head =
        git_cmd::run_status(&["rev-parse", "-q", "--verify", "REBASE_HEAD"], &workspace).await?;
    let status = git_cmd::run(&["status", "--porcelain"], &workspace).await?;
    if !status.status.success() {
        return Err(AppError::GitOperation(format!(
            "failed to inspect mutation workspace {}: {}",
            workspace.display(),
            String::from_utf8_lossy(&status.stderr).trim()
        )));
    }
    if merge_head || rebase_head || !status.stdout.is_empty() {
        return Ok(Err(
            "workspace still contains an in-progress or dirty Git mutation".into(),
        ));
    }
    Ok(Ok(()))
}

async fn terminate_process_group(claim: &GitMutationClaim) -> Result<(), String> {
    #[cfg(unix)]
    if let Some(process_group_id) = claim.process_group_id {
        use nix::sys::signal::{killpg, Signal};
        use nix::unistd::Pid;

        let process_group = Pid::from_raw(process_group_id as i32);
        if killpg(process_group, None).is_err() {
            return Ok(());
        }
        let _ = killpg(process_group, Signal::SIGTERM);
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if killpg(process_group, None).is_err() {
                return Ok(());
            }
        }
        let _ = killpg(process_group, Signal::SIGKILL);
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if killpg(process_group, None).is_err() {
                return Ok(());
            }
        }
        return Err(format!(
            "mutation process group {process_group_id} survived termination"
        ));
    }
    Ok(())
}

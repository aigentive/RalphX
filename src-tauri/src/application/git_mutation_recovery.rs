use crate::application::git_service::git_cmd;
use crate::domain::entities::{
    AgentRunId, AgentRunStatus, BranchUpdateContinuation, BranchUpdateDirection,
    BranchUpdatePhase, GitMutationClaim, InternalStatus,
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
    Completed { operation_id: String },
    NeedsRepair { operation_id: String, reason: String },
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
            if operation.continuation
                == BranchUpdateContinuation::FinalizePostMergePrPublication
            {
                let project = project_repository
                    .get_by_id(&task.project_id)
                    .await?
                    .ok_or_else(|| AppError::ProjectNotFound(task.project_id.as_str().to_string()))?;
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
            Ok(_) => outcomes.push(BranchUpdateContinuationRecoveryOutcome::Completed {
                operation_id,
            }),
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
/// only when its operation-owned workspace is present and Git reports neither
/// an in-progress merge/rebase nor uncommitted changes. Ambiguous state remains
/// fenced for explicit repair.
pub async fn recover_in_flight_git_mutations(
    repository: Arc<dyn BranchUpdateRepository>,
) -> AppResult<Vec<GitMutationRecoveryOutcome>> {
    let claims = repository.list_in_flight_mutations().await?;
    let mut outcomes = Vec::with_capacity(claims.len());
    for claim in claims {
        terminate_process_group(&claim).await;
        let safe = inspect_operation_workspace(repository.as_ref(), &claim).await?;
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

async fn terminate_process_group(claim: &GitMutationClaim) {
    #[cfg(unix)]
    if let Some(process_group_id) = claim.process_group_id {
        use nix::sys::signal::{killpg, Signal};
        use nix::unistd::Pid;

        let process_group = Pid::from_raw(process_group_id as i32);
        if killpg(process_group, None).is_err() {
            return;
        }
        let _ = killpg(process_group, Signal::SIGTERM);
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if killpg(process_group, None).is_err() {
                return;
            }
        }
        let _ = killpg(process_group, Signal::SIGKILL);
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if killpg(process_group, None).is_err() {
                return;
            }
        }
    }
}

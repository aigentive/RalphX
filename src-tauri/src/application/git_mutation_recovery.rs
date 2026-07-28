use crate::application::agent_conversation_workspace::resolve_effective_agent_conversation_workspace_path;
use crate::application::agent_workspace_publish_repair_state::{
    block_agent_workspace_repair_completion, validate_agent_workspace_repair_target_lease,
    AgentWorkspaceRepairTransitionOutcome,
};
use crate::application::git_service::git_cmd;
use crate::application::{AppState, GitService};
use crate::domain::entities::{
    AgentRunId, AgentRunStatus, AgentWorkspaceRepairAttempt, AgentWorkspaceRepairAttemptId,
    AgentWorkspaceRepairEffect, AgentWorkspaceRepairEffectKind, AgentWorkspaceRepairEffectStatus,
    AgentWorkspaceRepairPhase, BranchUpdateContinuation, BranchUpdateDirection, BranchUpdatePhase,
    GitMutationClaim, GitTargetLeaseOwnerKind, InternalStatus, TaskId,
};
use crate::domain::repositories::{
    AgentRunRepository, AgentWorkspaceRepairRepository, BranchUpdateCasOutcome,
    BranchUpdateRepository, CompleteAgentWorkspaceRepairEffect,
    CompleteAgentWorkspaceRepairEffectOutcome, CompleteGitMutation, GitAuthorityCasOutcome,
    ProjectRepository, TaskRepository, UnbindBranchUpdateRun,
};
use crate::error::{AppError, AppResult};
use chrono::{DateTime, Utc};
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
        // Repair-owned claims are reconciled through the durable attempt/effect coordinator.
        // This generic branch-update loop must not terminate, clear, or downgrade their lease.
        if claim.owner.kind == GitTargetLeaseOwnerKind::AgentWorkspaceRepair {
            continue;
        }
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
            GitTargetLeaseOwnerKind::AgentWorkspaceRepair => unreachable!("filtered above"),
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

/// Startup and recurring callers use this entry point so repair-owned mutation claims converge
/// with the same attempt/effect reconciler as live and terminal-run recovery.
pub async fn recover_in_flight_git_mutations_for_state(
    state: &AppState,
) -> AppResult<Vec<GitMutationRecoveryOutcome>> {
    let mut outcomes = recover_repair_owned_in_flight_git_mutations(state).await?;
    crate::application::agent_workspace_publish_recovery::
        recover_agent_workspace_repair_attempts_for_state(state)
        .await?;
    outcomes.extend(
        recover_in_flight_git_mutations(
            Arc::clone(&state.branch_update_repo),
            Arc::clone(&state.task_repo),
            Arc::clone(&state.project_repo),
        )
        .await?,
    );
    Ok(outcomes)
}

/// Reconcile only repair-owned claims before durable continuation recovery reads a `Busy` claim.
/// The exact attempt, owner, fencing epoch, pre-push remote OID, and intended post-push OID must
/// agree before this clears a claim or records an observed receipt. This path only fetches and
/// reads Git state; it never pushes or calls GitHub.
pub(crate) async fn recover_repair_owned_in_flight_git_mutations(
    state: &AppState,
) -> AppResult<Vec<GitMutationRecoveryOutcome>> {
    let claims = state.branch_update_repo.list_in_flight_mutations().await?;
    let mut outcomes = Vec::new();
    for claim in claims
        .into_iter()
        .filter(|claim| claim.owner.kind == GitTargetLeaseOwnerKind::AgentWorkspaceRepair)
    {
        outcomes.push(recover_repair_owned_git_mutation_claim(state, claim).await?);
    }
    Ok(outcomes)
}

async fn recover_repair_owned_git_mutation_claim(
    state: &AppState,
    claim: GitMutationClaim,
) -> AppResult<GitMutationRecoveryOutcome> {
    if let Err(reason) = terminate_process_group(&claim).await {
        return Ok(GitMutationRecoveryOutcome::NeedsRepair {
            claim_id: claim.claim_id,
            reason,
        });
    }

    let attempt_id = AgentWorkspaceRepairAttemptId::from_string(claim.owner.owner_id.clone());
    let Some(attempt) = state
        .agent_workspace_repair_repo
        .get_repair_attempt(&attempt_id)
        .await?
    else {
        return Ok(repair_claim_needs_repair(
            claim,
            "repair mutation owner has no durable attempt",
        ));
    };
    let Some(current) = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&attempt.conversation_id)
        .await?
    else {
        return Ok(repair_claim_needs_repair(
            claim,
            "repair mutation owner is no longer the current durable attempt",
        ));
    };
    if current.id != attempt.id
        || current.generation != attempt.generation
        || current.updated_at != attempt.updated_at
        || current.phase != AgentWorkspaceRepairPhase::Continuing
    {
        return Ok(repair_claim_needs_repair(
            claim,
            "repair mutation claim does not match the current continuing attempt",
        ));
    }

    let identity = match validate_agent_workspace_repair_target_lease(
        state.branch_update_repo.as_ref(),
        &current,
    )
    .await
    {
        Ok(identity) if identity == claim.identity => identity,
        Ok(_) => {
            return block_repair_claim_recovery(
                state,
                current,
                claim,
                "repair mutation canonical target differs from its durable attempt",
            )
            .await
        }
        Err(error) => {
            return block_repair_claim_recovery(
                state,
                current,
                claim,
                &format!("repair mutation lease proof failed: {error}"),
            )
            .await
        }
    };
    if current.target_lease_epoch != Some(claim.fencing_epoch) {
        return block_repair_claim_recovery(
            state,
            current,
            claim,
            "repair mutation fencing epoch differs from its durable attempt",
        )
        .await;
    }

    let effect_key = repair_push_effect_key(&current);
    let Some(effect) = state
        .agent_workspace_repair_repo
        .get_repair_effect_by_idempotency_key(&effect_key)
        .await?
    else {
        return block_repair_claim_recovery(
            state,
            current,
            claim,
            "repair mutation has no durable push-effect checkpoint",
        )
        .await;
    };
    // A crash after the push preflight reserves the deterministic effect/claim but before it
    // observes local or remote Git state. No mutation was authorized yet, so release only this
    // exact claim and let the normal continuation initialize the same effect on its next pass.
    // Treating this as a blocker would turn a concurrent Busy loser into a false failure.
    if is_uninitialized_repair_push_preflight(&effect) {
        return complete_repair_claim(state.branch_update_repo.as_ref(), claim, effect.id.as_str())
            .await;
    }
    if let Err(reason) = validate_repair_push_claim(&claim, &current, &effect) {
        return block_repair_claim_recovery(state, current, claim, &reason).await;
    }

    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&current.conversation_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "workspace {} for repair mutation recovery",
                current.conversation_id
            ))
        })?;
    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "project {} for repair mutation recovery",
                workspace.project_id
            ))
        })?;
    let target = resolve_effective_agent_conversation_workspace_path(
        &project,
        &workspace,
        state.plan_branch_repo.as_ref(),
    )
    .await?;
    let observed_identity =
        GitService::canonical_target_identity(&target.path, &target.branch_name).await?;
    if observed_identity != identity {
        return block_repair_claim_recovery(
            state,
            current,
            claim,
            "repair mutation workspace no longer resolves to the persisted canonical target",
        )
        .await;
    }
    let remote_oid = read_repair_origin_branch_oid(&target.path, &target.branch_name).await?;
    if remote_oid.as_deref() == effect.intended_head_oid.as_deref() {
        let observed = observe_repair_push_effect(
            state.agent_workspace_repair_repo.as_ref(),
            &current,
            effect,
            identity.full_ref(),
            remote_oid
                .as_deref()
                .expect("matching intended OID is present"),
        )
        .await?;
        return complete_repair_claim(
            state.branch_update_repo.as_ref(),
            claim,
            observed.id.as_str(),
        )
        .await;
    }
    if exact_repair_push_precondition(&effect, remote_oid.as_deref()) {
        return complete_repair_claim(state.branch_update_repo.as_ref(), claim, effect.id.as_str())
            .await;
    }

    let reason = "repair push remote OID does not match either the recorded pre-push or intended post-push OID";
    let failed = fail_repair_push_effect(
        state.agent_workspace_repair_repo.as_ref(),
        &current,
        effect,
        reason,
    )
    .await?;
    let cleared =
        complete_repair_claim(state.branch_update_repo.as_ref(), claim, failed.id.as_str()).await?;
    let GitMutationRecoveryOutcome::Cleared { claim_id } = cleared else {
        return Ok(cleared);
    };
    block_repair_attempt_after_claim_recovery(state, current, claim_id, reason).await
}

fn repair_claim_needs_repair(
    claim: GitMutationClaim,
    reason: impl Into<String>,
) -> GitMutationRecoveryOutcome {
    GitMutationRecoveryOutcome::NeedsRepair {
        claim_id: claim.claim_id,
        reason: reason.into(),
    }
}

fn repair_push_effect_key(attempt: &AgentWorkspaceRepairAttempt) -> String {
    format!(
        "agent_workspace_repair:{}:{}:push_branch",
        attempt.id, attempt.generation
    )
}

fn validate_repair_push_claim(
    claim: &GitMutationClaim,
    attempt: &AgentWorkspaceRepairAttempt,
    effect: &AgentWorkspaceRepairEffect,
) -> Result<(), String> {
    if effect.attempt_id != attempt.id
        || effect.kind != AgentWorkspaceRepairEffectKind::PushBranch
        || claim.claim_id != format!("{}:push", effect.id)
    {
        return Err("repair mutation claim does not match its durable push effect".to_string());
    }
    if effect
        .intended_head_oid
        .as_deref()
        .is_none_or(str::is_empty)
        || (!effect.expected_remote_absent
            && effect
                .expected_remote_oid
                .as_deref()
                .is_none_or(str::is_empty))
        || (effect.expected_remote_absent && effect.expected_remote_oid.is_some())
    {
        return Err("repair push effect is missing exact remote OID proof".to_string());
    }
    if matches!(effect.status, AgentWorkspaceRepairEffectStatus::Failed) {
        return Err("repair push effect was already marked failed".to_string());
    }
    Ok(())
}

fn is_uninitialized_repair_push_preflight(effect: &AgentWorkspaceRepairEffect) -> bool {
    effect.kind == AgentWorkspaceRepairEffectKind::PushBranch
        && effect.status == AgentWorkspaceRepairEffectStatus::InFlight
        && effect.intended_head_oid.is_none()
        && effect.expected_remote_oid.is_none()
        && !effect.expected_remote_absent
}

async fn read_repair_origin_branch_oid(
    workspace_path: &std::path::Path,
    branch_name: &str,
) -> AppResult<Option<String>> {
    GitService::fetch_origin(workspace_path).await?;
    let remote_ref =
        crate::application::publish_resilience::remote_tracking_ref_for_publish(branch_name);
    if !GitService::ref_exists(workspace_path, &remote_ref).await? {
        return Ok(None);
    }
    GitService::get_branch_sha(workspace_path, &remote_ref)
        .await
        .map(Some)
}

fn exact_repair_push_precondition(
    effect: &AgentWorkspaceRepairEffect,
    remote_oid: Option<&str>,
) -> bool {
    (effect.expected_remote_absent && remote_oid.is_none())
        || (!effect.expected_remote_absent && remote_oid == effect.expected_remote_oid.as_deref())
}

async fn observe_repair_push_effect(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    attempt: &AgentWorkspaceRepairAttempt,
    mut effect: AgentWorkspaceRepairEffect,
    remote_ref: &str,
    remote_oid: &str,
) -> AppResult<AgentWorkspaceRepairEffect> {
    let expected_effect_updated_at = effect.updated_at;
    let expected_effect_status = effect.status;
    if effect.status != AgentWorkspaceRepairEffectStatus::Observed {
        effect.status = AgentWorkspaceRepairEffectStatus::Observed;
        effect.receipt_json = Some(
            serde_json::json!({ "remote_ref": remote_ref, "remote_oid": remote_oid }).to_string(),
        );
        effect.last_error = None;
        effect.updated_at = next_repair_recovery_checkpoint_at(effect.updated_at);
        effect.completed_at = Some(effect.updated_at);
    }
    match repair_repo
        .complete_repair_effect(CompleteAgentWorkspaceRepairEffect {
            attempt_id: attempt.id.clone(),
            generation: attempt.generation,
            expected_phase: AgentWorkspaceRepairPhase::Continuing,
            expected_attempt_updated_at: attempt.updated_at,
            expected_effect_updated_at,
            expected_effect_status,
            effect,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await?
    {
        CompleteAgentWorkspaceRepairEffectOutcome::Applied(effect) => Ok(effect),
        CompleteAgentWorkspaceRepairEffectOutcome::Stale(_) => Err(AppError::Conflict(
            "repair push receipt lost current attempt authority during recovery".to_string(),
        )),
        CompleteAgentWorkspaceRepairEffectOutcome::Missing => Err(AppError::NotFound(
            "repair push effect disappeared during recovery".to_string(),
        )),
    }
}

async fn fail_repair_push_effect(
    repair_repo: &dyn AgentWorkspaceRepairRepository,
    attempt: &AgentWorkspaceRepairAttempt,
    mut effect: AgentWorkspaceRepairEffect,
    reason: &str,
) -> AppResult<AgentWorkspaceRepairEffect> {
    let expected_effect_updated_at = effect.updated_at;
    let expected_effect_status = effect.status;
    effect.status = AgentWorkspaceRepairEffectStatus::Failed;
    effect.last_error = Some(reason.to_string());
    effect.updated_at = next_repair_recovery_checkpoint_at(effect.updated_at);
    match repair_repo
        .complete_repair_effect(CompleteAgentWorkspaceRepairEffect {
            attempt_id: attempt.id.clone(),
            generation: attempt.generation,
            expected_phase: AgentWorkspaceRepairPhase::Continuing,
            expected_attempt_updated_at: attempt.updated_at,
            expected_effect_updated_at,
            expected_effect_status,
            effect,
            compatibility_projection: None,
            events: Vec::new(),
        })
        .await?
    {
        CompleteAgentWorkspaceRepairEffectOutcome::Applied(effect) => Ok(effect),
        CompleteAgentWorkspaceRepairEffectOutcome::Stale(_) => Err(AppError::Conflict(
            "repair push failure lost current attempt authority during recovery".to_string(),
        )),
        CompleteAgentWorkspaceRepairEffectOutcome::Missing => Err(AppError::NotFound(
            "repair push effect disappeared during recovery".to_string(),
        )),
    }
}

fn next_repair_recovery_checkpoint_at(previous: DateTime<Utc>) -> DateTime<Utc> {
    let now = Utc::now();
    if now > previous {
        now
    } else {
        previous + chrono::Duration::microseconds(1)
    }
}

async fn complete_repair_claim(
    repository: &dyn BranchUpdateRepository,
    claim: GitMutationClaim,
    effect_id: &str,
) -> AppResult<GitMutationRecoveryOutcome> {
    let completion = repository
        .complete_git_mutation(CompleteGitMutation {
            identity: claim.identity,
            owner: claim.owner,
            fencing_epoch: claim.fencing_epoch,
            claim_id: claim.claim_id.clone(),
        })
        .await?;
    if matches!(completion, GitAuthorityCasOutcome::Applied { .. }) {
        Ok(GitMutationRecoveryOutcome::Cleared {
            claim_id: claim.claim_id,
        })
    } else {
        Ok(GitMutationRecoveryOutcome::NeedsRepair {
            claim_id: claim.claim_id,
            reason: format!(
                "repair push effect {effect_id} lost exact mutation authority during recovery: {completion:?}"
            ),
        })
    }
}

async fn block_repair_claim_recovery(
    state: &AppState,
    attempt: AgentWorkspaceRepairAttempt,
    claim: GitMutationClaim,
    reason: &str,
) -> AppResult<GitMutationRecoveryOutcome> {
    block_repair_attempt_after_claim_recovery(state, attempt, claim.claim_id, reason).await
}

async fn block_repair_attempt_after_claim_recovery(
    state: &AppState,
    attempt: AgentWorkspaceRepairAttempt,
    claim_id: String,
    reason: &str,
) -> AppResult<GitMutationRecoveryOutcome> {
    let auto_merge_current = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&attempt.conversation_id)
        .await?
        .and_then(|workspace| workspace.pr_auto_merge_current);
    match block_agent_workspace_repair_completion(
        Arc::clone(&state.agent_workspace_repair_repo),
        Arc::clone(&state.branch_update_repo),
        attempt,
        "Workspace repair recovery is blocked.",
        reason,
        auto_merge_current,
    )
    .await?
    {
        AgentWorkspaceRepairTransitionOutcome::Applied(_) => {
            Ok(GitMutationRecoveryOutcome::NeedsRepair {
                claim_id,
                reason: reason.to_string(),
            })
        }
        AgentWorkspaceRepairTransitionOutcome::Stale(_)
        | AgentWorkspaceRepairTransitionOutcome::Missing => {
            Ok(GitMutationRecoveryOutcome::NeedsRepair {
                claim_id,
                reason:
                    "repair mutation recovery lost current attempt authority before it could block"
                        .to_string(),
            })
        }
    }
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

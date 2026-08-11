use crate::application::git_service::git_cmd::{run_authorized_mutation, AuthorizedGitMutation};
use crate::application::GitService;
use crate::domain::entities::{
    BranchUpdateContinuation, BranchUpdateDirection, BranchUpdateFailureKind,
    BranchUpdateOperation, BranchUpdatePhase, GitMutationKind, GitTargetLeaseOwner,
    GitTargetLeaseOwnerKind, InternalStatus,
};
use crate::domain::repositories::{
    BlockBranchUpdate, BranchUpdateCasOutcome, BranchUpdateRepository,
    CheckpointBranchUpdateResult, ClaimBranchUpdateContinuation, CompleteBranchUpdateContinuation,
    MarkBranchUpdateResolving, SettleBranchUpdateProgrammatic, TaskRepository,
    TransferBranchUpdateTargetLease,
};
use crate::error::{AppError, AppResult};
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchUpdateExecutionOutcome {
    Completed { destination: InternalStatus },
    ContinuationPending,
    NeedsAgent,
    Blocked,
}

fn destination(continuation: BranchUpdateContinuation) -> InternalStatus {
    match continuation {
        BranchUpdateContinuation::ResumeExecution => InternalStatus::Executing,
        BranchUpdateContinuation::ResumeReExecution => InternalStatus::ReExecuting,
        BranchUpdateContinuation::ResumeReview => InternalStatus::Reviewing,
        BranchUpdateContinuation::RetryPendingMerge => InternalStatus::PendingMerge,
        BranchUpdateContinuation::ResumeWaitingOnPr => InternalStatus::WaitingOnPr,
        BranchUpdateContinuation::FinalizePostMergePrPublication => InternalStatus::Merged,
    }
}

async fn authority(
    repository: Arc<dyn BranchUpdateRepository>,
    operation: &BranchUpdateOperation,
    owner: &GitTargetLeaseOwner,
    epoch: u64,
    kind: GitMutationKind,
) -> AppResult<AuthorizedGitMutation> {
    AuthorizedGitMutation::from_current_lease(
        repository,
        operation.target_identity.clone(),
        owner.clone(),
        epoch,
        uuid::Uuid::new_v4().to_string(),
        kind,
    )
    .await
}

async fn read_ref(repo: &Path, reference: &str) -> AppResult<String> {
    let output =
        crate::application::git_service::git_cmd::run(&["rev-parse", reference], repo).await?;
    if !output.status.success() {
        return Err(AppError::GitOperation(format!(
            "failed to resolve {reference}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

async fn is_strict_ancestor(repo: &Path, ancestor: &str, descendant: &str) -> AppResult<bool> {
    let output = crate::application::git_service::git_cmd::run(
        &["merge-base", "--is-ancestor", ancestor, descendant],
        repo,
    )
    .await?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(AppError::GitOperation(format!(
            "failed to verify commit ancestry: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))),
    }
}

async fn ensure_registered_workspace(repo: &Path, workspace: &Path) -> AppResult<()> {
    let canonical_workspace = std::fs::canonicalize(workspace).map_err(|error| {
        AppError::Infrastructure(format!(
            "Failed to resolve branch update workspace {}: {error}",
            workspace.display()
        ))
    })?;
    let worktrees = GitService::list_worktrees(repo).await?;
    let registered = worktrees.iter().any(|worktree| {
        std::fs::canonicalize(&worktree.path)
            .map(|path| path == canonical_workspace)
            .unwrap_or(false)
    });
    if !registered {
        return Err(AppError::Validation(format!(
            "Branch update workspace is not registered to the target repository: {}",
            workspace.display()
        )));
    }
    Ok(())
}

async fn checkpoint_result(
    repository: &Arc<dyn BranchUpdateRepository>,
    operation: &BranchUpdateOperation,
    update_status: InternalStatus,
    owner: &GitTargetLeaseOwner,
    fencing_epoch: u64,
    resulting_sha: &str,
) -> AppResult<()> {
    let outcome = repository
        .checkpoint_result(CheckpointBranchUpdateResult {
            operation_id: operation.id.clone(),
            task_id: operation.task_id.clone(),
            originating_history_id: operation.originating_history_id.clone(),
            update_status,
            owner: owner.clone(),
            fencing_epoch,
            resulting_sha: resulting_sha.to_string(),
        })
        .await?;
    if outcome != BranchUpdateCasOutcome::Applied {
        return Err(AppError::Conflict(format!(
            "Branch update result checkpoint lost authority: {outcome:?}"
        )));
    }
    Ok(())
}

async fn block(
    repository: &Arc<dyn BranchUpdateRepository>,
    operation: &BranchUpdateOperation,
    update_status: InternalStatus,
    owner: GitTargetLeaseOwner,
    epoch: u64,
    failure_kind: BranchUpdateFailureKind,
    diagnostics: String,
) -> AppResult<BranchUpdateExecutionOutcome> {
    let outcome = repository
        .block_operation(BlockBranchUpdate {
            operation_id: operation.id.clone(),
            task_id: operation.task_id.clone(),
            originating_history_id: operation.originating_history_id.clone(),
            update_status,
            owner,
            fencing_epoch: epoch,
            failure_kind,
            diagnostics,
            conflict_files: Vec::new(),
        })
        .await?;
    if outcome != BranchUpdateCasOutcome::Applied {
        return Err(AppError::Conflict(format!(
            "Branch update blocking lost authority: {outcome:?}"
        )));
    }
    Ok(BranchUpdateExecutionOutcome::Blocked)
}

/// Execute the fast-path branch synchronization under durable target authority.
pub async fn execute_programmatic_branch_update(
    repository: Arc<dyn BranchUpdateRepository>,
    task_repository: Arc<dyn TaskRepository>,
    repo_path: &Path,
    operation: &BranchUpdateOperation,
    update_status: InternalStatus,
    fencing_epoch: u64,
) -> AppResult<BranchUpdateExecutionOutcome> {
    if !GitService::check_ref_format(repo_path, &operation.source_branch).await?
        || !GitService::check_ref_format(repo_path, &operation.target_branch).await?
    {
        return Err(AppError::Validation(
            "Branch update contains an invalid source or target branch".to_string(),
        ));
    }
    let workspace = operation.workspace_path.as_ref().ok_or_else(|| {
        AppError::Validation("Branch update operation has no workspace path".to_string())
    })?;
    let workspace = crate::utils::path_safety::validate_absolute_non_root_path(
        workspace,
        "branch update workspace",
    )?;
    let owner =
        GitTargetLeaseOwner::branch_update(operation.task_id.as_str(), operation.id.as_str());
    let expected_source = operation.observed_source_sha.clone().ok_or_else(|| {
        AppError::Validation("Branch update is missing its observed source SHA".to_string())
    })?;
    let expected_target = operation.observed_target_sha.clone().ok_or_else(|| {
        AppError::Validation("Branch update is missing its observed target SHA".to_string())
    })?;
    let mut resulting_sha = operation.resulting_sha.clone();
    let current_source = if resulting_sha.is_none() {
        Some(read_ref(repo_path, &operation.source_branch).await?)
    } else {
        None
    };
    let current_target = read_ref(repo_path, &operation.target_branch).await?;
    let target_is_expected = current_target == expected_target;
    let target_is_result = resulting_sha
        .as_deref()
        .is_some_and(|result| current_target == result);
    if (!target_is_expected && !target_is_result)
        || current_source
            .as_deref()
            .is_some_and(|source| source != expected_source)
    {
        return block(
            &repository,
            operation,
            update_status,
            owner,
            fencing_epoch,
            BranchUpdateFailureKind::CheckoutBusy,
            format!(
                "Branch tips changed after preflight (source {expected_source}->{}, target {expected_target}->{current_target})",
                current_source.as_deref().unwrap_or("<checkpointed>")
            ),
        )
        .await;
    }

    let workspace_arg = workspace.to_string_lossy().into_owned();
    let workspace_exists = workspace.exists();
    let mut needs_merge = false;
    if workspace_exists {
        ensure_registered_workspace(repo_path, &workspace).await?;
        let workspace_head = read_ref(&workspace, "HEAD").await?;
        let conflicts = GitService::get_conflict_files(&workspace).await?;
        if !conflicts.is_empty() {
            let outcome = repository
                .mark_resolving(MarkBranchUpdateResolving {
                    operation_id: operation.id.clone(),
                    task_id: operation.task_id.clone(),
                    originating_history_id: operation.originating_history_id.clone(),
                    update_status,
                    owner,
                    fencing_epoch,
                    conflict_files: conflicts,
                })
                .await?;
            if outcome != BranchUpdateCasOutcome::Applied {
                return Err(AppError::Conflict(format!(
                    "Branch update conflict recovery lost authority: {outcome:?}"
                )));
            }
            return Ok(BranchUpdateExecutionOutcome::NeedsAgent);
        }
        if GitService::is_merge_in_progress(&workspace) {
            return block(
                &repository,
                operation,
                update_status,
                owner,
                fencing_epoch,
                BranchUpdateFailureKind::Incomplete,
                "Branch update workspace has an incomplete conflict-free merge".to_string(),
            )
            .await;
        }
        if let Some(checkpointed) = resulting_sha.as_deref() {
            if workspace_head != checkpointed {
                return block(
                    &repository,
                    operation,
                    update_status,
                    owner,
                    fencing_epoch,
                    BranchUpdateFailureKind::CheckoutBusy,
                    format!(
                        "Branch update workspace HEAD differs from checkpoint ({checkpointed}->{workspace_head})"
                    ),
                )
                .await;
            }
        } else if workspace_head == expected_target {
            needs_merge = true;
        } else {
            let contains_source =
                is_strict_ancestor(&workspace, &expected_source, &workspace_head).await?;
            let contains_target =
                is_strict_ancestor(&workspace, &expected_target, &workspace_head).await?;
            if !contains_source || !contains_target {
                return block(
                    &repository,
                    operation,
                    update_status,
                    owner,
                    fencing_epoch,
                    BranchUpdateFailureKind::Incomplete,
                    "Branch update workspace HEAD does not contain both preflight tips".to_string(),
                )
                .await;
            }
            checkpoint_result(
                &repository,
                operation,
                update_status,
                &owner,
                fencing_epoch,
                &workspace_head,
            )
            .await?;
            resulting_sha = Some(workspace_head);
        }
    } else if resulting_sha.is_none() {
        let parent = workspace.parent().ok_or_else(|| {
            AppError::Validation("Branch update workspace has no parent".to_string())
        })?;
        let parent = crate::utils::path_safety::validate_absolute_non_root_path(
            parent,
            "branch update workspace parent",
        )?;
        tokio::fs::create_dir_all(&parent).await.map_err(|error| {
            AppError::Infrastructure(format!(
                "Failed to create branch update workspace parent {}: {error}",
                parent.display()
            ))
        })?;
        let add_output = run_authorized_mutation(
            &[
                "worktree",
                "add",
                "--detach",
                &workspace_arg,
                &expected_target,
            ],
            repo_path,
            authority(
                Arc::clone(&repository),
                operation,
                &owner,
                fencing_epoch,
                GitMutationKind::WorktreeCreate,
            )
            .await?,
        )
        .await?;
        if !add_output.status.success() {
            return block(
                &repository,
                operation,
                update_status,
                owner,
                fencing_epoch,
                BranchUpdateFailureKind::CheckoutBusy,
                String::from_utf8_lossy(&add_output.stderr).into_owned(),
            )
            .await;
        }
        needs_merge = true;
    }

    if needs_merge {
        let merge_output = run_authorized_mutation(
            &["merge", "--no-edit", &expected_source],
            &workspace,
            authority(
                Arc::clone(&repository),
                operation,
                &owner,
                fencing_epoch,
                GitMutationKind::Merge,
            )
            .await?,
        )
        .await?;
        if !merge_output.status.success() {
            let conflicts = GitService::get_conflict_files(&workspace).await?;
            if !conflicts.is_empty() {
                let outcome = repository
                    .mark_resolving(MarkBranchUpdateResolving {
                        operation_id: operation.id.clone(),
                        task_id: operation.task_id.clone(),
                        originating_history_id: operation.originating_history_id.clone(),
                        update_status,
                        owner,
                        fencing_epoch,
                        conflict_files: conflicts,
                    })
                    .await?;
                if outcome != BranchUpdateCasOutcome::Applied {
                    return Err(AppError::Conflict(format!(
                        "Branch update conflict settlement lost authority: {outcome:?}"
                    )));
                }
                return Ok(BranchUpdateExecutionOutcome::NeedsAgent);
            }
            return block(
                &repository,
                operation,
                update_status,
                owner,
                fencing_epoch,
                BranchUpdateFailureKind::Incomplete,
                String::from_utf8_lossy(&merge_output.stderr).into_owned(),
            )
            .await;
        }
        let merged_sha = read_ref(&workspace, "HEAD").await?;
        checkpoint_result(
            &repository,
            operation,
            update_status,
            &owner,
            fencing_epoch,
            &merged_sha,
        )
        .await?;
        resulting_sha = Some(merged_sha);
    }

    let resulting_sha = resulting_sha.ok_or_else(|| {
        AppError::Conflict("Branch update completed Git work without a result checkpoint".into())
    })?;
    let full_ref = operation.target_identity.full_ref();
    let target_before_update = read_ref(repo_path, &operation.target_branch).await?;
    if target_before_update == expected_target {
        let update_output = run_authorized_mutation(
            &["update-ref", full_ref, &resulting_sha, &expected_target],
            repo_path,
            authority(
                Arc::clone(&repository),
                operation,
                &owner,
                fencing_epoch,
                GitMutationKind::Merge,
            )
            .await?,
        )
        .await?;
        if !update_output.status.success() {
            return block(
                &repository,
                operation,
                update_status,
                owner,
                fencing_epoch,
                BranchUpdateFailureKind::CheckoutBusy,
                String::from_utf8_lossy(&update_output.stderr).into_owned(),
            )
            .await;
        }
    } else if target_before_update != resulting_sha {
        return block(
            &repository,
            operation,
            update_status,
            owner,
            fencing_epoch,
            BranchUpdateFailureKind::CheckoutBusy,
            format!(
                "Branch update target differs from both preflight and checkpoint ({expected_target}->{target_before_update}, checkpoint {resulting_sha})"
            ),
        )
        .await;
    }

    if workspace.exists() {
        ensure_registered_workspace(repo_path, &workspace).await?;
        let remove_output = run_authorized_mutation(
            &["worktree", "remove", "--force", &workspace_arg],
            repo_path,
            authority(
                Arc::clone(&repository),
                operation,
                &owner,
                fencing_epoch,
                GitMutationKind::WorktreeDelete,
            )
            .await?,
        )
        .await?;
        if !remove_output.status.success() {
            return block(
                &repository,
                operation,
                update_status,
                owner,
                fencing_epoch,
                BranchUpdateFailureKind::Incomplete,
                String::from_utf8_lossy(&remove_output.stderr).into_owned(),
            )
            .await;
        }
    }

    let settled = repository
        .settle_programmatic(SettleBranchUpdateProgrammatic {
            operation_id: operation.id.clone(),
            task_id: operation.task_id.clone(),
            originating_history_id: operation.originating_history_id.clone(),
            update_status,
            owner: owner.clone(),
            fencing_epoch,
            resulting_sha: resulting_sha.clone(),
        })
        .await?;
    if settled != BranchUpdateCasOutcome::Applied {
        return Err(AppError::Conflict(format!(
            "Branch update settlement lost authority: {settled:?}"
        )));
    }

    if let Some(mut task) = task_repository.get_by_id(&operation.task_id).await? {
        let mut metadata = task
            .metadata
            .as_deref()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        let key = match operation.direction {
            BranchUpdateDirection::PlanBranch => "last_plan_freshness_check_at",
            BranchUpdateDirection::TaskBranch => "last_task_freshness_check_at",
        };
        metadata[key] = serde_json::Value::String(chrono::Utc::now().to_rfc3339());
        metadata
            .as_object_mut()
            .expect("freshness metadata is an object")
            .remove("last_freshness_check_at");
        task.metadata = Some(metadata.to_string());
        task_repository
            .update_metadata(&operation.task_id, task.metadata)
            .await?;
    }

    if operation.continuation == BranchUpdateContinuation::FinalizePostMergePrPublication {
        return Ok(BranchUpdateExecutionOutcome::ContinuationPending);
    }

    let claim_id = uuid::Uuid::new_v4().to_string();
    let idempotency_key = format!("{}:{resulting_sha}", operation.id.as_str());
    let claimed = repository
        .claim_continuation(ClaimBranchUpdateContinuation {
            operation_id: operation.id.clone(),
            task_id: operation.task_id.clone(),
            originating_history_id: operation.originating_history_id.clone(),
            update_status,
            claim_id: claim_id.clone(),
            idempotency_key: idempotency_key.clone(),
        })
        .await?;
    if claimed != BranchUpdateCasOutcome::Applied {
        return Err(AppError::Conflict(format!(
            "Branch update continuation claim failed: {claimed:?}"
        )));
    }
    let next_status = destination(operation.continuation);
    let completed = repository
        .complete_continuation(CompleteBranchUpdateContinuation {
            operation_id: operation.id.clone(),
            task_id: operation.task_id.clone(),
            originating_history_id: operation.originating_history_id.clone(),
            update_status,
            destination_status: next_status,
            owner,
            fencing_epoch,
            claim_id,
            idempotency_key,
            receipt: format!("programmatic:{resulting_sha}"),
            history_id: uuid::Uuid::new_v4().to_string(),
        })
        .await?;
    if completed != BranchUpdateCasOutcome::Applied {
        return Err(AppError::Conflict(format!(
            "Branch update continuation failed: {completed:?}"
        )));
    }
    Ok(BranchUpdateExecutionOutcome::Completed {
        destination: next_status,
    })
}

/// Finalize files edited by the branch-updater agent. Git index/ref/worktree
/// mutations remain backend-owned and pass through the same durable claim gate.
pub async fn complete_resolved_branch_update(
    repository: Arc<dyn BranchUpdateRepository>,
    task_repository: Arc<dyn TaskRepository>,
    repo_path: &Path,
    operation: &BranchUpdateOperation,
    update_status: InternalStatus,
) -> AppResult<InternalStatus> {
    let workspace = operation.workspace_path.as_ref().ok_or_else(|| {
        AppError::Validation("Branch update operation has no workspace path".to_string())
    })?;
    let workspace = crate::utils::path_safety::validate_absolute_non_root_path(
        workspace,
        "resolved branch update workspace",
    )?;
    let owner =
        GitTargetLeaseOwner::branch_update(operation.task_id.as_str(), operation.id.as_str());
    let epoch = operation.target_lease_epoch;
    let expected_target = operation.observed_target_sha.as_deref().ok_or_else(|| {
        AppError::Validation("Branch update is missing its observed target SHA".to_string())
    })?;
    let expected_source = operation.observed_source_sha.as_deref().ok_or_else(|| {
        AppError::Validation("Branch update is missing its observed source SHA".to_string())
    })?;
    let mut resulting_sha = operation.resulting_sha.clone();
    if workspace.exists() {
        ensure_registered_workspace(repo_path, &workspace).await?;
        let workspace_head = read_ref(&workspace, "HEAD").await?;
        if let Some(checkpointed) = resulting_sha.as_deref() {
            if workspace_head != checkpointed {
                return Err(AppError::Conflict(format!(
                    "Resolved branch update workspace HEAD differs from checkpoint ({checkpointed}->{workspace_head})"
                )));
            }
        } else if !GitService::is_merge_in_progress(&workspace) && workspace_head != expected_target
        {
            if !is_strict_ancestor(&workspace, expected_source, &workspace_head).await?
                || !is_strict_ancestor(&workspace, expected_target, &workspace_head).await?
            {
                return Err(AppError::Conflict(
                    "Resolved branch update commit does not contain both preflight tips".into(),
                ));
            }
            checkpoint_result(
                &repository,
                operation,
                update_status,
                &owner,
                epoch,
                &workspace_head,
            )
            .await?;
            resulting_sha = Some(workspace_head);
        } else {
            let conflicts = if operation.conflict_files.is_empty() {
                GitService::get_conflict_files(&workspace).await?
            } else {
                operation.conflict_files.clone()
            };
            if conflicts.is_empty() {
                return Err(AppError::Validation(
                    "Resolved branch update has no persisted conflict paths".to_string(),
                ));
            }
            let mut add_args = vec!["add".to_string(), "--".to_string()];
            for path in &conflicts {
                if path.is_absolute()
                    || path.components().any(|component| {
                        matches!(
                            component,
                            std::path::Component::ParentDir | std::path::Component::RootDir
                        )
                    })
                {
                    return Err(AppError::Validation(format!(
                        "Unsafe branch update conflict path: {}",
                        path.display()
                    )));
                }
                add_args.push(path.to_string_lossy().into_owned());
            }
            let add_refs: Vec<&str> = add_args.iter().map(String::as_str).collect();
            let add = run_authorized_mutation(
                &add_refs,
                &workspace,
                authority(
                    Arc::clone(&repository),
                    operation,
                    &owner,
                    epoch,
                    GitMutationKind::Merge,
                )
                .await?,
            )
            .await?;
            if !add.status.success() {
                return Err(AppError::GitOperation(
                    String::from_utf8_lossy(&add.stderr).into_owned(),
                ));
            }
            let unresolved = GitService::get_conflict_files(&workspace).await?;
            if !unresolved.is_empty() {
                return Err(AppError::Conflict(format!(
                    "Unresolved branch update paths remain: {}",
                    unresolved
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
            if GitService::has_conflict_markers(&workspace).await? {
                return Err(AppError::Conflict(
                    "Resolved branch update still contains conflict markers".to_string(),
                ));
            }
            let commit = run_authorized_mutation(
                &["-c", "core.editor=true", "commit", "--no-edit"],
                &workspace,
                authority(
                    Arc::clone(&repository),
                    operation,
                    &owner,
                    epoch,
                    GitMutationKind::Merge,
                )
                .await?,
            )
            .await?;
            if !commit.status.success() {
                return Err(AppError::GitOperation(
                    String::from_utf8_lossy(&commit.stderr).into_owned(),
                ));
            }
            let committed_sha = read_ref(&workspace, "HEAD").await?;
            checkpoint_result(
                &repository,
                operation,
                update_status,
                &owner,
                epoch,
                &committed_sha,
            )
            .await?;
            resulting_sha = Some(committed_sha);
        }
    }
    let resulting_sha = resulting_sha.ok_or_else(|| {
        AppError::Conflict("Resolved branch update has no durable result checkpoint".into())
    })?;
    let target_before_update = read_ref(repo_path, &operation.target_branch).await?;
    if target_before_update == expected_target {
        let updated = run_authorized_mutation(
            &[
                "update-ref",
                operation.target_identity.full_ref(),
                &resulting_sha,
                expected_target,
            ],
            repo_path,
            authority(
                Arc::clone(&repository),
                operation,
                &owner,
                epoch,
                GitMutationKind::Merge,
            )
            .await?,
        )
        .await?;
        if !updated.status.success() {
            return Err(AppError::GitOperation(
                String::from_utf8_lossy(&updated.stderr).into_owned(),
            ));
        }
    } else if target_before_update != resulting_sha {
        return Err(AppError::Conflict(format!(
            "Resolved branch update target differs from both preflight and checkpoint ({expected_target}->{target_before_update}, checkpoint {resulting_sha})"
        )));
    }
    let workspace_arg = workspace.to_string_lossy().into_owned();
    if workspace.exists() {
        ensure_registered_workspace(repo_path, &workspace).await?;
        let removed = run_authorized_mutation(
            &["worktree", "remove", "--force", &workspace_arg],
            repo_path,
            authority(
                Arc::clone(&repository),
                operation,
                &owner,
                epoch,
                GitMutationKind::WorktreeDelete,
            )
            .await?,
        )
        .await?;
        if !removed.status.success() {
            return Err(AppError::GitOperation(
                String::from_utf8_lossy(&removed.stderr).into_owned(),
            ));
        }
    }
    let settled = repository
        .settle_programmatic(SettleBranchUpdateProgrammatic {
            operation_id: operation.id.clone(),
            task_id: operation.task_id.clone(),
            originating_history_id: operation.originating_history_id.clone(),
            update_status,
            owner: owner.clone(),
            fencing_epoch: epoch,
            resulting_sha: resulting_sha.clone(),
        })
        .await?;
    if settled != BranchUpdateCasOutcome::Applied {
        return Err(AppError::Conflict(format!(
            "Resolved branch update settlement failed: {settled:?}"
        )));
    }
    if let Some(task) = task_repository.get_by_id(&operation.task_id).await? {
        let mut metadata = task
            .metadata
            .as_deref()
            .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok())
            .unwrap_or_else(|| serde_json::json!({}));
        let key = match operation.direction {
            BranchUpdateDirection::PlanBranch => "last_plan_freshness_check_at",
            BranchUpdateDirection::TaskBranch => "last_task_freshness_check_at",
        };
        metadata[key] = serde_json::Value::String(chrono::Utc::now().to_rfc3339());
        task_repository
            .update_metadata(&operation.task_id, Some(metadata.to_string()))
            .await?;
    }
    if operation.continuation == BranchUpdateContinuation::FinalizePostMergePrPublication {
        return Ok(update_status);
    }
    let claim_id = uuid::Uuid::new_v4().to_string();
    let idempotency_key = format!("{}:{resulting_sha}", operation.id.as_str());
    if repository
        .claim_continuation(ClaimBranchUpdateContinuation {
            operation_id: operation.id.clone(),
            task_id: operation.task_id.clone(),
            originating_history_id: operation.originating_history_id.clone(),
            update_status,
            claim_id: claim_id.clone(),
            idempotency_key: idempotency_key.clone(),
        })
        .await?
        != BranchUpdateCasOutcome::Applied
    {
        return Err(AppError::Conflict(
            "Resolved branch update continuation claim failed".to_string(),
        ));
    }
    let next_status = destination(operation.continuation);
    let completed = repository
        .complete_continuation(CompleteBranchUpdateContinuation {
            operation_id: operation.id.clone(),
            task_id: operation.task_id.clone(),
            originating_history_id: operation.originating_history_id.clone(),
            update_status,
            destination_status: next_status,
            owner,
            fencing_epoch: epoch,
            claim_id,
            idempotency_key,
            receipt: format!("resolved:{resulting_sha}"),
            history_id: uuid::Uuid::new_v4().to_string(),
        })
        .await?;
    if completed != BranchUpdateCasOutcome::Applied {
        return Err(AppError::Conflict(format!(
            "Resolved branch update continuation failed: {completed:?}"
        )));
    }
    Ok(next_status)
}

/// Publish a locally-settled post-merge plan-branch update and finish the task only
/// after `origin` reports the exact resulting commit. The target lease handoff,
/// push, remote-head observation, continuation claim, and final CAS are all
/// restart-safe: a crash at any point leaves the operation pending under one
/// durable owner and a retry can safely repeat the fast-forward push.
pub async fn publish_post_merge_branch_update(
    repository: Arc<dyn BranchUpdateRepository>,
    repo_path: &Path,
    operation_snapshot: &BranchUpdateOperation,
    update_status: InternalStatus,
) -> AppResult<InternalStatus> {
    let operation = repository
        .get_operation(&operation_snapshot.id)
        .await?
        .ok_or_else(|| AppError::Conflict("Branch update operation is missing".to_string()))?;
    if operation.task_id != operation_snapshot.task_id
        || operation.originating_history_id != operation_snapshot.originating_history_id
    {
        return Err(AppError::Conflict(
            "Branch update publication snapshot does not match durable operation".to_string(),
        ));
    }
    if operation.continuation != BranchUpdateContinuation::FinalizePostMergePrPublication
        || !matches!(
            operation.phase,
            BranchUpdatePhase::ContinuationPending | BranchUpdatePhase::ContinuationInProgress
        )
        || operation.settled_at.is_some()
    {
        return Err(AppError::Conflict(
            "Branch update is not awaiting post-merge publication".to_string(),
        ));
    }
    let resulting_sha = operation.resulting_sha.as_deref().ok_or_else(|| {
        AppError::Validation("Post-merge publication continuation has no resulting SHA".to_string())
    })?;
    let local_sha = read_ref(repo_path, operation.target_identity.full_ref()).await?;
    if local_sha != resulting_sha {
        return Err(AppError::Conflict(format!(
            "Post-merge publication target changed after settlement ({resulting_sha}->{local_sha})"
        )));
    }

    let branch_owner =
        GitTargetLeaseOwner::branch_update(operation.task_id.as_str(), operation.id.as_str());
    let publication_owner = GitTargetLeaseOwner::publication_recovery(
        operation.task_id.as_str(),
        operation.id.as_str(),
    );
    let lease = repository
        .get_target_lease(&operation.target_identity)
        .await?
        .ok_or_else(|| {
            AppError::Conflict("Post-merge publication target lease is missing".to_string())
        })?;
    let publication_epoch = match lease.owner().kind {
        GitTargetLeaseOwnerKind::BranchUpdateOperation
            if lease.owner() == &branch_owner
                && lease.fencing_epoch() == operation.target_lease_epoch =>
        {
            match repository
                .transfer_operation_target_lease(TransferBranchUpdateTargetLease {
                    operation_id: operation.id.clone(),
                    task_id: operation.task_id.clone(),
                    originating_history_id: operation.originating_history_id.clone(),
                    update_status,
                    owner: branch_owner,
                    fencing_epoch: operation.target_lease_epoch,
                    next_owner: publication_owner.clone(),
                })
                .await?
            {
                crate::domain::repositories::GitAuthorityCasOutcome::Applied { fencing_epoch } => {
                    fencing_epoch
                }
                outcome => {
                    return Err(AppError::Conflict(format!(
                        "Post-merge publication lease handoff failed: {outcome:?}"
                    )))
                }
            }
        }
        GitTargetLeaseOwnerKind::PublicationRecovery
            if lease.owner() == &publication_owner
                && lease.fencing_epoch() == operation.target_lease_epoch =>
        {
            lease.fencing_epoch()
        }
        _ => {
            return Err(AppError::Conflict(format!(
                "Post-merge publication target is owned by {:?}",
                lease.owner().kind
            )))
        }
    };

    let full_ref = operation.target_identity.full_ref();
    let refspec = format!("{full_ref}:{full_ref}");
    let push = run_authorized_mutation(
        &["push", "origin", &refspec],
        repo_path,
        authority(
            Arc::clone(&repository),
            &operation,
            &publication_owner,
            publication_epoch,
            GitMutationKind::Push,
        )
        .await?,
    )
    .await?;
    if !push.status.success() {
        return Err(AppError::GitOperation(format!(
            "Post-merge publication push failed: {}",
            String::from_utf8_lossy(&push.stderr).trim()
        )));
    }

    let remote = crate::application::git_service::git_cmd::run(
        &["ls-remote", "--exit-code", "origin", full_ref],
        repo_path,
    )
    .await?;
    if !remote.status.success() {
        return Err(AppError::GitOperation(format!(
            "Post-merge publication receipt lookup failed: {}",
            String::from_utf8_lossy(&remote.stderr).trim()
        )));
    }
    let remote_output = String::from_utf8_lossy(&remote.stdout);
    let remote_sha = remote_output
        .lines()
        .find_map(|line| line.split_whitespace().next())
        .ok_or_else(|| {
            AppError::GitOperation(
                "Post-merge publication receipt did not contain a remote SHA".to_string(),
            )
        })?;
    if remote_sha != resulting_sha {
        return Err(AppError::Conflict(format!(
            "Post-merge publication receipt mismatch (expected {resulting_sha}, observed {remote_sha})"
        )));
    }

    let (claim_id, idempotency_key) = if operation.phase == BranchUpdatePhase::ContinuationPending {
        let claim_id = uuid::Uuid::new_v4().to_string();
        let idempotency_key = format!("publish:{full_ref}:{resulting_sha}");
        if repository
            .claim_continuation(ClaimBranchUpdateContinuation {
                operation_id: operation.id.clone(),
                task_id: operation.task_id.clone(),
                originating_history_id: operation.originating_history_id.clone(),
                update_status,
                claim_id: claim_id.clone(),
                idempotency_key: idempotency_key.clone(),
            })
            .await?
            != BranchUpdateCasOutcome::Applied
        {
            return Err(AppError::Conflict(
                "Post-merge publication continuation claim failed".to_string(),
            ));
        }
        (claim_id, idempotency_key)
    } else {
        (
            operation.continuation_claim_id.clone().ok_or_else(|| {
                AppError::Conflict(
                    "In-progress publication continuation has no durable claim".to_string(),
                )
            })?,
            operation
                .continuation_idempotency_key
                .clone()
                .ok_or_else(|| {
                    AppError::Conflict(
                        "In-progress publication continuation has no idempotency key".to_string(),
                    )
                })?,
        )
    };
    let receipt = format!("origin:{full_ref}:{remote_sha}");
    let completed = repository
        .complete_continuation(CompleteBranchUpdateContinuation {
            operation_id: operation.id.clone(),
            task_id: operation.task_id.clone(),
            originating_history_id: operation.originating_history_id.clone(),
            update_status,
            destination_status: InternalStatus::Merged,
            owner: publication_owner,
            fencing_epoch: publication_epoch,
            claim_id,
            idempotency_key,
            receipt,
            history_id: uuid::Uuid::new_v4().to_string(),
        })
        .await?;
    if completed != BranchUpdateCasOutcome::Applied {
        return Err(AppError::Conflict(format!(
            "Post-merge publication continuation failed: {completed:?}"
        )));
    }
    Ok(InternalStatus::Merged)
}

/// Resume an ordinary continuation after a crash between local settlement and
/// the final status CAS. Existing in-progress claim ids are reused exactly;
/// pending continuations receive one new durable claim.
pub async fn resume_branch_update_continuation(
    repository: Arc<dyn BranchUpdateRepository>,
    operation_snapshot: &BranchUpdateOperation,
    update_status: InternalStatus,
) -> AppResult<InternalStatus> {
    let operation = repository
        .get_operation(&operation_snapshot.id)
        .await?
        .ok_or_else(|| AppError::Conflict("Branch update operation is missing".to_string()))?;
    if operation.task_id != operation_snapshot.task_id
        || operation.originating_history_id != operation_snapshot.originating_history_id
        || operation.continuation == BranchUpdateContinuation::FinalizePostMergePrPublication
        || !matches!(
            operation.phase,
            BranchUpdatePhase::ContinuationPending | BranchUpdatePhase::ContinuationInProgress
        )
    {
        return Err(AppError::Conflict(
            "Branch update is not awaiting an ordinary continuation".to_string(),
        ));
    }
    let resulting_sha = operation.resulting_sha.as_deref().ok_or_else(|| {
        AppError::Validation("Branch update continuation has no resulting SHA".to_string())
    })?;
    let owner =
        GitTargetLeaseOwner::branch_update(operation.task_id.as_str(), operation.id.as_str());
    let lease = repository
        .get_target_lease(&operation.target_identity)
        .await?
        .ok_or_else(|| AppError::Conflict("Branch update target lease is missing".to_string()))?;
    if lease.owner() != &owner
        || lease.fencing_epoch() != operation.target_lease_epoch
        || lease.active_mutation().is_some()
    {
        return Err(AppError::Conflict(
            "Branch update continuation no longer owns clean target authority".to_string(),
        ));
    }
    let (claim_id, idempotency_key) = if operation.phase == BranchUpdatePhase::ContinuationPending {
        let claim_id = uuid::Uuid::new_v4().to_string();
        let idempotency_key = format!("{}:{resulting_sha}", operation.id.as_str());
        if repository
            .claim_continuation(ClaimBranchUpdateContinuation {
                operation_id: operation.id.clone(),
                task_id: operation.task_id.clone(),
                originating_history_id: operation.originating_history_id.clone(),
                update_status,
                claim_id: claim_id.clone(),
                idempotency_key: idempotency_key.clone(),
            })
            .await?
            != BranchUpdateCasOutcome::Applied
        {
            return Err(AppError::Conflict(
                "Branch update continuation claim failed".to_string(),
            ));
        }
        (claim_id, idempotency_key)
    } else {
        (
            operation.continuation_claim_id.clone().ok_or_else(|| {
                AppError::Conflict("In-progress continuation has no durable claim".to_string())
            })?,
            operation
                .continuation_idempotency_key
                .clone()
                .ok_or_else(|| {
                    AppError::Conflict(
                        "In-progress continuation has no idempotency key".to_string(),
                    )
                })?,
        )
    };
    let next_status = destination(operation.continuation);
    let completed = repository
        .complete_continuation(CompleteBranchUpdateContinuation {
            operation_id: operation.id.clone(),
            task_id: operation.task_id.clone(),
            originating_history_id: operation.originating_history_id.clone(),
            update_status,
            destination_status: next_status,
            owner,
            fencing_epoch: operation.target_lease_epoch,
            claim_id,
            idempotency_key,
            receipt: format!("recovered:{resulting_sha}"),
            history_id: uuid::Uuid::new_v4().to_string(),
        })
        .await?;
    if completed != BranchUpdateCasOutcome::Applied {
        return Err(AppError::Conflict(format!(
            "Branch update continuation recovery failed: {completed:?}"
        )));
    }
    Ok(next_status)
}

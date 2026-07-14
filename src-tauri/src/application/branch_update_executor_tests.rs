use super::branch_update_executor::{
    complete_resolved_branch_update, execute_programmatic_branch_update,
    publish_post_merge_branch_update, BranchUpdateExecutionOutcome,
};
use crate::application::GitService;
use crate::domain::entities::{
    BranchUpdateCapacityOwnership, BranchUpdateContinuation, BranchUpdateDirection,
    BranchUpdateOperation, BranchUpdatePhase, BranchUpdateWorkspaceOwnership, GitTargetLeaseOwner,
    InternalStatus,
};
use crate::domain::repositories::{
    BranchUpdateActivation, BranchUpdateActivationOutcome, BranchUpdateRepository,
    CheckpointBranchUpdateResult, TaskRepository,
};
use crate::infrastructure::sqlite::{SqliteBranchUpdateRepository, SqliteTaskRepository};
use crate::testing::SqliteTestDb;
use chrono::Utc;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

fn git(repo: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init_branches(conflict: bool) -> tempfile::TempDir {
    let repository = tempfile::tempdir().unwrap();
    git(repository.path(), &["init", "-b", "main"]);
    git(
        repository.path(),
        &["config", "user.email", "test@example.com"],
    );
    git(repository.path(), &["config", "user.name", "Test User"]);
    fs::write(repository.path().join("shared.txt"), "base\n").unwrap();
    git(repository.path(), &["add", "shared.txt"]);
    git(repository.path(), &["commit", "-m", "base"]);
    git(repository.path(), &["branch", "target"]);
    git(repository.path(), &["checkout", "-b", "source"]);
    fs::write(
        repository.path().join("shared.txt"),
        if conflict {
            "source\n"
        } else {
            "base\nsource\n"
        },
    )
    .unwrap();
    git(repository.path(), &["add", "shared.txt"]);
    git(repository.path(), &["commit", "-m", "source"]);
    if conflict {
        git(repository.path(), &["checkout", "target"]);
        fs::write(repository.path().join("shared.txt"), "target\n").unwrap();
        git(repository.path(), &["add", "shared.txt"]);
        git(repository.path(), &["commit", "-m", "target"]);
    } else {
        git(repository.path(), &["checkout", "main"]);
    }
    repository
}

async fn setup(
    repository: &std::path::Path,
    workspace: PathBuf,
) -> (
    Arc<SqliteBranchUpdateRepository>,
    Arc<SqliteTaskRepository>,
    BranchUpdateOperation,
    u64,
) {
    setup_with_continuation(
        repository,
        workspace,
        BranchUpdateContinuation::ResumeExecution,
    )
    .await
}

async fn setup_with_continuation(
    repository: &std::path::Path,
    workspace: PathBuf,
    continuation: BranchUpdateContinuation,
) -> (
    Arc<SqliteBranchUpdateRepository>,
    Arc<SqliteTaskRepository>,
    BranchUpdateOperation,
    u64,
) {
    let db = SqliteTestDb::new("branch-update-executor");
    let project = db.seed_project("project");
    let task = db.seed_task(project.id, "task");
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE tasks SET internal_status = 'executing' WHERE id = ?1",
            [task.id.as_str()],
        )
        .unwrap();
    });
    let shared = db.shared_conn();
    let branch_repo = Arc::new(SqliteBranchUpdateRepository::from_shared(shared.clone()));
    let task_repo = Arc::new(SqliteTaskRepository::from_shared(shared));
    let identity = GitService::canonical_target_identity(repository, "target")
        .await
        .unwrap();
    let mut operation = BranchUpdateOperation::new(
        task.id,
        BranchUpdateDirection::PlanBranch,
        continuation,
        "executor-history",
        "source",
        "target",
        BranchUpdateWorkspaceOwnership::OperationWorktree,
        BranchUpdateCapacityOwnership::Inherited,
        identity,
        Utc::now(),
    );
    operation.workspace_path = Some(workspace);
    operation.observed_source_sha = Some(
        GitService::resolve_ref_sha(repository, "source")
            .await
            .unwrap(),
    );
    operation.observed_target_sha = Some(
        GitService::resolve_ref_sha(repository, "target")
            .await
            .unwrap(),
    );
    let BranchUpdateActivationOutcome::Applied { fencing_epoch, .. } = branch_repo
        .activate(BranchUpdateActivation {
            operation: operation.clone(),
            expected_status: InternalStatus::Executing,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "test".into(),
        })
        .await
        .unwrap()
    else {
        panic!("activation should apply");
    };
    (branch_repo, task_repo, operation, fencing_epoch)
}

#[tokio::test]
async fn post_merge_publication_never_marks_merged_before_publication_receipt() {
    let repository = init_branches(false);
    let workspace_parent = tempfile::tempdir().unwrap();
    let workspace = workspace_parent.path().join("operation");
    let (branch_repo, task_repo, operation, epoch) = setup_with_continuation(
        repository.path(),
        workspace,
        BranchUpdateContinuation::FinalizePostMergePrPublication,
    )
    .await;

    let outcome = execute_programmatic_branch_update(
        branch_repo.clone(),
        task_repo.clone(),
        repository.path(),
        &operation,
        InternalStatus::UpdatingPlanBranch,
        epoch,
    )
    .await
    .unwrap();

    assert_eq!(outcome, BranchUpdateExecutionOutcome::ContinuationPending);
    let task = task_repo
        .get_by_id(&operation.task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(task.internal_status, InternalStatus::UpdatingPlanBranch);
    let stored = branch_repo
        .get_operation(&operation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.phase, BranchUpdatePhase::ContinuationPending);
    assert!(stored.continuation_receipt.is_none());
}

#[tokio::test]
async fn post_merge_publication_transfers_authority_and_requires_matching_remote_receipt() {
    let repository = init_branches(false);
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare"]);
    git(
        repository.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    git(repository.path(), &["push", "origin", "target"]);
    let workspace_parent = tempfile::tempdir().unwrap();
    let workspace = workspace_parent.path().join("operation");
    let (branch_repo, task_repo, operation, epoch) = setup_with_continuation(
        repository.path(),
        workspace,
        BranchUpdateContinuation::FinalizePostMergePrPublication,
    )
    .await;
    assert_eq!(
        execute_programmatic_branch_update(
            branch_repo.clone(),
            task_repo.clone(),
            repository.path(),
            &operation,
            InternalStatus::UpdatingPlanBranch,
            epoch,
        )
        .await
        .unwrap(),
        BranchUpdateExecutionOutcome::ContinuationPending
    );
    let pending = branch_repo
        .get_operation(&operation.id)
        .await
        .unwrap()
        .unwrap();

    let destination = publish_post_merge_branch_update(
        branch_repo.clone(),
        repository.path(),
        &pending,
        InternalStatus::UpdatingPlanBranch,
    )
    .await
    .unwrap();

    assert_eq!(destination, InternalStatus::Merged);
    let task = task_repo
        .get_by_id(&operation.task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(task.internal_status, InternalStatus::Merged);
    let stored = branch_repo
        .get_operation(&operation.id)
        .await
        .unwrap()
        .unwrap();
    let resulting_sha = stored.resulting_sha.as_deref().unwrap();
    let expected_receipt = format!("origin:refs/heads/target:{resulting_sha}");
    assert_eq!(stored.phase, BranchUpdatePhase::Settled);
    assert_eq!(
        stored.continuation_receipt.as_deref(),
        Some(expected_receipt.as_str())
    );
    let remote_target = Command::new("git")
        .args(["rev-parse", "refs/heads/target"])
        .current_dir(remote.path())
        .output()
        .unwrap();
    assert!(remote_target.status.success());
    assert_eq!(
        String::from_utf8_lossy(&remote_target.stdout).trim(),
        resulting_sha
    );
}

#[tokio::test]
async fn programmatic_update_fences_each_git_mutation_and_resumes_origin() {
    let repository = init_branches(false);
    let workspace_parent = tempfile::tempdir().unwrap();
    let workspace = workspace_parent.path().join("operation");
    let (branch_repo, task_repo, operation, epoch) =
        setup(repository.path(), workspace.clone()).await;

    let outcome = execute_programmatic_branch_update(
        branch_repo.clone(),
        task_repo.clone(),
        repository.path(),
        &operation,
        InternalStatus::UpdatingPlanBranch,
        epoch,
    )
    .await
    .unwrap();

    assert_eq!(
        outcome,
        BranchUpdateExecutionOutcome::Completed {
            destination: InternalStatus::Executing
        }
    );
    assert!(!workspace.exists());
    assert!(
        GitService::is_ancestor(repository.path(), "source", "target")
            .await
            .unwrap()
    );
    let task = task_repo
        .get_by_id(&operation.task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(task.internal_status, InternalStatus::Executing);
    let stored = branch_repo
        .get_operation(&operation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.phase, BranchUpdatePhase::Settled);
    assert!(branch_repo
        .list_in_flight_mutations()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn conflicting_update_keeps_operation_workspace_and_authority_for_branch_updater() {
    let repository = init_branches(true);
    let workspace_parent = tempfile::tempdir().unwrap();
    let workspace = workspace_parent.path().join("operation");
    let (branch_repo, task_repo, operation, epoch) =
        setup(repository.path(), workspace.clone()).await;

    let outcome = execute_programmatic_branch_update(
        branch_repo.clone(),
        task_repo.clone(),
        repository.path(),
        &operation,
        InternalStatus::UpdatingPlanBranch,
        epoch,
    )
    .await
    .unwrap();

    assert_eq!(outcome, BranchUpdateExecutionOutcome::NeedsAgent);
    assert!(workspace.exists());
    let task = task_repo
        .get_by_id(&operation.task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(task.internal_status, InternalStatus::UpdatingPlanBranch);
    let stored = branch_repo
        .get_operation(&operation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.phase, BranchUpdatePhase::Resolving);
    assert!(!stored.conflict_files.is_empty());
    assert!(branch_repo
        .list_in_flight_mutations()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn backend_finalizes_agent_edited_conflicts_and_exact_continuation() {
    let repository = init_branches(true);
    let workspace_parent = tempfile::tempdir().unwrap();
    let workspace = workspace_parent.path().join("operation");
    let (branch_repo, task_repo, operation, epoch) =
        setup(repository.path(), workspace.clone()).await;

    let outcome = execute_programmatic_branch_update(
        branch_repo.clone(),
        task_repo.clone(),
        repository.path(),
        &operation,
        InternalStatus::UpdatingPlanBranch,
        epoch,
    )
    .await
    .unwrap();
    assert_eq!(outcome, BranchUpdateExecutionOutcome::NeedsAgent);

    fs::write(workspace.join("shared.txt"), "resolved source and target\n").unwrap();
    let resolving = branch_repo
        .get_active_operation(&operation.task_id)
        .await
        .unwrap()
        .unwrap();
    let destination = complete_resolved_branch_update(
        branch_repo.clone(),
        task_repo.clone(),
        repository.path(),
        &resolving,
        InternalStatus::UpdatingPlanBranch,
    )
    .await
    .unwrap();

    assert_eq!(destination, InternalStatus::Executing);
    assert!(!workspace.exists());
    assert!(
        GitService::is_ancestor(repository.path(), "source", "target")
            .await
            .unwrap()
    );
    let task = task_repo
        .get_by_id(&operation.task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(task.internal_status, InternalStatus::Executing);
    let stored = branch_repo
        .get_operation(&operation.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.phase, BranchUpdatePhase::Settled);
    assert!(branch_repo
        .list_in_flight_mutations()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn restart_adopts_resolved_merge_commit_created_before_checkpoint() {
    let repository = init_branches(true);
    let workspace_parent = tempfile::tempdir().unwrap();
    let workspace = workspace_parent.path().join("operation");
    let (branch_repo, task_repo, operation, epoch) =
        setup(repository.path(), workspace.clone()).await;
    assert_eq!(
        execute_programmatic_branch_update(
            branch_repo.clone(),
            task_repo.clone(),
            repository.path(),
            &operation,
            InternalStatus::UpdatingPlanBranch,
            epoch,
        )
        .await
        .unwrap(),
        BranchUpdateExecutionOutcome::NeedsAgent
    );
    fs::write(workspace.join("shared.txt"), "resolved before crash\n").unwrap();
    git(&workspace, &["add", "shared.txt"]);
    git(
        &workspace,
        &["-c", "core.editor=true", "commit", "--no-edit"],
    );
    let resolving = branch_repo
        .get_active_operation(&operation.task_id)
        .await
        .unwrap()
        .unwrap();

    let destination = complete_resolved_branch_update(
        branch_repo.clone(),
        task_repo.clone(),
        repository.path(),
        &resolving,
        InternalStatus::UpdatingPlanBranch,
    )
    .await
    .unwrap();

    assert_eq!(destination, InternalStatus::Executing);
    assert!(!workspace.exists());
    assert!(
        GitService::is_ancestor(repository.path(), "source", "target")
            .await
            .unwrap()
    );
    assert_eq!(
        branch_repo
            .get_operation(&operation.id)
            .await
            .unwrap()
            .unwrap()
            .phase,
        BranchUpdatePhase::Settled
    );
}

#[tokio::test]
async fn backend_rejects_agent_completion_when_conflict_markers_remain() {
    let repository = init_branches(true);
    let workspace_parent = tempfile::tempdir().unwrap();
    let workspace = workspace_parent.path().join("operation");
    let (branch_repo, task_repo, operation, epoch) =
        setup(repository.path(), workspace.clone()).await;
    let target_before = GitService::resolve_ref_sha(repository.path(), "target")
        .await
        .unwrap();

    let outcome = execute_programmatic_branch_update(
        branch_repo.clone(),
        task_repo.clone(),
        repository.path(),
        &operation,
        InternalStatus::UpdatingPlanBranch,
        epoch,
    )
    .await
    .unwrap();
    assert_eq!(outcome, BranchUpdateExecutionOutcome::NeedsAgent);
    fs::write(
        workspace.join("shared.txt"),
        "<<<<<<< ours\ntarget\n=======\nsource\n>>>>>>> theirs\n",
    )
    .unwrap();
    let resolving = branch_repo
        .get_active_operation(&operation.task_id)
        .await
        .unwrap()
        .unwrap();

    let error = complete_resolved_branch_update(
        branch_repo.clone(),
        task_repo,
        repository.path(),
        &resolving,
        InternalStatus::UpdatingPlanBranch,
    )
    .await
    .expect_err("conflict markers must fail closed");

    assert!(error.to_string().contains("conflict marker"));
    assert!(workspace.exists());
    assert_eq!(
        GitService::resolve_ref_sha(repository.path(), "target")
            .await
            .unwrap(),
        target_before
    );
    assert!(branch_repo
        .list_in_flight_mutations()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn changed_source_tip_blocks_before_any_git_mutation() {
    let repository = init_branches(false);
    let workspace_parent = tempfile::tempdir().unwrap();
    let workspace = workspace_parent.path().join("operation");
    let (branch_repo, task_repo, operation, epoch) =
        setup(repository.path(), workspace.clone()).await;
    git(repository.path(), &["checkout", "source"]);
    fs::write(repository.path().join("late.txt"), "late\n").unwrap();
    git(repository.path(), &["add", "late.txt"]);
    git(repository.path(), &["commit", "-m", "late source change"]);
    let target_before = GitService::resolve_ref_sha(repository.path(), "target")
        .await
        .unwrap();

    let outcome = execute_programmatic_branch_update(
        branch_repo.clone(),
        task_repo.clone(),
        repository.path(),
        &operation,
        InternalStatus::UpdatingPlanBranch,
        epoch,
    )
    .await
    .unwrap();

    assert_eq!(outcome, BranchUpdateExecutionOutcome::Blocked);
    assert!(!workspace.exists());
    assert_eq!(
        GitService::resolve_ref_sha(repository.path(), "target")
            .await
            .unwrap(),
        target_before
    );
    let task = task_repo
        .get_by_id(&operation.task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(task.internal_status, InternalStatus::BranchUpdateBlocked);
    assert!(branch_repo
        .list_in_flight_mutations()
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn restart_adopts_operation_worktree_created_before_crash() {
    let repository = init_branches(false);
    let workspace_parent = tempfile::tempdir().unwrap();
    let workspace = workspace_parent.path().join("operation");
    let (branch_repo, task_repo, operation, epoch) =
        setup(repository.path(), workspace.clone()).await;
    let expected_target = operation.observed_target_sha.as_deref().unwrap();
    git(
        repository.path(),
        &[
            "worktree",
            "add",
            "--detach",
            workspace.to_str().unwrap(),
            expected_target,
        ],
    );

    let outcome = execute_programmatic_branch_update(
        branch_repo.clone(),
        task_repo.clone(),
        repository.path(),
        &operation,
        InternalStatus::UpdatingPlanBranch,
        epoch,
    )
    .await
    .unwrap();

    assert_eq!(
        outcome,
        BranchUpdateExecutionOutcome::Completed {
            destination: InternalStatus::Executing
        }
    );
    assert!(!workspace.exists());
    assert!(
        GitService::is_ancestor(repository.path(), "source", "target")
            .await
            .unwrap()
    );
    assert_eq!(
        task_repo
            .get_by_id(&operation.task_id)
            .await
            .unwrap()
            .unwrap()
            .internal_status,
        InternalStatus::Executing
    );
}

#[tokio::test]
async fn restart_adopts_exact_target_ref_updated_before_crash() {
    let repository = init_branches(false);
    let workspace_parent = tempfile::tempdir().unwrap();
    let workspace = workspace_parent.path().join("operation");
    let (branch_repo, task_repo, operation, epoch) =
        setup(repository.path(), workspace.clone()).await;
    let expected_target = operation.observed_target_sha.as_deref().unwrap();
    let workspace_arg = workspace.to_str().unwrap();
    git(
        repository.path(),
        &[
            "worktree",
            "add",
            "--detach",
            workspace_arg,
            expected_target,
        ],
    );
    git(&workspace, &["merge", "--no-edit", "source"]);
    let resulting_sha = GitService::resolve_ref_sha(&workspace, "HEAD")
        .await
        .unwrap();
    assert_eq!(
        branch_repo
            .checkpoint_result(CheckpointBranchUpdateResult {
                operation_id: operation.id.clone(),
                task_id: operation.task_id.clone(),
                originating_history_id: operation.originating_history_id.clone(),
                update_status: InternalStatus::UpdatingPlanBranch,
                owner: GitTargetLeaseOwner::branch_update(
                    operation.task_id.as_str(),
                    operation.id.as_str(),
                ),
                fencing_epoch: epoch,
                resulting_sha: resulting_sha.clone(),
            })
            .await
            .unwrap(),
        crate::domain::repositories::BranchUpdateCasOutcome::Applied
    );
    git(
        repository.path(),
        &[
            "update-ref",
            operation.target_identity.full_ref(),
            &resulting_sha,
            expected_target,
        ],
    );
    git(
        repository.path(),
        &["worktree", "remove", "--force", workspace_arg],
    );
    git(repository.path(), &["branch", "-D", "source"]);

    let outcome = execute_programmatic_branch_update(
        branch_repo.clone(),
        task_repo.clone(),
        repository.path(),
        &branch_repo
            .get_operation(&operation.id)
            .await
            .unwrap()
            .unwrap(),
        InternalStatus::UpdatingPlanBranch,
        epoch,
    )
    .await
    .unwrap();

    assert_eq!(
        outcome,
        BranchUpdateExecutionOutcome::Completed {
            destination: InternalStatus::Executing
        }
    );
    assert_eq!(
        GitService::resolve_ref_sha(repository.path(), "target")
            .await
            .unwrap(),
        resulting_sha
    );
    assert_eq!(
        task_repo
            .get_by_id(&operation.task_id)
            .await
            .unwrap()
            .unwrap()
            .internal_status,
        InternalStatus::Executing
    );
}

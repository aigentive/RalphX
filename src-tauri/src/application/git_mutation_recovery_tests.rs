use super::git_mutation_recovery::{recover_in_flight_git_mutations, GitMutationRecoveryOutcome};
use crate::application::GitService;
use crate::domain::entities::{
    BranchUpdateCapacityOwnership, BranchUpdateContinuation, BranchUpdateDirection,
    BranchUpdateOperation, BranchUpdateWorkspaceOwnership, GitMutationKind, GitTargetLeaseOwner,
    InternalStatus, Project,
};
use crate::domain::repositories::{
    AcquireGitTargetLease, AcquireGitTargetLeaseOutcome, BeginGitMutation, BranchUpdateActivation,
    BranchUpdateActivationOutcome, BranchUpdateRepository,
};
use crate::infrastructure::sqlite::{
    SqliteBranchUpdateRepository, SqliteProjectRepository, SqliteTaskRepository,
};
use crate::testing::SqliteTestDb;
use chrono::Utc;
use std::fs;
use std::process::Command;
use std::sync::Arc;

fn init_repository() -> tempfile::TempDir {
    let repository = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .args(args)
            .current_dir(repository.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-b", "main"]);
    git(&["config", "user.email", "test@example.com"]);
    git(&["config", "user.name", "Test User"]);
    fs::write(repository.path().join("README.md"), "test").unwrap();
    git(&["add", "README.md"]);
    git(&["commit", "-m", "initial"]);
    repository
}

async fn claimed_repository(
    workspace: &std::path::Path,
) -> (
    Arc<SqliteBranchUpdateRepository>,
    Arc<SqliteTaskRepository>,
    Arc<SqliteProjectRepository>,
    crate::domain::entities::GitTargetIdentity,
) {
    let db = SqliteTestDb::new("git-mutation-recovery");
    let project = db.seed_project("project");
    let task = db.seed_task(project.id, "task");
    let shared = db.shared_conn();
    let repository = Arc::new(SqliteBranchUpdateRepository::from_shared(Arc::clone(
        &shared,
    )));
    let task_repository = Arc::new(SqliteTaskRepository::from_shared(Arc::clone(&shared)));
    let project_repository = Arc::new(SqliteProjectRepository::from_shared(shared));
    let identity = GitService::canonical_target_identity(workspace, "main")
        .await
        .unwrap();
    let mut operation = BranchUpdateOperation::new(
        task.id.clone(),
        BranchUpdateDirection::PlanBranch,
        BranchUpdateContinuation::ResumeExecution,
        "recovery-history",
        "main",
        "main",
        BranchUpdateWorkspaceOwnership::OperationWorktree,
        BranchUpdateCapacityOwnership::Inherited,
        identity.clone(),
        Utc::now(),
    );
    operation.workspace_path = Some(workspace.to_path_buf());
    let owner = GitTargetLeaseOwner::branch_update(task.id.as_str(), operation.id.as_str());
    let BranchUpdateActivationOutcome::Applied { fencing_epoch, .. } = repository
        .activate(BranchUpdateActivation {
            operation,
            expected_status: InternalStatus::Backlog,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "test".into(),
        })
        .await
        .unwrap()
    else {
        panic!("activation should apply");
    };
    repository
        .begin_git_mutation(BeginGitMutation {
            identity: identity.clone(),
            owner,
            fencing_epoch,
            claim_id: "recovery-claim".into(),
            kind: GitMutationKind::Merge,
        })
        .await
        .unwrap();
    (repository, task_repository, project_repository, identity)
}

async fn claimed_merge_repository() -> (
    tempfile::TempDir,
    Arc<SqliteBranchUpdateRepository>,
    Arc<SqliteTaskRepository>,
    Arc<SqliteProjectRepository>,
    crate::domain::entities::GitTargetIdentity,
) {
    let git_repository = init_repository();
    let worktree_parent = tempfile::tempdir().unwrap();
    let db = SqliteTestDb::new("git-merge-mutation-recovery");
    let mut project = Project::new(
        "project".into(),
        git_repository.path().to_string_lossy().into_owned(),
    );
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().into_owned());
    let project = db.insert_project(project);
    let task = db.seed_task(project.id, "task");
    let shared = db.shared_conn();
    let repository = Arc::new(SqliteBranchUpdateRepository::from_shared(Arc::clone(
        &shared,
    )));
    let tasks = Arc::new(SqliteTaskRepository::from_shared(Arc::clone(&shared)));
    let projects = Arc::new(SqliteProjectRepository::from_shared(shared));
    let identity = GitService::canonical_target_identity(git_repository.path(), "main")
        .await
        .unwrap();
    let owner = GitTargetLeaseOwner::merge_attempt(
        task.id.as_str(),
        format!("pending-merge:{}:main", task.id.as_str()),
    );
    let AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch } = repository
        .acquire_target_lease(AcquireGitTargetLease {
            identity: identity.clone(),
            owner: owner.clone(),
        })
        .await
        .unwrap()
    else {
        panic!("merge attempt should acquire target authority");
    };
    repository
        .begin_git_mutation(BeginGitMutation {
            identity: identity.clone(),
            owner,
            fencing_epoch,
            claim_id: "merge-recovery-claim".into(),
            kind: GitMutationKind::Merge,
        })
        .await
        .unwrap();
    (git_repository, repository, tasks, projects, identity)
}

async fn claimed_repair_repository() -> (
    tempfile::TempDir,
    Arc<SqliteBranchUpdateRepository>,
    Arc<SqliteTaskRepository>,
    Arc<SqliteProjectRepository>,
    crate::domain::entities::GitTargetIdentity,
) {
    let git_repository = init_repository();
    let db = SqliteTestDb::new("git-repair-mutation-recovery");
    let project = db.seed_project("project");
    let task = db.seed_task(project.id, "task");
    let shared = db.shared_conn();
    let repository = Arc::new(SqliteBranchUpdateRepository::from_shared(Arc::clone(
        &shared,
    )));
    let tasks = Arc::new(SqliteTaskRepository::from_shared(Arc::clone(&shared)));
    let projects = Arc::new(SqliteProjectRepository::from_shared(shared));
    let identity = GitService::canonical_target_identity(git_repository.path(), "main")
        .await
        .unwrap();
    let owner = GitTargetLeaseOwner::agent_workspace_repair("durable-repair-attempt");
    let AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch } = repository
        .acquire_target_lease(AcquireGitTargetLease {
            identity: identity.clone(),
            owner: owner.clone(),
        })
        .await
        .unwrap()
    else {
        panic!("repair attempt should acquire target authority");
    };
    repository
        .begin_git_mutation(BeginGitMutation {
            identity: identity.clone(),
            owner,
            fencing_epoch,
            claim_id: format!("repair-recovery-claim:{}", task.id),
            kind: GitMutationKind::Push,
        })
        .await
        .unwrap();
    (git_repository, repository, tasks, projects, identity)
}

#[tokio::test]
async fn recovery_clears_claim_only_after_a_clean_workspace_inspection() {
    let workspace = init_repository();
    let (repository, tasks, projects, identity) = claimed_repository(workspace.path()).await;

    let outcomes = recover_in_flight_git_mutations(repository.clone(), tasks, projects)
        .await
        .unwrap();
    assert_eq!(
        outcomes,
        vec![GitMutationRecoveryOutcome::Cleared {
            claim_id: "recovery-claim".into()
        }]
    );
    assert!(repository
        .get_target_lease(&identity)
        .await
        .unwrap()
        .unwrap()
        .active_mutation()
        .is_none());
}

#[tokio::test]
async fn recovery_keeps_dirty_workspace_fenced_for_repair() {
    let workspace = init_repository();
    let (repository, tasks, projects, identity) = claimed_repository(workspace.path()).await;
    fs::write(workspace.path().join("README.md"), "dirty").unwrap();

    let outcomes = recover_in_flight_git_mutations(repository.clone(), tasks, projects)
        .await
        .unwrap();
    assert!(matches!(
        outcomes.as_slice(),
        [GitMutationRecoveryOutcome::NeedsRepair { .. }]
    ));
    assert!(repository
        .get_target_lease(&identity)
        .await
        .unwrap()
        .unwrap()
        .active_mutation()
        .is_some());
}

#[tokio::test]
async fn recovery_clears_clean_merge_attempt_claim_for_pending_merge_retry() {
    let (_git_repository, repository, tasks, projects, identity) = claimed_merge_repository().await;

    let outcomes = recover_in_flight_git_mutations(repository.clone(), tasks, projects)
        .await
        .unwrap();

    assert_eq!(
        outcomes,
        vec![GitMutationRecoveryOutcome::Cleared {
            claim_id: "merge-recovery-claim".into()
        }]
    );
    assert!(repository
        .get_target_lease(&identity)
        .await
        .unwrap()
        .unwrap()
        .active_mutation()
        .is_none());
}

#[tokio::test]
async fn generic_recovery_leaves_repair_owned_mutation_for_durable_attempt_reconciliation() {
    let (_git_repository, repository, tasks, projects, identity) =
        claimed_repair_repository().await;

    let outcomes = recover_in_flight_git_mutations(repository.clone(), tasks, projects)
        .await
        .unwrap();

    assert!(outcomes.is_empty());
    assert!(repository
        .get_target_lease(&identity)
        .await
        .unwrap()
        .unwrap()
        .active_mutation()
        .is_some());
}

#[tokio::test]
async fn recovery_clears_stable_dirty_merge_claim_but_keeps_target_owned() {
    let (git_repository, repository, tasks, projects, identity) = claimed_merge_repository().await;
    fs::write(git_repository.path().join("README.md"), "dirty").unwrap();

    let outcomes = recover_in_flight_git_mutations(repository.clone(), tasks, projects)
        .await
        .unwrap();

    assert_eq!(
        outcomes,
        vec![GitMutationRecoveryOutcome::Cleared {
            claim_id: "merge-recovery-claim".into()
        }]
    );
    let lease = repository
        .get_target_lease(&identity)
        .await
        .unwrap()
        .unwrap();
    assert!(lease.active_mutation().is_none());
    assert!(!lease.is_released());
    assert_eq!(
        lease.owner().kind,
        crate::domain::entities::GitTargetLeaseOwnerKind::MergeAttempt
    );
}

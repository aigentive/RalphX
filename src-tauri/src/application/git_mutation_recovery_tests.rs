use super::git_mutation_recovery::{recover_in_flight_git_mutations, GitMutationRecoveryOutcome};
use crate::application::GitService;
use crate::domain::entities::{
    BranchUpdateCapacityOwnership, BranchUpdateContinuation, BranchUpdateDirection,
    BranchUpdateOperation, BranchUpdateWorkspaceOwnership, GitMutationKind, GitTargetLeaseOwner,
    InternalStatus,
};
use crate::domain::repositories::{
    BeginGitMutation, BranchUpdateActivation, BranchUpdateActivationOutcome, BranchUpdateRepository,
};
use crate::infrastructure::sqlite::SqliteBranchUpdateRepository;
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
    crate::domain::entities::GitTargetIdentity,
) {
    let db = SqliteTestDb::new("git-mutation-recovery");
    let project = db.seed_project("project");
    let task = db.seed_task(project.id, "task");
    let repository = Arc::new(SqliteBranchUpdateRepository::from_shared(db.shared_conn()));
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
    (repository, identity)
}

#[tokio::test]
async fn recovery_clears_claim_only_after_a_clean_workspace_inspection() {
    let workspace = init_repository();
    let (repository, identity) = claimed_repository(workspace.path()).await;

    let outcomes = recover_in_flight_git_mutations(repository.clone())
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
    let (repository, identity) = claimed_repository(workspace.path()).await;
    fs::write(workspace.path().join("README.md"), "dirty").unwrap();

    let outcomes = recover_in_flight_git_mutations(repository.clone())
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

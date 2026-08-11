use super::*;
use chrono::Utc;
use std::path::PathBuf;

fn target() -> GitTargetIdentity {
    GitTargetIdentity::new(
        PathBuf::from("/tmp/ralphx-repository/.git"),
        "refs/heads/ralphx/example/plan-123",
    )
    .unwrap()
}

#[test]
fn git_target_identity_accepts_only_absolute_common_dir_and_local_full_ref() {
    assert!(GitTargetIdentity::new(PathBuf::from("relative/.git"), "refs/heads/main").is_err());
    assert!(GitTargetIdentity::new(PathBuf::from("/repo/.git"), "main").is_err());
    assert!(
        GitTargetIdentity::new(PathBuf::from("/repo/.git"), "refs/remotes/origin/main").is_err()
    );
    assert!(GitTargetIdentity::new(PathBuf::from("/repo/.git"), "refs/heads/../main").is_err());
    assert!(GitTargetIdentity::new(PathBuf::from("/repo/.git"), "refs/heads/main.lock").is_err());

    let identity = GitTargetIdentity::new(
        PathBuf::from("/repo/.git"),
        "refs/heads/ralphx/example/task-1",
    )
    .unwrap();
    assert_eq!(identity.full_ref(), "refs/heads/ralphx/example/task-1");
}

#[test]
fn failure_policy_is_closed_and_stale_authority_is_not_persistable() {
    assert_eq!(
        BranchUpdateFailureKind::Conflict.policy(),
        BranchUpdateFailurePolicy::Retryable
    );
    assert_eq!(
        BranchUpdateFailureKind::Incomplete.policy(),
        BranchUpdateFailurePolicy::Retryable
    );
    assert_eq!(
        BranchUpdateFailureKind::CheckoutBusy.policy(),
        BranchUpdateFailurePolicy::Retryable
    );
    assert_eq!(
        BranchUpdateFailureKind::DirtyWorkspace.policy(),
        BranchUpdateFailurePolicy::OperatorActionRequired
    );
    assert_eq!(
        BranchUpdateFailureKind::ContextCorrupt.policy(),
        BranchUpdateFailurePolicy::TerminalForOperation
    );
    assert!("stale_authority"
        .parse::<BranchUpdateFailureKind>()
        .is_err());
}

#[test]
fn mutation_claim_blocks_lease_transfer_until_exact_claim_completes() {
    let now = Utc::now();
    let owner = GitTargetLeaseOwner::branch_update("task-1", "operation-1");
    let mut lease = GitTargetLease::new(target(), owner.clone(), 7, now);

    let claim = lease
        .begin_mutation(&owner, 7, GitMutationKind::Merge, "claim-1", now)
        .unwrap();
    let next_owner = GitTargetLeaseOwner::merge_attempt("task-1", "attempt-2");

    assert_eq!(
        lease.transfer(&owner, 7, next_owner.clone(), now),
        Err(GitTargetLeaseError::MutationInFlight)
    );
    assert_eq!(
        lease.complete_mutation(&owner, 7, "wrong-claim"),
        Err(GitTargetLeaseError::StaleMutationClaim)
    );
    assert_eq!(lease.active_mutation(), Some(&claim));

    lease.complete_mutation(&owner, 7, "claim-1").unwrap();
    lease.transfer(&owner, 7, next_owner.clone(), now).unwrap();
    assert_eq!(lease.owner(), &next_owner);
    assert_eq!(lease.fencing_epoch(), 8);
}

#[test]
fn stale_owner_cannot_begin_complete_transfer_or_release() {
    let now = Utc::now();
    let owner = GitTargetLeaseOwner::branch_update("task-1", "operation-1");
    let stale = GitTargetLeaseOwner::branch_update("task-1", "operation-old");
    let mut lease = GitTargetLease::new(target(), owner.clone(), 3, now);

    assert_eq!(
        lease.begin_mutation(&stale, 2, GitMutationKind::Push, "claim", now),
        Err(GitTargetLeaseError::StaleAuthority)
    );
    assert_eq!(
        lease.transfer(&stale, 2, GitTargetLeaseOwner::manual("manual-1"), now),
        Err(GitTargetLeaseError::StaleAuthority)
    );
    assert_eq!(
        lease.release(&stale, 2),
        Err(GitTargetLeaseError::StaleAuthority)
    );
    assert!(!lease.is_released());
}

#[test]
fn operation_continuation_and_phase_are_closed_wire_contracts() {
    for continuation in [
        BranchUpdateContinuation::ResumeExecution,
        BranchUpdateContinuation::ResumeReExecution,
        BranchUpdateContinuation::ResumeReview,
        BranchUpdateContinuation::RetryPendingMerge,
        BranchUpdateContinuation::ResumeWaitingOnPr,
        BranchUpdateContinuation::FinalizePostMergePrPublication,
    ] {
        let wire = continuation.as_str();
        assert_eq!(
            wire.parse::<BranchUpdateContinuation>().unwrap(),
            continuation
        );
    }

    assert!("merged".parse::<BranchUpdateContinuation>().is_err());
    assert!("unknown".parse::<BranchUpdatePhase>().is_err());
}

#[test]
fn workspace_repair_lease_owner_is_unscoped_to_a_task() {
    let owner = GitTargetLeaseOwner::agent_workspace_repair("repair-attempt-1");

    assert_eq!(owner.kind, GitTargetLeaseOwnerKind::AgentWorkspaceRepair);
    assert_eq!(owner.task_id, None);
    assert_eq!(owner.owner_id, "repair-attempt-1");
}

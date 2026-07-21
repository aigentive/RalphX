use chrono::Utc;
use std::path::PathBuf;

use crate::domain::entities::{
    BranchUpdateCapacityOwnership, BranchUpdateContinuation, BranchUpdateDirection,
    BranchUpdateOperation, BranchUpdateWorkspaceOwnership, GitMutationKind, GitTargetIdentity,
    GitTargetLeaseOwner, InternalStatus,
};
use crate::domain::repositories::{
    AcquireGitTargetLease, AcquireGitTargetLeaseOutcome, BeginGitMutation, BindBranchUpdateRun,
    BlockBranchUpdate, BranchUpdateActivation, BranchUpdateActivationOutcome,
    BranchUpdateCasOutcome, BranchUpdateRepository, CheckpointBranchUpdateResult,
    ClaimBranchUpdateContinuation, CompleteBranchUpdateContinuation, CompleteGitMutation,
    GitAuthorityCasOutcome, MarkBranchUpdateResolving, PauseBranchUpdate, ResumeBranchUpdate,
    RetryBranchUpdate, SettleBranchUpdateProgrammatic, StopBranchUpdate,
    TransferBranchUpdateTargetLease, UnbindBranchUpdateRun,
};
use crate::infrastructure::sqlite::SqliteBranchUpdateRepository;
use crate::testing::SqliteTestDb;

fn operation(task_id: crate::domain::entities::TaskId, history_id: &str) -> BranchUpdateOperation {
    BranchUpdateOperation::new(
        task_id,
        BranchUpdateDirection::PlanBranch,
        BranchUpdateContinuation::ResumeExecution,
        history_id,
        "main",
        "ralphx/project/plan-1",
        BranchUpdateWorkspaceOwnership::OperationWorktree,
        BranchUpdateCapacityOwnership::Inherited,
        GitTargetIdentity::new(
            PathBuf::from("/repo/.git"),
            "refs/heads/ralphx/project/plan-1",
        )
        .unwrap(),
        Utc::now(),
    )
}

#[tokio::test]
async fn retry_rechecks_tasks_feature_state_inside_the_write_transaction() {
    let db = SqliteTestDb::new("branch-update-retry-tasks-off-race");
    let project = db.seed_project("project");
    let task = db.seed_task(project.id, "task");
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE ideation_settings
             SET tasks_enabled = 1, tasks_feature_state = 'enabled' WHERE id = 1",
            [],
        )
        .unwrap();
    });
    let repo =
        SqliteBranchUpdateRepository::from_shared(db.shared_conn()).with_tasks_feature_policy();
    let operation = operation(task.id.clone(), "history-retry-tasks-off");
    let operation_id = operation.id.clone();
    let owner = GitTargetLeaseOwner::branch_update(task.id.as_str(), operation_id.as_str());
    let BranchUpdateActivationOutcome::Applied { fencing_epoch, .. } = repo
        .activate(BranchUpdateActivation {
            operation,
            expected_status: InternalStatus::Backlog,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "retry".into(),
        })
        .await
        .unwrap()
    else {
        panic!("activation should apply");
    };
    repo.block_operation(BlockBranchUpdate {
        operation_id: operation_id.clone(),
        task_id: task.id.clone(),
        originating_history_id: "history-retry-tasks-off".into(),
        update_status: InternalStatus::UpdatingPlanBranch,
        owner: owner.clone(),
        fencing_epoch,
        failure_kind: crate::domain::entities::BranchUpdateFailureKind::Conflict,
        diagnostics: "resolve conflict".into(),
        conflict_files: vec![PathBuf::from("src/lib.rs")],
    })
    .await
    .unwrap();
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE ideation_settings
             SET tasks_enabled = 0, tasks_feature_state = 'draining' WHERE id = 1",
            [],
        )
        .unwrap();
    });

    let error = repo
        .retry_operation(RetryBranchUpdate {
            operation_id: operation_id.clone(),
            new_operation_id: crate::domain::entities::BranchUpdateOperationId::new(),
            task_id: task.id.clone(),
            originating_history_id: "history-retry-tasks-off".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            owner,
            fencing_epoch,
            history_id: "history-retry-tasks-off-new".into(),
        })
        .await
        .expect_err("a retry authorized before draining must be rejected at the write");
    assert!(error.to_string().starts_with("ralphx:tasks_disabled"));
    assert_eq!(
        repo.get_operation(&operation_id)
            .await
            .unwrap()
            .unwrap()
            .phase,
        crate::domain::entities::BranchUpdatePhase::Blocked
    );
    db.with_connection(|conn| {
        let status: String = conn
            .query_row(
                "SELECT internal_status FROM tasks WHERE id = ?1",
                [task.id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "branch_update_blocked");
        let retry_history: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_state_history WHERE id = 'history-retry-tasks-off-new'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retry_history, 0);
    });
}

#[tokio::test]
async fn begin_git_mutation_rechecks_tasks_feature_state_inside_the_write_transaction() {
    let db = SqliteTestDb::new("branch-update-begin-mutation-tasks-off-race");
    let project = db.seed_project("project");
    let task = db.seed_task(project.id, "task");
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE ideation_settings
             SET tasks_enabled = 1, tasks_feature_state = 'enabled' WHERE id = 1",
            [],
        )
        .unwrap();
    });
    let repo =
        SqliteBranchUpdateRepository::from_shared(db.shared_conn()).with_tasks_feature_policy();
    let operation = operation(task.id.clone(), "history-begin-mutation-tasks-off");
    let operation_id = operation.id.clone();
    let identity = operation.target_identity.clone();
    let owner = GitTargetLeaseOwner::branch_update(task.id.as_str(), operation_id.as_str());
    let BranchUpdateActivationOutcome::Applied { fencing_epoch, .. } = repo
        .activate(BranchUpdateActivation {
            operation,
            expected_status: InternalStatus::Backlog,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "begin-mutation".into(),
        })
        .await
        .unwrap()
    else {
        panic!("activation should apply");
    };
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE ideation_settings
             SET tasks_enabled = 0, tasks_feature_state = 'draining' WHERE id = 1",
            [],
        )
        .unwrap();
    });

    let error = repo
        .begin_git_mutation(BeginGitMutation {
            identity: identity.clone(),
            owner,
            fencing_epoch,
            claim_id: "must-not-start".into(),
            kind: GitMutationKind::Merge,
        })
        .await
        .expect_err("a task-owned Git mutation must not start after draining begins");

    assert!(error.to_string().starts_with("ralphx:tasks_disabled"));
    assert!(
        repo.get_target_lease(&identity)
            .await
            .unwrap()
            .unwrap()
            .active_mutation()
            .is_none(),
        "rejected admission must leave the durable mutation claim empty"
    );
}

fn operation_with_continuation(
    task_id: crate::domain::entities::TaskId,
    history_id: &str,
    continuation: BranchUpdateContinuation,
) -> BranchUpdateOperation {
    let mut operation = operation(task_id, history_id);
    operation.continuation = continuation;
    operation
}

#[tokio::test]
async fn standalone_target_lease_acquisition_fences_competing_merge_owners() {
    let db = SqliteTestDb::new("branch-update-standalone-lease");
    let project = db.seed_project("project");
    let task_a = db.seed_task(project.id.clone(), "task-a");
    let task_b = db.seed_task(project.id, "task-b");
    let repo = SqliteBranchUpdateRepository::from_shared(db.shared_conn());
    let identity = operation(task_a.id.clone(), "history-unused").target_identity;
    let owner_a = GitTargetLeaseOwner::merge_attempt(task_a.id.as_str(), "attempt-a");
    let owner_b = GitTargetLeaseOwner::merge_attempt(task_b.id.as_str(), "attempt-b");

    assert_eq!(
        repo.acquire_target_lease(AcquireGitTargetLease {
            identity: identity.clone(),
            owner: owner_a.clone(),
        })
        .await
        .unwrap(),
        AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch: 1 }
    );
    assert_eq!(
        repo.acquire_target_lease(AcquireGitTargetLease {
            identity: identity.clone(),
            owner: owner_a.clone(),
        })
        .await
        .unwrap(),
        AcquireGitTargetLeaseOutcome::AlreadyOwned { fencing_epoch: 1 }
    );
    assert_eq!(
        repo.acquire_target_lease(AcquireGitTargetLease {
            identity: identity.clone(),
            owner: owner_b.clone(),
        })
        .await
        .unwrap(),
        AcquireGitTargetLeaseOutcome::TargetBusy {
            owner: owner_a.clone(),
            fencing_epoch: 1,
        }
    );
    assert_eq!(
        repo.release_target_lease(&identity, &owner_a, 1)
            .await
            .unwrap(),
        GitAuthorityCasOutcome::Applied { fencing_epoch: 1 }
    );
    assert_eq!(
        repo.acquire_target_lease(AcquireGitTargetLease {
            identity,
            owner: owner_b,
        })
        .await
        .unwrap(),
        AcquireGitTargetLeaseOutcome::Acquired { fencing_epoch: 2 }
    );
}

#[tokio::test]
async fn activation_atomically_writes_status_history_operation_and_lease() {
    let db = SqliteTestDb::new("branch-update-activation");
    let project = db.seed_project("project");
    let task = db.seed_task(project.id, "task");
    let repo = SqliteBranchUpdateRepository::from_shared(db.shared_conn());
    let operation = operation(task.id.clone(), "history-update-1");
    let operation_id = operation.id.clone();

    let outcome = repo
        .activate(BranchUpdateActivation {
            operation,
            expected_status: InternalStatus::Backlog,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "branch_freshness_required".into(),
        })
        .await
        .unwrap();

    let BranchUpdateActivationOutcome::Applied {
        history_id,
        fencing_epoch,
        ..
    } = outcome
    else {
        panic!("activation should apply");
    };
    assert_eq!(history_id, "history-update-1");
    assert_eq!(fencing_epoch, 1);

    let stored = repo.get_operation(&operation_id).await.unwrap().unwrap();
    assert_eq!(stored.target_lease_epoch, 1);
    assert_eq!(
        stored.phase,
        crate::domain::entities::BranchUpdatePhase::Programmatic
    );
    db.with_connection(|conn| {
        let status: String = conn
            .query_row(
                "SELECT internal_status FROM tasks WHERE id = ?1",
                [task.id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "updating_plan_branch");
        let history_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_state_history WHERE id = 'history-update-1' AND to_status = 'updating_plan_branch'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(history_count, 1);
    });
}

#[tokio::test]
async fn stale_status_leaves_every_authority_surface_untouched() {
    let db = SqliteTestDb::new("branch-update-stale-status");
    let project = db.seed_project("project");
    let task = db.seed_task(project.id, "task");
    let repo = SqliteBranchUpdateRepository::from_shared(db.shared_conn());

    let outcome = repo
        .activate(BranchUpdateActivation {
            operation: operation(task.id.clone(), "history-update-stale"),
            expected_status: InternalStatus::Executing,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "branch_freshness_required".into(),
        })
        .await
        .unwrap();
    assert_eq!(outcome, BranchUpdateActivationOutcome::StaleTask);

    assert!(repo.get_active_operation(&task.id).await.unwrap().is_none());
    db.with_connection(|conn| {
        let history_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_state_history WHERE id = 'history-update-stale'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let lease_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM git_target_leases", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(history_count, 0);
        assert_eq!(lease_count, 0);
    });
}

#[tokio::test]
async fn target_contention_preserves_winner_and_does_not_transition_loser() {
    let db = SqliteTestDb::new("branch-update-target-contention");
    let project = db.seed_project("project");
    let task_a = db.seed_task(project.id.clone(), "task-a");
    let task_b = db.seed_task(project.id, "task-b");
    let repo = SqliteBranchUpdateRepository::from_shared(db.shared_conn());

    let first = repo
        .activate(BranchUpdateActivation {
            operation: operation(task_a.id.clone(), "history-a"),
            expected_status: InternalStatus::Backlog,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "freshness".into(),
        })
        .await
        .unwrap();
    assert!(matches!(
        first,
        BranchUpdateActivationOutcome::Applied { .. }
    ));

    let second = repo
        .activate(BranchUpdateActivation {
            operation: operation(task_b.id.clone(), "history-b"),
            expected_status: InternalStatus::Backlog,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "freshness".into(),
        })
        .await
        .unwrap();
    assert!(matches!(
        second,
        BranchUpdateActivationOutcome::TargetBusy { .. }
    ));
    assert!(repo
        .get_active_operation(&task_b.id)
        .await
        .unwrap()
        .is_none());
    db.with_connection(|conn| {
        let loser_status: String = conn
            .query_row(
                "SELECT internal_status FROM tasks WHERE id = ?1",
                [task_b.id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(loser_status, "backlog");
    });
}

#[tokio::test]
async fn branch_update_controls_are_fenced_and_preserve_pause_authority() {
    let db = SqliteTestDb::new("branch-update-controls");
    let project = db.seed_project("project");
    let task = db.seed_task(project.id, "task");
    let repo = SqliteBranchUpdateRepository::from_shared(db.shared_conn());
    let operation = operation(task.id.clone(), "history-controls");
    let operation_id = operation.id.clone();
    let identity = operation.target_identity.clone();
    let owner = GitTargetLeaseOwner::branch_update(task.id.as_str(), operation_id.as_str());
    let BranchUpdateActivationOutcome::Applied { fencing_epoch, .. } = repo
        .activate(BranchUpdateActivation {
            operation,
            expected_status: InternalStatus::Backlog,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "controls".into(),
        })
        .await
        .unwrap()
    else {
        panic!("activation should apply");
    };
    let pause = || PauseBranchUpdate {
        operation_id: operation_id.clone(),
        task_id: task.id.clone(),
        originating_history_id: "history-controls".into(),
        update_status: InternalStatus::UpdatingPlanBranch,
        owner: owner.clone(),
        fencing_epoch,
        history_id: uuid::Uuid::new_v4().to_string(),
        task_metadata: None,
    };

    repo.begin_git_mutation(BeginGitMutation {
        identity: identity.clone(),
        owner: owner.clone(),
        fencing_epoch,
        claim_id: "control-mutation".into(),
        kind: GitMutationKind::Merge,
    })
    .await
    .unwrap();
    assert_eq!(
        repo.pause_operation(pause()).await.unwrap(),
        BranchUpdateCasOutcome::Stale,
        "a control transition must not move task state while Git can still mutate"
    );
    repo.complete_git_mutation(CompleteGitMutation {
        identity: identity.clone(),
        owner: owner.clone(),
        fencing_epoch,
        claim_id: "control-mutation".into(),
    })
    .await
    .unwrap();
    assert_eq!(
        repo.pause_operation(pause()).await.unwrap(),
        BranchUpdateCasOutcome::Applied
    );
    assert!(repo.get_active_operation(&task.id).await.unwrap().is_some());
    assert_eq!(
        repo.resume_operation(ResumeBranchUpdate {
            operation_id: operation_id.clone(),
            task_id: task.id.clone(),
            originating_history_id: "history-controls".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            owner: owner.clone(),
            fencing_epoch,
            history_id: "history-controls-resumed".into(),
        })
        .await
        .unwrap(),
        BranchUpdateCasOutcome::Applied
    );
    assert_eq!(
        repo.stop_operation(StopBranchUpdate {
            operation_id: operation_id.clone(),
            task_id: task.id.clone(),
            originating_history_id: "history-controls".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            owner,
            fencing_epoch,
            history_id: "history-controls-stopped".into(),
            reason: Some("operator stop".into()),
        })
        .await
        .unwrap(),
        BranchUpdateCasOutcome::Applied
    );
    assert!(repo.get_active_operation(&task.id).await.unwrap().is_none());
    let lease = repo.get_target_lease(&identity).await.unwrap().unwrap();
    assert!(lease.is_released());
    db.with_connection(|conn| {
        let status: String = conn
            .query_row(
                "SELECT internal_status FROM tasks WHERE id = ?1",
                [task.id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "stopped");
    });
}

#[tokio::test]
async fn blocked_retry_settles_old_operation_and_atomically_transfers_authority() {
    let db = SqliteTestDb::new("branch-update-retry");
    let project = db.seed_project("project");
    let task = db.seed_task(project.id, "task");
    let repo = SqliteBranchUpdateRepository::from_shared(db.shared_conn());
    let operation = operation(task.id.clone(), "history-retry-old");
    let operation_id = operation.id.clone();
    let owner = GitTargetLeaseOwner::branch_update(task.id.as_str(), operation_id.as_str());
    let BranchUpdateActivationOutcome::Applied { fencing_epoch, .. } = repo
        .activate(BranchUpdateActivation {
            operation,
            expected_status: InternalStatus::Backlog,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "retry".into(),
        })
        .await
        .unwrap()
    else {
        panic!("activation should apply");
    };
    repo.block_operation(BlockBranchUpdate {
        operation_id: operation_id.clone(),
        task_id: task.id.clone(),
        originating_history_id: "history-retry-old".into(),
        update_status: InternalStatus::UpdatingPlanBranch,
        owner: owner.clone(),
        fencing_epoch,
        failure_kind: crate::domain::entities::BranchUpdateFailureKind::Conflict,
        diagnostics: "resolve conflict".into(),
        conflict_files: vec![PathBuf::from("src/lib.rs")],
    })
    .await
    .unwrap();
    let new_operation_id = crate::domain::entities::BranchUpdateOperationId::new();
    assert_eq!(
        repo.retry_operation(RetryBranchUpdate {
            operation_id: operation_id.clone(),
            new_operation_id: new_operation_id.clone(),
            task_id: task.id.clone(),
            originating_history_id: "history-retry-old".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            owner,
            fencing_epoch,
            history_id: "history-retry-new".into(),
        })
        .await
        .unwrap(),
        BranchUpdateCasOutcome::Applied
    );
    let old = repo.get_operation(&operation_id).await.unwrap().unwrap();
    assert_eq!(
        old.phase,
        crate::domain::entities::BranchUpdatePhase::Settled
    );
    let retry = repo
        .get_operation(&new_operation_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        retry.phase,
        crate::domain::entities::BranchUpdatePhase::Resolving
    );
    assert_eq!(retry.retry_count, 1);
    assert_eq!(retry.target_lease_epoch, fencing_epoch + 1);
    let lease = repo
        .get_target_lease(&retry.target_identity)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.owner().owner_id, new_operation_id.as_str());
    assert_eq!(lease.fencing_epoch(), fencing_epoch + 1);
}

#[tokio::test]
async fn durable_mutation_claim_blocks_transfer_until_exact_completion() {
    let db = SqliteTestDb::new("branch-update-mutation-claim");
    let project = db.seed_project("project");
    let task = db.seed_task(project.id, "task");
    let repo = SqliteBranchUpdateRepository::from_shared(db.shared_conn());
    let operation = operation(task.id.clone(), "history-claim");
    let identity = operation.target_identity.clone();
    let owner = GitTargetLeaseOwner::branch_update(task.id.as_str(), operation.id.as_str());
    let activation = repo
        .activate(BranchUpdateActivation {
            operation,
            expected_status: InternalStatus::Backlog,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "freshness".into(),
        })
        .await
        .unwrap();
    let BranchUpdateActivationOutcome::Applied { fencing_epoch, .. } = activation else {
        panic!("activation should apply");
    };
    assert_eq!(
        repo.begin_git_mutation(BeginGitMutation {
            identity: identity.clone(),
            owner: owner.clone(),
            fencing_epoch,
            claim_id: "claim-1".into(),
            kind: GitMutationKind::Merge,
        })
        .await
        .unwrap(),
        GitAuthorityCasOutcome::Applied { fencing_epoch }
    );
    assert_eq!(
        repo.transfer_target_lease(
            &identity,
            &owner,
            fencing_epoch,
            GitTargetLeaseOwner::merge_attempt(task.id.as_str(), "attempt-2"),
        )
        .await
        .unwrap(),
        GitAuthorityCasOutcome::MutationInFlight
    );
    assert_eq!(
        repo.complete_git_mutation(CompleteGitMutation {
            identity: identity.clone(),
            owner: owner.clone(),
            fencing_epoch,
            claim_id: "wrong".into(),
        })
        .await
        .unwrap(),
        GitAuthorityCasOutcome::StaleMutationClaim
    );
    assert_eq!(
        repo.complete_git_mutation(CompleteGitMutation {
            identity: identity.clone(),
            owner: owner.clone(),
            fencing_epoch,
            claim_id: "claim-1".into(),
        })
        .await
        .unwrap(),
        GitAuthorityCasOutcome::Applied { fencing_epoch }
    );
    assert_eq!(
        repo.transfer_target_lease(
            &identity,
            &owner,
            fencing_epoch,
            GitTargetLeaseOwner::merge_attempt(task.id.as_str(), "attempt-2"),
        )
        .await
        .unwrap(),
        GitAuthorityCasOutcome::Applied {
            fencing_epoch: fencing_epoch + 1
        }
    );
}

#[tokio::test]
async fn operation_lease_handoff_advances_operation_epoch_and_rejects_stale_owner() {
    let db = SqliteTestDb::new("branch-update-operation-handoff");
    let project = db.seed_project("project");
    let task = db.seed_task(project.id, "task");
    let repo = SqliteBranchUpdateRepository::from_shared(db.shared_conn());
    let operation = operation_with_continuation(
        task.id.clone(),
        "history-handoff",
        BranchUpdateContinuation::FinalizePostMergePrPublication,
    );
    let operation_id = operation.id.clone();
    let owner = GitTargetLeaseOwner::branch_update(task.id.as_str(), operation_id.as_str());
    let BranchUpdateActivationOutcome::Applied { fencing_epoch, .. } = repo
        .activate(BranchUpdateActivation {
            operation,
            expected_status: InternalStatus::Backlog,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "publication".into(),
        })
        .await
        .unwrap()
    else {
        panic!("activation should apply");
    };
    let next_owner =
        GitTargetLeaseOwner::publication_recovery(task.id.as_str(), operation_id.as_str());
    assert_eq!(
        repo.transfer_operation_target_lease(TransferBranchUpdateTargetLease {
            operation_id: operation_id.clone(),
            task_id: task.id.clone(),
            originating_history_id: "history-handoff".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            owner: owner.clone(),
            fencing_epoch,
            next_owner: next_owner.clone(),
        })
        .await
        .unwrap(),
        GitAuthorityCasOutcome::Applied {
            fencing_epoch: fencing_epoch + 1
        }
    );
    let stored = repo.get_operation(&operation_id).await.unwrap().unwrap();
    assert_eq!(stored.target_lease_epoch, fencing_epoch + 1);
    let lease = repo
        .get_target_lease(&stored.target_identity)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.owner(), &next_owner);
    assert_eq!(lease.fencing_epoch(), fencing_epoch + 1);
    assert_eq!(
        repo.transfer_operation_target_lease(TransferBranchUpdateTargetLease {
            operation_id,
            task_id: task.id,
            originating_history_id: "history-handoff".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            owner,
            fencing_epoch,
            next_owner,
        })
        .await
        .unwrap(),
        GitAuthorityCasOutcome::StaleAuthority
    );
}

#[tokio::test]
async fn continuation_requires_exact_claim_and_receipt_before_releasing_authority() {
    let db = SqliteTestDb::new("branch-update-continuation");
    let project = db.seed_project("project");
    let task = db.seed_task(project.id, "task");
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE tasks SET internal_status = 'executing' WHERE id = ?1",
            [task.id.as_str()],
        )
        .unwrap();
    });
    let repo = SqliteBranchUpdateRepository::from_shared(db.shared_conn());
    let operation = operation(task.id.clone(), "history-continuation");
    let operation_id = operation.id.clone();
    let identity = operation.target_identity.clone();
    let owner = GitTargetLeaseOwner::branch_update(task.id.as_str(), operation_id.as_str());
    let activation = repo
        .activate(BranchUpdateActivation {
            operation,
            expected_status: InternalStatus::Executing,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "freshness".into(),
        })
        .await
        .unwrap();
    let BranchUpdateActivationOutcome::Applied { fencing_epoch, .. } = activation else {
        panic!("activation should apply");
    };
    assert_eq!(
        repo.checkpoint_result(CheckpointBranchUpdateResult {
            operation_id: operation_id.clone(),
            task_id: task.id.clone(),
            originating_history_id: "history-continuation".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            owner: owner.clone(),
            fencing_epoch,
            resulting_sha: "abc123".into(),
        })
        .await
        .unwrap(),
        BranchUpdateCasOutcome::Applied
    );
    assert_eq!(
        repo.checkpoint_result(CheckpointBranchUpdateResult {
            operation_id: operation_id.clone(),
            task_id: task.id.clone(),
            originating_history_id: "history-continuation".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            owner: owner.clone(),
            fencing_epoch,
            resulting_sha: "different-result".into(),
        })
        .await
        .unwrap(),
        BranchUpdateCasOutcome::Stale
    );
    assert_eq!(
        repo.settle_programmatic(SettleBranchUpdateProgrammatic {
            operation_id: operation_id.clone(),
            task_id: task.id.clone(),
            originating_history_id: "history-continuation".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            owner: owner.clone(),
            fencing_epoch,
            resulting_sha: "different-result".into(),
        })
        .await
        .unwrap(),
        BranchUpdateCasOutcome::Stale
    );
    assert_eq!(
        repo.settle_programmatic(SettleBranchUpdateProgrammatic {
            operation_id: operation_id.clone(),
            task_id: task.id.clone(),
            originating_history_id: "history-continuation".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            owner: owner.clone(),
            fencing_epoch,
            resulting_sha: "abc123".into(),
        })
        .await
        .unwrap(),
        BranchUpdateCasOutcome::Applied
    );
    assert_eq!(
        repo.claim_continuation(ClaimBranchUpdateContinuation {
            operation_id: operation_id.clone(),
            task_id: task.id.clone(),
            originating_history_id: "history-continuation".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            claim_id: "continuation-claim".into(),
            idempotency_key: "resume-execution:abc123".into(),
        })
        .await
        .unwrap(),
        BranchUpdateCasOutcome::Applied
    );
    let completion =
        |claim_id: &str, receipt: &str, history_id: &str| CompleteBranchUpdateContinuation {
            operation_id: operation_id.clone(),
            task_id: task.id.clone(),
            originating_history_id: "history-continuation".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            destination_status: InternalStatus::Executing,
            owner: owner.clone(),
            fencing_epoch,
            claim_id: claim_id.into(),
            idempotency_key: "resume-execution:abc123".into(),
            receipt: receipt.into(),
            history_id: history_id.into(),
        };
    assert_eq!(
        repo.complete_continuation(completion("wrong", "receipt-wrong", "history-wrong"))
            .await
            .unwrap(),
        BranchUpdateCasOutcome::Stale
    );
    assert_eq!(
        repo.complete_continuation(completion(
            "continuation-claim",
            "receipt-exact",
            "history-resumed"
        ))
        .await
        .unwrap(),
        BranchUpdateCasOutcome::Applied
    );
    assert!(repo
        .get_target_lease(&identity)
        .await
        .unwrap()
        .unwrap()
        .is_released());
    let stored = repo.get_operation(&operation_id).await.unwrap().unwrap();
    assert_eq!(
        stored.continuation_receipt.as_deref(),
        Some("receipt-exact")
    );
    assert_eq!(
        stored.phase,
        crate::domain::entities::BranchUpdatePhase::Settled
    );
}

#[tokio::test]
async fn post_merge_publication_continuation_goes_directly_from_update_to_merged() {
    let db = SqliteTestDb::new("branch-update-post-merge-publication");
    let project = db.seed_project("project");
    let task = db.seed_task(project.id, "task");
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE tasks SET internal_status = 'waiting_on_pr' WHERE id = ?1",
            [task.id.as_str()],
        )
        .unwrap();
    });
    let repo = SqliteBranchUpdateRepository::from_shared(db.shared_conn());
    let operation = operation_with_continuation(
        task.id.clone(),
        "history-publication",
        BranchUpdateContinuation::FinalizePostMergePrPublication,
    );
    let operation_id = operation.id.clone();
    let owner = GitTargetLeaseOwner::branch_update(task.id.as_str(), operation_id.as_str());
    let BranchUpdateActivationOutcome::Applied { fencing_epoch, .. } = repo
        .activate(BranchUpdateActivation {
            operation,
            expected_status: InternalStatus::WaitingOnPr,
            update_status: InternalStatus::UpdatingPlanBranch,
            trigger: "publication_freshness".into(),
        })
        .await
        .unwrap()
    else {
        panic!("activation should apply");
    };
    assert_eq!(
        repo.checkpoint_result(CheckpointBranchUpdateResult {
            operation_id: operation_id.clone(),
            task_id: task.id.clone(),
            originating_history_id: "history-publication".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            owner: owner.clone(),
            fencing_epoch,
            resulting_sha: "merged-sha".into(),
        })
        .await
        .unwrap(),
        BranchUpdateCasOutcome::Applied
    );
    repo.settle_programmatic(SettleBranchUpdateProgrammatic {
        operation_id: operation_id.clone(),
        task_id: task.id.clone(),
        originating_history_id: "history-publication".into(),
        update_status: InternalStatus::UpdatingPlanBranch,
        owner: owner.clone(),
        fencing_epoch,
        resulting_sha: "merged-sha".into(),
    })
    .await
    .unwrap();
    repo.claim_continuation(ClaimBranchUpdateContinuation {
        operation_id: operation_id.clone(),
        task_id: task.id.clone(),
        originating_history_id: "history-publication".into(),
        update_status: InternalStatus::UpdatingPlanBranch,
        claim_id: "publication-claim".into(),
        idempotency_key: "publish:merged-sha".into(),
    })
    .await
    .unwrap();
    assert_eq!(
        repo.complete_continuation(CompleteBranchUpdateContinuation {
            operation_id: operation_id.clone(),
            task_id: task.id.clone(),
            originating_history_id: "history-publication".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            destination_status: InternalStatus::Merged,
            owner,
            fencing_epoch,
            claim_id: "publication-claim".into(),
            idempotency_key: "publish:merged-sha".into(),
            receipt: "publication-receipt".into(),
            history_id: "history-merged".into(),
        })
        .await
        .unwrap(),
        BranchUpdateCasOutcome::Applied
    );
    db.with_connection(|conn| {
        let status: String = conn
            .query_row(
                "SELECT internal_status FROM tasks WHERE id = ?1",
                [task.id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "merged");
        let merge_commit_sha: Option<String> = conn
            .query_row(
                "SELECT merge_commit_sha FROM tasks WHERE id = ?1",
                [task.id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(merge_commit_sha.as_deref(), Some("merged-sha"));
        let metadata: String = conn
            .query_row(
                "SELECT metadata FROM tasks WHERE id = ?1",
                [task.id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&metadata).unwrap()["pending_cleanup"],
            true
        );
        let merging_hops: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM task_state_history WHERE task_id = ?1 AND to_status = 'merging'",
                [task.id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(merging_hops, 0);
    });
}

#[tokio::test]
async fn agent_run_binding_updates_only_the_exact_operation_history_before_spawn() {
    let db = SqliteTestDb::new("branch-update-run-binding");
    let project = db.seed_project("project");
    let task = db.seed_task(project.id, "task");
    let repo = SqliteBranchUpdateRepository::from_shared(db.shared_conn());
    let operation = operation(task.id.clone(), "history-bound-operation");
    let operation_id = operation.id.clone();
    let owner = GitTargetLeaseOwner::branch_update(task.id.as_str(), operation_id.as_str());
    let BranchUpdateActivationOutcome::Applied { fencing_epoch, .. } = repo
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
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO task_state_history (id, task_id, from_status, to_status, changed_by, reason, metadata)
             VALUES ('newer-unrelated-history', ?1, 'updating_plan_branch', 'updating_plan_branch', 'test', 'unrelated', '{}')",
            [task.id.as_str()],
        )
        .unwrap();
    });
    assert_eq!(
        repo.mark_resolving(MarkBranchUpdateResolving {
            operation_id: operation_id.clone(),
            task_id: task.id.clone(),
            originating_history_id: "history-bound-operation".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            owner,
            fencing_epoch,
            conflict_files: vec![PathBuf::from("src/lib.rs")],
        })
        .await
        .unwrap(),
        BranchUpdateCasOutcome::Applied
    );
    assert_eq!(
        repo.bind_agent_run(BindBranchUpdateRun {
            operation_id: operation_id.clone(),
            task_id: task.id.clone(),
            originating_history_id: "history-bound-operation".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            conversation_id: "conversation-current".into(),
            agent_run_id: "run-current".into(),
        })
        .await
        .unwrap(),
        BranchUpdateCasOutcome::Applied
    );
    assert_eq!(
        repo.bind_agent_run(BindBranchUpdateRun {
            operation_id: operation_id.clone(),
            task_id: task.id.clone(),
            originating_history_id: "history-bound-operation".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            conversation_id: "conversation-stale".into(),
            agent_run_id: "run-stale".into(),
        })
        .await
        .unwrap(),
        BranchUpdateCasOutcome::Stale
    );
    db.with_connection(|conn| {
        let exact: String = conn
            .query_row(
                "SELECT metadata FROM task_state_history WHERE id = 'history-bound-operation'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let unrelated: String = conn
            .query_row(
                "SELECT metadata FROM task_state_history WHERE id = 'newer-unrelated-history'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exact.contains("conversation-current"));
        assert!(exact.contains("run-current"));
        assert_eq!(unrelated, "{}");
    });

    assert_eq!(
        repo.unbind_agent_run(UnbindBranchUpdateRun {
            operation_id: operation_id.clone(),
            task_id: task.id.clone(),
            originating_history_id: "history-bound-operation".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            conversation_id: "conversation-current".into(),
            agent_run_id: "run-stale".into(),
        })
        .await
        .unwrap(),
        BranchUpdateCasOutcome::Stale
    );
    assert_eq!(
        repo.unbind_agent_run(UnbindBranchUpdateRun {
            operation_id,
            task_id: task.id.clone(),
            originating_history_id: "history-bound-operation".into(),
            update_status: InternalStatus::UpdatingPlanBranch,
            conversation_id: "conversation-current".into(),
            agent_run_id: "run-current".into(),
        })
        .await
        .unwrap(),
        BranchUpdateCasOutcome::Applied
    );
    db.with_connection(|conn| {
        let exact: String = conn
            .query_row(
                "SELECT metadata FROM task_state_history WHERE id = 'history-bound-operation'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let (conversation_id, agent_run_id): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT conversation_id, agent_run_id FROM branch_update_operations WHERE task_id = ?1",
                [task.id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(!exact.contains("conversation_id"));
        assert!(!exact.contains("agent_run_id"));
        assert_eq!((conversation_id, agent_run_id), (None, None));
    });
}

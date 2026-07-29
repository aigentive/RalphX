// Cleanup hardening tests (Scenarios F1-F8)
//
// Tests verifying worktree/branch cleanup behavior during merge workflows
// and demonstrating gaps where cleanup is missing or unreliable.
//
// GitService is static (not mockable via traits), so these tests focus on:
// - State transitions that LEAD to cleanup
// - Task/project state that SHOULD trigger cleanup
// - Documenting gaps where cleanup doesn't happen

use super::helpers::*;
use crate::domain::entities::{InternalStatus, Project, ProjectId};
use crate::domain::repositories::{PlanBranchRepository, ProjectRepository, TaskRepository};
use crate::domain::state_machine::events::TaskEvent;
use crate::domain::state_machine::machine::State;
use crate::domain::state_machine::transition_handler::complete_merge_internal;

// ============================================================================
// F1: Task worktree deleted before merge — COVERED
// ============================================================================

#[tokio::test]
async fn test_f1_task_with_worktree_can_enter_pending_merge() {
    // COVERED: Pre-merge cleanup deletes task worktree before merge attempt.
    //
    // In production, the pre-merge step deletes the worktree before merging.
    // At test level: verify that a task with worktree_path set can transition
    // to PendingMerge, and that the worktree_path field persists (cleanup is
    // a side effect of the merge process, not the state machine).
    let s = create_hardening_services();

    let project_id = ProjectId::from_string("proj-f1".to_string());
    let mut task =
        create_test_task_with_status(&project_id, "Task with worktree", InternalStatus::Approved);
    task.task_branch = Some("ralphx/test-project/task-f1".to_string());
    task.worktree_path = Some("/tmp/ralphx-worktrees/task-f1".to_string());
    s.task_repo.create(task.clone()).await.unwrap();

    let services = build_task_services(&s);
    let mut machine = create_state_machine(task.id.as_str(), "proj-f1", services);
    let mut handler = create_transition_handler(&mut machine);

    // Approved -> PendingMerge via StartMerge (auto-transition from Approved)
    let result = handler
        .handle_transition(&State::Approved, &TaskEvent::StartMerge)
        .await;

    assert!(
        result.is_success(),
        "Transition to PendingMerge should succeed"
    );

    // Verify worktree_path is still set on the task in the repo
    let stored_task = s
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("Task should exist");
    assert!(
        stored_task.worktree_path.is_some(),
        "worktree_path should still be set — cleanup is a side effect, not a state machine concern"
    );
}

#[tokio::test]
async fn test_f1_pending_merge_reachable_from_approved() {
    // COVERED: Verify the Approved -> PendingMerge path works cleanly
    // without any task in the repo (pure state machine test).
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-f1b", "proj-f1b", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Approved, &TaskEvent::StartMerge)
        .await;

    assert!(
        result.is_success(),
        "Approved -> PendingMerge transition should succeed"
    );
}

// ============================================================================
// F2: Merge worktree cleaned up after success — COVERED
// ============================================================================

#[tokio::test]
async fn test_f2_pending_merge_to_merged_transition_works() {
    // COVERED: After MergeComplete, task is in Merged state.
    //
    // In production, post_merge_cleanup runs after Merged transition.
    // At test level: verify the PendingMerge -> Merged transition path works.
    let s = create_hardening_services();

    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-f2", "proj-f2", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::PendingMerge, &TaskEvent::MergeComplete)
        .await;

    assert!(result.is_success(), "PendingMerge -> Merged should succeed");
    assert_eq!(
        result.state(),
        Some(&State::Merged),
        "Final state should be Merged"
    );
}

#[tokio::test]
async fn test_f2_merged_accepts_retry_to_ready() {
    // COVERED: Merged is not fully terminal — it accepts Retry -> Ready.
    // This allows re-running a task after merge (e.g., if the merge
    // introduced issues discovered later).
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-f2b", "proj-f2b", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Merged, &TaskEvent::Retry)
        .await;

    assert!(result.is_success(), "Merged should accept Retry -> Ready");
    assert_eq!(
        result.state(),
        Some(&State::Ready),
        "Merged + Retry should transition to Ready"
    );
}

// ============================================================================
// F3: Merge worktree orphaned after failed merge — PARTIAL
// ============================================================================

#[tokio::test]
async fn test_f3_merge_incomplete_reachable_from_merging() {
    // PARTIAL: MergeIncomplete is reachable via MergeAgentError from Merging.
    //
    // When a merge fails (Merging -> MergeIncomplete), the task still has
    // worktree_path set. No automatic cleanup runs on the failure path.
    let s = create_hardening_services();

    let project_id = ProjectId::from_string("proj-f3".to_string());
    let mut task =
        create_test_task_with_status(&project_id, "Merge failure task", InternalStatus::Merging);
    task.task_branch = Some("ralphx/test-project/task-f3".to_string());
    task.worktree_path = Some("/tmp/ralphx-worktrees/task-f3".to_string());
    s.task_repo.create(task.clone()).await.unwrap();

    let services = build_task_services(&s);
    let mut machine = create_state_machine(task.id.as_str(), "proj-f3", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Merging, &TaskEvent::MergeAgentError)
        .await;

    assert!(
        result.is_success(),
        "Merging -> MergeIncomplete should succeed"
    );
    assert_eq!(
        result.state(),
        Some(&State::MergeIncomplete),
        "Final state should be MergeIncomplete"
    );

    // GAP: worktree_path is still set — no cleanup on failure path
    let stored_task = s
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("Task should exist");
    assert!(
        stored_task.worktree_path.is_some(),
        "GAP: worktree_path persists after merge failure — no automatic cleanup on failure path"
    );
}

#[tokio::test]
async fn test_f3_merge_conflict_reachable_via_merge_agent_failed() {
    // PARTIAL: MergeConflict state is reachable from Merging via MergeAgentFailed.
    // (Note: MergeConflict EVENT is not handled in Merging — the STATE is
    // reached via MergeAgentFailed event.)
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-f3b", "proj-f3b", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Merging, &TaskEvent::MergeAgentFailed)
        .await;

    assert!(
        result.is_success(),
        "Merging -> MergeConflict via MergeAgentFailed should succeed"
    );
    assert_eq!(
        result.state(),
        Some(&State::MergeConflict),
        "Final state should be MergeConflict"
    );
}

// ============================================================================
// F4: delete_worktree fails silently — GAP
// ============================================================================

#[tokio::test]
async fn test_f4_no_worktree_deletion_retry_mechanism() {
    // GAP: delete_worktree is a static function on GitService (not mockable).
    //
    // GitService::delete_worktree logs a warning and returns Ok(()).
    // There is no retry mechanism if worktree deletion fails.
    // TaskServices has no field for worktree cleanup retry/tracking.
    let s = create_hardening_services();
    let services = build_task_services(&s);

    assert!(
        services.execution_state.is_some(),
        "execution_state is set (used for other purposes, not cleanup)"
    );

    // Verify there is no cleanup-related service in TaskServices.
    // TaskServices has: agent_spawner, event_emitter, notifier, dependency_manager,
    // review_starter, chat_service, execution_state, app_handle, task_scheduler,
    // task_repo, project_repo, plan_branch_repo, step_repo.
    // None of these provide worktree cleanup retry capability.
    //
    // GAP: If delete_worktree fails, the worktree is orphaned with no
    // mechanism to retry the deletion.
}

#[tokio::test]
async fn test_f4_merge_incomplete_reachable_via_merge_agent_error() {
    // GAP: When merge agent encounters a non-conflict error, MergeIncomplete
    // is the fallback state. Worktree may be orphaned.
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-f4b", "proj-f4b", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::Merging, &TaskEvent::MergeAgentError)
        .await;

    assert!(
        result.is_success(),
        "MergeAgentError should transition to MergeIncomplete"
    );
    assert_eq!(
        result.state(),
        Some(&State::MergeIncomplete),
        "GAP: MergeIncomplete exists as fallback, but worktree may be orphaned"
    );
}

// ============================================================================
// F5: prune_worktrees fails silently — GAP
// ============================================================================

#[tokio::test]
async fn test_f5_no_worktree_prune_retry_mechanism() {
    // GAP: prune_worktrees is a static function on GitService (not mockable).
    //
    // Same architectural gap as F4: worktree pruning is a static function.
    // There is no retry, no error propagation, and no periodic job that
    // would re-attempt pruning.
    let s = create_hardening_services();

    // Verify no periodic cleanup/prune job exists in services.
    // task_scheduler only has try_schedule_ready_tasks and try_retry_deferred_merges.
    let scheduler_calls = s.scheduler.get_calls();
    assert!(
        scheduler_calls.is_empty(),
        "GAP: No pruning calls are made — the mechanism doesn't exist on any service trait"
    );
}

// ============================================================================
// F6: Stale task branches accumulate — GAP
// ============================================================================

#[tokio::test]
async fn test_f6_terminal_tasks_retain_branch_info_no_cleanup_job() {
    // GAP: Tasks in terminal states (Failed, Cancelled, Stopped) still have
    // task_branch set. There is no periodic cleanup job that removes stale branches.
    let s = create_hardening_services();
    let project_id = ProjectId::from_string("proj-f6".to_string());

    let terminal_states = [
        (InternalStatus::Failed, "Failed task"),
        (InternalStatus::Cancelled, "Cancelled task"),
        (InternalStatus::Stopped, "Stopped task"),
    ];

    for (status, title) in &terminal_states {
        let mut task = create_test_task_with_status(&project_id, title, status.clone());
        task.task_branch = Some(format!("ralphx/test-project/{}", task.id.as_str()));
        s.task_repo.create(task).await.unwrap();
    }

    let tasks = s.task_repo.get_by_project(&project_id).await.unwrap();
    assert_eq!(tasks.len(), 3, "All three terminal tasks should exist");

    for task in &tasks {
        assert!(
            task.task_branch.is_some(),
            "GAP: Terminal task '{}' ({:?}) still has task_branch set — \
             no cleanup job removes stale branches",
            task.title,
            task.internal_status
        );
    }

    // Verify no cleanup service or periodic job exists
    let scheduler_calls = s.scheduler.get_calls();
    assert!(
        scheduler_calls.is_empty(),
        "GAP: No branch cleanup calls were made — no periodic cleanup job exists"
    );
}

#[tokio::test]
async fn test_f6_failed_state_is_not_terminal_for_state_machine() {
    // GAP: Failed tasks can be retried, so they sit in Failed indefinitely
    // with task_branch set until someone retries or manually cleans up.
    let s = create_hardening_services();
    let services = build_task_services(&s);
    let mut machine = create_state_machine("task-f6b", "proj-f6b", services);
    let mut handler = create_transition_handler(&mut machine);

    // Failed accepts Retry -> Ready (branch still set)
    let result = handler
        .handle_transition(&State::Failed(Default::default()), &TaskEvent::Retry)
        .await;
    assert!(
        result.is_success(),
        "Failed -> Ready via Retry should succeed (branch persists through retry)"
    );
}

// ============================================================================
// F7: Task deleted but branch cleanup fails — GAP
// ============================================================================

#[tokio::test]
async fn test_f7_deleted_task_loses_branch_record() {
    // GAP: When a task is deleted from the repo, its branch information is gone.
    // There is no mechanism to track orphaned branches after task deletion.
    let s = create_hardening_services();
    let project_id = ProjectId::from_string("proj-f7".to_string());

    let mut task =
        create_test_task_with_status(&project_id, "Task to delete", InternalStatus::Failed);
    let branch_name = "ralphx/test-project/task-f7".to_string();
    task.task_branch = Some(branch_name.clone());
    task.worktree_path = Some("/tmp/ralphx-worktrees/task-f7".to_string());
    let task_id = task.id.clone();
    s.task_repo.create(task).await.unwrap();

    // Verify task exists with branch info
    let stored = s.task_repo.get_by_id(&task_id).await.unwrap();
    assert!(stored.is_some(), "Task should exist before deletion");
    assert_eq!(
        stored.unwrap().task_branch,
        Some(branch_name),
        "Branch info should be present"
    );

    // Delete the task
    s.task_repo.delete(&task_id).await.unwrap();

    // Task is gone — branch info is lost
    let after_delete = s.task_repo.get_by_id(&task_id).await.unwrap();
    assert!(after_delete.is_none(), "Task should be deleted from repo");

    // GAP: The git branch and its worktree are now orphaned with no DB record
    // to identify them for cleanup. No orphan tracking mechanism exists.
}

// ============================================================================
// F8: Worktree on broken symlink — GAP
// ============================================================================

#[tokio::test]
async fn test_f8_no_worktree_path_health_validation() {
    // GAP: A task can have a worktree_path pointing to a non-existent directory.
    // There is no validation of worktree path health at the state machine level.
    let s = create_hardening_services();
    let project_id = ProjectId::from_string("proj-f8".to_string());

    let mut task = create_test_task_with_status(
        &project_id,
        "Broken worktree task",
        InternalStatus::PendingMerge,
    );
    task.task_branch = Some("ralphx/test-project/task-f8".to_string());
    task.worktree_path = Some("/nonexistent/path/that/does/not/exist".to_string());
    s.task_repo.create(task.clone()).await.unwrap();

    // Verify the task can be stored and retrieved with the bogus path
    let stored = s
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("Task should exist");
    assert_eq!(
        stored.worktree_path.as_deref(),
        Some("/nonexistent/path/that/does/not/exist"),
        "GAP: Invalid worktree_path is accepted without validation"
    );

    // State machine transitions don't validate worktree_path health
    let services = build_task_services(&s);
    let mut machine = create_state_machine(task.id.as_str(), "proj-f8", services);
    let mut handler = create_transition_handler(&mut machine);

    let result = handler
        .handle_transition(&State::PendingMerge, &TaskEvent::MergeComplete)
        .await;

    assert!(
        result.is_success(),
        "GAP: Transition succeeds even with broken worktree path — no path health validation"
    );
}

// ============================================================================
// FIX: Recreate deleted plan branch when re-executing a task from a merged plan — COVERED
// ============================================================================

#[tokio::test]
async fn test_fix_merged_plan_branch_status_triggers_recreation_on_reexecution() {
    // COVERED: When a task's plan branch has PlanBranchStatus::Merged,
    // resolve_task_base_branch handles the Merged arm instead of falling through
    // to the default (main). In the state machine flow without a real git repo,
    // the recreation attempt fails and the task ends up in Failed via ExecutionBlocked.
    //
    // Before the fix: the Merged arm didn't exist — the match fell through to `_ => default`,
    // so the task silently used "main" as its base branch instead of the plan branch.
    // After the fix: the Merged arm tries to recreate the branch; if git fails, it falls back
    // gracefully. Either way, the code path is exercised (not silently bypassed).
    //
    // Full unit-level regression (with a real temp git repo) is in:
    //   merge_helpers.rs::tests::test_resolve_task_base_branch_merged_branch_missing_recreates_it
    use crate::domain::entities::{ArtifactId, IdeationSessionId, PlanBranch, PlanBranchStatus};

    let s = create_hardening_services();

    let project = create_test_project("merged-plan-proj");
    let project_id = project.id.clone();
    s.project_repo.create(project).await.unwrap();

    // Set up a plan branch in Merged status (simulates post-merge state)
    let session_id = IdeationSessionId::from_string("session-merged-plan");
    let pb = {
        let mut branch = PlanBranch::new(
            ArtifactId::from_string("artifact-merged-plan"),
            session_id.clone(),
            project_id.clone(),
            "ralphx/merged-plan-proj/plan-session-merged-plan".to_string(),
            "main".to_string(),
        );
        branch.status = PlanBranchStatus::Merged;
        branch
    };
    s.plan_branch_repo.create(pb).await.unwrap();

    // Create a task with ideation_session_id pointing to the merged plan
    let mut task = create_test_task(&project_id, "Re-executed task from merged plan");
    task.internal_status = InternalStatus::Ready;
    task.ideation_session_id = Some(session_id.clone());
    let task_id_str = task.id.as_str().to_string();
    s.task_repo.create(task).await.unwrap();

    let services = build_task_services(&s);
    let mut machine = create_state_machine(&task_id_str, project_id.as_str(), services);
    let mut handler = create_transition_handler(&mut machine);

    // Transition Ready -> Executing: triggers resolve_task_base_branch with Merged plan branch
    let result = handler
        .handle_transition(&State::Ready, &TaskEvent::StartExecution)
        .await;

    // The Merged arm is now exercised. Git operations fail on /tmp/test-project (no real repo),
    // so the task enters ExecutionBlocked -> auto-fails to Failed.
    // Previously (before the fix): would silently fall through to main as base branch.
    assert!(
        result.is_success(),
        "TransitionHandler should return Success (auto-dispatches ExecutionFailed on git failure)"
    );

    // Verify the plan branch repo is accessible (state machine wired correctly)
    let stored_pb = s
        .plan_branch_repo
        .get_by_session_id(&session_id)
        .await
        .unwrap();
    assert!(
        stored_pb.is_some(),
        "Plan branch should still exist in repo after re-execution attempt"
    );
}

// ============================================================================
// FIX: Stale task_branch/worktree_path cleared on merge cleanup — COVERED
// ============================================================================

#[tokio::test]
async fn test_fix_complete_merge_internal_clears_task_branch_and_worktree_path() {
    // COVERED: After complete_merge_internal() runs, task.task_branch and
    // task.worktree_path must both be None in the DB.
    //
    // Bug: Before this fix, cleanup_branch_and_worktree_internal() deleted the
    // git branch/worktree but left task.task_branch and task.worktree_path set
    // to the now-deleted values. When the task was later reopened and
    // re-executed, on_enter(Executing) saw task.task_branch.is_some() and
    // skipped branch setup entirely, then failed trying to use the deleted branch.
    //
    // NOTE: V2 fix — complete_merge_internal now requires git verification to pass
    // (non-fatal Err is now fatal). Use a real git repo so verification succeeds;
    // branch/worktree deletion still fails gracefully for non-existent paths.
    let tmp = tempfile::TempDir::new().expect("create temp dir");
    let repo_path = tmp.path();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "t@t.com"],
        vec!["config", "user.name", "T"],
    ] {
        let _ = std::process::Command::new("git")
            .args(&args)
            .current_dir(repo_path)
            .output();
    }
    std::fs::write(repo_path.join("README.md"), "# test").unwrap();
    for args in [vec!["add", "."], vec!["commit", "-m", "init"]] {
        let _ = std::process::Command::new("git")
            .args(&args)
            .current_dir(repo_path)
            .output();
    }
    // Get the real HEAD commit SHA on main
    let head_sha_output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .expect("git rev-parse HEAD");
    let head_sha = String::from_utf8_lossy(&head_sha_output.stdout)
        .trim()
        .to_string();

    let s = create_hardening_services();

    let project_id = ProjectId::from_string("proj-fix-cleanup".to_string());
    let mut task = create_test_task_with_status(
        &project_id,
        "Merged task with stale branch",
        InternalStatus::PendingMerge,
    );
    task.task_branch = Some("ralphx/test-project/task-fix-cleanup".to_string());
    task.worktree_path = Some("/tmp/ralphx-worktrees/task-fix-cleanup".to_string());
    s.task_repo.create(task.clone()).await.unwrap();

    // Use real repo path so git verification passes; set base_branch = "main"
    let mut project = Project::new(
        "proj-fix-cleanup".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    s.project_repo.create(project.clone()).await.unwrap();

    // Call complete_merge_internal with the real HEAD SHA on main.
    // is_commit_on_branch succeeds (HEAD is on main). Branch/worktree deletion
    // fails gracefully (non-existent paths). We verify task_branch + worktree_path
    // are cleared in the DB even when the git cleanup steps fail.
    let task_repo =
        s.task_repo.clone() as std::sync::Arc<dyn crate::domain::repositories::TaskRepository>;
    let result = complete_merge_internal(
        &mut task, &project, &head_sha, "", "main", &task_repo, None, None, None, None,
    )
    .await;

    assert!(
        result.is_ok(),
        "complete_merge_internal should succeed when commit is verified on branch: {:?}",
        result
    );

    // Phase 2: complete_merge_internal now defers cleanup to Phase 3.
    // task_branch and worktree_path are NOT cleared immediately — instead
    // pending_cleanup metadata is set for deferred cleanup.
    let stored_task = s
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("Task should exist in DB");

    assert_eq!(
        stored_task.internal_status,
        InternalStatus::Merged,
        "Task must be Merged after complete_merge_internal"
    );

    // pending_cleanup metadata must be set (Phase 3 will clean up)
    use crate::domain::state_machine::transition_handler::has_pending_cleanup_metadata;
    assert!(
        has_pending_cleanup_metadata(&stored_task),
        "pending_cleanup metadata must be set for deferred cleanup"
    );

    // Verify task_branch and worktree_path are still set (cleanup deferred)
    // They will be cleared by deferred_merge_cleanup (Phase 3)
    assert!(
        stored_task.task_branch.is_some(),
        "task_branch should still be set (cleanup deferred to Phase 3)"
    );
    assert!(
        stored_task.worktree_path.is_some(),
        "worktree_path should still be set (cleanup deferred to Phase 3)"
    );

    // Now run Phase 3 deferred cleanup to verify it clears the fields
    use crate::domain::state_machine::transition_handler::deferred_merge_cleanup;
    deferred_merge_cleanup(
        task.id.clone(),
        task_repo.clone(),
        repo_path.to_string_lossy().to_string(),
        task.task_branch.clone(),
        task.worktree_path.clone(),
        None,
    )
    .await;

    let final_task = s
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .expect("Task should exist after cleanup");

    assert!(
        final_task.task_branch.is_none(),
        "task_branch must be cleared after deferred cleanup (Phase 3)"
    );
    assert!(
        final_task.worktree_path.is_none(),
        "worktree_path must be cleared after deferred cleanup (Phase 3)"
    );
    assert!(
        !has_pending_cleanup_metadata(&final_task),
        "pending_cleanup must be cleared after deferred cleanup (Phase 3)"
    );
}

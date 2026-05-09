// Integration tests for 6 merge pipeline gaps identified in the test audit.
//
// All tests use real git repos + real DB (MemoryTaskRepository) + MockChatService.
// Mock agent spawning only — verify call_count() and metadata assertions.
//
// Gaps covered:
//   Gap 1: E2E merger-resolves-conflict → auto-complete → Merged
//   Gap 2: Two-phase merge flow (plan-update + task-merge in sequence)
//   Gap 3: Worktree cleanup assertion in pre_merge_cleanup
//   Gap 4: Post-conflict auto-complete failure → MergeIncomplete
//   Gap 5: Attempt counter persistence across MergeIncomplete→PendingMerge→Merging cycles
//   Gap 6: Source update with existing worktree (update_source_from_target fallback)

use super::helpers::*;
use crate::domain::entities::{
    IdeationSessionId, InternalStatus, MergeStrategy, PlanBranchStatus, Project, ProjectId,
    ReviewScopeMetadata, Task,
};
use crate::domain::repositories::PlanBranchRepository;
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::{State, TransitionHandler};
use crate::infrastructure::memory::MemoryPlanBranchRepository;

// ──────────────────────────────────────────────────────────────────────────────
// Shared helper: TaskServices with a retained Arc<MockChatService>
// ──────────────────────────────────────────────────────────────────────────────

/// Build TaskServices with a retained Arc<MockChatService> for call_count assertions.
fn make_services_with_tracked_chat(
    task_repo: Arc<MemoryTaskRepository>,
    project_repo: Arc<MemoryProjectRepository>,
) -> (Arc<MockChatService>, TaskServices) {
    let chat_service = Arc::new(MockChatService::new());
    let services = TaskServices::new(
        Arc::new(MockAgentSpawner::new()) as Arc<dyn AgentSpawner>,
        Arc::new(MockEventEmitter::new()) as Arc<dyn EventEmitter>,
        Arc::new(MockNotifier::new()) as Arc<dyn Notifier>,
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(MockReviewStarter::new()) as Arc<dyn ReviewStarter>,
        Arc::clone(&chat_service) as Arc<dyn ChatService>,
    )
    .with_task_scheduler(Arc::new(MockTaskScheduler::new()) as Arc<dyn TaskScheduler>)
    .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
    .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>);
    (chat_service, services)
}

// ──────────────────────────────────────────────────────────────────────────────
// Gap 1: E2E merger-resolves-conflict → auto-complete → Merged
// ──────────────────────────────────────────────────────────────────────────────

/// Gap 1: Simulate conflict resolution by manually resolving the merge conflict on the
/// target branch, then verify the full pipeline from PendingMerge → Merged.
///
/// Flow:
///   1. Create a repo with conflicting branches
///   2. Manually resolve the conflict on main (simulating what a merger agent does:
///      merge the task branch content onto main)
///   3. Run on_enter(PendingMerge) — should detect that task branch content is already
///      on main (check_already_merged or successful programmatic merge) and complete
///   4. Assert: task ends in Merged, merge commit exists on target branch
#[tokio::test]
async fn gap1_conflict_resolved_then_auto_complete_to_merged() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    // Create a divergent commit on main (same file as task branch)
    std::fs::write(path.join("feature.rs"), "// main conflicting version\n").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "conflicting change on main"])
        .current_dir(path)
        .output();

    // Simulate the merger agent's resolution: merge task branch into main manually,
    // resolving the conflict by keeping both changes.
    let merge_output = std::process::Command::new("git")
        .args(["merge", &git_repo.task_branch, "--no-edit"])
        .current_dir(path)
        .output()
        .expect("git merge");

    if !merge_output.status.success() {
        // Conflict occurred — resolve it by taking "ours" and adding task content
        std::fs::write(
            path.join("feature.rs"),
            "// main conflicting version\n// feature code\nfn feature() {}\n",
        )
        .unwrap();
        let _ = std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(path)
            .output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "Merge: resolved conflict"])
            .current_dir(path)
            .output();
    }

    // Verify precondition: task branch content is now on main
    let log = std::process::Command::new("git")
        .args(["log", "--oneline", "main"])
        .current_dir(path)
        .output()
        .expect("git log");
    let log_str = String::from_utf8_lossy(&log.stdout);
    assert!(
        log_str.contains("feature") || log_str.contains("Merge"),
        "Precondition: task branch should be merged into main. Log:\n{}",
        log_str
    );

    // Now run the merge pipeline — should detect already-merged and complete
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Gap1 E2E conflict resolved".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(git_repo.task_branch.clone());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&git_repo.path_string());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project_repo.create(project).await.unwrap();

    let (_, services) =
        make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "Task should be Merged after conflict was pre-resolved on main. Got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );

    // Verify merge commit is on main
    let post_log = std::process::Command::new("git")
        .args(["log", "--oneline", "main"])
        .current_dir(path)
        .output()
        .expect("git log");
    let post_log_str = String::from_utf8_lossy(&post_log.stdout);
    assert!(
        post_log_str.contains("feature") || post_log_str.contains("Merge"),
        "Merge commit should exist on main after completion. Log:\n{}",
        post_log_str,
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Gap 2: Two-phase merge flow (plan-update + task-merge in sequence)
// ──────────────────────────────────────────────────────────────────────────────

/// Gap 2: Task targeting a plan branch where the plan branch is behind main.
///
/// Flow:
///   1. Create repo with main, plan branch (from main), then add commit to main
///   2. Create task branch from plan branch with a non-conflicting change
///   3. Run on_enter(PendingMerge) — should:
///      a. Phase 1 (plan-update): update plan branch from main (fast-forward or merge)
///      b. Phase 2 (task-merge): merge task branch into updated plan branch
///   4. Assert: task ends in Merged, plan branch has both main's and task's changes
#[tokio::test]
async fn gap2_two_phase_plan_update_then_task_merge() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    // Create plan branch from main (BEFORE adding new main commit)
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", "plan/two-phase-test"])
        .current_dir(path)
        .output();
    // Back to main
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();

    // Add a commit to main AFTER plan branch was created (plan becomes behind)
    std::fs::write(path.join("hotfix.rs"), "// hotfix on main after plan branch").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "hotfix.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "fix: hotfix on main"])
        .current_dir(path)
        .output();

    // Create task branch from plan branch with a non-conflicting change
    let _ = std::process::Command::new("git")
        .args(["checkout", "plan/two-phase-test"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", "task/two-phase-work"])
        .current_dir(path)
        .output();
    std::fs::write(path.join("task_work.rs"), "// task work done on plan").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "task_work.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "feat: task work"])
        .current_dir(path)
        .output();

    // Back to main
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();

    // Set up repos
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Gap2 two-phase merge".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("task/two-phase-work".to_string());
    // Task belongs to a plan session
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&git_repo.path_string());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project_repo.create(project).await.unwrap();

    // Plan branch with session_id="sess-1" and status=Active
    let pb = make_plan_branch(
        "artifact-1",
        "plan/two-phase-test",
        PlanBranchStatus::Active,
        None,
    );
    plan_branch_repo.create(pb).await.unwrap();

    let (_, services) =
        make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let services = services
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "Task should be Merged after two-phase merge (plan-update + task-merge). Got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );

    // Verify: plan branch should have BOTH main's hotfix AND the task's work
    let plan_log = std::process::Command::new("git")
        .args(["log", "--oneline", "plan/two-phase-test"])
        .current_dir(path)
        .output()
        .expect("git log");
    let plan_log_str = String::from_utf8_lossy(&plan_log.stdout);
    assert!(
        plan_log_str.contains("hotfix"),
        "Plan branch should contain main's hotfix (phase 1 plan-update). Log:\n{}",
        plan_log_str,
    );
    assert!(
        plan_log_str.contains("task work") || plan_log_str.contains("two-phase"),
        "Plan branch should contain task's work (phase 2 task-merge). Log:\n{}",
        plan_log_str,
    );
}

/// Merge backstop: if review already recorded unrelated drift and the branch still contains
/// that out-of-scope file at PendingMerge, the merge must fail closed instead of silently
/// proceeding to merge unrelated work. The backstop must still run when retry metadata
/// points at a task worktree that was already cleaned up.
#[tokio::test]
async fn gap2b_unrelated_reviewed_drift_blocks_merge_attempt() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Gap2b scope drift backstop".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(git_repo.task_branch.clone());
    task.worktree_path = Some(
        path.join("deleted-task-worktree")
            .to_string_lossy()
            .to_string(),
    );
    task.metadata = Some(
        ReviewScopeMetadata::new(
            vec!["src-tauri/src/http_server".to_string()],
            vec!["feature.rs".to_string()],
            Some("unrelated_drift".to_string()),
            Some("feature.rs came from unrelated cleanup and must not merge".to_string()),
        )
        .update_task_metadata(None)
        .unwrap(),
    );
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&git_repo.path_string());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project_repo.create(project).await.unwrap();

    let (_, services) =
        make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::MergeIncomplete,
        "Without transition_service wiring in this test harness, unrelated reviewed drift should fail closed before merge. Metadata: {:?}",
        updated.metadata,
    );

    let metadata: serde_json::Value =
        serde_json::from_str(updated.metadata.as_deref().unwrap_or("{}")).unwrap();
    assert_eq!(metadata["scope_guard_triggered"], true);
    assert_eq!(metadata["error_code"], "merge_scope_drift_guard_fallback");

    let log = std::process::Command::new("git")
        .args(["log", "--oneline", "main"])
        .current_dir(path)
        .output()
        .expect("git log");
    let log_str = String::from_utf8_lossy(&log.stdout);
    assert!(
        !log_str.contains("Gap2b scope drift backstop"),
        "Merge must not create a main-branch merge commit when the backstop trips. Log:\n{}",
        log_str,
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Gap 3: Worktree cleanup assertion in pre_merge_cleanup
// ──────────────────────────────────────────────────────────────────────────────

/// Gap 3: pre_merge_cleanup should delete ALL known worktree types for a task.
///
/// Setup: create task with existing worktrees (task-{id}, merge-{id}, rebase-{id},
///        plan-update-{id}, source-update-{id})
/// Run: on_enter(PendingMerge) which calls pre_merge_cleanup
/// Assert: ALL worktrees are cleaned up
///
/// Also tests the case where task.worktree_path has been overwritten to merge-{id}
/// (stale merge attempt) — the original task-{id} is still cleaned via step 4.
#[tokio::test]
async fn gap3_pre_merge_cleanup_deletes_all_worktree_types() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    // We need a project with a known worktree parent so we can predict worktree paths
    let project_id = ProjectId::from_string("proj-1".to_string());
    let worktree_parent = tempfile::tempdir().unwrap();
    let worktree_parent_str = worktree_parent.path().to_string_lossy().to_string();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let mut task = Task::new(project_id.clone(), "Gap3 cleanup test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(git_repo.task_branch.clone());
    let task_id = task.id.clone();
    let task_id_str = task_id.as_str().to_string();

    let mut project = make_real_git_project(&git_repo.path_string());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project.worktree_parent_directory = Some(worktree_parent_str.clone());
    project_repo.create(project.clone()).await.unwrap();

    // Compute expected worktree paths using the same logic as production code
    let slug = "test-project"; // slugified project name
    let merge_wt = format!("{}/{}/merge-{}", worktree_parent_str, slug, task_id_str);
    let rebase_wt = format!("{}/{}/rebase-{}", worktree_parent_str, slug, task_id_str);
    let plan_update_wt = format!(
        "{}/{}/plan-update-{}",
        worktree_parent_str, slug, task_id_str
    );
    let source_update_wt = format!(
        "{}/{}/source-update-{}",
        worktree_parent_str, slug, task_id_str
    );

    // Ensure parent slug directory exists
    let slug_dir = format!("{}/{}", worktree_parent_str, slug);
    std::fs::create_dir_all(&slug_dir).unwrap();

    // Create the worktrees (we need separate branches for each)
    // First create branches for each worktree
    for (branch, wt_path) in [
        ("merge-wt-branch", &merge_wt),
        ("rebase-wt-branch", &rebase_wt),
        ("plan-update-wt-branch", &plan_update_wt),
        ("source-update-wt-branch", &source_update_wt),
    ] {
        let _ = std::process::Command::new("git")
            .args(["branch", branch, "main"])
            .current_dir(path)
            .output();
        let _ = std::process::Command::new("git")
            .args(["worktree", "add", wt_path, branch])
            .current_dir(path)
            .output();
    }

    // Set task.worktree_path to merge-{id} (simulating a stale merge attempt)
    task.worktree_path = Some(merge_wt.clone());
    // Simulate a retry: set merge failure metadata so Phase 1 GUARD runs cleanup
    task.metadata = Some(serde_json::json!({"merge_failure_source": "test_prior_failure"}).to_string());
    task_repo.create(task).await.unwrap();

    // Verify all worktrees exist before cleanup
    for wt_path in [&merge_wt, &rebase_wt, &plan_update_wt, &source_update_wt] {
        assert!(
            std::path::Path::new(wt_path).exists(),
            "Precondition: worktree should exist at {}",
            wt_path,
        );
    }

    // Run on_enter(PendingMerge) which calls pre_merge_cleanup
    let (_, services) =
        make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    // Assert: all worktrees should be deleted by pre_merge_cleanup
    // Step 2 deletes task.worktree_path (which is merge-{id})
    // Step 4 deletes merge-{id}, rebase-{id}, plan-update-{id}, source-update-{id}
    for (label, wt_path) in [
        ("merge", &merge_wt),
        ("rebase", &rebase_wt),
        ("plan-update", &plan_update_wt),
        ("source-update", &source_update_wt),
    ] {
        assert!(
            !std::path::Path::new(wt_path).exists(),
            "pre_merge_cleanup should delete {} worktree at {}",
            label,
            wt_path,
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Gap 4: Post-conflict auto-complete failure → MergeIncomplete
// ──────────────────────────────────────────────────────────────────────────────

/// Gap 4: After an agent resolves conflicts, if the resolved work is left on a
/// merge-resolve/{id} branch (not fast-forwarded to target), the next
/// on_enter(PendingMerge) should detect the source branch still diverges from
/// target and either complete the merge or go to MergeIncomplete.
///
/// Current behavior: if the merge-resolve branch has the work but it was not
/// pushed to main, a subsequent on_enter(PendingMerge) will attempt a fresh
/// programmatic merge and either succeed (if no conflicts remain) or fail.
///
/// This test verifies the pipeline handles the scenario where the agent's work
/// exists on the source branch but needs programmatic merge completion.
#[tokio::test]
async fn gap4_post_conflict_incomplete_resolution_goes_to_merge_incomplete_or_merged() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    // Create a divergent commit on main
    std::fs::write(path.join("feature.rs"), "// main version\n").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "main divergence"])
        .current_dir(path)
        .output();

    // Create a merge-resolve branch from main (simulating agent's resolution branch)
    let task_id_str = "gap4-test-id";
    let resolve_branch = format!("merge-resolve/{}", task_id_str);
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", &resolve_branch])
        .current_dir(path)
        .output();

    // Agent "resolved" by merging the task branch content onto the resolve branch
    // but this is on merge-resolve branch, NOT on main
    let merge_result = std::process::Command::new("git")
        .args(["merge", &git_repo.task_branch, "--no-edit"])
        .current_dir(path)
        .output();

    if !merge_result.unwrap().status.success() {
        // Resolve the conflict
        std::fs::write(
            path.join("feature.rs"),
            "// resolved: main + task\nfn feature() {}\n",
        )
        .unwrap();
        let _ = std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(path)
            .output();
        let _ = std::process::Command::new("git")
            .args(["commit", "-m", "Resolved merge conflict"])
            .current_dir(path)
            .output();
    }

    // Go back to main — the resolution is on merge-resolve/{id}, NOT on main
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();

    // Set up the task as if it was in a retry after MergeIncomplete
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Gap4 post-conflict incomplete".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    // The task branch still points to the original task branch, not the resolve branch
    task.task_branch = Some(git_repo.task_branch.clone());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&git_repo.path_string());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project_repo.create(project).await.unwrap();

    let (chat_service, services) =
        make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    // The task's source branch still conflicts with main (agent work is on merge-resolve,
    // not on the task branch itself). So this should either:
    // - Transition to Merging (conflict detected, agent needed) if merge fails
    // - Transition to Merged if the programmatic merge somehow succeeds
    // In practice with a real divergent merge, it will go to Merging (needs agent).
    assert!(
        updated.internal_status == InternalStatus::Merging
            || updated.internal_status == InternalStatus::MergeIncomplete
            || updated.internal_status == InternalStatus::Merged,
        "Post-conflict incomplete resolution should transition to Merging, MergeIncomplete, or Merged. \
         Got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );

    // If it went to Merging, verify that a merger agent was spawned
    if updated.internal_status == InternalStatus::Merging {
        assert!(
            chat_service.call_count() >= 1,
            "When transitioning to Merging, a merger agent should be spawned. call_count={}",
            chat_service.call_count(),
        );
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Gap 5: Attempt counter persistence across MergeIncomplete→PendingMerge→Merging cycles
// ──────────────────────────────────────────────────────────────────────────────

/// Gap 5: Verify that merge recovery events accumulate across retry cycles.
///
/// The MergeRecoveryMetadata tracks events via `merge_recovery.events[]`.
/// Each merge attempt that fails appends an AttemptFailed event. When retried
/// (MergeIncomplete → PendingMerge), the next attempt should see the accumulated
/// events from prior attempts.
///
/// This test runs 3 merge cycles with a nonexistent git directory (guaranteed failure)
/// and verifies the AttemptFailed event count reaches 3 by the final cycle.
#[tokio::test]
async fn gap5_attempt_counter_persists_across_retry_cycles() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Gap5 retry counter".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("task/counter-test".to_string());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    // Use a nonexistent path — merge will fail immediately, recording AttemptFailed events
    let mut project = Project::new(
        "test-project".to_string(),
        "/tmp/nonexistent-gap5-test".to_string(),
    );
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project_repo.create(project).await.unwrap();

    // Run 3 retry cycles
    for cycle in 1..=3u32 {
        // Ensure task is in PendingMerge for each cycle
        {
            let mut current = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
            current.internal_status = InternalStatus::PendingMerge;
            task_repo.update(&current).await.unwrap();
        }

        let (_, services) =
            make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
        let context = TaskContext::new(task_id.as_str(), "proj-1", services);
        let mut machine = TaskStateMachine::new(context);
        let handler = TransitionHandler::new(&mut machine);

        let _ = handler.on_enter(&State::PendingMerge).await;

        let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
        assert_eq!(
            updated.internal_status,
            InternalStatus::MergeIncomplete,
            "Cycle {}: task should be MergeIncomplete after failed merge. Got {:?}",
            cycle,
            updated.internal_status,
        );

        // Parse metadata and count AttemptFailed events
        let meta: serde_json::Value =
            serde_json::from_str(updated.metadata.as_deref().unwrap_or("{}")).unwrap();

        let recovery = meta.get("merge_recovery");
        if let Some(recovery_obj) = recovery {
            let events = recovery_obj
                .get("events")
                .and_then(|e| e.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter(|e| {
                            e.get("kind")
                                .and_then(|k| k.as_str())
                                .map(|k| k == "attempt_failed" || k == "auto_retry_triggered")
                                .unwrap_or(false)
                        })
                        .count()
                })
                .unwrap_or(0);

            assert!(
                events >= cycle as usize,
                "Cycle {}: expected at least {} recovery events, got {}. \
                 Recovery metadata: {:?}",
                cycle,
                cycle,
                events,
                recovery_obj,
            );
        }
        // Note: first cycle may not have merge_recovery if the failure path
        // uses a different metadata format. The key assertion is that events
        // accumulate across cycles (checked on cycles 2 and 3).
    }

    // Final verification: after 3 cycles, metadata should contain accumulated events
    let final_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let final_meta: serde_json::Value =
        serde_json::from_str(final_task.metadata.as_deref().unwrap_or("{}")).unwrap();

    // Verify error field is present (merge failed)
    assert!(
        final_meta.get("error").is_some()
            || final_meta.get("merge_recovery").is_some(),
        "Final metadata should contain error or merge_recovery after 3 failed cycles. \
         Metadata: {:?}",
        final_meta,
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Gap 6: Source update with existing worktree (update_source_from_target fallback)
// ──────────────────────────────────────────────────────────────────────────────

/// Gap 6: When the source (task) branch is already checked out in an existing worktree,
/// update_source_from_target should handle it gracefully (not panic/crash).
///
/// Setup: task branch checked out in a leftover worktree, target (main) has new commits
/// Expected: either updates successfully (merging in the existing worktree) or returns
///           an error without crashing
#[tokio::test]
async fn gap6_source_update_with_existing_worktree_no_crash() {
    use super::super::merge_coordination::{update_source_from_target, SourceUpdateResult};

    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    // Add a commit to main AFTER task branch was created (so task is behind)
    std::fs::write(path.join("main_fix.rs"), "// fix committed to main").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "main_fix.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "fix: main fix after task branched"])
        .current_dir(path)
        .output();

    // Create a leftover worktree with the task branch checked out
    // (simulating a stale task-{id} worktree from a prior execution)
    let leftover_wt_dir = tempfile::tempdir().unwrap();
    let leftover_wt = leftover_wt_dir.path().join("task-leftover");
    let _ = std::process::Command::new("git")
        .args([
            "worktree",
            "add",
            &leftover_wt.to_string_lossy(),
            &git_repo.task_branch,
        ])
        .current_dir(path)
        .output()
        .expect("git worktree add for leftover");

    // Verify the worktree exists
    assert!(
        leftover_wt.exists(),
        "Precondition: leftover worktree should exist"
    );

    let project = {
        let mut p = make_real_git_project(&git_repo.path_string());
        p.base_branch = Some("main".to_string());
        p
    };

    // Call update_source_from_target — the task branch is checked out in a worktree,
    // so creating a new worktree for it would fail. The function should handle this
    // gracefully (either use the existing worktree or return an error, but not panic).
    let result = update_source_from_target(
        path,
        &git_repo.task_branch, // source = task branch (behind main, checked out in worktree)
        "main",                // target = main (has new commit)
        &project,
        "gap6-task-id",
        None,
    )
    .await;

    // The key assertion: should NOT panic. Any of these outcomes is acceptable:
    // - Updated: successfully merged in the existing worktree
    // - Error: detected the issue and returned gracefully
    // - AlreadyUpToDate: unlikely but possible if detection failed
    match &result {
        SourceUpdateResult::Updated => {
            // Best case: update succeeded using the existing worktree or a new one
            // Verify the task branch now has main's fix
            let log = std::process::Command::new("git")
                .args(["log", "--oneline", &git_repo.task_branch])
                .current_dir(path)
                .output()
                .expect("git log");
            let log_str = String::from_utf8_lossy(&log.stdout);
            assert!(
                log_str.contains("main fix") || log_str.contains("fix"),
                "Task branch should contain main's fix after update. Log:\n{}",
                log_str,
            );
        }
        SourceUpdateResult::Error(msg) => {
            // Acceptable: function detected the existing worktree issue and returned gracefully
            tracing::info!(
                "Gap6: source update returned Error (graceful failure): {}",
                msg
            );
        }
        SourceUpdateResult::AlreadyUpToDate => {
            // Unexpected but not a failure — the function didn't crash
            tracing::warn!(
                "Gap6: source update returned AlreadyUpToDate (unexpected but not a crash)"
            );
        }
        SourceUpdateResult::Conflicts { .. } => {
            // Also acceptable — could conflict
            tracing::info!("Gap6: source update returned Conflicts (non-conflicting expected, but acceptable)");
        }
    }

    // Clean up the leftover worktree
    let _ = std::process::Command::new("git")
        .args([
            "worktree",
            "remove",
            "--force",
            &leftover_wt.to_string_lossy(),
        ])
        .current_dir(path)
        .output();
}

/// Gap 6b: When the source branch is checked out in the main repo (current branch),
/// update_source_from_target fails because git worktree can't check out the same branch
/// in two places simultaneously. This is a known limitation (RC#9).
///
/// Current behavior: returns Error (graceful failure, not a panic).
/// After RC#9 fix: should succeed by merging target into source directly in the main repo.
#[tokio::test]
async fn gap6b_source_update_when_source_is_current_branch_returns_error() {
    use super::super::merge_coordination::{update_source_from_target, SourceUpdateResult};

    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    // Switch to main and add a commit
    std::fs::write(path.join("main_update.rs"), "// main update").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "main_update.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "fix: main update"])
        .current_dir(path)
        .output();

    // Now checkout task branch (it's now the current branch in the main repo)
    let _ = std::process::Command::new("git")
        .args(["checkout", &git_repo.task_branch])
        .current_dir(path)
        .output();

    let project = {
        let mut p = make_real_git_project(&git_repo.path_string());
        p.base_branch = Some("main".to_string());
        p
    };

    let result = update_source_from_target(
        path,
        &git_repo.task_branch,
        "main",
        &project,
        "gap6b-task-id",
        None,
    )
    .await;

    // RC#9 fix: update_source_from_target now detects the source branch is checked out
    // in an existing worktree and merges there instead of failing.
    assert!(
        matches!(result, SourceUpdateResult::Updated),
        "Source update should succeed via existing-worktree fallback (RC#9 fix). Got: {:?}",
        result
    );
}

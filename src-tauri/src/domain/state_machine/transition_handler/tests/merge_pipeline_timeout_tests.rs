// Integration tests for merge pipeline timeout & cleanup fixes.
//
// Real git repos + memory repos + mock agent spawns only.
// Follows patterns from orchestration_chain_tests.rs and real_git_integration.rs.
//
// Scenarios:
//   1. lsof timeout returns within bounded time on a real worktree
//   2. Pre-merge cleanup completes in bounded time with a real merge
//   3. Stale index.lock doesn't block merge — cleanup removes it
//   4. Full pipeline end-to-end: PendingMerge → Merged in bounded time

use super::helpers::*;
use crate::domain::entities::{InternalStatus, MergeStrategy, ProjectId, Task};
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::{State, TransitionHandler};

// ──────────────────────────────────────────────────────────────────────────────
// Helper: services with tracked chat + scheduler (mirrors orchestration_chain_tests)
// ──────────────────────────────────────────────────────────────────────────────

fn make_services_with_repos(
    task_repo: Arc<MemoryTaskRepository>,
    project_repo: Arc<MemoryProjectRepository>,
) -> TaskServices {
    let services = TaskServices::new(
        Arc::new(MockAgentSpawner::new()) as Arc<dyn AgentSpawner>,
        Arc::new(MockEventEmitter::new()) as Arc<dyn EventEmitter>,
        Arc::new(MockNotifier::new()) as Arc<dyn Notifier>,
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(MockReviewStarter::new()) as Arc<dyn ReviewStarter>,
        Arc::new(MockChatService::new()) as Arc<dyn ChatService>,
    )
    .with_branch_update_repo(
        crate::testing::memory_branch_update_repository_with_task_repository(
            Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        ),
    )
    .with_branch_update_workflow(crate::testing::branch_update_workflow(Arc::new(
        MockChatService::new(),
    )))
    .with_task_scheduler(Arc::new(MockTaskScheduler::new()) as Arc<dyn TaskScheduler>)
    .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
    .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>);
    services
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 1: lsof timeout returns within bounded time
// ──────────────────────────────────────────────────────────────────────────────

/// kill_worktree_processes_async with a short timeout returns within the timeout
/// period, not blocking for minutes even on a real directory tree.
#[tokio::test]
async fn test_lsof_timeout_returns_within_bound() {
    let git_repo = setup_real_git_repo();

    // Create a worktree to scan (just the repo itself — real directory with files)
    let start = std::time::Instant::now();
    crate::domain::services::kill_worktree_processes_async(git_repo.path(), 2, false).await;
    let elapsed = start.elapsed();

    // Should complete quickly (well within 5s); on timeout it would still return
    // within the 2s + overhead, not block for 120s
    assert!(
        elapsed.as_secs() < 5,
        "kill_worktree_processes_async should return within the timeout bound, took {}s",
        elapsed.as_secs()
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 2: Pre-merge cleanup + merge completes in bounded time
// ──────────────────────────────────────────────────────────────────────────────

/// Full merge pipeline (cleanup + strategy dispatch) completes within 30 seconds
/// with a real git repo. This ensures pre_merge_cleanup's step 0 (agent kill +
/// settle time) and step 1-6 (worktree cleanup) don't cause unbounded delays.
#[tokio::test]
async fn test_pre_merge_cleanup_completes_in_bounded_time() {
    let git_repo = setup_real_git_repo();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Bounded cleanup test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(git_repo.task_branch.clone());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&git_repo.path_string());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project_repo.create(project).await.unwrap();

    let services = make_services_with_repos(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = crate::domain::state_machine::context::TaskContext::new(
        task_id.as_str(),
        "proj-1",
        services,
    );
    let mut machine = crate::domain::state_machine::TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let start = std::time::Instant::now();
    let _ = handler.on_enter(&State::PendingMerge).await;
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_secs() < 30,
        "Full merge pipeline should complete within 30s, took {}s",
        elapsed.as_secs()
    );

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "Task should be Merged after bounded-time cleanup test, got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 3: Stale index.lock doesn't block merge
// ──────────────────────────────────────────────────────────────────────────────

/// A stale .git/index.lock is cleaned up by pre_merge_cleanup step 1 and the
/// merge still succeeds.
///
/// Before the fix, a leftover index.lock from a killed git process would cause
/// every subsequent git operation to fail with "Unable to create index.lock: File exists".
#[tokio::test]
async fn test_stale_index_lock_doesnt_block_merge() {
    let git_repo = setup_real_git_repo();

    // Create a stale index.lock (simulates a killed git process)
    let lock_path = git_repo.path().join(".git").join("index.lock");
    std::fs::write(&lock_path, "stale lock").unwrap();

    // Backdate the lock file so it's considered stale (>5s old).
    // Use `touch -t` to set mtime to 60 seconds ago (YYYYMMDDHHSS format).
    let backdated = chrono::Utc::now() - chrono::Duration::seconds(60);
    let touch_ts = backdated.format("%Y%m%d%H%M.%S").to_string();
    let _ = std::process::Command::new("touch")
        .args(["-t", &touch_ts, lock_path.to_str().unwrap()])
        .output()
        .expect("backdate index.lock");

    assert!(lock_path.exists(), "Precondition: index.lock should exist");

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Stale lock test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(git_repo.task_branch.clone());
    // Simulate a retry so Phase 1 GUARD runs cleanup (including stale lock removal)
    task.metadata =
        Some(serde_json::json!({"merge_failure_source": "test_prior_failure"}).to_string());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&git_repo.path_string());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project_repo.create(project).await.unwrap();

    let services = make_services_with_repos(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = crate::domain::state_machine::context::TaskContext::new(
        task_id.as_str(),
        "proj-1",
        services,
    );
    let mut machine = crate::domain::state_machine::TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "Merge should succeed despite stale index.lock, got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );

    // Lock should have been cleaned up
    assert!(
        !lock_path.exists(),
        "Stale index.lock should be removed by pre_merge_cleanup"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 4: Full pipeline end-to-end with all strategies
// ──────────────────────────────────────────────────────────────────────────────

/// Full end-to-end merge pipeline: PendingMerge → Merged, verifying all phases
/// complete and the git log shows the merge commit on main.
///
/// This exercises the complete path including:
/// - pre_merge_cleanup (step 0 agent kill, step 1 lock removal, step 2-5 worktrees)
/// - branch freshness checks (update_plan_from_main, update_source_from_target)
/// - strategy dispatch (Merge strategy, checkout-free path)
/// - finalize (complete_merge_internal)
#[tokio::test]
async fn test_full_pipeline_end_to_end() {
    let git_repo = setup_real_git_repo();

    let setup = setup_pending_merge_with_real_repo(
        "Full pipeline E2E test",
        &git_repo.task_branch,
        &git_repo.path_string(),
        MergeStrategy::Merge,
    )
    .await;

    let task_id = setup.task_id.clone();
    let task_repo = Arc::clone(&setup.task_repo);
    let (mut machine, _task_repo, _task_id) = setup.into_machine();
    let handler = TransitionHandler::new(&mut machine);

    let start = std::time::Instant::now();
    let _ = handler.on_enter(&State::PendingMerge).await;
    let elapsed = start.elapsed();

    // Verify task reached Merged
    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "Full pipeline should reach Merged, got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );

    // Verify bounded time
    assert!(
        elapsed.as_secs() < 30,
        "Full pipeline should complete within 30s, took {}s",
        elapsed.as_secs()
    );

    // Verify git log on main contains the feature commit
    let log_output = std::process::Command::new("git")
        .args(["log", "--oneline", "main"])
        .current_dir(git_repo.path())
        .output()
        .expect("git log");
    let log_str = String::from_utf8_lossy(&log_output.stdout);
    assert!(
        log_str.contains("feature") || log_str.contains("add feature"),
        "Git log on main should contain the merged feature commit. Log:\n{}",
        log_str,
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 5: Stale task worktree gets cleaned before merge
// ──────────────────────────────────────────────────────────────────────────────

/// A stale task worktree from a prior execution is cleaned up by pre_merge_cleanup
/// step 2 and step 4, and the merge still succeeds.
///
/// Simulates: task executed in a worktree, agent died, worktree left behind.
/// The merge pipeline should delete it to unlock the task branch.
#[tokio::test]
async fn test_stale_task_worktree_cleaned_before_merge() {
    let git_repo = setup_real_git_repo();
    let repo_path = git_repo.path();

    // Create a stale worktree (simulates prior task execution)
    let task_wt_path = repo_path.join(".worktrees").join("stale-task-wt");
    let _ = std::process::Command::new("git")
        .args([
            "worktree",
            "add",
            task_wt_path.to_str().unwrap(),
            &git_repo.task_branch,
        ])
        .current_dir(repo_path)
        .output()
        .expect("create stale worktree");

    assert!(
        task_wt_path.exists(),
        "Precondition: stale task worktree should exist"
    );

    // Set up the task with worktree_path pointing to the stale worktree
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(
        project_id.clone(),
        "Stale worktree cleanup test".to_string(),
    );
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(git_repo.task_branch.clone());
    task.worktree_path = Some(task_wt_path.to_string_lossy().to_string());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&git_repo.path_string());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project_repo.create(project).await.unwrap();

    let services = make_services_with_repos(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = crate::domain::state_machine::context::TaskContext::new(
        task_id.as_str(),
        "proj-1",
        services,
    );
    let mut machine = crate::domain::state_machine::TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "Merge should succeed after stale worktree cleanup, got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );

    // Phase 3 deferred cleanup runs as tokio::spawn — yield to let it execute
    // before checking the worktree is removed.
    for _ in 0..20 {
        if !task_wt_path.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    // Stale worktree should have been removed by deferred cleanup (Phase 3)
    assert!(
        !task_wt_path.exists(),
        "Stale task worktree should be removed by deferred merge cleanup (Phase 3)"
    );
}

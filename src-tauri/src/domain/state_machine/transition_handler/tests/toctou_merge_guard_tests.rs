// TOCTOU race guard tests for pre_merge_cleanup Steps 2 and 4.
//
// These tests verify that pre_merge_cleanup skips worktree deletion when the task
// has transitioned to InternalStatus::Merging (indicating a merge agent has claimed
// the worktree), and proceeds with deletion when the task is in other states.
//
// Test strategy: state-based tests (not actual concurrent interleaving):
//   - Store task with target status in DB
//   - Create worktree directories on disk so filter_map and exists() checks pass
//   - Run on_enter(PendingMerge) which calls pre_merge_cleanup internally
//   - Assert directory presence/absence proves guard behaviour

use super::helpers::*;
use crate::domain::entities::{InternalStatus, Project, ProjectId, Task};
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::transition_handler::merge_helpers::{
    compute_merge_worktree_path, compute_plan_update_worktree_path, compute_rebase_worktree_path,
    compute_task_worktree_path,
};
use crate::domain::state_machine::{State, TransitionHandler};

// ──────────────────────────────────────────────────────────────────────────────
// Shared helper: TaskServices with repos wired (same pattern as merge_pipeline_gaps)
// ──────────────────────────────────────────────────────────────────────────────

fn make_services_for_guard_test(
    task_repo: Arc<MemoryTaskRepository>,
    project_repo: Arc<MemoryProjectRepository>,
) -> TaskServices {
    TaskServices::new(
        Arc::new(MockAgentSpawner::new()) as Arc<dyn AgentSpawner>,
        Arc::new(MockEventEmitter::new()) as Arc<dyn EventEmitter>,
        Arc::new(MockNotifier::new()) as Arc<dyn Notifier>,
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(MockReviewStarter::new()) as Arc<dyn ReviewStarter>,
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    )
    .with_task_scheduler(Arc::new(MockTaskScheduler::new()) as Arc<dyn TaskScheduler>)
    .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
    .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
}

// ──────────────────────────────────────────────────────────────────────────────
// Step 2 guard: task's own worktree_path deletion
// ──────────────────────────────────────────────────────────────────────────────

/// Step 2 guard: task in Merging status → skip deletion of task.worktree_path.
///
/// Setup:
///   - Task status = Merging, worktree_path set to an existing temp directory
///   - Debris metadata present → forces is_first_clean_attempt() = false → cleanup runs
/// Expected: guard detects Merging, skips remove_worktree_fast, directory still exists.
#[tokio::test]
async fn step2_guard_merging_skips_task_worktree_deletion() {
    // Create a real temp dir — this is the task's "worktree" that should be preserved
    let task_worktree_dir = tempfile::tempdir().expect("create temp dir");
    let task_worktree_path = task_worktree_dir.path().to_path_buf();
    // Create a file inside so non-empty dir survives
    std::fs::write(task_worktree_path.join("sentinel"), "alive").expect("write sentinel");

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Guard test: step 2 skip".to_string());
    task.internal_status = InternalStatus::Merging; // <- guard should fire
    task.task_branch = Some("feature/guard-test".to_string());
    // Set worktree_path → ensures is_first_clean_attempt() returns false
    task.worktree_path = Some(task_worktree_path.to_string_lossy().to_string());
    // Debris metadata → also ensures is_first_clean_attempt() returns false
    task.metadata =
        Some(serde_json::json!({"merge_failure_source": "guard_test_prior_failure"}).to_string());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = Project::new(
        "test-project".to_string(),
        // Nonexistent repo path — git commands fail silently, rm -rf is blocked by guard
        "/tmp/nonexistent-guard-step2-merging".to_string(),
    );
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project_repo.create(project).await.unwrap();

    assert!(
        task_worktree_path.exists(),
        "Precondition: task worktree dir must exist before test"
    );

    let services = make_services_for_guard_test(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = crate::domain::state_machine::TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    assert!(
        task_worktree_path.exists(),
        "Step 2 guard must skip deletion when task is Merging — directory should still exist"
    );
}

/// Step 2 guard: task in PendingMerge status → proceed with deletion of task.worktree_path.
///
/// This verifies the guard is inactive for non-Merging statuses, so cleanup still works.
#[tokio::test]
async fn step2_guard_pending_merge_proceeds_with_task_worktree_deletion() {
    let task_worktree_dir = tempfile::tempdir().expect("create temp dir");
    let task_worktree_path = task_worktree_dir.path().to_path_buf();
    std::fs::write(task_worktree_path.join("sentinel"), "to_delete").expect("write sentinel");

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Guard test: step 2 proceed".to_string());
    task.internal_status = InternalStatus::PendingMerge; // <- guard should NOT fire
    task.task_branch = Some("feature/guard-test".to_string());
    task.worktree_path = Some(task_worktree_path.to_string_lossy().to_string());
    task.metadata =
        Some(serde_json::json!({"merge_failure_source": "guard_test_prior_failure"}).to_string());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = Project::new(
        "test-project".to_string(),
        "/tmp/nonexistent-guard-step2-pending".to_string(),
    );
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project_repo.create(project).await.unwrap();

    assert!(
        task_worktree_path.exists(),
        "Precondition: task worktree dir must exist before test"
    );

    let services = make_services_for_guard_test(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = crate::domain::state_machine::TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    // The TempDir drop would normally clean up, but we check before drop.
    // Use into_path() to prevent TempDir from deleting, then check manually.
    // Actually the directory was deleted by pre_merge_cleanup, so path no longer exists.
    assert!(
        !task_worktree_path.exists(),
        "Step 2 should delete worktree when task is PendingMerge (guard inactive)"
    );
    // Prevent TempDir from trying to clean up a nonexistent dir (ignore error)
    std::mem::forget(task_worktree_dir);
}

// ──────────────────────────────────────────────────────────────────────────────
// Step 4 guard: merge-{task_id} worktree in parallel deletion list
// ──────────────────────────────────────────────────────────────────────────────

/// Step 4 guard: task in Merging status → skip deletion of merge-{id} worktree.
///
/// Setup:
///   - Task status = Merging, task.worktree_path = None (step 2 skipped)
///   - Project has worktree_parent_directory set to a known temp dir
///   - merge-{task_id} directory exists on disk
///   - Debris metadata → cleanup runs, enters step 4
/// Expected: guard inside async future body fires, skips remove_worktree_fast, dir survives.
#[tokio::test]
async fn step4_guard_merging_skips_merge_worktree_deletion() {
    let worktree_parent = tempfile::tempdir().expect("create worktree parent");
    let worktree_parent_str = worktree_parent.path().to_string_lossy().to_string();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Guard test: step 4 skip".to_string());
    task.internal_status = InternalStatus::Merging; // <- step 4 guard should fire
    task.task_branch = Some("feature/guard-test".to_string());
    // worktree_path = None so step 2 is skipped (no task worktree registered)
    task.worktree_path = None;
    // Debris metadata ensures cleanup runs (is_first_clean_attempt returns false)
    task.metadata =
        Some(serde_json::json!({"merge_failure_source": "guard_test_prior_failure"}).to_string());
    let task_id = task.id.clone();
    let task_id_str = task_id.as_str().to_string();
    task_repo.create(task).await.unwrap();

    // Project with worktree_parent_directory so we can compute the merge worktree path
    let slug = "test-project"; // slugify("test-project") == "test-project"
    let merge_wt_path = format!("{}/{}/merge-{}", worktree_parent_str, slug, task_id_str);

    let mut project = Project::new(
        "test-project".to_string(),
        "/tmp/nonexistent-guard-step4-merging".to_string(),
    );
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent_str.clone());
    project_repo.create(project).await.unwrap();

    // Create the merge worktree directory on disk (required for filter_map path.exists() check)
    std::fs::create_dir_all(&merge_wt_path).expect("create merge worktree dir");
    std::fs::write(
        format!("{}/sentinel", merge_wt_path),
        "merge_agent_using_this",
    )
    .expect("write sentinel");

    assert!(
        std::path::Path::new(&merge_wt_path).exists(),
        "Precondition: merge worktree dir must exist before test"
    );

    let services = make_services_for_guard_test(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = TaskContext::new(task_id_str.as_str(), "proj-1", services);
    let mut machine = crate::domain::state_machine::TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    assert!(
        std::path::Path::new(&merge_wt_path).exists(),
        "Step 4 guard must skip deletion of merge-{{id}} worktree when task is Merging — \
         directory should still exist at {}",
        merge_wt_path
    );
}

/// An active merge owns only the merge worktree. Cleanup must preserve that path
/// while still removing stale task, rebase, plan-update, and source-update paths.
#[tokio::test]
async fn step4_guard_merging_preserves_only_active_merge_worktree() {
    let worktree_parent = tempfile::tempdir().expect("create worktree parent");
    let worktree_parent_str = worktree_parent.path().to_string_lossy().to_string();
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let project_id = ProjectId::from_string("proj-1".to_string());

    let mut task = Task::new(
        project_id.clone(),
        "Guard test: selective step 4 cleanup".to_string(),
    );
    task.internal_status = InternalStatus::Merging;
    task.task_branch = Some("feature/guard-test".to_string());
    task.metadata =
        Some(serde_json::json!({"merge_failure_source": "guard_test_prior_failure"}).to_string());
    let task_id = task.id.clone();
    let task_id_str = task_id.as_str().to_string();
    task_repo.create(task).await.unwrap();

    let mut project = Project::new(
        "test-project".to_string(),
        "/tmp/nonexistent-guard-step4-selective".to_string(),
    );
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent_str.clone());
    project_repo.create(project.clone()).await.unwrap();

    let merge_path = std::path::PathBuf::from(compute_merge_worktree_path(&project, &task_id_str));
    let stale_paths = vec![
        std::path::PathBuf::from(compute_task_worktree_path(&project, &task_id_str)),
        std::path::PathBuf::from(compute_rebase_worktree_path(&project, &task_id_str)),
        std::path::PathBuf::from(compute_plan_update_worktree_path(&project, &task_id_str)),
        worktree_parent
            .path()
            .join("test-project")
            .join(format!("source-update-{task_id_str}")),
    ];
    for path in stale_paths.iter().chain(std::iter::once(&merge_path)) {
        std::fs::create_dir_all(path).expect("create cleanup worktree path");
        std::fs::write(path.join("sentinel"), "stale").expect("write cleanup sentinel");
    }

    let services = make_services_for_guard_test(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = crate::domain::state_machine::TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::PendingMerge).await;

    assert!(
        merge_path.exists(),
        "the merge agent's active worktree must survive cleanup"
    );
    for stale_path in stale_paths {
        assert!(
            !stale_path.exists(),
            "non-merge stale worktree must be removed: {}",
            stale_path.display()
        );
    }
}

/// Step 4 guard: task in PendingMerge status → proceed with deletion of merge-{id} worktree.
///
/// Verifies the guard is inactive for PendingMerge — stale merge worktrees ARE cleaned up.
#[tokio::test]
async fn step4_guard_pending_merge_proceeds_with_merge_worktree_deletion() {
    let worktree_parent = tempfile::tempdir().expect("create worktree parent");
    let worktree_parent_str = worktree_parent.path().to_string_lossy().to_string();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Guard test: step 4 proceed".to_string());
    task.internal_status = InternalStatus::PendingMerge; // <- guard should NOT fire
    task.task_branch = Some("feature/guard-test".to_string());
    task.worktree_path = None;
    task.metadata =
        Some(serde_json::json!({"merge_failure_source": "guard_test_prior_failure"}).to_string());
    let task_id = task.id.clone();
    let task_id_str = task_id.as_str().to_string();
    task_repo.create(task).await.unwrap();

    let slug = "test-project";
    let merge_wt_path = format!("{}/{}/merge-{}", worktree_parent_str, slug, task_id_str);

    let mut project = Project::new(
        "test-project".to_string(),
        "/tmp/nonexistent-guard-step4-pending".to_string(),
    );
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(worktree_parent_str.clone());
    project_repo.create(project).await.unwrap();

    // Create merge worktree dir — it should be deleted since task is PendingMerge
    std::fs::create_dir_all(&merge_wt_path).expect("create merge worktree dir");

    assert!(
        std::path::Path::new(&merge_wt_path).exists(),
        "Precondition: merge worktree dir must exist before test"
    );

    let services = make_services_for_guard_test(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = TaskContext::new(task_id_str.as_str(), "proj-1", services);
    let mut machine = crate::domain::state_machine::TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    assert!(
        !std::path::Path::new(&merge_wt_path).exists(),
        "Step 4 should delete stale merge-{{id}} worktree when task is PendingMerge (guard inactive)"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Stale path cleanup after Step 2 timeout
// ──────────────────────────────────────────────────────────────────────────────

use crate::domain::state_machine::transition_handler::merge_coordination::clear_stale_worktree_path_on_timeout;

/// Stale path cleanup: task in non-Merging status → worktree_path cleared to None.
///
/// Simulates what happens after a Step 2 deletion timeout when the task is still
/// in PendingMerge status. The function should clear worktree_path from the DB.
#[tokio::test]
async fn stale_path_cleanup_clears_worktree_path_when_not_merging() {
    let task_repo = Arc::new(MemoryTaskRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(
        project_id,
        "Stale path cleanup test: not merging".to_string(),
    );
    task.internal_status = InternalStatus::PendingMerge;
    task.worktree_path = Some("/some/stale/worktree/path".to_string());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let task_repo_arc: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
    clear_stale_worktree_path_on_timeout(&task_id, task_id.as_str(), &task_repo_arc).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.worktree_path, None,
        "worktree_path should be cleared to None when task is PendingMerge (not Merging)"
    );
}

/// Stale path cleanup: task in Merging status → worktree_path NOT cleared (race guard).
///
/// When the task is actively Merging, the worktree is still needed by the merge agent.
/// The function must NOT clear worktree_path in this case.
#[tokio::test]
async fn stale_path_cleanup_preserves_worktree_path_when_merging() {
    let task_repo = Arc::new(MemoryTaskRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id, "Stale path cleanup test: merging".to_string());
    task.internal_status = InternalStatus::Merging;
    task.worktree_path = Some("/active/merge/worktree/path".to_string());
    let task_id = task.id.clone();
    let expected_path = task.worktree_path.clone();
    task_repo.create(task).await.unwrap();

    let task_repo_arc: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
    clear_stale_worktree_path_on_timeout(&task_id, task_id.as_str(), &task_repo_arc).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.worktree_path, expected_path,
        "worktree_path must NOT be cleared when task is actively Merging (race guard)"
    );
}

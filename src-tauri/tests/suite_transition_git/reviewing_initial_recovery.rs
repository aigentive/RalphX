// Integration tests for the initial on_enter error recovery paths in
// TaskTransitionService::execute_entry_actions when called with Reviewing state.
//
// These tests reproduce the startup-recovery bug where:
// - BranchFreshnessConflict during on_enter(Reviewing) should route → PendingReview
// - ReviewWorktreeMissing during on_enter(Reviewing) should route → Escalated
//
// The initial path (called by StartupJobRunner) differs from the auto-transition path
// (triggered after transition_task). Before this fix, the initial path had no match arm
// for either error, leaving the task stuck in Reviewing on app restart.

use ralphx_lib::application::{AppState, TaskTransitionService};
use ralphx_lib::commands::ExecutionState;
use ralphx_lib::domain::entities::{InternalStatus, Project, ProjectId, Task};

use std::sync::Arc;

// ============================================================================
// Helpers
// ============================================================================

/// Initialize a minimal git repo at `path` so freshness checks don't fail.
fn init_git_repo(path: &std::path::Path) {
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .expect("git command failed");
    };
    run(&["init", "-b", "main"]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    std::fs::write(path.join("README.md"), "# test").unwrap();
    run(&["add", "."]);
    run(&["commit", "-m", "initial"]);
}

/// Build a TaskTransitionService backed by the given AppState's memory repos.
fn build_service(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> TaskTransitionService<tauri::Wry> {
    TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    )
}

/// Seed a project (pointing to `project_dir`) and a task in Reviewing state.
///
/// Returns `(task_id, task)` after storing both in the app_state repos.
async fn seed_reviewing_task(
    app_state: &AppState,
    project_dir: &str,
    worktree_path: Option<&str>,
) -> (
    ralphx_lib::domain::entities::TaskId,
    ralphx_lib::domain::entities::Task,
) {
    let project_id = ProjectId::from_string("proj-review-recovery-test".to_string());

    let mut project = Project::new("test-project".to_string(), project_dir.to_string());
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    app_state.project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id, "Reviewing initial recovery test".to_string());
    task.internal_status = InternalStatus::Reviewing;
    task.worktree_path = worktree_path.map(|s| s.to_string());
    // task_branch = "main" matches base_branch so freshness check sees no divergence.
    task.task_branch = Some("main".to_string());

    let task_id = task.id.clone();
    app_state.task_repo.create(task.clone()).await.unwrap();
    (task_id, task)
}

// ============================================================================
// Test 1: BranchFreshnessConflict with conflict-marker evidence in initial
// on_enter path → Merging
//
// Reproduces the startup-recovery bug: when execute_entry_actions is called
// directly (not through transition_task), a BranchFreshnessConflict returned
// from on_enter(Reviewing) previously had no handler and was silently dropped,
// leaving the task stuck in Reviewing forever. Current semantics route review-
// origin freshness conflicts with merge-conflict evidence into Merging so the
// merger path resolves the conflict instead of looping back into Reviewing.
// ============================================================================

#[tokio::test]
async fn test_branch_freshness_conflict_initial_on_enter_routes_to_merging() {
    // Clean project git repo — freshness check (run_reviewing_freshness_check)
    // runs against this path and must pass without raising BranchFreshnessConflict.
    let project_temp = tempfile::TempDir::new().unwrap();
    init_git_repo(project_temp.path());

    // Task worktree with conflict markers — ensure_review_worktree_ready checks
    // this path and returns Err(BranchFreshnessConflict) when markers are found.
    let worktree_temp = tempfile::TempDir::new().unwrap();
    init_git_repo(worktree_temp.path());

    // Create a tracked file, commit it, then overwrite with conflict markers (unstaged).
    let conflict_file = worktree_temp.path().join("conflict.rs");
    std::fs::write(&conflict_file, "fn clean() {}").unwrap();
    std::process::Command::new("git")
        .args(["add", "conflict.rs"])
        .current_dir(worktree_temp.path())
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["commit", "-m", "add conflict.rs"])
        .current_dir(worktree_temp.path())
        .output()
        .unwrap();
    std::fs::write(
        &conflict_file,
        "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> theirs\n",
    )
    .unwrap();

    // Precondition: verify conflict markers are actually detectable.
    let has_markers =
        ralphx_lib::application::git_service::GitService::has_conflict_markers(worktree_temp.path())
            .await
            .expect("has_conflict_markers should succeed");
    assert!(
        has_markers,
        "Precondition: has_conflict_markers must detect markers in task worktree"
    );

    let execution_state = Arc::new(ExecutionState::new());
    let app_state = AppState::new_test();
    let service = build_service(&app_state, &execution_state);

    let project_dir = project_temp.path().to_string_lossy().to_string();
    let worktree_dir = worktree_temp.path().to_string_lossy().to_string();
    let (task_id, task) =
        seed_reviewing_task(&app_state, &project_dir, Some(&worktree_dir)).await;

    // Act: call execute_entry_actions directly — simulates the StartupJobRunner
    // recovery path (NOT the auto-transition path triggered by transition_task).
    service
        .execute_entry_actions(&task_id, &task, InternalStatus::Reviewing)
        .await;

    // Assert: task must have been routed to Merging because the worktree
    // contained actual conflict markers.
    let updated = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .expect("task_repo.get_by_id must succeed")
        .expect("task must still exist");

    assert_eq!(
        updated.internal_status,
        InternalStatus::Merging,
        "BranchFreshnessConflict with conflict-marker evidence during initial on_enter(Reviewing) must route to Merging; got: {:?}",
        updated.internal_status
    );
}

// ============================================================================
// Test 2: ReviewWorktreeMissing in initial on_enter path → Escalated
//
// When a task's worktree is absent (no worktree_path set), the reviewer cannot
// be spawned. The initial on_enter handler must route the task to Escalated so
// a human can investigate — not leave it stuck in Reviewing.
// ============================================================================

#[tokio::test]
async fn test_review_worktree_missing_initial_on_enter_routes_to_escalated() {
    // Clean project git repo — freshness check must pass.
    let project_temp = tempfile::TempDir::new().unwrap();
    init_git_repo(project_temp.path());

    let execution_state = Arc::new(ExecutionState::new());
    let app_state = AppState::new_test();
    let service = build_service(&app_state, &execution_state);

    let project_dir = project_temp.path().to_string_lossy().to_string();
    // worktree_path = None → ensure_review_worktree_ready returns ReviewWorktreeMissing.
    let (task_id, task) = seed_reviewing_task(&app_state, &project_dir, None).await;

    // Act: call execute_entry_actions directly — simulates the StartupJobRunner
    // recovery path (NOT the auto-transition path triggered by transition_task).
    service
        .execute_entry_actions(&task_id, &task, InternalStatus::Reviewing)
        .await;

    // Assert: task must have been routed to Escalated for human investigation.
    let updated = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .expect("task_repo.get_by_id must succeed")
        .expect("task must still exist");

    assert_eq!(
        updated.internal_status,
        InternalStatus::Escalated,
        "ReviewWorktreeMissing during initial on_enter(Reviewing) must route to Escalated; got: {:?}",
        updated.internal_status
    );
}

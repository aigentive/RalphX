// Integration tests for worktree restoration helpers and their integration points.
//
// Three tests covering the three layers where restore_task_worktree is used:
//
//   Test 1 (re_review_worktree_restore) — direct function call (L3, review_commands.rs path)
//     Calls restore_task_worktree() directly with a task whose worktree_path points to a
//     non-existent merge-{id} directory. The task branch exists in a real git repo so the
//     function should recreate the worktree via checkout_existing_branch_worktree.
//
//   Test 2 (merging_to_pending_review_worktree_path_reset) — reviewing-origin corrective handoff
//     Simulates the reviewing-origin corrective transition from Merging to PendingReview.
//     Current behavior validates that the corrective transition succeeds; the actual worktree
//     restore for the review path is covered by the on_enter(Reviewing) test below.
//
//   Test 3 (on_enter_reviewing_restores_merge_prefixed_worktree) — L2 via on_enter(Reviewing)
//     on_enter(Reviewing) in on_enter_states.rs detects that worktree_path has a merge-prefix,
//     calls restore_task_worktree, and updates the task in the repo with the correct path.

use super::helpers::*;
use crate::application::chat_service::freshness_routing::{
    freshness_return_route, FreshnessRouteResult,
};
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::entities::{GitMode, InternalStatus, Project, ProjectId, Task};
use crate::domain::services::{MemoryRunningAgentRegistry, MessageQueue};
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::transition_handler::merge_helpers::{
    compute_task_worktree_path, is_merge_worktree_path, restore_task_worktree,
};
use crate::domain::state_machine::{State, TransitionHandler};
use crate::utils::path_safety::validate_absolute_non_root_path;

// ──────────────────────────────────────────────────────────────────────────────
// Shared helper: build TaskServices with retained MockChatService
// ──────────────────────────────────────────────────────────────────────────────

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

fn assert_task_runtime_bootstrap_metadata(
    metadata: Option<&str>,
    task_id: &str,
    task_state: &str,
    context_type: &str,
) {
    let metadata = metadata.expect("bootstrap send must include metadata");
    let value: serde_json::Value =
        serde_json::from_str(metadata).expect("bootstrap metadata must be JSON");
    assert_eq!(value["hidden_from_ui"], true);
    assert_eq!(value["source"], "task_runtime_bootstrap");
    assert_eq!(value["task_id"], task_id);
    assert_eq!(value["task_state"], task_state);
    assert_eq!(value["context_type"], context_type);
}

fn git_rev_parse(repo_path: &std::path::Path, ref_name: &str) -> String {
    let repo_path = validate_absolute_non_root_path(repo_path, "worktree restore test repository")
        .expect("safe test repo");
    let output = std::process::Command::new("git")
        .args(["rev-parse", ref_name])
        .current_dir(&repo_path)
        .output()
        .expect("git rev-parse failed");
    assert!(
        output.status.success(),
        "git rev-parse {ref_name} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 1: restore_task_worktree() direct call (L3 path)
//
// Precondition: task.worktree_path = "/tmp/nonexistent/merge-{id}" (does not exist)
//               task.task_branch exists in the real git repo
// Expected:     task.worktree_path updated to task-{id} path after the call
//               returned PathBuf exists on disk (worktree was created)
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn re_review_worktree_restore() {
    let git_repo = setup_real_git_repo();
    let repo_path = git_repo.path();

    // Build a minimal Project pointing at the real git repo.
    // We use the repo path as working_directory; no worktree_parent_directory so
    // compute_task_worktree_path will use the default ~/ralphx-worktrees convention.
    // However, to keep the worktree under the temp dir (for reliable cleanup), we
    // set worktree_parent_directory explicitly to repo_path.
    let project_id = ProjectId::from_string("proj-wt-restore-1".to_string());
    let mut project = Project::new(
        "wt-restore-project".to_string(),
        repo_path.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    // Place worktrees under the temp dir so they are auto-cleaned on drop.
    project.worktree_parent_directory = Some(repo_path.to_string_lossy().to_string());

    // Build a task with a stale merge-prefixed worktree_path.
    let mut task = Task::new(project_id.clone(), "WTR restore test".to_string());
    let task_id_str = task.id.as_str().to_string();
    task.task_branch = Some(git_repo.task_branch.clone());
    task.worktree_path = Some(format!("/tmp/nonexistent/merge-{}", task_id_str));

    // Precondition: the stale path must not exist.
    assert!(
        !std::path::Path::new(task.worktree_path.as_deref().unwrap()).exists(),
        "Precondition: merge-prefixed worktree path must not exist before restoration"
    );

    // Compute what the expected restored path should be.
    let expected_path_str = compute_task_worktree_path(&project, &task_id_str);

    // Call restore_task_worktree — it should detect the task branch and create the worktree.
    let result = restore_task_worktree(&mut task, &project, repo_path).await;

    assert!(
        result.is_ok(),
        "restore_task_worktree should succeed when task branch exists. Got: {:?}",
        result.err()
    );

    let returned_path = result.unwrap();

    // The returned PathBuf must exist on disk.
    assert!(
        returned_path.exists(),
        "Restored worktree path must exist on disk after restoration. Path: {}",
        returned_path.display()
    );

    // task.worktree_path must have been updated in memory.
    assert_eq!(
        task.worktree_path.as_deref(),
        Some(expected_path_str.as_str()),
        "task.worktree_path must be updated to the task-{{id}} path after restoration"
    );

    // Must no longer be a merge-prefixed path.
    assert!(
        !is_merge_worktree_path(task.worktree_path.as_deref().unwrap()),
        "Restored worktree_path must not have a merge-pipeline prefix. Got: {:?}",
        task.worktree_path
    );

    // Cleanup: remove the git worktree that was created so the TempDir can be removed cleanly.
    let _ = std::process::Command::new("git")
        .args([
            "worktree",
            "remove",
            "--force",
            returned_path.to_str().unwrap(),
        ])
        .current_dir(repo_path)
        .output();
}

// ──────────────────────────────────────────────────────────────────────────────
// Shared helper: build a TaskTransitionService from AppState (mirrors freshness_return_path tests)
// ──────────────────────────────────────────────────────────────────────────────

#[rustfmt::skip]
fn build_transition_service(
    app_state: &AppState,
) -> crate::application::TaskTransitionService {
    let execution_state = Arc::new(ExecutionState::new());
    let message_queue = Arc::new(MessageQueue::new());
    let running_registry = Arc::new(MemoryRunningAgentRegistry::new());

    crate::application::TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        message_queue,
        running_registry,
        execution_state,
        None,
        Arc::clone(&app_state.memory_event_repo),
    )
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 2: reviewing-origin corrective transition succeeds
//
// This test exercises the L1 reviewing-origin restore path by:
//   1. Seeding a task in Merging with freshness metadata (origin = "reviewing") and a
//      stale merge-prefixed worktree_path.
//   2. Calling the corrective PendingReview transition that the reviewing-origin freshness
//      path ultimately uses while the reviewing-origin metadata is still present.
//   3. After the transition, asserting that the task is handed off to the review path.
//
// Uses a real git repo so restore_task_worktree can actually recreate the worktree.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn merging_to_pending_review_worktree_path_reset() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    let app_state = AppState::new_test();

    // Wire a real project pointing at the git repo.
    // Place worktrees under the temp dir so they are cleaned up on drop.
    let mut project = Project::new(
        "wt-restore-l1-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(path.to_string_lossy().to_string());
    let project_id = project.id.clone();
    app_state.project_repo.create(project).await.unwrap();

    // Build a task in Merging state with:
    //   - freshness metadata: reviewing origin + source_update_conflict = true
    //   - stale merge-prefixed worktree_path
    let meta = serde_json::json!({
        "branch_freshness_conflict": true,
        "freshness_origin_state": "reviewing",
        "freshness_conflict_count": 1,
        "source_update_conflict": true,
        "source_branch": &git_repo.task_branch,
        "target_branch": "main",
    });
    let mut task = Task::new(project_id.clone(), "L1 worktree restore test".to_string());
    let task_id = task.id.clone();
    let task_id_str = task_id.as_str().to_string();
    task.internal_status = InternalStatus::Merging;
    task.task_branch = Some(git_repo.task_branch.clone());
    task.worktree_path = Some(format!("/nonexistent/merge-{}", task_id_str));
    task.metadata = Some(meta.to_string());
    app_state.task_repo.create(task).await.unwrap();

    // Step 1: Call the corrective transition path used by the reviewing-origin freshness
    // return flow while the reviewing-origin metadata is still present.
    let service = build_transition_service(&app_state);
    let result = service
        .transition_task_corrective_with_exit(
            &task_id,
            InternalStatus::PendingReview,
            None,
            "system",
        )
        .await;

    assert!(
        result.is_ok(),
        "corrective transition to PendingReview must succeed: {:?}",
        result.err()
    );

    // Step 2: Verify the task was handed off to the review path. The actual worktree restore
    // happens on the later review entry path and is covered by the dedicated L2 test below.
    let updated = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("Task must still exist");

    assert!(
        matches!(
            updated.internal_status,
            InternalStatus::PendingReview | InternalStatus::Reviewing
        ),
        "Corrective reviewing-origin transition must hand the task off to review. Got: {:?}",
        updated.internal_status
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 3: Freshness return to execution restores stale merge-prefixed worktree_path
//
// Covers the execution-origin path:
//   Merging + plan_update_conflict + freshness_origin_state="executing"
//   → freshness_return_route() → Ready
//
// Regression: without this repair, the task could return to Ready with
// worktree_path still pointing at merge-{id}, and the next Executing spawn would
// fail with "context points to merge worktree".
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn freshness_return_to_ready_restores_merge_prefixed_worktree_path() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    let app_state = AppState::new_test();

    let mut project = Project::new(
        "wt-restore-execution-origin-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(path.to_string_lossy().to_string());
    let project_id = project.id.clone();
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(
        project_id.clone(),
        "Execution-origin worktree restore test".to_string(),
    );
    let task_id = task.id.clone();
    let task_id_str = task_id.as_str().to_string();
    task.internal_status = InternalStatus::Merging;
    task.task_branch = Some(git_repo.task_branch.clone());
    task.worktree_path = Some(format!("/nonexistent/merge-{}", task_id_str));
    task.metadata = Some(
        serde_json::json!({
            "plan_update_conflict": true,
            "branch_freshness_conflict": true,
            "freshness_origin_state": "executing",
            "freshness_conflict_count": 1,
        })
        .to_string(),
    );
    app_state.task_repo.create(task).await.unwrap();

    let service = build_transition_service(&app_state);
    let current_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task must exist");

    let result = freshness_return_route(
        &current_task,
        Arc::clone(&app_state.task_repo),
        &service,
        &project,
        None,
        None,
        None,
    )
    .await
    .expect("freshness return to execution must succeed");

    match result {
        FreshnessRouteResult::FreshnessRouted(state) => assert_eq!(state, "executing"),
        FreshnessRouteResult::NormalMerge => panic!("Expected FreshnessRouted"),
    }

    let updated = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task must exist after routing");
    assert_eq!(updated.internal_status, InternalStatus::Ready);

    let restored_wt = updated
        .worktree_path
        .as_deref()
        .expect("execution-origin routing should restore task worktree_path");
    assert!(
        !is_merge_worktree_path(restored_wt),
        "Execution-origin routing must not leave a merge-prefixed worktree_path. Got: {}",
        restored_wt
    );
    assert!(
        restored_wt.contains(&format!("task-{}", task_id_str)),
        "Restored execution worktree_path should reference task-{{id}}. Got: {}",
        restored_wt
    );
    assert!(
        std::path::Path::new(restored_wt).exists(),
        "Restored execution worktree should exist on disk. Path: {}",
        restored_wt
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 3: L2 — on_enter_reviewing_restores_merge_prefixed_worktree
//
// on_enter(Reviewing) in on_enter_states.rs detects that worktree_path has a
// merge-prefix, calls restore_task_worktree, and persists the corrected path to
// the task repo before the reviewer spawn attempt.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn on_enter_reviewing_restores_merge_prefixed_worktree() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());

    // Build a task in PendingReview with a stale merge-prefixed worktree_path.
    let mut task = Task::new(project_id.clone(), "L2 reviewing restore test".to_string());
    let task_id = task.id.clone();
    let task_id_str = task_id.as_str().to_string();
    task.internal_status = InternalStatus::PendingReview;
    task.task_branch = Some(git_repo.task_branch.clone());
    task.worktree_path = Some(format!("/nonexistent/merge-{}", task_id_str));
    task_repo.create(task).await.unwrap();

    let mut project = Project::new(
        "test-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    // Place worktrees under the temp dir for cleanup.
    project.worktree_parent_directory = Some(path.to_string_lossy().to_string());
    project_repo.create(project).await.unwrap();

    let (chat_service, services) =
        make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = crate::domain::state_machine::context::TaskContext::new(
        task_id_str.as_str(),
        "proj-1",
        services,
    );
    let mut machine = crate::domain::state_machine::TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // Call on_enter(Reviewing) — the guard detects the merge-prefix and calls
    // restore_task_worktree before the reviewer spawn attempt.
    let _ = handler.on_enter(&State::Reviewing).await;

    let sent_options = chat_service.get_sent_options().await;
    assert_eq!(sent_options.len(), 1, "expected exactly one bootstrap send");
    assert_task_runtime_bootstrap_metadata(
        sent_options
            .first()
            .and_then(|options| options.metadata.as_deref()),
        &task_id_str,
        "reviewing",
        "review",
    );

    // Allow the async path to write the restored path back to the repo.
    let task_repo_poll = Arc::clone(&task_repo);
    let task_id_poll = task_id.clone();
    let settled = wait_for_condition(
        || {
            let r = Arc::clone(&task_repo_poll);
            let id = task_id_poll.clone();
            async move {
                r.get_by_id(&id)
                    .await
                    .ok()
                    .flatten()
                    .map(|t| {
                        // Condition: worktree_path no longer has a merge-pipeline prefix.
                        t.worktree_path
                            .as_deref()
                            .map(|wt| !is_merge_worktree_path(wt))
                            .unwrap_or(true) // None is also acceptable (missing worktree cleared it)
                    })
                    .unwrap_or(false)
            }
        },
        5000,
    )
    .await;

    assert!(
        settled,
        "Task worktree_path should no longer have a merge-pipeline prefix within 5s"
    );

    let updated = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("Task must still exist");

    // Validate the path is no longer merge-prefixed.
    if let Some(ref wt) = updated.worktree_path {
        assert!(
            !is_merge_worktree_path(wt),
            "After on_enter(Reviewing), worktree_path must not have a merge-pipeline prefix. Got: {}",
            wt
        );
    }
    // If worktree_path is None, ReviewWorktreeMissing was returned and the caller cleared it — acceptable.
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 4: on_enter(ReExecuting) restores/recreates the execution worktree before spawn
//
// Regression: merge-hook reroutes previously skipped ensure_executing_branch_and_worktree(),
// so ReExecuting could try to spawn with a stale merge-prefixed/missing worktree and bounce
// through Failed -> Ready -> Executing recovery.
// ──────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn on_enter_reexecuting_restores_execution_worktree_before_spawn() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();
    let base_sha = git_rev_parse(path, "main");

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());

    let mut task = Task::new(
        project_id.clone(),
        "L2 re-executing restore test".to_string(),
    );
    let task_id = task.id.clone();
    let task_id_str = task_id.as_str().to_string();
    task.internal_status = InternalStatus::ReExecuting;
    task.task_branch = Some(git_repo.task_branch.clone());
    task.task_branch_base_ref = Some("main".to_string());
    task.task_branch_base_sha = Some(base_sha);
    task.worktree_path = Some(format!("/nonexistent/merge-{}", task_id_str));
    task_repo.create(task).await.unwrap();

    let mut project = Project::new(
        "test-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(path.to_string_lossy().to_string());
    project_repo.create(project).await.unwrap();

    let (chat_service, services) =
        make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let services = with_default_test_branch_update_authority(
        services,
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
    );
    let context = crate::domain::state_machine::context::TaskContext::new(
        task_id_str.as_str(),
        "proj-1",
        services,
    );
    let mut machine = crate::domain::state_machine::TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::ReExecuting).await;
    assert!(
        result.is_ok(),
        "on_enter(ReExecuting) should restore the execution worktree before spawn: {:?}",
        result.err()
    );

    let updated = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("Task must still exist");

    let restored_wt = updated
        .worktree_path
        .as_deref()
        .expect("ReExecuting must restore a task execution worktree before spawn");
    assert!(
        !is_merge_worktree_path(restored_wt),
        "ReExecuting must not leave a merge-prefixed worktree_path. Got: {}",
        restored_wt
    );
    assert!(
        std::path::Path::new(restored_wt).exists(),
        "ReExecuting should recreate the execution worktree before spawn. Path: {}",
        restored_wt
    );

    let sent_messages = chat_service.get_sent_messages().await;
    assert_eq!(
        sent_messages,
        vec![format!("Re-execute task (revision): {}", task_id_str)],
        "ReExecuting should spawn once after worktree restoration"
    );
    let sent_options = chat_service.get_sent_options().await;
    assert_eq!(sent_options.len(), 1, "expected exactly one bootstrap send");
    assert_task_runtime_bootstrap_metadata(
        sent_options
            .first()
            .and_then(|options| options.metadata.as_deref()),
        &task_id_str,
        "re_executing",
        "task_execution",
    );
}

#[tokio::test]
async fn on_enter_executing_repairs_wrong_existing_worktree_path_before_spawn() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();
    let base_sha = git_rev_parse(path, "main");

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-wrong-worktree".to_string());
    let mut project = Project::new(
        "wrong-worktree-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.git_mode = GitMode::Worktree;
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(path.to_string_lossy().to_string());
    project_repo.create(project.clone()).await.unwrap();

    let wrong_path = path.join("wrong-existing-worktree");
    std::fs::create_dir_all(&wrong_path).expect("create wrong existing worktree path");
    let wrong_path_str = wrong_path.to_string_lossy().to_string();

    let mut task = Task::new(project_id.clone(), "Wrong worktree path".to_string());
    let task_id = task.id.clone();
    let task_id_str = task_id.as_str().to_string();
    task.internal_status = InternalStatus::Executing;
    task.task_branch = Some(git_repo.task_branch.clone());
    task.task_branch_base_ref = Some("main".to_string());
    task.task_branch_base_sha = Some(base_sha);
    task.worktree_path = Some(wrong_path_str.clone());
    task_repo.create(task).await.unwrap();

    let (chat_service, services) =
        make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let services = with_default_test_branch_update_authority(
        services,
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
    );
    let context = crate::domain::state_machine::context::TaskContext::new(
        task_id_str.as_str(),
        project_id.as_str(),
        services,
    );
    let mut machine = crate::domain::state_machine::TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::Executing).await;
    assert!(
        result.is_ok(),
        "on_enter(Executing) should repair a wrong existing worktree_path before spawn: {:?}",
        result.err()
    );

    let updated = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("Task must still exist");
    let expected_path = compute_task_worktree_path(&project, &task_id_str);
    assert_eq!(
        updated.worktree_path.as_deref(),
        Some(expected_path.as_str()),
        "Executing must persist the authoritative task worktree path"
    );
    assert!(
        std::path::Path::new(&expected_path).exists(),
        "Executing should recreate the authoritative worktree before spawn"
    );
    assert_ne!(
        updated.worktree_path.as_deref(),
        Some(wrong_path_str.as_str()),
        "Executing must not spawn from the wrong existing path"
    );

    let sent_messages = chat_service.get_sent_messages().await;
    assert_eq!(
        sent_messages,
        vec![format!("Execute task: {}", task_id_str)],
        "Executing should spawn once after worktree path repair"
    );
    let sent_options = chat_service.get_sent_options().await;
    assert_eq!(sent_options.len(), 1, "expected exactly one bootstrap send");
    assert_task_runtime_bootstrap_metadata(
        sent_options
            .first()
            .and_then(|options| options.metadata.as_deref()),
        &task_id_str,
        "executing",
        "task_execution",
    );
}

#[tokio::test]
async fn pre_execution_setup_blocks_worktree_task_without_worktree_path() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-missing-worktree".to_string());
    let mut project = Project::new(
        "missing-worktree-project".to_string(),
        "/tmp/ralphx-test-main".to_string(),
    );
    project.id = project_id.clone();
    project.git_mode = GitMode::Worktree;
    project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id.clone(), "Missing worktree path".to_string());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    let task_id_str = task_id.as_str().to_string();
    task_repo.create(task).await.unwrap();

    let (_chat_service, services) =
        make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = crate::domain::state_machine::context::TaskContext::new(
        task_id_str.as_str(),
        project_id.as_str(),
        services,
    );
    let mut machine = crate::domain::state_machine::TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler
        .run_and_store_pre_execution_setup(
            task_id_str.as_str(),
            project_id.as_str(),
            "execution",
            "execution_setup_log",
        )
        .await;

    assert!(
        matches!(
            result,
            Err(crate::error::AppError::ExecutionBlocked(ref message))
                if message.contains("no worktree_path")
        ),
        "Worktree execution setup must block without worktree_path; got {result:?}"
    );
}

#[tokio::test]
async fn pre_execution_setup_blocks_worktree_task_with_missing_worktree_path() {
    let temp = tempfile::tempdir().expect("temp dir");
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-missing-worktree-on-disk".to_string());
    let mut project = Project::new(
        "missing-worktree-on-disk-project".to_string(),
        temp.path().join("main").to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.git_mode = GitMode::Worktree;
    project_repo.create(project).await.unwrap();

    let mut task = Task::new(
        project_id.clone(),
        "Missing worktree path on disk".to_string(),
    );
    task.internal_status = InternalStatus::Executing;
    task.worktree_path = Some(
        temp.path()
            .join("missing-execution-worktree")
            .to_string_lossy()
            .to_string(),
    );
    let task_id = task.id.clone();
    let task_id_str = task_id.as_str().to_string();
    task_repo.create(task).await.unwrap();

    let (_chat_service, services) =
        make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = crate::domain::state_machine::context::TaskContext::new(
        task_id_str.as_str(),
        project_id.as_str(),
        services,
    );
    let mut machine = crate::domain::state_machine::TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler
        .run_and_store_pre_execution_setup(
            task_id_str.as_str(),
            project_id.as_str(),
            "execution",
            "execution_setup_log",
        )
        .await;

    assert!(
        matches!(
            result,
            Err(crate::error::AppError::ExecutionBlocked(ref message))
                if message.contains("does not exist before pre-execution setup")
        ),
        "Worktree execution setup must block when persisted worktree_path is missing; got {result:?}"
    );
}

#[tokio::test]
async fn on_enter_executing_adopts_existing_authoritative_worktree_path() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();
    let base_sha = git_rev_parse(path, "main");

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-existing-authoritative-worktree".to_string());
    let mut project = Project::new(
        "existing-authoritative-worktree-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.git_mode = GitMode::Worktree;
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory = Some(path.to_string_lossy().to_string());
    project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(
        project_id.clone(),
        "Adopt expected worktree path".to_string(),
    );
    let task_id = task.id.clone();
    let task_id_str = task_id.as_str().to_string();
    let stale_missing_path = path.join("stale-missing-worktree");
    let expected_path = compute_task_worktree_path(&project, &task_id_str);
    std::fs::create_dir_all(&expected_path).expect("create expected worktree path");
    task.internal_status = InternalStatus::Executing;
    task.task_branch = Some(git_repo.task_branch.clone());
    task.task_branch_base_ref = Some("main".to_string());
    task.task_branch_base_sha = Some(base_sha);
    task.worktree_path = Some(stale_missing_path.to_string_lossy().to_string());
    task_repo.create(task).await.unwrap();

    let (chat_service, services) =
        make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = crate::domain::state_machine::context::TaskContext::new(
        task_id_str.as_str(),
        project_id.as_str(),
        services,
    );
    let mut machine = crate::domain::state_machine::TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::Executing).await;
    assert!(
        result.is_ok(),
        "on_enter(Executing) should adopt the existing authoritative worktree path: {:?}",
        result.err()
    );

    let updated = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("Task must still exist");
    assert_eq!(
        updated.worktree_path.as_deref(),
        Some(expected_path.as_str()),
        "Executing must replace the stale missing path with the authoritative existing path"
    );
    let sent_messages = chat_service.get_sent_messages().await;
    assert_eq!(
        sent_messages,
        vec![format!("Execute task: {}", task_id_str)]
    );
    let sent_options = chat_service.get_sent_options().await;
    assert_eq!(sent_options.len(), 1, "expected exactly one bootstrap send");
    assert_task_runtime_bootstrap_metadata(
        sent_options
            .first()
            .and_then(|options| options.metadata.as_deref()),
        &task_id_str,
        "executing",
        "task_execution",
    );
}

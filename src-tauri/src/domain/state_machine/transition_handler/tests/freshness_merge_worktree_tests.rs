// Compatibility tests for tasks persisted by the pre-dedicated branch-update workflow.
//
// New freshness conflicts use UpdatingPlanBranch/UpdatingTaskBranch and never enter
// Merging. These cases preserve safe recovery for legacy Merging rows whose metadata
// still identifies the old freshness-conflict route.
//
// Test matrix:
//   Test 1: source_update_conflict flag → task branch checked out in merge-{id}
//   Test 2: plan_update_conflict flag → target (plan) branch checked out in merge-{id}
//   Test 3: Normal merge pipeline path (merge worktree pre-exists) → no-op
//   Test 4: No conflict flags → worktree not created (skip creation)
//   Test 5: clear_routing_flags() unit test — all routing flags cleared, counts preserved
//
// Pattern: real git repo + MemoryTaskRepository + MemoryProjectRepository + MockChatService.
// Per CLAUDE.md rule 1.5.

use super::helpers::*;
use crate::application::git_service::GitService;
use crate::domain::entities::{GitMode, InternalStatus, MergeStrategy, Project, ProjectId, Task};
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::transition_handler::freshness::FreshnessMetadata;
use crate::domain::state_machine::transition_handler::merge_helpers::{
    compute_merge_worktree_path, compute_task_worktree_path,
};
use crate::domain::state_machine::{State, TransitionHandler};
use std::process::Command;

// ──────────────────────────────────────────────────────────────────────────────
// Shared helper: build TaskServices with chat service retained
// ──────────────────────────────────────────────────────────────────────────────

fn make_services_with_chat(
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

/// Create a task pre-loaded with metadata flags and a Merging status.
async fn setup_merging_task_with_meta(
    task_repo: &Arc<MemoryTaskRepository>,
    project_id: &ProjectId,
    task_branch: &str,
    metadata: serde_json::Value,
) -> crate::domain::entities::TaskId {
    let mut task = Task::new(
        project_id.clone(),
        "Freshness merge worktree test".to_string(),
    );
    task.internal_status = InternalStatus::Merging;
    task.task_branch = Some(task_branch.to_string());
    task.metadata = Some(metadata.to_string());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();
    task_id
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 1: source_update_conflict → task branch checked out in merge worktree
// ──────────────────────────────────────────────────────────────────────────────

/// When on_enter(Merging) is reached via the BranchFreshnessConflict path with
/// source_update_conflict=true (and no merge worktree yet), the fix must:
///   1. Delete any existing task-{id} worktree (git rejects two worktrees on same branch)
///   2. Create merge-{id} worktree with the task branch checked out
///   3. Persist task.worktree_path = merge-{id} path
///   4. Attempt to spawn the merger agent (chat_service.send_message called)
#[tokio::test]
async fn test_freshness_conflict_source_update_creates_merge_worktree() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    // Worktree parent inside the temp dir to avoid polluting ~/ralphx-worktrees
    let worktree_parent = path.join("worktrees");
    std::fs::create_dir_all(&worktree_parent).unwrap();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut project = Project::new(
        "test-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project.git_mode = GitMode::Worktree;
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    project_repo.create(project.clone()).await.unwrap();

    // Metadata: source_update_conflict=true, source_branch=task_branch, target_branch=main
    let meta = serde_json::json!({
        "branch_freshness_conflict": true,
        "source_update_conflict": true,
        "plan_update_conflict": false,
        "source_branch": git_repo.task_branch,
        "target_branch": "main",
        "freshness_origin_state": "executing",
    });

    let task_id =
        setup_merging_task_with_meta(&task_repo, &project_id, &git_repo.task_branch, meta).await;
    let task_id_str = task_id.as_str().to_string();

    // Create task worktree to simulate what on_enter(Executing) would have done.
    // on_enter(Merging) must pre-delete it before creating the merge worktree,
    // since git rejects two worktrees checked out on the same branch.
    let task_wt_path_str = compute_task_worktree_path(&project, &task_id_str);
    let task_wt_path = std::path::PathBuf::from(&task_wt_path_str);
    std::fs::create_dir_all(task_wt_path.parent().unwrap()).unwrap();
    GitService::checkout_existing_branch_worktree(path, &task_wt_path, &git_repo.task_branch)
        .await
        .expect("create task worktree for test setup");
    assert!(
        task_wt_path.exists(),
        "Precondition: task worktree must exist before on_enter"
    );

    let merge_wt_path_str = compute_merge_worktree_path(&project, &task_id_str);
    let merge_wt_path = std::path::PathBuf::from(&merge_wt_path_str);
    assert!(
        !merge_wt_path.exists(),
        "Precondition: merge worktree must NOT exist yet"
    );

    let (chat_service, services) =
        make_services_with_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = TaskContext::new(&task_id_str, "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::Merging).await;

    // Verify: task worktree was deleted (git rejects two worktrees on same branch)
    assert!(
        !task_wt_path.exists(),
        "Task worktree must be deleted before creating merge worktree (same branch guard). Path: {}",
        task_wt_path.display()
    );

    // Verify: merge worktree now exists
    assert!(
        merge_wt_path.exists(),
        "source_update_conflict: merge-{{id}} worktree must be created by on_enter(Merging). Path: {}",
        merge_wt_path.display()
    );

    // Verify: merge worktree is on the task (source) branch
    let head = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&merge_wt_path)
        .output()
        .expect("git rev-parse HEAD in merge worktree");
    let branch = String::from_utf8_lossy(&head.stdout).trim().to_string();
    assert_eq!(
        branch, git_repo.task_branch,
        "source_update_conflict: merge worktree must check out the task (source) branch"
    );

    // Verify: task.worktree_path persisted to DB
    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let wt_path_in_db = updated.worktree_path.as_deref().unwrap_or("");
    assert!(
        wt_path_in_db.contains("merge-"),
        "task.worktree_path must point to the merge-{{id}} worktree. Got: {:?}",
        updated.worktree_path
    );

    // Verify: merger agent spawn was attempted (chat_service.send_message called)
    assert!(
        chat_service.call_count() >= 1,
        "Merger agent must be spawned after merge worktree creation (call_count={})",
        chat_service.call_count()
    );

    // Cleanup
    let _ = GitService::delete_worktree(path, &merge_wt_path).await;
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 2: plan_update_conflict → plan (target) branch checked out in merge worktree
// ──────────────────────────────────────────────────────────────────────────────

/// When plan_update_conflict=true, the merge worktree must check out the target
/// (plan) branch, NOT the task branch. The merger agent merges base_branch into
/// the plan branch in this worktree.
#[tokio::test]
async fn test_freshness_conflict_plan_update_creates_merge_worktree() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    // Create a plan branch for the test
    let plan_branch = "plan/feature-1";
    let _ = Command::new("git")
        .args(["branch", plan_branch])
        .current_dir(path)
        .output()
        .expect("git branch plan/feature-1");

    let worktree_parent = path.join("worktrees");
    std::fs::create_dir_all(&worktree_parent).unwrap();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut project = Project::new(
        "test-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project.git_mode = GitMode::Worktree;
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    project_repo.create(project.clone()).await.unwrap();

    // Metadata: plan_update_conflict=true, target_branch=plan branch
    let meta = serde_json::json!({
        "branch_freshness_conflict": true,
        "plan_update_conflict": true,
        "source_update_conflict": false,
        "source_branch": git_repo.task_branch,
        "target_branch": plan_branch,
        "freshness_origin_state": "executing",
    });

    let task_id =
        setup_merging_task_with_meta(&task_repo, &project_id, &git_repo.task_branch, meta).await;
    let task_id_str = task_id.as_str().to_string();

    let merge_wt_path_str = compute_merge_worktree_path(&project, &task_id_str);
    let merge_wt_path = std::path::PathBuf::from(&merge_wt_path_str);
    assert!(
        !merge_wt_path.exists(),
        "Precondition: merge worktree must NOT exist yet"
    );

    let (chat_service, services) =
        make_services_with_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = TaskContext::new(&task_id_str, "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::Merging).await;

    // Verify: merge worktree now exists
    assert!(
        merge_wt_path.exists(),
        "plan_update_conflict: merge-{{id}} worktree must be created by on_enter(Merging). Path: {}",
        merge_wt_path.display()
    );

    // Verify: merge worktree is on the plan (target) branch, NOT the task branch
    let head = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(&merge_wt_path)
        .output()
        .expect("git rev-parse HEAD in merge worktree");
    let branch = String::from_utf8_lossy(&head.stdout).trim().to_string();
    assert_eq!(
        branch, plan_branch,
        "plan_update_conflict: merge worktree must check out the plan (target) branch, not the task branch. Got: {}",
        branch
    );

    // Verify: merger agent spawn was attempted
    assert!(
        chat_service.call_count() >= 1,
        "Merger agent must be spawned (call_count={})",
        chat_service.call_count()
    );

    // Cleanup
    let _ = GitService::delete_worktree(path, &merge_wt_path).await;
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 3: Normal merge pipeline path — merge worktree pre-exists → no-op
// ──────────────────────────────────────────────────────────────────────────────

/// Regression: When on_enter(Merging) is reached via the normal merge pipeline
/// (side_effects.rs creates the merge worktree before transitioning), the
/// `!wt_path.exists()` guard must skip worktree creation entirely. The existing
/// worktree must remain intact (not deleted and recreated).
#[tokio::test]
async fn test_merge_pipeline_path_no_op_when_worktree_exists() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    let worktree_parent = path.join("worktrees");
    std::fs::create_dir_all(&worktree_parent).unwrap();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut project = Project::new(
        "test-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project.git_mode = GitMode::Worktree;
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    project_repo.create(project.clone()).await.unwrap();

    // No conflict flags: this simulates the normal merge pipeline path
    let meta = serde_json::json!({
        "branch_freshness_conflict": false,
        "plan_update_conflict": false,
        "source_update_conflict": false,
    });

    let task_id =
        setup_merging_task_with_meta(&task_repo, &project_id, &git_repo.task_branch, meta).await;
    let task_id_str = task_id.as_str().to_string();

    // Pre-create the merge worktree (simulating what the normal pipeline does)
    let merge_wt_path_str = compute_merge_worktree_path(&project, &task_id_str);
    let merge_wt_path = std::path::PathBuf::from(&merge_wt_path_str);
    std::fs::create_dir_all(merge_wt_path.parent().unwrap()).unwrap();
    GitService::checkout_existing_branch_worktree(path, &merge_wt_path, &git_repo.task_branch)
        .await
        .expect("pre-create merge worktree (simulates normal pipeline)");
    assert!(
        merge_wt_path.exists(),
        "Precondition: merge worktree must exist before on_enter"
    );

    // Record the pre-existing worktree's HEAD to verify it wasn't recreated
    let pre_head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&merge_wt_path)
        .output()
        .expect("git rev-parse HEAD");
    let pre_head_sha = String::from_utf8_lossy(&pre_head.stdout).trim().to_string();

    // Set worktree_path on the task to the merge-{id} path (as pipeline would have done)
    {
        let mut task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
        task.worktree_path = Some(merge_wt_path_str.clone());
        task_repo.update(&task).await.unwrap();
    }

    let (_chat_service, services) =
        make_services_with_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = TaskContext::new(&task_id_str, "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::Merging).await;

    // Verify: merge worktree still exists (not deleted)
    assert!(
        merge_wt_path.exists(),
        "Normal pipeline path: merge worktree must NOT be deleted by on_enter guard (no-op). Path: {}",
        merge_wt_path.display()
    );

    // Verify: merge worktree HEAD is unchanged (not recreated)
    let post_head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&merge_wt_path)
        .output()
        .expect("git rev-parse HEAD post on_enter");
    let post_head_sha = String::from_utf8_lossy(&post_head.stdout)
        .trim()
        .to_string();
    assert_eq!(
        pre_head_sha, post_head_sha,
        "Normal pipeline path: merge worktree HEAD must be unchanged (no-op). pre={} post={}",
        pre_head_sha, post_head_sha
    );

    // Cleanup
    let _ = GitService::delete_worktree(path, &merge_wt_path).await;
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 4: No conflict flags + no merge worktree → worktree not created
// ──────────────────────────────────────────────────────────────────────────────

/// When no conflict flags are set and no merge worktree exists, the worktree
/// creation block runs but no branch can be determined → no worktree created.
/// The agent spawn still runs (chat_service.send_message called).
///
/// This simulates on_enter(Merging) reached via a path that did NOT set
/// freshness flags (e.g., some future entry path or a metadata-free recovery).
#[tokio::test]
async fn test_freshness_no_flags_skips_worktree_creation() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    let worktree_parent = path.join("worktrees");
    std::fs::create_dir_all(&worktree_parent).unwrap();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut project = Project::new(
        "test-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project.git_mode = GitMode::Worktree;
    project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
    project_repo.create(project.clone()).await.unwrap();

    // No conflict flags — task_branch also absent to prevent fallback checkout
    let meta = serde_json::json!({
        "branch_freshness_conflict": false,
        "plan_update_conflict": false,
        "source_update_conflict": false,
    });

    // Task with NO task_branch set — forces the "no branch to checkout" code path
    let mut task = Task::new(project_id.clone(), "No-flags test".to_string());
    task.internal_status = InternalStatus::Merging;
    task.task_branch = None; // No branch → checkout_branch will be None
    task.metadata = Some(meta.to_string());
    let task_id = task.id.clone();
    let task_id_str = task_id.as_str().to_string();
    task_repo.create(task).await.unwrap();

    let merge_wt_path_str = compute_merge_worktree_path(&project, &task_id_str);
    let merge_wt_path = std::path::PathBuf::from(&merge_wt_path_str);
    assert!(
        !merge_wt_path.exists(),
        "Precondition: merge worktree must NOT exist"
    );

    let (_chat_service, services) =
        make_services_with_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = TaskContext::new(&task_id_str, "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::Merging).await;

    // Verify: no merge worktree was created (no flags + no task_branch → no checkout)
    assert!(
        !merge_wt_path.exists(),
        "No conflict flags + no task_branch: merge worktree must NOT be created. Path: {}",
        merge_wt_path.display()
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 5: clear_routing_flags() unit test
// ──────────────────────────────────────────────────────────────────────────────

/// Verify that clear_routing_flags() clears all routing-related flags but
/// preserves conflict count, backoff_until, and auto_reset_count.
///
/// This is a pure unit test on FreshnessMetadata — no git repo required.
#[test]
fn test_clear_routing_flags_preserves_counts() {
    let mut meta = FreshnessMetadata {
        branch_freshness_conflict: true,
        freshness_origin_state: Some("executing".to_string()),
        plan_update_conflict: true,
        source_update_conflict: true,
        conflict_files: vec!["src/lib.rs".to_string(), "Cargo.toml".to_string()],
        source_branch: Some("task/feature-branch".to_string()),
        target_branch: Some("plan/feature-1".to_string()),
        freshness_conflict_count: 3,
        freshness_backoff_until: Some(chrono::Utc::now()),
        freshness_auto_reset_count: 1,
        last_freshness_check_at: Some("2026-03-10T12:00:00Z".to_string()),
        last_plan_freshness_check_at: Some("2026-03-10T12:00:00Z".to_string()),
        last_task_freshness_check_at: Some("2026-03-10T12:00:00Z".to_string()),
        freshness_count_incremented_by: None,
    };

    meta.clear_routing_flags();

    // Routing flags must be cleared
    assert!(
        !meta.branch_freshness_conflict,
        "branch_freshness_conflict must be false after clear_routing_flags"
    );
    assert!(
        meta.freshness_origin_state.is_none(),
        "freshness_origin_state must be None after clear_routing_flags"
    );
    assert!(
        !meta.plan_update_conflict,
        "plan_update_conflict must be false after clear_routing_flags"
    );
    assert!(
        !meta.source_update_conflict,
        "source_update_conflict must be false after clear_routing_flags"
    );
    assert!(
        meta.conflict_files.is_empty(),
        "conflict_files must be empty after clear_routing_flags"
    );
    assert!(
        meta.source_branch.is_none(),
        "source_branch must be None after clear_routing_flags"
    );
    assert!(
        meta.target_branch.is_none(),
        "target_branch must be None after clear_routing_flags"
    );

    // Counts and backoff must be preserved
    assert_eq!(
        meta.freshness_conflict_count, 3,
        "freshness_conflict_count must be preserved"
    );
    assert!(
        meta.freshness_backoff_until.is_some(),
        "freshness_backoff_until must be preserved"
    );
    assert_eq!(
        meta.freshness_auto_reset_count, 1,
        "freshness_auto_reset_count must be preserved"
    );

    // last_freshness_check_at is not a routing flag — it must also be preserved
    assert_eq!(
        meta.last_freshness_check_at.as_deref(),
        Some("2026-03-10T12:00:00Z"),
        "last_freshness_check_at must be preserved (not a routing flag)"
    );
}

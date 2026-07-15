// Integration tests for merge target resolution bugs.
//
// Three bugs were found where a task belonging to a plan branch was merged
// directly to main instead of to its plan branch. These tests verify:
//
//   Test 1: task_with_plan_branch_merges_to_plan_not_main
//   Test 2: check_already_merged_detects_prior_merge_on_plan_branch
//   Test 3: metadata_toctou_guard_survives_conflict_metadata
//   Test 4: plan_update_conflict_retry_uses_correct_target
//   Test 5: plan_branch_repo_none_fallback_uses_metadata_guard
//
// All tests use real git repos + real DB (MemoryTaskRepository) + MockChatService.
// Mock agent spawning only — verify call_count() and metadata assertions.

use super::helpers::*;
use crate::domain::entities::{
    IdeationSessionId, InternalStatus, MergeStrategy, PlanBranchStatus, ProjectId, Task,
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
    let services = with_test_branch_update_authority(
        TaskServices::new(
            Arc::new(MockAgentSpawner::new()) as Arc<dyn AgentSpawner>,
            Arc::new(MockEventEmitter::new()) as Arc<dyn EventEmitter>,
            Arc::new(MockNotifier::new()) as Arc<dyn Notifier>,
            Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
            Arc::new(MockReviewStarter::new()) as Arc<dyn ReviewStarter>,
            Arc::clone(&chat_service) as Arc<dyn ChatService>,
        ),
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
        Arc::clone(&chat_service) as Arc<dyn ChatService>,
    )
    .with_task_scheduler(Arc::new(MockTaskScheduler::new()) as Arc<dyn TaskScheduler>)
    .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
    .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>);
    (chat_service, services)
}

/// Create a real git repo with main, a plan branch (from main), and a task branch
/// (from the plan branch) with a non-conflicting commit. Leaves repo on main.
///
/// Returns (RealGitRepo-like TempDir, plan_branch_name, task_branch_name).
fn setup_plan_branch_repo() -> (tempfile::TempDir, String, String) {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let path = dir.path();

    // git init -b main
    let _ = std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .output()
        .expect("git init");
    let _ = std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output();

    // Initial commit on main
    std::fs::write(path.join("README.md"), "# test repo").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(path)
        .output();

    // Create plan branch from main
    let plan_branch = "plan/feature-x".to_string();
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", &plan_branch])
        .current_dir(path)
        .output();
    std::fs::write(path.join("plan-init.txt"), "plan branch init").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "init plan branch"])
        .current_dir(path)
        .output();

    // Create task branch from plan branch with a non-conflicting change
    let task_branch = "task/feature-x-work".to_string();
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", &task_branch])
        .current_dir(path)
        .output();
    std::fs::write(path.join("feature.rs"), "// task work\nfn task_work() {}").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
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

    (dir, plan_branch, task_branch)
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 1: task_with_plan_branch_merges_to_plan_not_main
// ──────────────────────────────────────────────────────────────────────────────

/// A task belonging to a plan should merge its task branch INTO the plan branch,
/// NOT into main. After merge, the commit must be on the plan branch.
///
/// Flow:
///   1. Create repo with main + plan branch + task branch
///   2. Task has ideation_session_id linking it to the plan branch
///   3. on_enter(PendingMerge) → resolve_merge_branches → target = plan branch
///   4. Assert: merge_target_branch metadata = plan branch name (NOT "main")
///   5. Assert: task ends in Merged, commit is on plan branch
#[tokio::test]
async fn task_with_plan_branch_merges_to_plan_not_main() {
    let (dir, plan_branch, task_branch) = setup_plan_branch_repo();
    let path = dir.path();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(
        project_id.clone(),
        "Plan task merge to plan branch".to_string(),
    );
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(task_branch.clone());
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&path.to_string_lossy());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project_repo.create(project).await.unwrap();

    // Plan branch with session_id="sess-1" and status=Active
    let pb = make_plan_branch("artifact-1", &plan_branch, PlanBranchStatus::Active, None);
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

    // Verify metadata: merge_target_branch must be the plan branch, NOT "main"
    let meta: serde_json::Value =
        serde_json::from_str(updated.metadata.as_deref().unwrap_or("{}")).unwrap();
    assert_eq!(
        meta.get("merge_target_branch").and_then(|v| v.as_str()),
        Some(plan_branch.as_str()),
        "merge_target_branch must be the plan branch '{}', NOT 'main'. Metadata: {:?}",
        plan_branch,
        updated.metadata,
    );

    // The task should be Merged (clean merge, no conflicts)
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "Task should be Merged after merging to plan branch. Got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );

    // Verify: the task's commit should be on the plan branch, not just main
    let plan_log = std::process::Command::new("git")
        .args(["log", "--oneline", &plan_branch])
        .current_dir(path)
        .output()
        .expect("git log plan branch");
    let plan_log_str = String::from_utf8_lossy(&plan_log.stdout);
    assert!(
        plan_log_str.contains("task work") || plan_log_str.contains("feature"),
        "Plan branch should contain the task's commit. Plan branch log:\n{}",
        plan_log_str,
    );

    // Verify: main should NOT contain the task's feature.rs commit
    let main_log = std::process::Command::new("git")
        .args(["log", "--oneline", "main"])
        .current_dir(path)
        .output()
        .expect("git log main");
    let main_log_str = String::from_utf8_lossy(&main_log.stdout);
    assert!(
        !main_log_str.contains("task work"),
        "Main should NOT contain the task's commit (it should be on plan branch only). Main log:\n{}",
        main_log_str,
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 2: check_already_merged_detects_prior_merge_on_plan_branch
// ──────────────────────────────────────────────────────────────────────────────

/// When a task has already been merged to its plan branch (from a prior agent run),
/// check_already_merged should detect this even if the target was incorrectly
/// resolved to "main". The defense-in-depth plan branch check catches the mismatch.
///
/// Flow:
///   1. Create repo with plan branch, manually merge task branch into plan branch
///   2. Set up task as PendingMerge with plan_branch_repo wired
///   3. on_enter(PendingMerge) → check_already_merged detects merge on plan branch
///   4. Assert: task transitions to Merged (not Merging or MergeIncomplete)
#[tokio::test]
async fn check_already_merged_detects_prior_merge_on_plan_branch() {
    let (dir, plan_branch, task_branch) = setup_plan_branch_repo();
    let path = dir.path();

    // Manually merge task branch into plan branch (simulating a prior agent's work)
    let _ = std::process::Command::new("git")
        .args(["checkout", &plan_branch])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["merge", &task_branch, "--no-edit"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(
        project_id.clone(),
        "Already merged to plan branch".to_string(),
    );
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(task_branch.clone());
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&path.to_string_lossy());
    project.id = project_id;
    project.merge_strategy = MergeStrategy::Merge;
    project_repo.create(project).await.unwrap();

    // Plan branch Active with session_id="sess-1"
    let pb = make_plan_branch("artifact-1", &plan_branch, PlanBranchStatus::Active, None);
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

    // The task should be Merged — check_already_merged should detect the prior merge
    // on the plan branch via the defense-in-depth session_id lookup.
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "Task should be Merged after check_already_merged detects prior merge on plan branch. \
         Got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 3: metadata_toctou_guard_survives_conflict_metadata
// ──────────────────────────────────────────────────────────────────────────────

/// When merge_metadata_into is used (as in plan_update_conflict), the
/// merge_target_branch key that was cached earlier must survive — it must NOT
/// be clobbered by the conflict metadata write.
///
/// This is a unit-level test of the merge_metadata_into function's behavior
/// when applied to a task that already has merge_target_branch in metadata.
#[tokio::test]
async fn metadata_toctou_guard_survives_conflict_metadata() {
    use crate::domain::state_machine::transition_handler::merge_helpers::merge_metadata_into;

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id, "TOCTOU metadata test".to_string());

    // Step 1: Simulate what attempt_programmatic_merge does at line 180-197:
    // Cache resolved branches in metadata
    let initial_meta = serde_json::json!({
        "merge_source_branch": "task/feature-x-work",
        "merge_target_branch": "plan/feature-x",
    });
    task.metadata = Some(initial_meta.to_string());

    // Step 2: Simulate plan_update_conflict writing conflict metadata via merge_metadata_into
    // (as done in side_effects.rs line 277-285)
    let conflict_meta = serde_json::json!({
        "error": "Conflicts detected while updating plan branch from main.",
        "conflict_files": ["file1.rs", "file2.rs"],
        "source_branch": "task/feature-x-work",
        "target_branch": "plan/feature-x",
        "base_branch": "main",
        "plan_update_conflict": true,
    });
    merge_metadata_into(&mut task, &conflict_meta);

    // Verify: merge_target_branch must survive the conflict metadata write
    let meta: serde_json::Value = serde_json::from_str(task.metadata.as_deref().unwrap()).unwrap();

    assert_eq!(
        meta.get("merge_target_branch").and_then(|v| v.as_str()),
        Some("plan/feature-x"),
        "merge_target_branch must survive conflict metadata write. Got metadata: {:?}",
        task.metadata,
    );
    assert_eq!(
        meta.get("merge_source_branch").and_then(|v| v.as_str()),
        Some("task/feature-x-work"),
        "merge_source_branch must survive conflict metadata write. Got metadata: {:?}",
        task.metadata,
    );

    // Verify: conflict metadata was also written
    assert_eq!(
        meta.get("plan_update_conflict").and_then(|v| v.as_bool()),
        Some(true),
        "plan_update_conflict flag must be present. Got metadata: {:?}",
        task.metadata,
    );
    assert!(
        meta.get("error").is_some(),
        "Error message must be present. Got metadata: {:?}",
        task.metadata,
    );
    assert!(
        meta.get("conflict_files").is_some(),
        "conflict_files must be present. Got metadata: {:?}",
        task.metadata,
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 4: plan_update_conflict_retry_uses_correct_target
// ──────────────────────────────────────────────────────────────────────────────

/// Full E2E: task with plan branch → plan behind main → plan_update conflict
/// detected → agent spawned for conflict resolution. Verify that the metadata
/// caches the correct plan branch as target (not "main"), so when the agent
/// finishes and the merge retries, it uses the plan branch.
///
/// Flow:
///   1. Create repo: main has hotfix AFTER plan branch was created (plan behind main)
///   2. Create conflicting content on both main and plan branch (same file)
///   3. Task with ideation_session_id → resolves target to plan branch
///   4. on_enter(PendingMerge) → update_plan_from_main → Conflicts
///   5. plan_update_conflict path: UpdatingPlanBranch + branch updater spawned
///   6. Assert: metadata.merge_target_branch = plan branch (NOT main)
///   7. Assert: task status = UpdatingPlanBranch (agent needed for conflict resolution)
#[tokio::test]
async fn plan_update_conflict_retry_uses_correct_target() {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let path = dir.path();

    // git init -b main
    let _ = std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .output()
        .expect("git init");
    let _ = std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output();

    // Initial commit on main
    std::fs::write(path.join("README.md"), "# test repo").unwrap();
    std::fs::write(path.join("shared.rs"), "// original content\n").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(path)
        .output();

    // Create plan branch from main
    let plan_branch = "plan/conflict-test".to_string();
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", &plan_branch])
        .current_dir(path)
        .output();
    // Modify the shared file on plan branch (will conflict with main's version)
    std::fs::write(path.join("shared.rs"), "// plan branch version\n").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "plan: modify shared file"])
        .current_dir(path)
        .output();

    // Create task branch from plan branch
    let task_branch = "task/conflict-test-work".to_string();
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", &task_branch])
        .current_dir(path)
        .output();
    std::fs::write(path.join("feature.rs"), "// task work\nfn task() {}").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "feat: task work"])
        .current_dir(path)
        .output();

    // Back to main and add conflicting hotfix
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();
    std::fs::write(path.join("shared.rs"), "// main hotfix version\n").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "fix: hotfix on main (conflicts with plan)"])
        .current_dir(path)
        .output();

    // Set up repos
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(
        project_id.clone(),
        "Plan update conflict with correct target".to_string(),
    );
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(task_branch.clone());
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&path.to_string_lossy());
    project.id = project_id;
    project.merge_strategy = MergeStrategy::Merge;
    project_repo.create(project).await.unwrap();

    // Plan branch with session_id="sess-1" and status=Active
    let pb = make_plan_branch("artifact-1", &plan_branch, PlanBranchStatus::Active, None);
    plan_branch_repo.create(pb).await.unwrap();

    let (chat_service, services) =
        make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let services = services
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let meta: serde_json::Value =
        serde_json::from_str(updated.metadata.as_deref().unwrap_or("{}")).unwrap();

    // The merge_target_branch must be the plan branch, not "main".
    // This is the key regression: before the fix, plan_update_conflict would
    // lose the merge_target_branch metadata, causing retry to merge to main.
    assert_eq!(
        meta.get("merge_target_branch").and_then(|v| v.as_str()),
        Some(plan_branch.as_str()),
        "merge_target_branch must be '{}' after plan_update_conflict, NOT 'main'. Metadata: {:?}",
        plan_branch,
        updated.metadata,
    );

    // The task should remain in the dedicated plan-update state for resolution.
    assert_eq!(
        updated.internal_status,
        InternalStatus::UpdatingPlanBranch,
        "Task should be UpdatingPlanBranch after plan_update_conflict. Got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );

    // Verify: plan_update_conflict flag is set
    assert_eq!(
        meta.get("plan_update_conflict").and_then(|v| v.as_bool()),
        Some(true),
        "plan_update_conflict must be set in metadata. Metadata: {:?}",
        updated.metadata,
    );

    // Verify: the branch updater was spawned.
    assert!(
        chat_service.call_count() >= 1,
        "plan_update_conflict must spawn a branch updater (call_count={}). Metadata: {:?}",
        chat_service.call_count(),
        updated.metadata,
    );

    // Verify: the task did NOT merge to main
    let main_log = std::process::Command::new("git")
        .args(["log", "--oneline", "main"])
        .current_dir(path)
        .output()
        .expect("git log main");
    let main_log_str = String::from_utf8_lossy(&main_log.stdout);
    assert!(
        !main_log_str.contains("task work"),
        "Main must NOT contain the task's commit during plan_update_conflict. Main log:\n{}",
        main_log_str,
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 5: plan_branch_repo_none_fallback_uses_metadata_guard
// ──────────────────────────────────────────────────────────────────────────────

/// When plan_branch_repo is None, resolve_merge_branches falls back to main.
/// But if merge_target_branch metadata was previously cached (from an earlier
/// correct resolution), the check_already_merged defense-in-depth should still
/// detect the merge on the plan branch.
///
/// This tests the scenario where:
///   1. First merge attempt: plan_branch_repo is available, resolves target correctly,
///      caches merge_target_branch in metadata, but merge fails → MergeIncomplete
///   2. Second merge attempt: plan_branch_repo is wired (but task has cached metadata),
///      resolve_merge_branches uses plan branch, and if task was already merged to
///      plan branch by a prior agent, check_already_merged detects it.
///
/// Simplified: verify that resolve_merge_branches correctly falls back to base_branch
/// when plan_branch_repo is None, and that metadata is the only safety net.
#[tokio::test]
async fn plan_branch_repo_none_fallback_uses_metadata_guard() {
    use crate::domain::state_machine::transition_handler::merge_helpers::resolve_merge_branches;

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Fallback test".to_string());
    task.task_branch = Some("task/feature-x".to_string());
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));

    // Pre-cache merge_target_branch in metadata (from a prior correct resolution)
    let cached_meta = serde_json::json!({
        "merge_source_branch": "task/feature-x",
        "merge_target_branch": "plan/feature-x",
    });
    task.metadata = Some(cached_meta.to_string());

    let project = {
        let mut p = make_project(Some("main"));
        p.id = project_id;
        p.base_branch = Some("main".to_string());
        p
    };

    // Call resolve_merge_branches with plan_branch_repo = None
    let plan_branch_repo: Option<Arc<dyn PlanBranchRepository>> = None;
    let (source, target) = resolve_merge_branches(&task, &project, &plan_branch_repo).await;

    // Without plan_branch_repo, resolve_merge_branches falls back to base_branch ("main")
    assert_eq!(
        source, "task/feature-x",
        "Source branch should be the task branch"
    );
    assert_eq!(
        target, "main",
        "Without plan_branch_repo, target should fall back to base_branch 'main'"
    );

    // Verify the metadata guard still has the correct plan branch cached
    let meta: serde_json::Value = serde_json::from_str(task.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(
        meta.get("merge_target_branch").and_then(|v| v.as_str()),
        Some("plan/feature-x"),
        "Metadata should still have cached merge_target_branch = plan branch. \
         This is the safety net when plan_branch_repo is unavailable."
    );

    // Now verify with plan_branch_repo wired — should correctly resolve to plan branch
    let plan_branch_repo_mem = Arc::new(MemoryPlanBranchRepository::new());
    let pb = make_plan_branch(
        "artifact-1",
        "plan/feature-x",
        PlanBranchStatus::Active,
        None,
    );
    plan_branch_repo_mem.create(pb).await.unwrap();

    let plan_branch_repo_opt: Option<Arc<dyn PlanBranchRepository>> =
        Some(Arc::clone(&plan_branch_repo_mem) as Arc<dyn PlanBranchRepository>);
    let (source2, target2) = resolve_merge_branches(&task, &project, &plan_branch_repo_opt).await;

    assert_eq!(
        source2, "task/feature-x",
        "Source should be the task branch"
    );
    assert_eq!(
        target2, "plan/feature-x",
        "With plan_branch_repo wired and Active plan branch, target should be 'plan/feature-x'"
    );
}

/// Regression: when a plan branch was accepted against a non-project base branch,
/// PendingMerge must keep using the plan branch's own source branch for freshness.
///
/// Before the fix, the merge pipeline always used `project.base_branch` ("main")
/// for plan freshness. Reaccepted plans based on another branch would immediately
/// route to `plan_update_conflict` and spawn a merger agent even though the plan
/// branch was already fresh relative to its actual source branch.
#[tokio::test]
async fn pending_merge_uses_plan_source_branch_instead_of_project_base() {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let path = dir.path();

    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .expect("git command failed")
    };

    run(&["init", "-b", "main"]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
    std::fs::write(path.join("README.md"), "# test repo").unwrap();
    run(&["add", "."]);
    run(&["commit", "-m", "initial commit"]);

    let release_branch = "release/2026-04".to_string();
    run(&["checkout", "-b", &release_branch]);
    std::fs::write(path.join("shared.txt"), "release baseline\n").unwrap();
    run(&["add", "shared.txt"]);
    run(&["commit", "-m", "release baseline"]);

    let plan_branch = "plan/non-main-base".to_string();
    run(&["checkout", "-b", &plan_branch]);
    std::fs::write(path.join("plan-init.txt"), "plan branch init\n").unwrap();
    run(&["add", "plan-init.txt"]);
    run(&["commit", "-m", "init plan branch"]);

    let task_branch = "task/non-main-base".to_string();
    run(&["checkout", "-b", &task_branch]);
    std::fs::write(path.join("feature.rs"), "// task work\nfn task_work() {}\n").unwrap();
    run(&["add", "feature.rs"]);
    run(&["commit", "-m", "feat: task work"]);

    run(&["checkout", "main"]);
    std::fs::write(path.join("shared.txt"), "main divergence\n").unwrap();
    run(&["add", "shared.txt"]);
    run(&["commit", "-m", "main divergence"]);

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Non-main plan merge".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(task_branch.clone());
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&path.to_string_lossy());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project_repo.create(project).await.unwrap();

    let mut pb = make_plan_branch("artifact-1", &plan_branch, PlanBranchStatus::Active, None);
    pb.source_branch = release_branch.clone();
    pb.base_branch_override = Some(release_branch.clone());
    plan_branch_repo.create(pb).await.unwrap();

    let (chat_service, services) =
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
        "Task should merge cleanly into the plan branch when the plan source branch is fresh. \
         A Merging status here means the pipeline incorrectly used project.base_branch. \
         Metadata: {:?}",
        updated.metadata,
    );
    assert_eq!(
        chat_service.call_count(),
        0,
        "A non-main-base plan freshness check should not spawn a merger agent"
    );

    let meta: serde_json::Value =
        serde_json::from_str(updated.metadata.as_deref().unwrap_or("{}")).unwrap();
    assert_eq!(
        meta.get("merge_target_branch").and_then(|v| v.as_str()),
        Some(plan_branch.as_str()),
        "Task should still merge into the plan branch"
    );

    let plan_log = std::process::Command::new("git")
        .args(["log", "--oneline", &plan_branch])
        .current_dir(path)
        .output()
        .expect("git log plan branch");
    let plan_log_str = String::from_utf8_lossy(&plan_log.stdout);
    assert!(
        plan_log_str.contains("task work"),
        "Plan branch should contain the task commit. Log:\n{}",
        plan_log_str,
    );
    assert!(
        !plan_log_str.contains("main divergence"),
        "Plan branch must not pull in project.base_branch ('main') during this merge. Log:\n{}",
        plan_log_str,
    );
}

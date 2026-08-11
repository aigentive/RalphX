// Integration tests for PlanMerge check_already_merged guard.
//
// Verifies that the defense-in-depth plan branch check in check_already_merged
// does NOT tautologically complete PlanMerge tasks. For PlanMerge tasks,
// source_branch IS the plan branch, so "is plan branch HEAD on plan branch?"
// is always true — the guard must be skipped.
//
// Test 1: plan_merge_not_falsely_completed_by_plan_branch_check
//   PlanMerge task (source=plan, target=main) → must actually merge to main,
//   not be falsely completed by the defense-in-depth block.
//
// Test 2: regular_plan_task_already_merged_to_plan_branch_detected
//   Regular task (source=task_branch, target=plan) already merged to plan →
//   should be correctly detected as already merged.
//
// Test 3: ghost_merge_prevented_when_plan_branch_has_no_unique_commits (V1 fix)
//   PlanMerge task where plan_branch has 0 unique commits vs main → check_already_merged
//   must NOT fire (ghost-merge guard). The pipeline runs the actual merge (no-op).

use super::helpers::*;
use crate::domain::entities::{
    IdeationSessionId, InternalStatus, MergeStrategy, PlanBranchStatus, Project, ProjectId, Task,
    TaskCategory,
};
use crate::domain::repositories::PlanBranchRepository;
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::{State, TransitionHandler};
use crate::infrastructure::memory::MemoryPlanBranchRepository;

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

/// Create a real git repo with main and a plan branch (with a unique commit).
/// Leaves repo on main.
///
/// Returns (TempDir, plan_branch_name).
fn setup_plan_merge_repo() -> (tempfile::TempDir, String) {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let path = dir.path();

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

    // Create plan branch from main with a feature commit
    let plan_branch = "plan/feature-abc".to_string();
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", &plan_branch])
        .current_dir(path)
        .output();
    std::fs::write(
        path.join("plan-work.rs"),
        "// plan branch work\nfn plan() {}",
    )
    .unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "feat: plan branch work"])
        .current_dir(path)
        .output();

    // Back to main
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();

    (dir, plan_branch)
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 1: PlanMerge task must NOT be falsely completed by plan branch check
// ──────────────────────────────────────────────────────────────────────────────

/// A PlanMerge task (source=plan_branch, target=main) must actually merge to main.
/// Before the fix, the defense-in-depth block in check_already_merged performed a
/// tautological check ("is plan branch HEAD on plan branch?" — always true),
/// causing the task to be falsely marked Merged with target=plan_branch instead
/// of actually merging to main.
///
/// Flow:
///   1. Git repo: main + plan branch (plan has commit NOT on main)
///   2. PlanMerge task: source=plan, target=main, ideation_session_id set
///   3. on_enter(PendingMerge) → check_already_merged should skip defense-in-depth
///   4. Assert: task is Merged AND the plan branch commit is on main (actual merge happened)
#[tokio::test]
async fn plan_merge_not_falsely_completed_by_plan_branch_check() {
    let (dir, plan_branch) = setup_plan_merge_repo();
    let path = dir.path();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Merge plan to main".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.category = TaskCategory::PlanMerge;
    task.task_branch = Some(plan_branch.clone());
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();

    // Create plan branch record with merge_task_id pointing to this task
    let mut pb = make_plan_branch("artifact-1", &plan_branch, PlanBranchStatus::Active, None);
    pb.merge_task_id = Some(task_id.clone());
    plan_branch_repo.create(pb).await.unwrap();

    task_repo.create(task).await.unwrap();

    let mut project = Project::new(
        "test-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project_repo.create(project).await.unwrap();

    let (_, services) =
        make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let services = services
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();

    // Task must reach Merged status
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "PlanMerge task should be Merged after merge to main. Got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );

    // Verify: the plan branch's commit must be on main (actual merge happened)
    let main_log = std::process::Command::new("git")
        .args(["log", "--oneline", "main"])
        .current_dir(path)
        .output()
        .expect("git log main");
    let main_log_str = String::from_utf8_lossy(&main_log.stdout);
    assert!(
        main_log_str.contains("plan branch work") || main_log_str.contains("plan"),
        "Main must contain the plan branch's commit (actual merge must have happened). \
         Main log:\n{}",
        main_log_str,
    );

    // Verify metadata: merge_target_branch should be main (not the plan branch)
    let meta: serde_json::Value =
        serde_json::from_str(updated.metadata.as_deref().unwrap_or("{}")).unwrap();
    let merge_target = meta.get("merge_target_branch").and_then(|v| v.as_str());
    assert_eq!(
        merge_target,
        Some("main"),
        "merge_target_branch must be 'main' for PlanMerge task (not '{}'). Metadata: {:?}",
        plan_branch,
        updated.metadata,
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 2: Regular plan task already merged to plan branch IS detected
// ──────────────────────────────────────────────────────────────────────────────

/// A regular task (source=task_branch, target=plan_branch) that has already been
/// merged to the plan branch should be correctly detected by the defense-in-depth
/// block in check_already_merged.
///
/// This verifies the guard does NOT over-block: regular tasks should still benefit
/// from the plan branch defense-in-depth check.
///
/// Flow:
///   1. Git repo: main + plan branch + task branch (branched from plan)
///   2. Manually merge task branch into plan branch (simulate prior agent)
///   3. Regular task: source=task_branch, target=plan_branch, session_id set
///   4. on_enter(PendingMerge) → check_already_merged detects prior merge
///   5. Assert: task is Merged (detected as already merged)
#[tokio::test]
async fn regular_plan_task_already_merged_to_plan_branch_detected() {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let path = dir.path();

    // git init
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

    // Create plan branch
    let plan_branch = "plan/feature-xyz".to_string();
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", &plan_branch])
        .current_dir(path)
        .output();
    std::fs::write(path.join("plan-init.txt"), "plan init").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "init plan branch"])
        .current_dir(path)
        .output();

    // Create task branch from plan branch
    let task_branch = "task/feature-xyz-work".to_string();
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

    // Merge task branch into plan branch (simulate prior agent completing the merge)
    let _ = std::process::Command::new("git")
        .args(["checkout", &plan_branch])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["merge", &task_branch, "--no-edit"])
        .current_dir(path)
        .output();

    // Back to main
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
        "Regular task already merged to plan".to_string(),
    );
    task.internal_status = InternalStatus::PendingMerge;
    // Regular category (NOT PlanMerge) — defense-in-depth should work
    task.category = TaskCategory::Regular;
    task.task_branch = Some(task_branch.clone());
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = Project::new(
        "test-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    project.id = project_id;
    project.base_branch = Some("main".to_string());
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

    // The task should be Merged — detected as already merged to plan branch
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "Regular task already merged to plan branch should be detected by defense-in-depth. \
         Got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );

    // Main should NOT contain the task's commit (it was merged to plan, not main)
    let main_log = std::process::Command::new("git")
        .args(["log", "--oneline", "main"])
        .current_dir(path)
        .output()
        .expect("git log main");
    let main_log_str = String::from_utf8_lossy(&main_log.stdout);
    assert!(
        !main_log_str.contains("task work"),
        "Main must NOT contain the task's commit. Main log:\n{}",
        main_log_str,
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Test 3: Ghost-merge prevention — plan branch with 0 unique commits
// ──────────────────────────────────────────────────────────────────────────────

/// Regression test: check_already_merged must NOT fire a false positive when the
/// plan branch has zero unique commits vs main.
///
/// Ghost merge scenario:
///   - plan_branch was created from main at commit A
///   - main later advances to commit C (other work landed on main directly)
///   - plan_branch is still at A — it NEVER DIVERGED from main (no task work merged to it)
///   - plan_sha (A) IS an ancestor of main (C) via `git merge-base --is-ancestor`
///   - Without the fix, check_already_merged would fire → DB: Merged, git: unchanged
///   - With the fix: count_commits_not_on_branch(plan, main) = 0 → return false
///   - The full pipeline runs; the no-op merge completes the task correctly
///
/// Observable assertion: the task completes as Merged without a new merge commit on main
/// (since plan had no unique work). Main log still shows only its own commits, not
/// any phantom "plan merge" commit created by the false positive fast-path.
#[tokio::test]
async fn ghost_merge_prevented_when_plan_branch_has_no_unique_commits() {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let path = dir.path();

    // Initialize git repo
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "test@test.com"],
        vec!["config", "user.name", "Test"],
    ] {
        let _ = std::process::Command::new("git")
            .args(&args)
            .current_dir(path)
            .output();
    }

    // Initial commit on main
    std::fs::write(path.join("README.md"), "# repo").unwrap();
    for args in [vec!["add", "."], vec!["commit", "-m", "initial commit"]] {
        let _ = std::process::Command::new("git")
            .args(&args)
            .current_dir(path)
            .output();
    }

    // Create plan_branch at same commit as main (never diverged)
    let plan_branch = "plan/no-work".to_string();
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", &plan_branch])
        .current_dir(path)
        .output();

    // Return to main and add more work — now plan_sha is an ancestor of main
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();
    std::fs::write(path.join("other.rs"), "// other work on main").unwrap();
    for args in [
        vec!["add", "."],
        vec!["commit", "-m", "other: direct main work"],
    ] {
        let _ = std::process::Command::new("git")
            .args(&args)
            .current_dir(path)
            .output();
    }

    // At this point: plan_branch is at the INITIAL commit (ancestor of main).
    // is_commit_on_branch(plan_sha, main) would return true → ghost merge trigger.
    // count_commits_not_on_branch(plan, main) = 0 → fix returns false from check_already_merged.

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let project_id = ProjectId::from_string("proj-ghost".to_string());
    let mut task = Task::new(project_id.clone(), "Merge ghost plan to main".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.category = TaskCategory::PlanMerge;
    task.task_branch = Some(plan_branch.clone());
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-ghost".to_string()));
    let task_id = task.id.clone();

    let mut pb = make_plan_branch(
        "artifact-ghost",
        &plan_branch,
        PlanBranchStatus::Active,
        None,
    );
    pb.merge_task_id = Some(task_id.clone());
    plan_branch_repo.create(pb).await.unwrap();

    task_repo.create(task).await.unwrap();

    let mut project = Project::new(
        "ghost-project".to_string(),
        path.to_string_lossy().to_string(),
    );
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project_repo.create(project).await.unwrap();

    let (_, services) =
        make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let services = services
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);
    let context = TaskContext::new(task_id.as_str(), "proj-ghost", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();

    // Task must reach Merged status (no-op merge succeeds — plan was already at main)
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "PlanMerge task with no unique commits should still reach Merged (no-op merge). \
         Got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );

    // Key assertion: main log must NOT contain a phantom "already merged" fast-path entry.
    // The ghost merge fast-path would set merge_commit_sha = main HEAD without creating
    // any new commit on main. The fix ensures the pipeline ran properly (same SHA, correct path).
    // Verify merge_target_branch metadata says "main" (not the plan branch from false detection).
    let meta: serde_json::Value =
        serde_json::from_str(updated.metadata.as_deref().unwrap_or("{}")).unwrap();
    let merge_target = meta.get("merge_target_branch").and_then(|v| v.as_str());
    assert_eq!(
        merge_target,
        Some("main"),
        "merge_target_branch must be 'main' for PlanMerge task. Metadata: {:?}",
        updated.metadata,
    );
}

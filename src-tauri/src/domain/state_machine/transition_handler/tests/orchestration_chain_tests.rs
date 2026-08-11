// Orchestration chain tests: end-to-end flows from PendingMerge that verify
// BOTH the status transition AND agent spawning in a single pass.
//
// Per CLAUDE.md rule 1.5: real git + real DB (MemoryTaskRepository) + MockChatService.
// Mock agent spawning only → verify call_count() and ChatContextType::Merge.
//
// Paths tested:
//   B2: Source freshness conflict → UpdatingTaskBranch + branch updater spawned
//   C1: AutoFix validation failure → Merging + validation_recovery=true + merger agent spawned
//   TOCTOU: Metadata caching at dispatch (merge_target_branch/merge_source_branch in metadata)

use super::helpers::*;
use crate::domain::entities::{
    IdeationSessionId, InternalStatus, MergeStrategy, MergeValidationMode, PlanBranchStatus,
    ProjectId, Task,
};
use crate::domain::repositories::PlanBranchRepository;
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::{State, TransitionHandler};
use crate::infrastructure::memory::MemoryPlanBranchRepository;

// ──────────────────────────────────────────────────────────────────────────────
// Shared helper: TaskServices with a retained Arc<MockChatService>
// ──────────────────────────────────────────────────────────────────────────────

/// Build TaskServices with a retained Arc<MockChatService> for call_count assertions.
///
/// Unlike `TaskServices::new_mock()`, this keeps a handle to the chat service
/// so callers can assert `call_count() >= 1` after the transition.
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

// ──────────────────────────────────────────────────────────────────────────────
// B2: Source freshness conflict (PendingMerge → UpdatingTaskBranch + agent spawned)
// ──────────────────────────────────────────────────────────────────────────────

/// B2: A source freshness conflict routes to the dedicated task-branch update state
/// and spawns the branch updater before the merge strategy is allowed to run.
///
/// Orchestration chain:
///   on_enter(PendingMerge)
///   → attempt_programmatic_merge
///   → dedicated freshness checkpoint detects task branch is stale
///   → programmatic task-branch update reports conflicts
///   → task.internal_status = UpdatingTaskBranch
///   → chat_service.send_message(ChatContextType::BranchUpdate, task_id, ...)
#[tokio::test]
async fn b2_source_freshness_conflict_transitions_to_task_update_and_spawns_agent() {
    let git_repo = setup_real_git_repo();

    // Create a conflicting commit on main: feature.rs was added on the task branch,
    // now main also modifies it — git merge will produce a conflict.
    std::fs::write(
        git_repo.path().join("feature.rs"),
        "// conflicting version on main",
    )
    .unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(git_repo.path())
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "conflicting change on main"])
        .current_dir(git_repo.path())
        .output();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(
        project_id.clone(),
        "B2 merge conflict chain test".to_string(),
    );
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(git_repo.task_branch.clone());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&git_repo.path_string());
    project.id = project_id;
    project.merge_strategy = MergeStrategy::Merge;
    project_repo.create(project).await.unwrap();

    let (chat_service, services) =
        make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::UpdatingTaskBranch,
        "Source freshness conflict must transition task to UpdatingTaskBranch. Got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );
    assert!(
        chat_service.call_count() >= 1,
        "Source freshness conflict must spawn a branch updater (call_count={}).",
        chat_service.call_count(),
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// C1: AutoFix validation failure (PendingMerge → Merging + agent spawned)
// ──────────────────────────────────────────────────────────────────────────────

/// C1: AutoFix validation failure → task transitions to Merging, validation_recovery=true,
/// AND a merger agent is spawned.
///
/// Orchestration chain:
///   on_enter(PendingMerge)
///   → attempt_programmatic_merge
///   → MergeStrategy::RebaseSquash → MergeOutcome::Success (clean merge into worktree)
///   → handle_outcome_success
///   → run_validation_commands → fails ("false" always exits 1)
///   → handle_validation_failure (AutoFix mode, merge_path != repo_path)
///   → task.internal_status = Merging, metadata.validation_recovery = true
///   → on_enter_dispatch(Merging) → chat_service.send_message(...)  ← verified by call_count
///
/// Key setup: repo is left on the TASK BRANCH (not main) so RebaseSquash uses worktree
/// isolation (current_branch != target_branch). This gives merge_path != repo_path, so
/// handle_validation_failure's else-branch reuses the existing merge worktree as the fixer
/// worktree — no new `git worktree add <main>` is attempted (which would fail since
/// `main` can't be checked out twice in the same repo).
#[tokio::test]
async fn c1_autofix_validation_failure_transitions_to_merging_and_spawns_agent() {
    let git_repo = setup_real_git_repo();

    // Check out task branch so `main` is NOT the current branch.
    // All strategies check `if current_branch == target_branch` and fall back to
    // checkout-free merge (returning merge_path = repo_path) when true. By staying
    // on the task branch, RebaseSquash uses its dual-worktree path and returns a
    // merge_path that points to the merge worktree, not the repo root.
    let _ = std::process::Command::new("git")
        .args(["checkout", &git_repo.task_branch])
        .current_dir(git_repo.path())
        .output();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(
        project_id.clone(),
        "C1 autofix validation chain test".to_string(),
    );
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(git_repo.task_branch.clone());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&git_repo.path_string());
    project.id = project_id;
    // RebaseSquash with current_branch != target_branch → dual-worktree path.
    // merge_path = merge_wt != repo_path → handle_validation_failure's else-branch
    // reuses the existing merge worktree; no new git worktree add is attempted.
    project.merge_strategy = MergeStrategy::RebaseSquash;
    // AutoFix: don't revert merge on validation failure — spawn a fixer agent instead.
    project.merge_validation_mode = MergeValidationMode::AutoFix;
    // Validation command that always fails → triggers AutoFix agent-spawn path.
    project.detected_analysis =
        Some(r#"[{"path": ".", "label": "Test", "validate": ["false"]}]"#.to_string());
    project_repo.create(project).await.unwrap();

    let (chat_service, services) =
        make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merging,
        "AutoFix validation failure must transition to Merging. Got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );

    let meta: serde_json::Value =
        serde_json::from_str(updated.metadata.as_deref().unwrap_or("{}")).unwrap();
    assert_eq!(
        meta.get("validation_recovery"),
        Some(&serde_json::json!(true)),
        "Metadata must have validation_recovery=true for AutoFix path. Metadata: {:?}",
        updated.metadata,
    );

    assert!(
        chat_service.call_count() >= 1,
        "AutoFix validation failure must spawn a merger agent (call_count={}). \
         handle_validation_failure must call on_enter_dispatch(Merging) → chat_service.send_message().",
        chat_service.call_count(),
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// TOCTOU: Metadata caching at merge dispatch
// ──────────────────────────────────────────────────────────────────────────────

/// TOCTOU guard — verify merge branches are cached in task metadata at dispatch.
///
/// Orchestration chain:
///   on_enter(PendingMerge)
///   → attempt_programmatic_merge
///   → resolve_merge_branches (task has ideation_session_id → looks up plan branch repo)
///   → plan branch found with status=Active → target = plan branch name (not "main")
///   → task.metadata["merge_target_branch"] = plan_branch_name  ← verified
///   → task.metadata["merge_source_branch"] = task_branch       ← verified
///
/// This ensures auto-complete always merges into the branch that was current at
/// dispatch time, even if plan state changes before the merger agent finishes.
#[tokio::test]
async fn toctou_merge_branches_cached_in_metadata_before_merge() {
    let git_repo = setup_real_git_repo(); // creates main + task/test-task-branch
    let plan_branch_name = "plan/toctou-test";

    // Create plan branch in git (from main, no conflicts with task branch)
    let _ = std::process::Command::new("git")
        .args(["checkout", "-b", plan_branch_name])
        .current_dir(git_repo.path())
        .output();
    std::fs::write(git_repo.path().join("plan-init.txt"), "plan branch init").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(git_repo.path())
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "init plan branch"])
        .current_dir(git_repo.path())
        .output();
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(git_repo.path())
        .output();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());

    let mut task = Task::new(project_id.clone(), "TOCTOU branch caching test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(git_repo.task_branch.clone());
    // Task belongs to a plan (sess-1 matches make_plan_branch's hardcoded session_id)
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&git_repo.path_string());
    project.id = project_id;
    project.merge_strategy = MergeStrategy::Merge;
    project_repo.create(project).await.unwrap();

    // Plan branch with session_id="sess-1" and status=Active
    // make_plan_branch hardcodes session_id="sess-1" — matches task.ideation_session_id above
    let pb = make_plan_branch(
        "artifact-1",
        plan_branch_name,
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
    let meta: serde_json::Value =
        serde_json::from_str(updated.metadata.as_deref().unwrap_or("{}")).unwrap();

    assert_eq!(
        meta.get("merge_target_branch").and_then(|v| v.as_str()),
        Some(plan_branch_name),
        "TOCTOU guard: merge_target_branch must be cached as plan branch (not 'main'). \
         Got metadata: {:?}",
        updated.metadata,
    );
    assert_eq!(
        meta.get("merge_source_branch").and_then(|v| v.as_str()),
        Some(git_repo.task_branch.as_str()),
        "TOCTOU guard: merge_source_branch must be cached as task branch. \
         Got metadata: {:?}",
        updated.metadata,
    );
}

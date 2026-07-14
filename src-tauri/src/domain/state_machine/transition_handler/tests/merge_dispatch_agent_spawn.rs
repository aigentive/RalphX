// Tests for on_enter_dispatch coverage across all Merging state entry paths.
//
// Regression tests: every path that transitions a task to Merging MUST call
// on_enter_dispatch(Merging) to spawn a merger agent via chat_service.
//
// The source_update_conflict fix (commit 849163c9) is specifically regression-tested
// end-to-end: before the fix, SourceUpdateResult::Conflicts set status=Merging but
// did NOT call on_enter_dispatch, leaving tasks stuck with no agent indefinitely.

use super::helpers::*;
use crate::domain::entities::plan_branch::PrStatus;
use crate::domain::entities::{
    InternalStatus, MergeStrategy, PlanBranchStatus, Project, ProjectId, Task, TaskCategory,
};
use crate::domain::repositories::PlanBranchRepository;
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::{State, TransitionHandler};
use crate::infrastructure::memory::MemoryPlanBranchRepository;

// ──────────────────────────────────────────────────────────────────────────────
// Shared setup helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Build TaskServices with a retained Arc<MockChatService> for call_count assertions.
///
/// Unlike `TaskServices::new_mock()`, this retains a handle to the chat service
/// so callers can assert `call_count() >= 1` after on_enter(Merging).
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

/// Create an in-memory task+project in Merging state with given metadata.
///
/// Returns (chat_service, machine, task_repo) so callers can:
///   1. Call handler.on_enter(&State::Merging).await
///   2. Assert chat_service.call_count() >= 1
async fn setup_merging_machine(
    metadata_json: &str,
) -> (
    Arc<MockChatService>,
    TaskStateMachine,
    Arc<MemoryTaskRepository>,
) {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "merge dispatch test".to_string());
    task.internal_status = InternalStatus::Merging;
    task.task_branch = Some("task/test-branch".to_string());
    task.metadata = Some(metadata_json.to_string());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    // Project points at a nonexistent path: git cleanup in on_enter(Merging)
    // is safely skipped (worktree path won't exist).
    let mut project = Project::new(
        "test-project".to_string(),
        "/tmp/nonexistent-dispatch-test".to_string(),
    );
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project_repo.create(project).await.unwrap();

    let (chat_service, services) =
        make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let machine = TaskStateMachine::new(context);
    (chat_service, machine, task_repo)
}

async fn setup_pr_mode_merging_machine(
    metadata_json: &str,
) -> (
    Arc<MockChatService>,
    TaskStateMachine,
    Arc<MemoryTaskRepository>,
) {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "pr merge dispatch test".to_string());
    task.internal_status = InternalStatus::Merging;
    task.category = TaskCategory::PlanMerge;
    task.task_branch = Some("task/test-branch".to_string());
    task.metadata = Some(metadata_json.to_string());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = Project::new(
        "test-project".to_string(),
        "/tmp/nonexistent-pr-dispatch-test".to_string(),
    );
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project_repo.create(project).await.unwrap();

    let mut plan_branch = make_plan_branch(
        "artifact-1",
        "plan/feature-1",
        PlanBranchStatus::Active,
        None,
    );
    plan_branch.merge_task_id = Some(task_id.clone());
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(68);
    plan_branch.pr_status = Some(PrStatus::Open);
    plan_branch_repo.create(plan_branch).await.unwrap();

    let (chat_service, services) =
        make_services_with_tracked_chat(Arc::clone(&task_repo), Arc::clone(&project_repo));
    let services = services
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let machine = TaskStateMachine::new(context);
    (chat_service, machine, task_repo)
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests: on_enter(Merging) spawns agent for all metadata flag variants
// ──────────────────────────────────────────────────────────────────────────────

/// validation_recovery → on_enter(Merging) → merger agent spawned.
///
/// Post-merge validation failures put the task in Merging with validation_recovery=true.
/// The agent receives a tailored "fix validation failures" prompt (not a conflict prompt).
#[tokio::test]
async fn merging_spawns_agent_for_validation_recovery() {
    let (chat_service, mut machine, _) =
        setup_merging_machine(r#"{"validation_recovery": true}"#).await;

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::Merging).await;

    assert!(
        chat_service.call_count() >= 1,
        "on_enter(Merging) with validation_recovery=true must spawn a merger agent via chat_service"
    );
}

/// plan_update_conflict → on_enter(Merging) → merger agent spawned.
///
/// The plan branch can't be updated from main due to conflicts. The agent receives a
/// plan-branch-specific prompt with step-by-step merge conflict resolution instructions.
#[tokio::test]
async fn merging_spawns_agent_for_plan_update_conflict() {
    let (chat_service, mut machine, _) = setup_merging_machine(
        r#"{"plan_update_conflict": true, "target_branch": "plan/feature-1", "base_branch": "main"}"#,
    )
    .await;

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::Merging).await;

    assert!(
        chat_service.call_count() >= 1,
        "on_enter(Merging) with plan_update_conflict=true must spawn a merger agent via chat_service"
    );
}

/// pr_branch_update_conflict → on_enter(Merging) bypasses PR poller shortcut and
/// spawns a merger agent to update the already-open PR branch from its base.
#[tokio::test]
async fn merging_spawns_agent_for_pr_branch_update_conflict_even_when_pr_mode_active() {
    let (chat_service, mut machine, _) = setup_pr_mode_merging_machine(
        r#"{"plan_update_conflict": true, "pr_branch_update_conflict": true, "target_branch": "plan/feature-1", "base_branch": "origin/main"}"#,
    )
    .await;

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::Merging).await;

    assert!(
        chat_service.call_count() >= 1,
        "PR branch update conflicts must spawn a merger agent instead of being swallowed by the PR poller shortcut"
    );
}

/// source_update_conflict → on_enter(Merging) → merger agent spawned.
///
/// The task branch can't incorporate target changes due to conflicts. The agent receives a
/// source-update-specific prompt with step-by-step git merge instructions.
///
/// This directly exercises the on_enter(Merging) dispatch path for source_update_conflict.
/// The end-to-end regression (from PendingMerge) is tested separately below.
#[tokio::test]
async fn merging_spawns_agent_for_source_update_conflict() {
    let (chat_service, mut machine, _) = setup_merging_machine(
        r#"{"source_update_conflict": true, "source_branch": "task/feature-branch", "target_branch": "main"}"#,
    )
    .await;

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::Merging).await;

    assert!(
        chat_service.call_count() >= 1,
        "on_enter(Merging) with source_update_conflict=true must spawn a merger agent via chat_service"
    );
}

/// Normal merge conflict → on_enter(Merging) → merger agent spawned.
///
/// When the merge strategy encounters conflicts (no special metadata flags), the default
/// "Resolve merge conflicts" prompt is used. An agent must still be spawned.
#[tokio::test]
async fn merging_spawns_agent_for_normal_conflict() {
    let (chat_service, mut machine, _) = setup_merging_machine(r#"{}"#).await;

    let handler = TransitionHandler::new(&mut machine);
    let _ = handler.on_enter(&State::Merging).await;

    assert!(
        chat_service.call_count() >= 1,
        "on_enter(Merging) with no special metadata must still spawn a merger agent via chat_service"
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// End-to-end regression test: source_update_conflict from PendingMerge
// ──────────────────────────────────────────────────────────────────────────────

/// Regression test: full PendingMerge→UpdatingTaskBranch path spawns the dedicated updater.
///
/// Before commit 849163c9, SourceUpdateResult::Conflicts in attempt_programmatic_merge
/// called persist_merge_transition(Merging) but did NOT call on_enter_dispatch(Merging).
/// Tasks were stuck in Merging indefinitely with no merger agent and no way to progress.
///
/// Expected flow after the fix:
///   on_enter(PendingMerge)
///   → attempt_programmatic_merge
///   → update_source_from_target → SourceUpdateResult::Conflicts
///   → source_update_conflict arm: create worktree + persist_merge_transition(Merging)
///   → on_enter_dispatch(Merging) → chat_service.send_message  ← THIS IS THE FIX
#[tokio::test]
async fn source_update_conflict_from_pending_merge_spawns_branch_updater() {
    let git_repo = setup_real_git_repo();
    let path = git_repo.path();

    // Add a conflicting commit on main AFTER task branch was created.
    // task branch has:  "// feature code\nfn feature() {}"  (in feature.rs)
    // main will have:   "// conflicting main version\nfn main_fn() {}"
    //
    // This makes the task branch BEHIND main with conflicting content in feature.rs.
    // update_source_from_target(source=task_branch, target=main) will return
    // SourceUpdateResult::Conflicts, triggering the source_update_conflict arm.
    std::fs::write(
        path.join("feature.rs"),
        "// conflicting main version\nfn main_fn() {}",
    )
    .unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "feature.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args([
            "commit",
            "-m",
            "fix: main also modifies feature.rs (source update conflict)",
        ])
        .current_dir(path)
        .output();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "source conflict regression".to_string());
    task.internal_status = InternalStatus::PendingMerge;
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
    assert_eq!(
        updated.internal_status,
        InternalStatus::UpdatingTaskBranch,
        "source_update_conflict path must transition task to UpdatingTaskBranch. Got: {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );
    assert!(
        chat_service.call_count() >= 1,
        "source_update_conflict path must spawn a branch updater (call_count={}).",
        chat_service.call_count(),
    );
}

// Dedicated plan-update conflict worktree creation tests (originally RC#6).
//
// Verifies that when plan branch update from main produces conflicts during
// on_enter(PendingMerge), a plan-update-* worktree is retained and the branch updater
// is spawned. Previously, the code set worktree_path to a merge-* path without
// creating the directory, causing "no valid merge worktree" spawn failures.
//
// Per CLAUDE.md rule 1.5: real git + real DB (MemoryTaskRepository) + MockChatService.

use super::helpers::*;
use crate::domain::entities::{
    GitMode, IdeationSessionId, InternalStatus, MergeStrategy, PlanBranchStatus, Project,
    ProjectId, Task,
};
use crate::domain::repositories::{BranchUpdateRepository, PlanBranchRepository};
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::{State, TransitionHandler};
use crate::infrastructure::memory::MemoryPlanBranchRepository;

// ──────────────────────────────────────────────────────────────────────────────
// Shared helper
// ──────────────────────────────────────────────────────────────────────────────

/// Build TaskServices with a retained Arc<MockChatService> and plan branch repo.
fn make_services_with_chat_and_plan_repo(
    task_repo: Arc<MemoryTaskRepository>,
    project_repo: Arc<MemoryProjectRepository>,
    plan_branch_repo: Arc<MemoryPlanBranchRepository>,
) -> (
    Arc<MockChatService>,
    Arc<dyn BranchUpdateRepository>,
    TaskServices,
) {
    let chat_service = Arc::new(MockChatService::new());
    let branch_update_repo = crate::testing::memory_branch_update_repository_with_task_repository(
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
    );
    let services = TaskServices::new(
        Arc::new(MockAgentSpawner::new()) as Arc<dyn AgentSpawner>,
        Arc::new(MockEventEmitter::new()) as Arc<dyn EventEmitter>,
        Arc::new(MockNotifier::new()) as Arc<dyn Notifier>,
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(MockReviewStarter::new()) as Arc<dyn ReviewStarter>,
        Arc::clone(&chat_service) as Arc<dyn ChatService>,
    )
    .with_branch_update_repo(Arc::clone(&branch_update_repo))
    .with_branch_update_workflow(crate::testing::branch_update_workflow(
        Arc::clone(&chat_service) as Arc<dyn ChatService>,
    ))
    .with_task_scheduler(Arc::new(MockTaskScheduler::new()) as Arc<dyn TaskScheduler>)
    .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
    .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
    .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);
    (chat_service, branch_update_repo, services)
}

/// Create a real git repo where a plan branch conflicts with main.
///
/// Layout:
///   main: README.md, feature.rs (task branch), shared.rs = "main version"
///   plan/feature-1: branched from main before shared.rs commit, has shared.rs = "plan version"
///   task/test-task-branch: branched from plan, has feature.rs
///
/// When merging main into plan/feature-1, shared.rs will conflict.
fn setup_plan_conflict_repo() -> (RealGitRepo, String, tempfile::TempDir) {
    let repo = setup_real_git_repo(); // main + task/test-task-branch with feature.rs
    let path = repo.path();

    // Create plan branch from main (before the conflicting commit)
    let _ = std::process::Command::new("git")
        .args(["branch", "plan/feature-1"])
        .current_dir(path)
        .output()
        .expect("git branch plan/feature-1");

    // Add conflicting commit to main: shared.rs with "main version"
    std::fs::write(path.join("shared.rs"), "// main version\nfn shared() {}").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "shared.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "add shared.rs on main"])
        .current_dir(path)
        .output();

    // Add conflicting commit to plan branch: shared.rs with "plan version"
    let _ = std::process::Command::new("git")
        .args(["checkout", "plan/feature-1"])
        .current_dir(path)
        .output();
    std::fs::write(
        path.join("shared.rs"),
        "// plan version\nfn shared() { todo!() }",
    )
    .unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "shared.rs"])
        .current_dir(path)
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "add shared.rs on plan"])
        .current_dir(path)
        .output();

    // Back to main
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output();

    // Create a temp dir for worktrees (avoids polluting ~/ralphx-worktrees)
    let worktree_parent = tempfile::TempDir::new().expect("create worktree parent dir");

    (repo, "plan/feature-1".to_string(), worktree_parent)
}

// ──────────────────────────────────────────────────────────────────────────────
// Plan update conflict creates an operation worktree and spawns the branch updater
// ──────────────────────────────────────────────────────────────────────────────

/// Plan branch update conflict → plan-update-* worktree retained → branch updater spawned.
///
/// Orchestration chain:
///   on_enter(PendingMerge)
///   → attempt_programmatic_merge
///   → resolve_merge_branches → target = plan/feature-1 (not main)
///   → dedicated update authority creates plan-update-{task_id}
///   → programmatic update reports conflicts and retains that worktree
///   → task.status = UpdatingPlanBranch
///   → branch updater receives the exact operation workspace
#[tokio::test]
async fn plan_update_conflict_retains_operation_worktree_and_spawns_branch_updater() {
    let (git_repo, plan_branch, worktree_parent) = setup_plan_conflict_repo();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(
        project_id.clone(),
        "RC#6 plan update conflict test".to_string(),
    );
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(git_repo.task_branch.clone());
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = Project::new("test-project".to_string(), git_repo.path_string());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project.git_mode = GitMode::Worktree;
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().to_string());
    project_repo.create(project).await.unwrap();

    // Plan branch: Active, session_id matches task.ideation_session_id
    let pb = make_plan_branch("artifact-1", &plan_branch, PlanBranchStatus::Active, None);
    plan_branch_repo.create(pb).await.unwrap();

    let (chat_service, branch_update_repo, services) = make_services_with_chat_and_plan_repo(
        Arc::clone(&task_repo),
        Arc::clone(&project_repo),
        Arc::clone(&plan_branch_repo),
    );
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();

    // Verify: the conflict remains in the truthful dedicated update state.
    assert_eq!(
        updated.internal_status,
        InternalStatus::UpdatingPlanBranch,
        "Plan update conflict must transition to UpdatingPlanBranch. Got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );

    // Verify: metadata has plan_update_conflict=true
    let meta: serde_json::Value =
        serde_json::from_str(updated.metadata.as_deref().unwrap_or("{}")).unwrap();
    assert_eq!(
        meta.get("plan_update_conflict"),
        Some(&serde_json::json!(true)),
        "Metadata must have plan_update_conflict=true. Metadata: {:?}",
        updated.metadata,
    );

    // The operation, not the task's normal execution workspace, owns the conflict worktree.
    assert!(updated.worktree_path.is_none());
    let operation = branch_update_repo
        .get_active_operation(&task_id)
        .await
        .unwrap()
        .expect("dedicated update state must have an active operation");
    let wt_path = operation
        .workspace_path
        .as_ref()
        .expect("branch update operation must own its worktree")
        .to_string_lossy();
    assert!(
        wt_path.contains("plan-update-"),
        "worktree_path must contain 'plan-update-' prefix. Got: {}",
        wt_path,
    );

    // Verify: the operation worktree directory actually exists on disk.
    let wt_dir = std::path::PathBuf::from(wt_path.as_ref());
    assert!(
        wt_dir.exists(),
        "Plan-update worktree directory must exist on disk. Path: {}",
        wt_path,
    );

    // Verify: branch updater was spawned (chat_service was called).
    assert!(
        chat_service.call_count() >= 1,
        "Plan update conflict must spawn a branch updater (call_count={}).",
        chat_service.call_count(),
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Stale plan-update worktree is cleaned before the operation workspace is recreated
// ──────────────────────────────────────────────────────────────────────────────

/// Verify that a stale plan-update-* worktree from a prior attempt is cleaned up
/// before recreating the operation worktree. Without cleanup, git would refuse to
/// create a second worktree for the same branch.
#[tokio::test]
async fn stale_plan_update_worktree_is_cleaned_before_operation_retry() {
    let (git_repo, plan_branch, worktree_parent) = setup_plan_conflict_repo();

    // Pre-create a stale plan-update worktree (simulates a prior failed attempt)
    let slug = "test-project";
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "RC#6 stale cleanup test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(git_repo.task_branch.clone());
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    let task_id_str = task_id.as_str().to_string();
    task_repo.create(task).await.unwrap();

    // Create stale plan-update worktree with plan branch checked out
    let stale_wt_path = worktree_parent
        .path()
        .join(slug)
        .join(format!("plan-update-{}", task_id_str));
    std::fs::create_dir_all(stale_wt_path.parent().unwrap()).unwrap();
    let stale_output = std::process::Command::new("git")
        .args([
            "worktree",
            "add",
            stale_wt_path.to_str().unwrap(),
            &plan_branch,
        ])
        .current_dir(git_repo.path())
        .output()
        .expect("create stale plan-update worktree");
    assert!(
        stale_output.status.success(),
        "Stale plan-update worktree creation must succeed: {}",
        String::from_utf8_lossy(&stale_output.stderr),
    );
    assert!(
        stale_wt_path.exists(),
        "Stale plan-update worktree must exist"
    );

    let mut project = Project::new("test-project".to_string(), git_repo.path_string());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::Merge;
    project.git_mode = GitMode::Worktree;
    project.worktree_parent_directory = Some(worktree_parent.path().to_string_lossy().to_string());
    project_repo.create(project).await.unwrap();

    let pb = make_plan_branch("artifact-1", &plan_branch, PlanBranchStatus::Active, None);
    plan_branch_repo.create(pb).await.unwrap();

    let (chat_service, branch_update_repo, services) = make_services_with_chat_and_plan_repo(
        Arc::clone(&task_repo),
        Arc::clone(&project_repo),
        Arc::clone(&plan_branch_repo),
    );
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();

    // Verify: task reaches the dedicated update state despite the stale worktree.
    assert_eq!(
        updated.internal_status,
        InternalStatus::UpdatingPlanBranch,
        "Must transition to UpdatingPlanBranch even with a stale plan-update worktree. Got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );

    // Verify: the recreated operation-owned plan-update-* worktree exists.
    assert!(updated.worktree_path.is_none());
    let operation = branch_update_repo
        .get_active_operation(&task_id)
        .await
        .unwrap()
        .expect("dedicated update state must have an active operation");
    let wt_path = operation
        .workspace_path
        .as_ref()
        .expect("branch update operation must own its worktree");
    let wt_dir = std::path::PathBuf::from(wt_path);
    assert!(
        wt_dir.exists(),
        "plan-update-* worktree must exist after stale cleanup. Path: {}",
        wt_path.display(),
    );

    // Verify: branch updater was spawned.
    assert!(
        chat_service.call_count() >= 1,
        "Branch updater must be spawned after stale worktree cleanup (call_count={})",
        chat_service.call_count(),
    );
}

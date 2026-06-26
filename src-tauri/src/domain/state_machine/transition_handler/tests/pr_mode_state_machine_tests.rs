// PR-mode state machine integration tests
//
// Tests for Phase 3 PR integration: PendingMerge + Merging + Merged paths
// when pr_eligible=true and GithubServiceTrait is wired.
//
// Covered scenarios:
//   1. PR-mode with existing pr_number: push_branch + mark_pr_ready, no create_draft_pr
//   2. PR-mode without pr_number: create_draft_pr + mark_pr_ready
//   3. pr_eligible=false: skips PR path entirely (no github calls)
//   4. Re-entry guard: pr_polling_active=true, no registry → proceeds normally
//   5. AD14: PR-polling task in Merging does not block a second PendingMerge task
//   6. post_merge_cleanup idempotency: plan_branch.status == Merged → early return

use super::helpers::*;
use crate::application::PrPollerRegistry;
use crate::domain::entities::plan_branch::{PrPushStatus, PrStatus};
use crate::domain::entities::{
    types::IdeationSessionId, Artifact, ArtifactId, ArtifactType, IdeationSession, InternalStatus,
    PlanBranch, PlanBranchStatus, Project, ProjectId, Task, TaskCategory, TaskId,
};
use crate::domain::repositories::{
    ArtifactRepository, IdeationSessionRepository, PlanBranchRepository, ProjectRepository,
    TaskRepository,
};
use crate::domain::services::{github_service::GithubServiceTrait, PlanPrDescriptionDrafter};
use crate::domain::state_machine::transition_handler::{
    complete_merge_internal_with_pr_sync, PlanBranchPrSyncServices,
};
use crate::domain::state_machine::{State, TransitionHandler};
use crate::infrastructure::memory::{
    MemoryArtifactRepository, MemoryIdeationSessionRepository, MemoryPlanBranchRepository,
    MemoryProjectRepository, MemoryTaskRepository,
};
use crate::tests::mock_github_service::MockGithubService;
use crate::AppError;
use async_trait::async_trait;

// ─────────────────────────────────────────────────────────────────────────────
// Shared helpers
// ─────────────────────────────────────────────────────────────────────────────

fn make_pr_eligible_plan_branch(
    task_id: &TaskId,
    pr_number: Option<i64>,
    pr_polling_active: bool,
) -> PlanBranch {
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("artifact-1".to_string()),
        IdeationSessionId::from_string("sess-1".to_string()),
        ProjectId::from_string("proj-1".to_string()),
        "plan/feature-branch".to_string(),
        "main".to_string(), // source_branch = base branch (PR target)
    );
    pb.merge_task_id = Some(task_id.clone());
    pb.pr_eligible = true;
    pb.pr_number = pr_number;
    pb.pr_polling_active = pr_polling_active;
    pb
}

async fn setup_project(project_repo: &MemoryProjectRepository) {
    setup_project_with_path(project_repo, "/tmp/pr-mode-test".to_string()).await;
}

async fn setup_project_with_path(
    project_repo: &MemoryProjectRepository,
    working_directory: String,
) {
    let mut project = Project::new("test-project".to_string(), working_directory);
    project.id = ProjectId::from_string("proj-1".to_string());
    project.base_branch = Some("main".to_string());
    project_repo.create(project).await.unwrap();
}

fn setup_plan_git_repo(branch_name: &str, ahead_of_base: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path();

    std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .output()
        .expect("git init");
    std::process::Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output()
        .expect("set git email");
    std::process::Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(path)
        .output()
        .expect("set git name");

    std::fs::write(path.join("README.md"), "# pr mode state machine repo\n").expect("write README");
    std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(path)
        .output()
        .expect("git add");
    std::process::Command::new("git")
        .args(["commit", "-m", "initial commit"])
        .current_dir(path)
        .output()
        .expect("initial commit");

    std::process::Command::new("git")
        .args(["checkout", "-b", branch_name])
        .current_dir(path)
        .output()
        .expect("create plan branch");
    if ahead_of_base {
        std::fs::write(path.join("plan.txt"), "plan branch work\n").expect("write plan file");
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(path)
            .output()
            .expect("git add plan file");
        std::process::Command::new("git")
            .args(["commit", "-m", "plan branch work"])
            .current_dir(path)
            .output()
            .expect("plan branch commit");
    }
    std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(path)
        .output()
        .expect("checkout main");

    dir
}

async fn create_pending_merge_task(task_repo: &MemoryTaskRepository, task_id_str: &str) -> TaskId {
    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "PR merge task".to_string(),
    );
    task.id = TaskId::from_string(task_id_str.to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.category = TaskCategory::PlanMerge;
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();
    task_id
}

#[derive(Default)]
struct StaticPlanPrDescriptionDrafter {
    calls: std::sync::Mutex<Vec<(String, String)>>,
}

#[async_trait]
impl PlanPrDescriptionDrafter for StaticPlanPrDescriptionDrafter {
    async fn draft_plan_description(
        &self,
        _project: &Project,
        plan_branch: &PlanBranch,
        review_base: &str,
    ) -> Option<String> {
        self.calls
            .lock()
            .expect("drafter calls lock should not be poisoned")
            .push((plan_branch.branch_name.clone(), review_base.to_string()));
        Some("## Summary\n\nDrafted by plan PR describer".to_string())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: PR-mode with existing pr_number → push_branch + mark_pr_ready
// ─────────────────────────────────────────────────────────────────────────────

/// PR-mode: plan_branch has pr_number=42.
/// Expected: push_branch(42) + mark_pr_ready(42) called, create_draft_pr NOT called.
#[tokio::test]
async fn test_pr_mode_with_existing_pr_number_calls_push_and_mark_ready() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;
    let task_id = create_pending_merge_task(&task_repo, "task-pr-existing").await;

    let pb = make_pr_eligible_plan_branch(&task_id, Some(42), false);
    plan_branch_repo.create(pb).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>);

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(
            state.push_branch_calls, 1,
            "push_branch should be called once"
        );
        assert_eq!(
            state.mark_pr_ready_calls, 1,
            "mark_pr_ready should be called once"
        );
        assert_eq!(
            state.update_pr_details_calls, 1,
            "PR details should be refreshed before marking ready"
        );
        assert_eq!(
            state.create_draft_pr_calls, 0,
            "create_draft_pr should NOT be called when pr_number already exists"
        );
    }
    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::WaitingOnPr,
        "PR-backed final merge should wait on the GitHub PR instead of entering local Merging"
    );
}

#[tokio::test]
async fn test_pr_mode_with_existing_pr_number_push_failure_stays_merge_incomplete() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;
    let task_id = create_pending_merge_task(&task_repo, "task-pr-existing-push-fails").await;

    let pb = make_pr_eligible_plan_branch(&task_id, Some(42), false);
    let plan_branch_id = pb.id.clone();
    plan_branch_repo.create(pb).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    mock_github.state().push_branch_result =
        Some(Err(AppError::GitOperation("push rejected".to_string())));

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>);

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should handle PR push failure visibly: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(state.push_branch_calls, 1);
        assert_eq!(
            state.mark_pr_ready_calls, 0,
            "PRs must not be marked ready when the branch push failed"
        );
    }

    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::MergeIncomplete,
        "failed PR branch publication must not advance to WaitingOnPr"
    );

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_plan_branch.pr_push_status,
        PrPushStatus::Failed,
        "failed push should be durable on the plan branch"
    );
}

#[tokio::test]
async fn test_pr_mode_marks_concurrently_created_pr_ready_after_guard_clears() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let repo = setup_plan_git_repo("plan/feature-branch", true);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;
    let task_id = create_pending_merge_task(&task_repo, "task-pr-concurrent-ready").await;

    let pb = make_pr_eligible_plan_branch(&task_id, None, false);
    let plan_branch_id = pb.id.clone();
    plan_branch_repo.create(pb).await.unwrap();

    let registry = Arc::new(PrPollerRegistry::new(
        None,
        Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>,
    ));
    registry
        .pr_creation_guard
        .insert(plan_branch_id.clone(), ());

    let repo_for_concurrent = Arc::clone(&plan_branch_repo);
    let guard_for_concurrent = Arc::clone(&registry.pr_creation_guard);
    let branch_id_for_concurrent = plan_branch_id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        repo_for_concurrent
            .update_pr_info(
                &branch_id_for_concurrent,
                314,
                "https://github.com/owner/repo/pull/314".to_string(),
                PrStatus::Open,
                true,
            )
            .await
            .expect("concurrent PR info update should succeed");
        guard_for_concurrent.remove(&branch_id_for_concurrent);
    });

    let mock_github = Arc::new(MockGithubService::new());

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_pr_poller_registry(Arc::clone(&registry))
        .with_pr_creation_guard(Arc::clone(&registry.pr_creation_guard))
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>);

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(state.push_branch_calls, 1);
        assert_eq!(state.mark_pr_ready_calls, 1);
        assert_eq!(state.update_pr_details_calls, 1);
        assert_eq!(
            state.create_draft_pr_calls, 0,
            "handler should reuse the concurrently-created PR instead of creating another"
        );
    }

    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    assert_eq!(updated_task.internal_status, InternalStatus::WaitingOnPr);

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&plan_branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_number, Some(314));
    assert_eq!(updated_plan_branch.pr_push_status, PrPushStatus::Pushed);
}

#[tokio::test]
async fn test_pr_mode_with_existing_pr_number_uses_drafted_description_override() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;
    let task_id = create_pending_merge_task(&task_repo, "task-pr-existing-drafted").await;

    let pb = make_pr_eligible_plan_branch(&task_id, Some(42), false);
    plan_branch_repo.create(pb).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let drafter = Arc::new(StaticPlanPrDescriptionDrafter::default());
    let drafter_trait: Arc<dyn PlanPrDescriptionDrafter> = drafter.clone();

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>)
        .with_plan_pr_description_drafter(drafter_trait);

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed: {:?}",
        result
    );

    let calls = drafter
        .calls
        .lock()
        .expect("drafter calls lock should not be poisoned")
        .clone();
    assert_eq!(
        calls,
        vec![("plan/feature-branch".to_string(), "main".to_string())],
        "drafter should receive the plan branch and resolved PR base"
    );

    let state = mock_github.state();
    assert_eq!(
        state.update_pr_details_calls, 1,
        "PR details should be refreshed with the drafted description"
    );
    let body = state
        .last_update_pr_details_body
        .as_deref()
        .expect("updated PR body should be captured");
    assert!(body.starts_with("## Summary\n\nDrafted by plan PR describer"));
    assert!(body.contains("## RalphX Status"));
    assert!(body.contains("<summary>View full plan</summary>"));
    assert!(body.contains("_Generated by [RalphX]("));
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: PR-mode without pr_number → creates new PR
// ─────────────────────────────────────────────────────────────────────────────

/// PR-mode: plan_branch has no pr_number yet.
/// Expected: push_branch called, create_draft_pr called (returns pr#99), mark_pr_ready called.
#[tokio::test]
async fn test_pr_mode_without_pr_number_creates_new_pr() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/feature-branch";
    let repo = setup_plan_git_repo(branch_name, true);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;
    let task_id = create_pending_merge_task(&task_repo, "task-pr-new").await;

    // No pr_number — should trigger PR creation path
    let pb = make_pr_eligible_plan_branch(&task_id, None, false);
    plan_branch_repo.create(pb).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    mock_github.will_create_pr(99, "https://github.com/owner/repo/pull/99");

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>);

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(
            state.push_branch_calls, 1,
            "push_branch should be called once"
        );
        assert_eq!(
            state.create_draft_pr_calls, 1,
            "create_draft_pr should be called when pr_number is absent"
        );
        assert_eq!(
            state.mark_pr_ready_calls, 1,
            "mark_pr_ready should be called after creation"
        );
        assert_eq!(
            state.update_pr_details_calls, 1,
            "newly-created final PR should be refreshed with ready-state title/body before marking ready"
        );
    } // drop MutexGuard before await

    // Verify pr_info was stored in plan branch repo
    let updated_pb = plan_branch_repo
        .get_by_merge_task_id(&task_id)
        .await
        .unwrap()
        .expect("plan branch should still exist");
    assert_eq!(
        updated_pb.pr_number,
        Some(99),
        "pr_number should be persisted after creation"
    );
    let updated_task = task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should exist");
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::WaitingOnPr,
        "newly-created final PR should put the merge task into WaitingOnPr"
    );
}

#[tokio::test]
async fn test_pr_mode_without_pr_number_skips_empty_plan_branch() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/feature-branch";
    let repo = setup_plan_git_repo(branch_name, false);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;
    let task_id = create_pending_merge_task(&task_repo, "task-pr-empty").await;

    let pb = make_pr_eligible_plan_branch(&task_id, None, false);
    plan_branch_repo.create(pb).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>);

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed: {:?}",
        result
    );

    let state = mock_github.state();
    assert_eq!(
        state.push_branch_calls, 0,
        "empty plan branch should not be pushed to GitHub"
    );
    assert_eq!(
        state.create_draft_pr_calls, 0,
        "empty plan branch should not create a PR in PendingMerge"
    );
    assert_eq!(
        state.mark_pr_ready_calls, 0,
        "empty plan branch should not enter the PR-ready flow"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: pr_eligible=false → skips PR path, no github calls
// ─────────────────────────────────────────────────────────────────────────────

/// When pr_eligible=false, the PR fork is not taken.
/// The push-to-main path runs instead (fails fast on nonexistent dir).
/// No GitHub service calls should be made.
#[tokio::test]
async fn test_pr_eligible_false_skips_pr_path() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;
    let task_id = create_pending_merge_task(&task_repo, "task-push-to-main").await;

    // pr_eligible = false → should NOT trigger PR path
    let mut pb = make_pr_eligible_plan_branch(&task_id, Some(42), false);
    pb.pr_eligible = false;
    plan_branch_repo.create(pb).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>);

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed: {:?}",
        result
    );

    let state = mock_github.state();
    assert_eq!(
        state.push_branch_calls, 0,
        "push_branch should NOT be called when pr_eligible=false"
    );
    assert_eq!(
        state.mark_pr_ready_calls, 0,
        "mark_pr_ready should NOT be called when pr_eligible=false"
    );
    assert_eq!(
        state.create_draft_pr_calls, 0,
        "create_draft_pr should NOT be called when pr_eligible=false"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4: Re-entry guard — pr_polling_active=true, no registry → proceeds
// ─────────────────────────────────────────────────────────────────────────────

/// pr_polling_active=true but no PrPollerRegistry wired.
/// The re-entry guard only triggers when BOTH flags are set AND registry.is_polling().
/// Without a registry, guard is skipped → PR operations proceed normally.
#[tokio::test]
async fn test_pr_mode_reentry_guard_no_registry_proceeds() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;
    let task_id = create_pending_merge_task(&task_repo, "task-reentry").await;

    // pr_polling_active=true simulates a previous run that set the flag.
    // Without a registry, is_polling() can't be checked → guard bypassed.
    let pb = make_pr_eligible_plan_branch(&task_id, Some(77), true);
    plan_branch_repo.create(pb).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>);
    // NOTE: no .with_pr_poller_registry() — guard must be bypassed

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed: {:?}",
        result
    );

    // With no registry, re-entry guard doesn't fire → operations proceed
    let state = mock_github.state();
    assert_eq!(
        state.push_branch_calls, 1,
        "push_branch should be called when no registry prevents re-entry"
    );
    assert_eq!(
        state.mark_pr_ready_calls, 1,
        "mark_pr_ready should be called when no registry prevents re-entry"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5: AD14 — PR-polling task in Merging does not block second PendingMerge task
// ─────────────────────────────────────────────────────────────────────────────

/// AD14: find_blocking_merge_task skips tasks whose merge_task_id is in pr_polling_ids.
///
/// Setup:
///   - Task A: in Merging, plan_branch has pr_polling_active=true
///   - Task B: in PendingMerge (pr_eligible=true, pr_number=55), test subject
///
/// Without AD14, Task A (Merging) would block Task B (PendingMerge) from proceeding.
/// With AD14, Task A is excluded from blocking → Task B proceeds → push_branch called.
#[tokio::test]
async fn test_ad14_pr_polling_task_does_not_block_pending_merge() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;

    // Task A: in Merging, is a PR-polling task
    let mut task_a = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Task A (merging)".to_string(),
    );
    task_a.id = TaskId::from_string("task-a-merging".to_string());
    task_a.internal_status = InternalStatus::Merging;
    task_a.category = TaskCategory::PlanMerge;
    let task_a_id = task_a.id.clone();
    task_repo.create(task_a).await.unwrap();

    // Plan branch for Task A: pr_polling_active=true makes it excluded by AD14
    let mut pb_a = PlanBranch::new(
        ArtifactId::from_string("artifact-a".to_string()),
        IdeationSessionId::from_string("sess-1".to_string()),
        ProjectId::from_string("proj-1".to_string()),
        "plan/branch-a".to_string(),
        "main".to_string(),
    );
    pb_a.merge_task_id = Some(task_a_id.clone());
    pb_a.pr_eligible = true;
    pb_a.pr_number = Some(10);
    pb_a.pr_polling_active = true;
    plan_branch_repo.create(pb_a).await.unwrap();

    // Task B: in PendingMerge — this is what we're testing
    let task_b_id = create_pending_merge_task(&task_repo, "task-b-pending").await;

    // Plan branch for Task B: pr_eligible=true, pr_number=55
    let pb_b = make_pr_eligible_plan_branch(&task_b_id, Some(55), false);
    plan_branch_repo.create(pb_b).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>);

    let context = TaskContext::new(task_b_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) should succeed: {:?}",
        result
    );

    // Task B should proceed (not deferred by Task A) and call push_branch
    let state = mock_github.state();
    assert_eq!(
        state.push_branch_calls, 1,
        "Task B should proceed despite Task A being in Merging (AD14 excludes PR-polling tasks)"
    );
    assert_eq!(
        state.mark_pr_ready_calls, 1,
        "Task B should mark PR ready after proceeding"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6: post_merge_cleanup idempotency — Merged plan branch returns early
// ─────────────────────────────────────────────────────────────────────────────

/// on_enter(Merged) calls post_merge_cleanup for PlanMerge tasks.
/// If plan_branch.status is already Merged, the cleanup returns early (idempotency guard).
/// Test verifies: no error, no infinite loop, expected guard branch executes.
#[tokio::test]
async fn test_post_merge_cleanup_idempotency_already_merged_plan_branch() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;

    // Task in Merged state (simulating successful merge)
    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Merged task".to_string(),
    );
    task.id = TaskId::from_string("task-already-merged".to_string());
    task.internal_status = InternalStatus::Merged;
    task.category = TaskCategory::PlanMerge;
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    // Plan branch already in Merged status — idempotency guard should trigger
    let mut pb = make_pr_eligible_plan_branch(&task_id, Some(88), false);
    pb.status = PlanBranchStatus::Merged;
    plan_branch_repo.create(pb).await.unwrap();

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // on_enter(Merged) → post_merge_cleanup → idempotency guard → early return
    let result = handler.on_enter(&State::Merged).await;
    assert!(
        result.is_ok(),
        "on_enter(Merged) with already-merged plan branch should succeed without error: {:?}",
        result
    );

    // Plan branch status should remain Merged (not double-transitioned)
    let pb_after = plan_branch_repo
        .get_by_merge_task_id(&task_id)
        .await
        .unwrap()
        .expect("plan branch should still exist");
    assert_eq!(
        pb_after.status,
        PlanBranchStatus::Merged,
        "plan branch should still be Merged after idempotent cleanup"
    );
}

#[tokio::test]
async fn test_regular_plan_task_merged_state_creates_draft_pr_after_first_merge() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let session_repo = Arc::new(MemoryIdeationSessionRepository::new());
    let artifact_repo = Arc::new(MemoryArtifactRepository::new());

    let branch_name = "plan/feature-branch";
    let repo = setup_plan_git_repo(branch_name, true);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;

    let session = IdeationSession::new_with_title(
        ProjectId::from_string("proj-1".to_string()),
        "Fix graph crash when no active plan selected",
    );
    let session_id = session.id.clone();
    session_repo.create(session).await.unwrap();

    let plan_artifact = Artifact::new_inline(
        "Execution Plan",
        ArtifactType::Specification,
        "## Goal\n\n- Preserve the empty state\n- Thread `executionPlanId` through the timeline components\n",
        "ralphx-plan",
    );
    let plan_artifact_id = plan_artifact.id.clone();
    artifact_repo.create(plan_artifact).await.unwrap();

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Merged plan task".to_string(),
    );
    task.id = TaskId::from_string("task-plan-merged".to_string());
    task.internal_status = InternalStatus::Merged;
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(session_id.clone());
    task.plan_artifact_id = Some(plan_artifact_id.clone());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut plan_branch = make_plan_branch(
        plan_artifact_id.as_str(),
        branch_name,
        PlanBranchStatus::Active,
        None,
    );
    plan_branch.session_id = session_id;
    plan_branch.pr_eligible = true;
    let branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    mock_github.will_create_pr(123, "https://github.com/owner/repo/pull/123");

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_ideation_session_repo(Arc::clone(&session_repo) as Arc<dyn IdeationSessionRepository>)
        .with_artifact_repo(Arc::clone(&artifact_repo) as Arc<dyn ArtifactRepository>)
        .with_pr_creation_guard(Arc::new(dashmap::DashMap::new()))
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>);

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::Merged).await;
    assert!(
        result.is_ok(),
        "on_enter(Merged) should succeed: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(
            state.push_branch_calls, 1,
            "first merged plan task should push the plan branch"
        );
        assert_eq!(
            state.create_draft_pr_calls, 1,
            "first merged plan task should create the draft PR once the plan branch has reviewable changes"
        );
    }

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_number, Some(123));

    let state = mock_github.state();
    let (_, _, title, _) = state
        .last_create_draft_pr_args
        .clone()
        .expect("expected draft PR arguments to be recorded");
    assert_eq!(title, "Plan: Fix graph crash when no active plan selected");

    let body = state
        .last_create_draft_pr_body
        .clone()
        .expect("expected draft PR body to be captured");
    assert!(body.contains("## RalphX Status"));
    assert!(body.contains("Draft while RalphX is still merging plan tasks"));
    assert!(body.contains("## How To Review"));
    assert!(body.contains("Merge this PR in GitHub"));
    assert!(body.contains("## Plan"));
    assert!(body.contains("<details>"));
    assert!(body.contains("<summary>View full plan</summary>"));
    assert!(body.contains("Current RalphX task"));
    assert!(body.contains("Merged plan task"));
    assert!(body.contains("Thread `executionPlanId` through the timeline components"));
    assert!(body.contains("</details>"));
    assert!(body.contains("Generated by [RalphX](https://github.com/aigentive/ralphx.app)"));
    assert!(!body.contains("## Delivered Changes"));
    assert!(!body.contains("Changed files"));

    assert_eq!(
        state.update_pr_details_calls, 1,
        "single-task plan should refresh PR details before marking ready"
    );
    assert_eq!(
        state.mark_pr_ready_calls, 1,
        "single-task plan should mark the PR ready immediately after creating it"
    );
    let (updated_pr_number, updated_title, _) = state
        .last_update_pr_details_args
        .clone()
        .expect("expected ready PR update arguments");
    assert_eq!(updated_pr_number, 123);
    assert_eq!(
        updated_title,
        "Fix graph crash when no active plan selected"
    );
    let ready_body = state
        .last_update_pr_details_body
        .clone()
        .expect("expected ready PR body");
    assert!(ready_body.contains("Ready for GitHub review"));
    assert!(ready_body.contains("<details>"));
    assert!(ready_body.contains("<summary>View full plan</summary>"));
    assert!(!ready_body.contains("opened this draft PR"));
}

#[tokio::test]
async fn test_regular_plan_task_completion_creates_draft_pr_after_first_local_merge() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/feature-branch";
    let repo = setup_plan_git_repo(branch_name, true);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;
    let project = project_repo
        .get_by_id(&ProjectId::from_string("proj-1".to_string()))
        .await
        .unwrap()
        .unwrap();

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Merged by programmatic merge".to_string(),
    );
    task.id = TaskId::from_string("task-programmatic-plan-merge".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    task.plan_artifact_id = Some(ArtifactId::from_string("artifact-1".to_string()));
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut plan_branch =
        make_plan_branch("artifact-1", branch_name, PlanBranchStatus::Active, None);
    plan_branch.pr_eligible = true;
    let branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let commit_output = std::process::Command::new("git")
        .args(["rev-parse", branch_name])
        .current_dir(repo.path())
        .output()
        .expect("read plan branch sha");
    assert!(
        commit_output.status.success(),
        "rev-parse plan branch should succeed"
    );
    let commit_sha = String::from_utf8_lossy(&commit_output.stdout)
        .trim()
        .to_string();

    let mock_github = Arc::new(MockGithubService::new());
    mock_github.will_create_pr(456, "https://github.com/owner/repo/pull/456");

    let mut task_for_merge = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let task_repo_dyn: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
    let result = complete_merge_internal_with_pr_sync::<tauri::Wry>(
        &mut task_for_merge,
        &project,
        &commit_sha,
        "task/feature",
        branch_name,
        &task_repo_dyn,
        None,
        None,
        None,
        None,
        Some(PlanBranchPrSyncServices {
            task_repo: Some(Arc::clone(&task_repo) as Arc<dyn TaskRepository>),
            plan_branch_repo: Some(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>),
            pr_creation_guard: Some(Arc::new(dashmap::DashMap::new())),
            github_service: Some(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>),
            ideation_session_repo: None,
            artifact_repo: None,
        }),
    )
    .await;
    assert!(
        result.is_ok(),
        "complete_merge_internal_with_pr_sync should succeed: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(
            state.push_branch_calls, 1,
            "programmatic local plan-task merge should push the plan branch"
        );
        assert_eq!(
            state.create_draft_pr_calls, 1,
            "programmatic local plan-task merge should create the first draft PR"
        );
    }

    let final_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(final_task.internal_status, InternalStatus::Merged);

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_number, Some(456));
}

#[tokio::test]
async fn test_regular_plan_task_completion_pushes_existing_pr_after_local_merge() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/existing-pr-branch";
    let repo = setup_plan_git_repo(branch_name, true);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;
    let project = project_repo
        .get_by_id(&ProjectId::from_string("proj-1".to_string()))
        .await
        .unwrap()
        .unwrap();

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Merged follow-up via programmatic merge".to_string(),
    );
    task.id = TaskId::from_string("task-programmatic-plan-pr-sync".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut plan_branch =
        make_plan_branch("artifact-1", branch_name, PlanBranchStatus::Active, None);
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(789);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/789".to_string());
    plan_branch.pr_push_status = crate::domain::entities::plan_branch::PrPushStatus::Pushed;
    let branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let commit_output = std::process::Command::new("git")
        .args(["rev-parse", branch_name])
        .current_dir(repo.path())
        .output()
        .expect("read plan branch sha");
    assert!(
        commit_output.status.success(),
        "rev-parse plan branch should succeed"
    );
    let commit_sha = String::from_utf8_lossy(&commit_output.stdout)
        .trim()
        .to_string();

    let mock_github = Arc::new(MockGithubService::new());

    let mut task_for_merge = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let task_repo_dyn: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
    let result = complete_merge_internal_with_pr_sync::<tauri::Wry>(
        &mut task_for_merge,
        &project,
        &commit_sha,
        "task/feature",
        branch_name,
        &task_repo_dyn,
        None,
        None,
        None,
        None,
        Some(PlanBranchPrSyncServices {
            task_repo: Some(Arc::clone(&task_repo) as Arc<dyn TaskRepository>),
            plan_branch_repo: Some(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>),
            pr_creation_guard: Some(Arc::new(dashmap::DashMap::new())),
            github_service: Some(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>),
            ideation_session_repo: None,
            artifact_repo: None,
        }),
    )
    .await;
    assert!(
        result.is_ok(),
        "complete_merge_internal_with_pr_sync should succeed: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(
            state.push_branch_calls, 1,
            "programmatic local plan-task merge should push existing PR branch updates"
        );
        assert_eq!(
            state.create_draft_pr_calls, 0,
            "existing PR-backed branches should sync instead of creating another PR"
        );
        assert_eq!(
            state.last_push_branch_name.as_deref(),
            Some(branch_name),
            "push should target the plan branch"
        );
    }

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_plan_branch.pr_push_status,
        crate::domain::entities::plan_branch::PrPushStatus::Pushed
    );
}

#[tokio::test]
async fn test_regular_plan_task_completion_updates_existing_pr_as_draft_when_plan_open() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;
    let project = project_repo
        .get_by_id(&ProjectId::from_string("proj-1".to_string()))
        .await
        .unwrap()
        .unwrap();

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Merged follow-up while plan remains open".to_string(),
    );
    task.id = TaskId::from_string("task-programmatic-plan-pr-draft".to_string());
    task.internal_status = InternalStatus::Merged;
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    task_repo.create(task.clone()).await.unwrap();

    let mut open_sibling = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Still executing sibling".to_string(),
    );
    open_sibling.id = TaskId::from_string("task-programmatic-plan-open-sibling".to_string());
    open_sibling.internal_status = InternalStatus::Executing;
    open_sibling.category = TaskCategory::Regular;
    open_sibling.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    task_repo.create(open_sibling).await.unwrap();

    let branch_name = "plan/existing-pr-draft-update";
    let mut plan_branch =
        make_plan_branch("artifact-1", branch_name, PlanBranchStatus::Active, None);
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(789);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/789".to_string());
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    let branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());
    let result = super::super::merge_helpers::sync_plan_branch_pr_after_regular_task_merge(
        &task,
        &project,
        &PlanBranchPrSyncServices {
            task_repo: Some(Arc::clone(&task_repo) as Arc<dyn TaskRepository>),
            plan_branch_repo: Some(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>),
            pr_creation_guard: Some(Arc::new(dashmap::DashMap::new())),
            github_service: Some(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>),
            ideation_session_repo: None,
            artifact_repo: None,
        },
    )
    .await;

    assert!(
        result.is_ok(),
        "draft PR sync should succeed while other plan tasks remain open: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(state.push_branch_calls, 1);
        assert_eq!(
            state.update_pr_details_calls, 1,
            "existing PR should be refreshed with draft details"
        );
        assert_eq!(
            state.mark_pr_ready_calls, 0,
            "open sibling tasks should keep the existing PR in draft mode"
        );
        let (_, title, _) = state
            .last_update_pr_details_args
            .as_ref()
            .expect("update PR details args");
        assert_eq!(title, "Plan: Merged follow-up while plan remains open");
        let body = state
            .last_update_pr_details_body
            .as_ref()
            .expect("update PR details body");
        assert!(
            body.contains("Draft while RalphX is still merging plan tasks into the plan branch."),
            "existing PR body should explain why it remains draft: {}",
            body
        );
    }

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_push_status, PrPushStatus::Pushed);
}

#[tokio::test]
async fn test_regular_plan_task_completion_push_failure_does_not_mark_task_merged() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/existing-pr-push-fails";
    let repo = setup_plan_git_repo(branch_name, true);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;
    let project = project_repo
        .get_by_id(&ProjectId::from_string("proj-1".to_string()))
        .await
        .unwrap()
        .unwrap();

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Merged follow-up with failed publication".to_string(),
    );
    task.id = TaskId::from_string("task-programmatic-plan-pr-push-fails".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut plan_branch =
        make_plan_branch("artifact-1", branch_name, PlanBranchStatus::Active, None);
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(789);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/789".to_string());
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    let branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let commit_output = std::process::Command::new("git")
        .args(["rev-parse", branch_name])
        .current_dir(repo.path())
        .output()
        .expect("read plan branch sha");
    assert!(
        commit_output.status.success(),
        "rev-parse plan branch should succeed"
    );
    let commit_sha = String::from_utf8_lossy(&commit_output.stdout)
        .trim()
        .to_string();

    let mock_github = Arc::new(MockGithubService::new());
    mock_github.state().push_branch_result =
        Some(Err(AppError::GitOperation("push rejected".to_string())));

    let mut task_for_merge = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let task_repo_dyn: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
    let result = complete_merge_internal_with_pr_sync::<tauri::Wry>(
        &mut task_for_merge,
        &project,
        &commit_sha,
        "task/feature",
        branch_name,
        &task_repo_dyn,
        None,
        None,
        None,
        None,
        Some(PlanBranchPrSyncServices {
            task_repo: Some(Arc::clone(&task_repo) as Arc<dyn TaskRepository>),
            plan_branch_repo: Some(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>),
            pr_creation_guard: Some(Arc::new(dashmap::DashMap::new())),
            github_service: Some(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>),
            ideation_session_repo: None,
            artifact_repo: None,
        }),
    )
    .await;

    assert!(
        result.is_err(),
        "merge completion must fail closed when the PR branch cannot be pushed"
    );

    let final_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        final_task.internal_status,
        InternalStatus::MergeIncomplete,
        "failed PR branch publication must not mark the task Merged"
    );

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_push_status, PrPushStatus::Failed);
}

#[tokio::test]
async fn test_regular_plan_task_completion_without_github_service_does_not_mark_task_merged() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/existing-pr-no-github";
    let repo = setup_plan_git_repo(branch_name, true);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;
    let project = project_repo
        .get_by_id(&ProjectId::from_string("proj-1".to_string()))
        .await
        .unwrap()
        .unwrap();

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Merged follow-up without GitHub service".to_string(),
    );
    task.id = TaskId::from_string("task-programmatic-plan-pr-no-github".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut plan_branch =
        make_plan_branch("artifact-1", branch_name, PlanBranchStatus::Active, None);
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(789);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/789".to_string());
    plan_branch.pr_push_status = PrPushStatus::Pushed;
    let branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let commit_output = std::process::Command::new("git")
        .args(["rev-parse", branch_name])
        .current_dir(repo.path())
        .output()
        .expect("read plan branch sha");
    assert!(
        commit_output.status.success(),
        "rev-parse plan branch should succeed"
    );
    let commit_sha = String::from_utf8_lossy(&commit_output.stdout)
        .trim()
        .to_string();

    let mut task_for_merge = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    let task_repo_dyn: Arc<dyn TaskRepository> = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
    let result = complete_merge_internal_with_pr_sync::<tauri::Wry>(
        &mut task_for_merge,
        &project,
        &commit_sha,
        "task/feature",
        branch_name,
        &task_repo_dyn,
        None,
        None,
        None,
        None,
        Some(PlanBranchPrSyncServices {
            task_repo: Some(Arc::clone(&task_repo) as Arc<dyn TaskRepository>),
            plan_branch_repo: Some(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>),
            pr_creation_guard: Some(Arc::new(dashmap::DashMap::new())),
            github_service: None,
            ideation_session_repo: None,
            artifact_repo: None,
        }),
    )
    .await;

    assert!(
        result.is_err(),
        "merge completion must fail closed when an existing PR cannot be published"
    );

    let final_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        final_task.internal_status,
        InternalStatus::MergeIncomplete,
        "missing GitHub service must not mark the task Merged"
    );
    let metadata: serde_json::Value =
        serde_json::from_str(final_task.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(
        metadata["error_code"], "pr_branch_publication_failed",
        "MergeIncomplete should carry the PR publication failure reason"
    );

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_plan_branch.pr_push_status, PrPushStatus::Pending);
}

#[tokio::test]
async fn test_regular_plan_task_merged_state_pushes_existing_pr_after_local_update() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    let branch_name = "plan/feature-branch";
    let repo = setup_plan_git_repo(branch_name, true);
    setup_project_with_path(&project_repo, repo.path().to_string_lossy().into_owned()).await;

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Merged follow-up task".to_string(),
    );
    task.id = TaskId::from_string("task-plan-pr-sync".to_string());
    task.internal_status = InternalStatus::Merged;
    task.category = TaskCategory::Regular;
    task.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut plan_branch =
        make_plan_branch("artifact-1", branch_name, PlanBranchStatus::Active, None);
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(321);
    plan_branch.pr_url = Some("https://github.com/owner/repo/pull/321".to_string());
    plan_branch.pr_push_status = crate::domain::entities::plan_branch::PrPushStatus::Pushed;
    let branch_id = plan_branch.id.clone();
    plan_branch_repo.create(plan_branch).await.unwrap();

    let mock_github = Arc::new(MockGithubService::new());

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_pr_creation_guard(Arc::new(dashmap::DashMap::new()))
        .with_github_service(Arc::clone(&mock_github) as Arc<dyn GithubServiceTrait>);

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let result = handler.on_enter(&State::Merged).await;
    assert!(
        result.is_ok(),
        "on_enter(Merged) should succeed: {:?}",
        result
    );

    {
        let state = mock_github.state();
        assert_eq!(
            state.push_branch_calls, 1,
            "existing PR branches should be pushed again when new local plan-branch work lands"
        );
        assert_eq!(
            state.create_draft_pr_calls, 0,
            "existing PR branches should sync instead of recreating the PR"
        );
    }

    let updated_plan_branch = plan_branch_repo
        .get_by_id(&branch_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_plan_branch.pr_push_status,
        crate::domain::entities::plan_branch::PrPushStatus::Pushed
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7: No github_service → falls through to push-to-main path
// ─────────────────────────────────────────────────────────────────────────────

/// pr_eligible=true but no GithubServiceTrait wired.
/// pr_mode = pr_eligible && github_service.is_some() → false.
/// Falls through to push-to-main path (no PR calls).
#[tokio::test]
async fn test_pr_eligible_true_but_no_github_service_falls_through() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());

    setup_project(&project_repo).await;
    let task_id = create_pending_merge_task(&task_repo, "task-no-github-svc").await;

    let pb = make_pr_eligible_plan_branch(&task_id, Some(99), false);
    plan_branch_repo.create(pb).await.unwrap();

    // No github_service wired → pr_mode = false
    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);
    // NOTE: no .with_github_service()

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // Should run the push-to-main path (fails fast on nonexistent git dir) without PR calls
    let result = handler.on_enter(&State::PendingMerge).await;
    assert!(
        result.is_ok(),
        "on_enter(PendingMerge) without github_service should fall through gracefully: {:?}",
        result
    );
    // No assertions on MockGithubService since it was never wired
}

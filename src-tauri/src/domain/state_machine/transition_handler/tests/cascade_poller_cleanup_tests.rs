// Tests for cascade stop poller cleanup + post_merge_cleanup PR fork (AD11, AD17)
//
// Covered scenarios:
//   1. cascade_stop_sibling_tasks calls stop_polling for each sibling BEFORE persist_status_change
//   2. cascade_stop decrements running_count for PR-mode Merging siblings
//   3. post_merge_cleanup in PR mode: calls delete_remote_branch for plan branch (not delete_feature_branch)
//   4. post_merge_cleanup in PR mode: calls delete_remote_branch for each task branch
//   5. post_merge_cleanup in push-to-main mode: does NOT call delete_remote_branch (unchanged)
//   6. cascade_stop with no poller registry: no panic, cascade proceeds normally

use super::helpers::*;
use crate::application::PrPollerRegistry;
use crate::commands::ExecutionState;
use crate::domain::entities::types::IdeationSessionId;
use crate::domain::entities::{
    ArtifactId, InternalStatus, PlanBranch, PlanBranchStatus, ProjectId, Task,
};
use crate::domain::repositories::{PlanBranchRepository, ProjectRepository, TaskRepository};
use crate::domain::state_machine::TaskStateMachine;
use crate::domain::state_machine::TransitionHandler;
use crate::infrastructure::memory::{
    MemoryPlanBranchRepository, MemoryProjectRepository, MemoryTaskRepository,
};
use crate::tests::mock_github_service::MockGithubService;
use std::sync::Arc;

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn make_registry() -> Arc<PrPollerRegistry> {
    let pb_repo = Arc::new(MemoryPlanBranchRepository::new());
    Arc::new(PrPollerRegistry::new(None, pb_repo))
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: cascade_stop_sibling_tasks calls stop_polling for each sibling
// ─────────────────────────────────────────────────────────────────────────────

/// cascade_stop_sibling_tasks: for each stopped sibling, stop_polling is called.
/// We verify the sibling is in the registry's `stopping` set after cascade.
#[tokio::test]
async fn cascade_stop_calls_stop_polling_for_each_sibling() {
    let task_repo = Arc::new(MemoryTaskRepository::new());

    // Create merge task
    let mut merge_task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "merge task".into(),
    );
    merge_task.internal_status = InternalStatus::PendingMerge;
    let merge_task_id = merge_task.id.clone();
    task_repo.create(merge_task).await.unwrap();

    // Create sibling in Executing state (has task in registry — simulated by inserting to stopping
    // would require a running poller; we test that stop_polling is called by checking stopping set)
    let mut sibling = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "sibling".into(),
    );
    sibling.internal_status = InternalStatus::Executing;
    sibling.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let sibling_id = sibling.id.clone();
    task_repo.create(sibling).await.unwrap();

    let registry = make_registry();

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_pr_poller_registry(Arc::clone(&registry));

    let context = create_context_with_services(merge_task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let pb = {
        let mut pb = PlanBranch::new(
            ArtifactId::from_string("art-1".to_string()),
            IdeationSessionId::from_string("sess-1".to_string()),
            ProjectId::from_string("proj-1".to_string()),
            "plan/feature-branch".to_string(),
            "main".to_string(),
        );
        pb.merge_task_id = Some(merge_task_id.clone());
        pb.status = PlanBranchStatus::Merged;
        pb
    };

    handler
        .cascade_stop_sibling_tasks(&merge_task_id, merge_task_id.as_str(), &pb)
        .await;

    // stop_polling inserts into stopping set (AD11)
    assert!(
        registry.stopping.contains_key(&sibling_id),
        "stop_polling should have inserted sibling into stopping set before persist_status_change"
    );

    // Sibling should be Stopped (cascaded)
    let updated = task_repo.get_by_id(&sibling_id).await.unwrap().unwrap();
    assert_eq!(updated.internal_status, InternalStatus::Stopped);
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: cascade_stop decrements running_count for PR-mode Merging siblings
// ─────────────────────────────────────────────────────────────────────────────

/// cascade_stop: if a sibling is Merging AND plan_branch.pr_number is set,
/// execution_state.decrement_running() is called explicitly.
#[tokio::test]
async fn cascade_stop_decrements_running_count_for_pr_mode_merging_sibling() {
    let task_repo = Arc::new(MemoryTaskRepository::new());

    // Create merge task
    let mut merge_task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "merge task".into(),
    );
    merge_task.internal_status = InternalStatus::PendingMerge;
    let merge_task_id = merge_task.id.clone();
    task_repo.create(merge_task).await.unwrap();

    // Create sibling in Merging state (PR-mode)
    let mut merging_sibling = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "merging sibling".into(),
    );
    merging_sibling.internal_status = InternalStatus::Merging;
    merging_sibling.ideation_session_id =
        Some(IdeationSessionId::from_string("sess-1".to_string()));
    task_repo.create(merging_sibling).await.unwrap();

    let execution_state = Arc::new(ExecutionState::new());
    execution_state.increment_running(); // simulate the PR-mode Merging increment

    let initial_count = execution_state.running_count();
    assert_eq!(initial_count, 1, "precondition: running_count should be 1");

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_execution_state(Arc::clone(&execution_state));

    let context = create_context_with_services(merge_task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // PR-mode: plan_branch has pr_number
    let pb = {
        let mut pb = PlanBranch::new(
            ArtifactId::from_string("art-1".to_string()),
            IdeationSessionId::from_string("sess-1".to_string()),
            ProjectId::from_string("proj-1".to_string()),
            "plan/feature-branch".to_string(),
            "main".to_string(),
        );
        pb.merge_task_id = Some(merge_task_id.clone());
        pb.pr_eligible = true;
        pb.pr_number = Some(42);
        pb.status = PlanBranchStatus::Merged;
        pb
    };

    handler
        .cascade_stop_sibling_tasks(&merge_task_id, merge_task_id.as_str(), &pb)
        .await;

    let after_count = execution_state.running_count();
    assert_eq!(
        after_count, 0,
        "running_count should be decremented to 0 after cascade-stopping PR-mode Merging sibling"
    );
}

/// cascade_stop: if plan_branch.pr_number is None (push-to-main), running_count is NOT decremented.
#[tokio::test]
async fn cascade_stop_does_not_decrement_running_for_push_to_main_merging_sibling() {
    let task_repo = Arc::new(MemoryTaskRepository::new());

    let mut merge_task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "merge task".into(),
    );
    merge_task.internal_status = InternalStatus::PendingMerge;
    let merge_task_id = merge_task.id.clone();
    task_repo.create(merge_task).await.unwrap();

    let mut merging_sibling = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "merging sibling".into(),
    );
    merging_sibling.internal_status = InternalStatus::Merging;
    merging_sibling.ideation_session_id =
        Some(IdeationSessionId::from_string("sess-1".to_string()));
    task_repo.create(merging_sibling).await.unwrap();

    let execution_state = Arc::new(ExecutionState::new());
    execution_state.increment_running();

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_execution_state(Arc::clone(&execution_state));

    let context = create_context_with_services(merge_task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // push-to-main: plan_branch has NO pr_number
    let pb = {
        let mut pb = PlanBranch::new(
            ArtifactId::from_string("art-1".to_string()),
            IdeationSessionId::from_string("sess-1".to_string()),
            ProjectId::from_string("proj-1".to_string()),
            "plan/feature-branch".to_string(),
            "main".to_string(),
        );
        pb.merge_task_id = Some(merge_task_id.clone());
        pb.pr_eligible = false;
        pb.pr_number = None;
        pb.status = PlanBranchStatus::Merged;
        pb
    };

    handler
        .cascade_stop_sibling_tasks(&merge_task_id, merge_task_id.as_str(), &pb)
        .await;

    // running_count should NOT have been decremented by cascade_stop in push-to-main mode
    assert_eq!(
        execution_state.running_count(),
        1,
        "push-to-main: cascade_stop should NOT decrement running_count for Merging sibling"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3+4: post_merge_cleanup PR mode: delete_remote_branch for plan + task branches
// ─────────────────────────────────────────────────────────────────────────────

/// post_merge_cleanup in PR mode: calls delete_remote_branch for plan branch and each sibling task branch.
#[tokio::test]
async fn post_merge_cleanup_pr_mode_deletes_remote_plan_and_task_branches() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let github = Arc::new(MockGithubService::new());

    // Create project
    let mut project = crate::domain::entities::Project::new(
        "test-project".to_string(),
        "/tmp/pr-cleanup-test".to_string(),
    );
    project.id = ProjectId::from_string("proj-1".to_string());
    project_repo.create(project).await.unwrap();

    // Create merge task (PlanMerge category)
    let mut merge_task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "merge task".into(),
    );
    merge_task.internal_status = InternalStatus::Merging;
    merge_task.category = crate::domain::entities::TaskCategory::PlanMerge;
    let merge_task_id = merge_task.id.clone();
    task_repo.create(merge_task).await.unwrap();

    // Create sibling task with a task_branch
    let mut sibling = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "sibling".into(),
    );
    sibling.internal_status = InternalStatus::Executing;
    sibling.task_branch = Some("ralphx/ralphx/task-sibling-1".to_string());
    sibling.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    task_repo.create(sibling).await.unwrap();

    // Create PR-eligible plan branch (not yet Merged — cleanup should proceed)
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("art-1".to_string()),
        IdeationSessionId::from_string("sess-1".to_string()),
        ProjectId::from_string("proj-1".to_string()),
        "plan/pr-feature-branch".to_string(),
        "main".to_string(),
    );
    pb.merge_task_id = Some(merge_task_id.clone());
    pb.pr_eligible = true;
    pb.pr_number = Some(42);
    pb.status = PlanBranchStatus::Active; // NOT merged yet — cleanup will run
    plan_branch_repo.create(pb).await.unwrap();

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_github_service(Arc::clone(&github)
            as Arc<dyn crate::domain::services::github_service::GithubServiceTrait>);

    let context = create_context_with_services(merge_task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let repo_path = std::path::Path::new("/tmp/pr-cleanup-test");
    let pb_repo_opt = Some(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);

    handler
        .post_merge_cleanup(
            merge_task_id.as_str(),
            &merge_task_id,
            repo_path,
            &pb_repo_opt,
        )
        .await;

    let state = github.state();
    // Should have called delete_remote_branch at least for plan branch + sibling task branch
    assert!(
        state.delete_remote_branch_calls >= 2,
        "Expected at least 2 delete_remote_branch calls (plan branch + sibling task branch), got {}",
        state.delete_remote_branch_calls
    );
    assert!(
        state
            .all_deleted_remote_branch_names
            .contains(&"plan/pr-feature-branch".to_string()),
        "Plan branch should be deleted via delete_remote_branch, got: {:?}",
        state.all_deleted_remote_branch_names
    );
    assert!(
        state
            .all_deleted_remote_branch_names
            .contains(&"ralphx/ralphx/task-sibling-1".to_string()),
        "Sibling task branch should be deleted via delete_remote_branch, got: {:?}",
        state.all_deleted_remote_branch_names
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5: push-to-main mode — delete_remote_branch NOT called
// ─────────────────────────────────────────────────────────────────────────────

/// post_merge_cleanup in push-to-main mode: delete_remote_branch is NOT called
/// (delete_feature_branch handles local branch deletion, which requires git and is skipped here).
#[tokio::test]
async fn post_merge_cleanup_push_to_main_does_not_call_delete_remote_branch() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let plan_branch_repo = Arc::new(MemoryPlanBranchRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());
    let github = Arc::new(MockGithubService::new());

    let mut project = crate::domain::entities::Project::new(
        "test-project".to_string(),
        "/tmp/push-to-main-test".to_string(),
    );
    project.id = ProjectId::from_string("proj-1".to_string());
    project_repo.create(project).await.unwrap();

    let mut merge_task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "merge task".into(),
    );
    merge_task.internal_status = InternalStatus::Merging;
    merge_task.category = crate::domain::entities::TaskCategory::PlanMerge;
    let merge_task_id = merge_task.id.clone();
    task_repo.create(merge_task).await.unwrap();

    // Push-to-main: pr_eligible = false, pr_number = None
    let mut pb = PlanBranch::new(
        ArtifactId::from_string("art-1".to_string()),
        IdeationSessionId::from_string("sess-1".to_string()),
        ProjectId::from_string("proj-1".to_string()),
        "plan/push-to-main-branch".to_string(),
        "main".to_string(),
    );
    pb.merge_task_id = Some(merge_task_id.clone());
    pb.pr_eligible = false;
    pb.pr_number = None;
    pb.status = PlanBranchStatus::Active;
    plan_branch_repo.create(pb).await.unwrap();

    let services = TaskServices::new_mock()
        .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
        .with_plan_branch_repo(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>)
        .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>)
        .with_github_service(Arc::clone(&github)
            as Arc<dyn crate::domain::services::github_service::GithubServiceTrait>);

    let context = create_context_with_services(merge_task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let repo_path = std::path::Path::new("/tmp/push-to-main-test");
    let pb_repo_opt = Some(Arc::clone(&plan_branch_repo) as Arc<dyn PlanBranchRepository>);

    handler
        .post_merge_cleanup(
            merge_task_id.as_str(),
            &merge_task_id,
            repo_path,
            &pb_repo_opt,
        )
        .await;

    let state = github.state();
    assert_eq!(
        state.delete_remote_branch_calls, 0,
        "push-to-main mode should NOT call delete_remote_branch"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6: cascade_stop with no poller registry — no panic, cascade proceeds
// ─────────────────────────────────────────────────────────────────────────────

/// cascade_stop with no pr_poller_registry wired: cascade proceeds normally without panic.
#[tokio::test]
async fn cascade_stop_without_registry_proceeds_normally() {
    let task_repo = Arc::new(MemoryTaskRepository::new());

    let mut merge_task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "merge task".into(),
    );
    merge_task.internal_status = InternalStatus::PendingMerge;
    let merge_task_id = merge_task.id.clone();
    task_repo.create(merge_task).await.unwrap();

    let mut sibling = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "sibling".into(),
    );
    sibling.internal_status = InternalStatus::Ready;
    sibling.ideation_session_id = Some(IdeationSessionId::from_string("sess-1".to_string()));
    let sibling_id = sibling.id.clone();
    task_repo.create(sibling).await.unwrap();

    // No pr_poller_registry wired (default None in new_mock)
    let services =
        TaskServices::new_mock().with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>);

    let context = create_context_with_services(merge_task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let pb = {
        let mut pb = PlanBranch::new(
            ArtifactId::from_string("art-1".to_string()),
            IdeationSessionId::from_string("sess-1".to_string()),
            ProjectId::from_string("proj-1".to_string()),
            "plan/feature-branch".to_string(),
            "main".to_string(),
        );
        pb.merge_task_id = Some(merge_task_id.clone());
        pb.status = PlanBranchStatus::Merged;
        pb
    };

    // Should not panic even without registry
    handler
        .cascade_stop_sibling_tasks(&merge_task_id, merge_task_id.as_str(), &pb)
        .await;

    // Sibling should still be cascade-stopped
    let updated = task_repo.get_by_id(&sibling_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Cancelled,
        "Cascade stop should proceed even without poller registry"
    );
}

// Tests for branchless task auto-transition: Approved → Merged (skipping PendingMerge).
//
// When a task has no task_branch (e.g., external repo work), the merge pipeline
// is skipped because there's no branch to merge into the main repo.

use super::helpers::*;
use crate::domain::entities::{ProjectId, Task};
use crate::domain::state_machine::transition_handler::{
    has_no_code_changes_metadata, set_no_code_changes_metadata,
};
use crate::domain::state_machine::{
    State, TaskEvent, TaskStateMachine, TransitionHandler, TransitionResult,
};

#[tokio::test]
async fn test_branchless_task_approved_skips_merge_pipeline() {
    // Create a task with NO branch (simulates external repo work)
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Create shared agentive/reefbot-strategy repo".to_string(),
    );
    task.task_branch = None;
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let services =
        TaskServices::new_mock().with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(&State::ReviewPassed, &TaskEvent::HumanApprove)
        .await;

    // Should auto-transition directly to Merged (NOT PendingMerge)
    if let TransitionResult::AutoTransition(state) = &result {
        assert_eq!(
            *state,
            State::Merged,
            "Branchless task should skip merge pipeline and go to Merged"
        );
    } else {
        panic!("Expected AutoTransition to Merged, got {:?}", result);
    }
}

#[tokio::test]
async fn test_branched_task_approved_enters_merge_pipeline() {
    // Create a task WITH a branch (normal workflow)
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Normal implementation task".to_string(),
    );
    task.task_branch = Some("ralphx/test-project/task-123".to_string());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let services =
        TaskServices::new_mock().with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(&State::ReviewPassed, &TaskEvent::HumanApprove)
        .await;

    // Should auto-transition to PendingMerge (normal merge pipeline)
    if let TransitionResult::AutoTransition(state) = &result {
        assert_eq!(
            *state,
            State::PendingMerge,
            "Branched task should enter merge pipeline"
        );
    } else {
        panic!("Expected AutoTransition to PendingMerge, got {:?}", result);
    }
}

#[tokio::test]
async fn test_no_task_repo_defaults_to_merge_pipeline() {
    // When task_repo is not available (e.g., old test setup), default to PendingMerge
    let (_spawner, _emitter, _notifier, _dep_manager, _review_starter, services) =
        create_test_services();
    let context = create_context_with_services("task-1", "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(&State::ReviewPassed, &TaskEvent::HumanApprove)
        .await;

    // Should default to PendingMerge when task_repo is not set
    if let TransitionResult::AutoTransition(state) = &result {
        assert_eq!(
            *state,
            State::PendingMerge,
            "Without task_repo, should default to merge pipeline"
        );
    } else {
        panic!("Expected AutoTransition to PendingMerge, got {:?}", result);
    }
}

// ==================
// Side-effect tests: unblock_dependents on branchless Merged transition
// ==================

/// Verify that `unblock_dependents` is called when a branchless task auto-transitions
/// to Merged (ReviewPassed → Approved → Merged).
///
/// The merge-pipeline skip calls `on_enter(Merged)` which must trigger
/// `dependency_manager.unblock_dependents(task_id)` — the same side effect
/// that fires when a normal task completes the full merge pipeline.
#[tokio::test]
async fn test_branchless_merged_unblocks_dependents() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let dep_manager = Arc::new(MockDependencyManager::new());

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "External repo task with dependent".to_string(),
    );
    task.task_branch = None;
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut services = TaskServices::new_mock();
    services.dependency_manager = Arc::clone(&dep_manager) as Arc<dyn DependencyManager>;
    let services = services.with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>);

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(&State::ReviewPassed, &TaskEvent::HumanApprove)
        .await;

    // Must land at Merged (branchless skip fires)
    assert!(
        matches!(&result, TransitionResult::AutoTransition(State::Merged)),
        "Expected AutoTransition to Merged, got {:?}",
        result
    );

    // on_enter(Merged) must have called unblock_dependents with this task_id
    let dep_calls = dep_manager.get_calls();
    assert!(
        dep_calls
            .iter()
            .any(|c| c.method == "unblock_dependents" && c.args[0] == task_id.as_str()),
        "unblock_dependents must be called with the branchless task_id on Merged entry. \
         Calls: {:?}",
        dep_calls
    );
}

/// Integration test: branchless task transitioning to Merged unblocks a waiting dependent.
///
/// Setup: Task A (branchless) blocks Task B.
/// After A reaches Merged, the dep_manager's blocker state for B should be cleared.
///
/// Verifies the full unblock chain without a real git repo (branchless tasks have no branch).
#[tokio::test]
async fn test_branchless_merged_unblocks_dependent_task() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let dep_manager = Arc::new(MockDependencyManager::new());

    // Task A: branchless (the blocker)
    let mut task_a = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Branchless task A (external repo work)".to_string(),
    );
    task_a.task_branch = None;
    let task_a_id = task_a.id.clone();
    task_repo.create(task_a).await.unwrap();

    // Task B: depends on A (the dependent)
    let task_b = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Dependent task B".to_string(),
    );
    let task_b_id = task_b.id.clone();
    task_repo.create(task_b).await.unwrap();

    // Wire B as blocked by A
    dep_manager.set_blockers(task_b_id.as_str(), vec![task_a_id.as_str().to_string()]);

    // Sanity: B is blocked before A merges
    assert!(
        dep_manager
            .has_unresolved_blockers(task_b_id.as_str())
            .await,
        "Task B should be blocked by A before A merges"
    );

    let mut services = TaskServices::new_mock();
    services.dependency_manager = Arc::clone(&dep_manager) as Arc<dyn DependencyManager>;
    let services = services.with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>);

    let context = TaskContext::new(task_a_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    // Transition A: ReviewPassed → Approved → Merged (branchless skip)
    let result = handler
        .handle_transition(&State::ReviewPassed, &TaskEvent::HumanApprove)
        .await;

    assert!(
        matches!(&result, TransitionResult::AutoTransition(State::Merged)),
        "Expected AutoTransition to Merged, got {:?}",
        result
    );

    // After A reaches Merged, B's blocker should be cleared
    assert!(
        !dep_manager
            .has_unresolved_blockers(task_b_id.as_str())
            .await,
        "Task B should be unblocked after branchless Task A reaches Merged"
    );
}

/// Verify that a task with a branch (even a nonexistent/deleted one) is NOT treated as
/// branchless — it must still enter the merge pipeline (PendingMerge).
///
/// The branchless skip fires only when `task.task_branch.is_none()`.
/// A `Some("deleted-branch")` means the task once had a branch and must go through merge.
#[tokio::test]
async fn test_branched_task_with_deleted_branch_enters_merge_pipeline() {
    let task_repo = Arc::new(MemoryTaskRepository::new());

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Task with deleted feature branch".to_string(),
    );
    // Branch is set but no longer exists in any git repo — Some(x) is still Some(x)
    task.task_branch = Some("task/deleted-feature-branch".to_string());
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let services =
        TaskServices::new_mock().with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(&State::ReviewPassed, &TaskEvent::HumanApprove)
        .await;

    // Must enter merge pipeline even though the branch is nonexistent
    if let TransitionResult::AutoTransition(state) = &result {
        assert_eq!(
            *state,
            State::PendingMerge,
            "Task with Some(branch) must enter merge pipeline regardless of branch existence. \
             Got {:?}",
            state
        );
    } else {
        panic!("Expected AutoTransition to PendingMerge, got {:?}", result);
    }
}

// ==================
// no_code_changes metadata skip tests
// ==================

/// Task with no_code_changes metadata skips merge pipeline (Approved → Merged).
///
/// When a task has a branch but the `no_code_changes` metadata flag is set,
/// the merge pipeline is skipped — it goes directly to Merged, same as branchless tasks.
/// This is the human-review-gate path for approved_no_changes: reviewer set the metadata,
/// task passed through ReviewPassed → human approved → Approved → skip → Merged.
#[tokio::test]
async fn test_no_code_changes_task_skips_merge_pipeline() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Research task — no code produced".to_string(),
    );
    // Task HAS a branch (was set up for execution) but no code changes were made
    task.task_branch = Some("ralphx/task/research-123".to_string());
    task.worktree_path = Some("/tmp/research-worktree".to_string());
    // Set the no_code_changes metadata flag (normally set by complete_review handler)
    set_no_code_changes_metadata(&mut task);
    assert!(
        has_no_code_changes_metadata(&task),
        "metadata must be set before test"
    );

    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let services =
        TaskServices::new_mock().with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(&State::ReviewPassed, &TaskEvent::HumanApprove)
        .await;

    // Should auto-transition directly to Merged (NOT PendingMerge)
    assert!(
        matches!(&result, TransitionResult::AutoTransition(State::Merged)),
        "Task with no_code_changes metadata should skip merge pipeline and go to Merged. \
         Got: {:?}",
        result
    );
}

/// Task WITH a branch and NO no_code_changes metadata enters the merge pipeline normally.
///
/// Confirms the skip only fires when the metadata flag is present.
#[tokio::test]
async fn test_task_without_no_code_changes_metadata_enters_merge_pipeline() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Normal implementation task".to_string(),
    );
    task.task_branch = Some("ralphx/task/impl-456".to_string());
    // NO no_code_changes metadata — must go through normal merge pipeline
    assert!(
        !has_no_code_changes_metadata(&task),
        "no metadata must be set"
    );

    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let services =
        TaskServices::new_mock().with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>);
    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(&State::ReviewPassed, &TaskEvent::HumanApprove)
        .await;

    // Should enter merge pipeline (PendingMerge), NOT skip to Merged
    assert!(
        matches!(
            &result,
            TransitionResult::AutoTransition(State::PendingMerge)
        ),
        "Task without no_code_changes metadata must enter merge pipeline. \
         Got: {:?}",
        result
    );
}

/// no_code_changes skip unblocks dependents via on_enter(Merged).
///
/// When a task with no_code_changes metadata reaches Merged via the skip,
/// on_enter(Merged) must fire dependency unblocking — same as branchless tasks.
#[tokio::test]
async fn test_no_code_changes_merged_unblocks_dependents() {
    let task_repo = Arc::new(MemoryTaskRepository::new());
    let dep_manager = Arc::new(MockDependencyManager::new());

    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Research task with dependent".to_string(),
    );
    task.task_branch = Some("ralphx/task/research-789".to_string());
    set_no_code_changes_metadata(&mut task);
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut services = TaskServices::new_mock();
    services.dependency_manager = Arc::clone(&dep_manager) as Arc<dyn DependencyManager>;
    let services = services.with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>);

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let mut handler = TransitionHandler::new(&mut machine);

    let result = handler
        .handle_transition(&State::ReviewPassed, &TaskEvent::HumanApprove)
        .await;

    assert!(
        matches!(&result, TransitionResult::AutoTransition(State::Merged)),
        "Expected AutoTransition to Merged, got {:?}",
        result
    );

    // on_enter(Merged) must have called unblock_dependents
    let dep_calls = dep_manager.get_calls();
    assert!(
        dep_calls
            .iter()
            .any(|c| c.method == "unblock_dependents" && c.args[0] == task_id.as_str()),
        "unblock_dependents must be called when no_code_changes task reaches Merged. \
         Calls: {:?}",
        dep_calls
    );
}

// Regression tests for source_conflict_resolved metadata flag.
//
// Tests the fix for the source_update_conflict retry loop:
// When a merger agent resolves a source←target conflict, the retry
// from PendingMerge must use squash-only (not rebase). Rebasing drops
// the agent's merge commit and replays individual commits, re-encountering
// the same conflicts.
//
// Coverage:
//   1. has_source_conflict_resolved: unit tests for metadata read
//   2. set_source_conflict_resolved: unit tests for metadata write
//   3. Integration: RebaseSquash + flag → squash-only (Merged, no agent spawned)
//   4. Integration: RebaseSquash without flag → full rebase-squash (no regression)
//   5. Integration: Exact bug scenario — source_conflict_resolved prevents second agent

use super::helpers::*;
use crate::domain::entities::{InternalStatus, MergeStrategy, ProjectId, Task};
use crate::domain::state_machine::services::TaskScheduler;
use crate::domain::state_machine::transition_handler::merge_helpers::{
    has_source_conflict_resolved, parse_metadata, set_source_conflict_resolved,
};
use crate::domain::state_machine::{State, TransitionHandler};

// ──────────────────────────────────────────────────────────────────────────────
// Unit tests: has_source_conflict_resolved
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn has_source_conflict_resolved_false_no_metadata() {
    let task = make_task(None, None);
    assert!(!has_source_conflict_resolved(&task));
}

#[test]
fn has_source_conflict_resolved_false_empty_metadata() {
    let mut task = make_task(None, None);
    task.metadata = Some("{}".to_string());
    assert!(!has_source_conflict_resolved(&task));
}

#[test]
fn has_source_conflict_resolved_false_unrelated_metadata() {
    let mut task = make_task(None, None);
    task.metadata = Some(r#"{"some_key": "value"}"#.to_string());
    assert!(!has_source_conflict_resolved(&task));
}

#[test]
fn has_source_conflict_resolved_true_when_flag_set() {
    let mut task = make_task(None, None);
    task.metadata = Some(r#"{"source_conflict_resolved": true}"#.to_string());
    assert!(has_source_conflict_resolved(&task));
}

#[test]
fn has_source_conflict_resolved_false_when_flag_is_false() {
    let mut task = make_task(None, None);
    task.metadata = Some(r#"{"source_conflict_resolved": false}"#.to_string());
    assert!(!has_source_conflict_resolved(&task));
}

#[test]
fn has_source_conflict_resolved_true_with_other_metadata() {
    let mut task = make_task(None, None);
    task.metadata = Some(
        serde_json::json!({
            "source_conflict_resolved": true,
            "other_field": "value",
            "conflict_type": "rebase"
        })
        .to_string(),
    );
    assert!(has_source_conflict_resolved(&task));
}

// ──────────────────────────────────────────────────────────────────────────────
// Unit tests: set_source_conflict_resolved
// ──────────────────────────────────────────────────────────────────────────────

#[test]
fn set_source_conflict_resolved_creates_metadata_when_none() {
    let mut task = make_task(None, None);
    set_source_conflict_resolved(&mut task);

    let meta = parse_metadata(&task).unwrap();
    assert_eq!(meta["source_conflict_resolved"], true);
}

#[test]
fn set_source_conflict_resolved_preserves_existing_metadata() {
    let mut task = make_task(None, None);
    task.metadata = Some(r#"{"other": "keep", "count": 42}"#.to_string());
    set_source_conflict_resolved(&mut task);

    let meta = parse_metadata(&task).unwrap();
    assert_eq!(meta["source_conflict_resolved"], true);
    assert_eq!(meta["other"], "keep");
    assert_eq!(meta["count"], 42);
}

#[test]
fn set_source_conflict_resolved_overwrites_false_flag() {
    let mut task = make_task(None, None);
    task.metadata = Some(r#"{"source_conflict_resolved": false}"#.to_string());
    set_source_conflict_resolved(&mut task);

    assert!(has_source_conflict_resolved(&task));
}

// ──────────────────────────────────────────────────────────────────────────────
// Integration: RebaseSquash + source_conflict_resolved → squash-only → Merged
// ──────────────────────────────────────────────────────────────────────────────

/// When source_conflict_resolved is set and strategy is RebaseSquash, the merge
/// pipeline must skip rebase and use squash-only. With a clean merge (no divergence),
/// this means the task goes directly to Merged — NO merger agent is spawned.
///
/// This is the core regression test for the source_update_conflict retry bug.
#[tokio::test]
async fn rebase_squash_with_source_conflict_resolved_uses_squash_and_merges_cleanly() {
    let git_repo = setup_real_git_repo();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(
        project_id.clone(),
        "source_conflict_resolved squash test".to_string(),
    );
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(git_repo.task_branch.clone());
    // Set the flag: simulates what handle_source_update_resolution does after agent resolves conflict
    set_source_conflict_resolved(&mut task);
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&git_repo.path_string());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::RebaseSquash;
    project_repo.create(project).await.unwrap();

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

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "RebaseSquash + source_conflict_resolved must use squash-only and reach Merged. \
         Got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );

    assert_eq!(
        chat_service.call_count(),
        0,
        "No merger agent should be spawned for a clean squash merge (call_count={})",
        chat_service.call_count(),
    );

    // Verify the squash commit landed on main using the task title as the subject.
    let log_output = std::process::Command::new("git")
        .args(["log", "--oneline", "main"])
        .current_dir(git_repo.path())
        .output()
        .expect("git log");
    let log_str = String::from_utf8_lossy(&log_output.stdout);
    assert!(
        log_str.contains("source_conflict_resolved squash test"),
        "Git log on main should contain the squash commit. Log:\n{}",
        log_str,
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Integration: RebaseSquash WITHOUT flag → full rebase-squash (no regression)
// ──────────────────────────────────────────────────────────────────────────────

/// When source_conflict_resolved is NOT set, RebaseSquash must use its normal
/// rebase-then-squash strategy. This verifies the fix doesn't regress the default path.
#[tokio::test]
async fn rebase_squash_without_flag_uses_full_rebase_squash() {
    let git_repo = setup_real_git_repo();

    // Stay on main so strategy uses checkout-free path (simpler, still tests dispatch logic)
    let setup = setup_pending_merge_with_real_repo(
        "RebaseSquash no-flag test",
        &git_repo.task_branch,
        &git_repo.path_string(),
        MergeStrategy::RebaseSquash,
    )
    .await;

    let task_id = setup.task_id.clone();
    let task_repo = Arc::clone(&setup.task_repo);
    let (mut machine, _task_repo, _task_id) = setup.into_machine();
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "RebaseSquash without flag should still reach Merged (clean merge). \
         Got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Integration: Exact bug scenario — conflict on source + retry must NOT rebase
// ──────────────────────────────────────────────────────────────────────────────

/// The EXACT scenario from the bug: after source←target conflict resolution,
/// the task retries with PendingMerge. With source_conflict_resolved set,
/// the retry must use squash-only and complete cleanly — NOT spawn a second agent.
///
/// Simulates the post-resolution state:
/// - Source branch has target's changes (agent merged target INTO source)
/// - source_conflict_resolved flag is set
/// - Task transitions to PendingMerge → dispatch_merge_strategy
/// - RebaseSquash detects flag → uses squash-only → Merged
#[tokio::test]
async fn exact_bug_scenario_source_update_conflict_retry_does_not_spawn_second_agent() {
    let git_repo = setup_real_git_repo();

    // Simulate the post-resolution state: merge main INTO task branch
    // This mirrors what the merger agent does when resolving source_update_conflict
    let _ = std::process::Command::new("git")
        .args(["checkout", &git_repo.task_branch])
        .current_dir(git_repo.path())
        .output();

    // Add a commit on main that will be merged INTO the task branch
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(git_repo.path())
        .output();
    std::fs::write(git_repo.path().join("main-change.txt"), "change on main").unwrap();
    let _ = std::process::Command::new("git")
        .args(["add", "."])
        .current_dir(git_repo.path())
        .output();
    let _ = std::process::Command::new("git")
        .args(["commit", "-m", "main change"])
        .current_dir(git_repo.path())
        .output();

    // Agent merges main INTO task branch (source update resolution)
    let _ = std::process::Command::new("git")
        .args(["checkout", &git_repo.task_branch])
        .current_dir(git_repo.path())
        .output();
    let _ = std::process::Command::new("git")
        .args(["merge", "main", "--no-edit"])
        .current_dir(git_repo.path())
        .output();

    // Back to main for the merge
    let _ = std::process::Command::new("git")
        .args(["checkout", "main"])
        .current_dir(git_repo.path())
        .output();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(
        project_id.clone(),
        "Exact bug scenario: source_update_conflict retry".to_string(),
    );
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(git_repo.task_branch.clone());
    // Simulate handle_source_update_resolution setting the flag
    set_source_conflict_resolved(&mut task);
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&git_repo.path_string());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::RebaseSquash;
    project_repo.create(project).await.unwrap();

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

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "Post source_update_conflict retry MUST reach Merged (squash-only, no rebase). \
         Got {:?}. This was the original bug — rebase drops the merge commit. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );

    assert_eq!(
        chat_service.call_count(),
        0,
        "REGRESSION: Second merger agent spawned during retry! call_count={}. \
         source_conflict_resolved flag should have prevented rebase and used squash-only.",
        chat_service.call_count(),
    );
}

// ──────────────────────────────────────────────────────────────────────────────
// Integration: Both flags set (prior_rebase_conflict + source_conflict_resolved)
// ──────────────────────────────────────────────────────────────────────────────

/// When BOTH has_prior_rebase_conflict and has_source_conflict_resolved are true,
/// the merge must still use squash-only and complete cleanly.
#[tokio::test]
async fn both_rebase_and_source_flags_use_squash_only() {
    let git_repo = setup_real_git_repo();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let project_repo = Arc::new(MemoryProjectRepository::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Both flags test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(git_repo.task_branch.clone());
    // Set BOTH flags
    task.metadata = Some(
        serde_json::json!({
            "source_conflict_resolved": true,
            "conflict_type": "rebase"
        })
        .to_string(),
    );
    let task_id = task.id.clone();
    task_repo.create(task).await.unwrap();

    let mut project = make_real_git_project(&git_repo.path_string());
    project.id = project_id;
    project.base_branch = Some("main".to_string());
    project.merge_strategy = MergeStrategy::RebaseSquash;
    project_repo.create(project).await.unwrap();

    let services = with_default_test_branch_update_authority(
        TaskServices::new_mock(),
        Arc::clone(&task_repo) as Arc<dyn TaskRepository>,
    )
    .with_task_scheduler(Arc::new(MockTaskScheduler::new()) as Arc<dyn TaskScheduler>)
    .with_task_repo(Arc::clone(&task_repo) as Arc<dyn TaskRepository>)
    .with_project_repo(Arc::clone(&project_repo) as Arc<dyn ProjectRepository>);

    let context = TaskContext::new(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let _ = handler.on_enter(&State::PendingMerge).await;

    let updated = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Merged,
        "Both flags set must still reach Merged via squash-only. Got {:?}. Metadata: {:?}",
        updated.internal_status,
        updated.metadata,
    );
}

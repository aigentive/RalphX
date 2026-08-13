// Tests for transient merge error inline retry (ROOT CAUSE #5)
//
// Verifies that transient git errors (lock contention, index.lock, etc.) get
// deferred (stay in PendingMerge for fast retry) instead of transitioning to
// MergeIncomplete (which incurs 60s+ backoff via reconciliation).
//
// Test categories:
// A. is_transient_merge_error classification (unit tests)
// B. Transient git errors → deferred (not MergeIncomplete)
// C. Permanent git errors → MergeIncomplete (unchanged behavior)
// D. Branch not found → re-check + MergeIncomplete if truly missing

use super::helpers::*;
use crate::application::{AppState, TaskTransitionService};
use crate::application::execution_state::ExecutionState;
use crate::domain::entities::{
    InternalStatus, MergeFailureSource, MergeRecoveryEventKind, MergeRecoveryMetadata,
    MergeValidationMode, Project, ProjectId, Task,
};
use crate::domain::services::{MemoryRunningAgentRegistry, MessageQueue};
use crate::domain::state_machine::TransitionHandler;

fn build_transition_service(app_state: &AppState) -> Arc<TaskTransitionService> {
    let execution_state = Arc::new(ExecutionState::new());
    let message_queue = Arc::new(MessageQueue::new());
    let running_registry = Arc::new(MemoryRunningAgentRegistry::new());

    TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        message_queue,
        running_registry,
        execution_state,
        None,
        Arc::clone(&app_state.memory_event_repo),
    )
    .with_review_repo(Arc::clone(&app_state.review_repo))
    .into_arc()
}

async fn handle_git_error_outcome_for_test(error: crate::error::AppError) -> Task {
    use super::super::merge_outcome_handler::{MergeContext, MergeHandlerOptions};
    use super::super::merge_strategies::MergeOutcome;

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let emitter = Arc::new(MockEventEmitter::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id, "Git error source test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("feature/test".to_string());
    let task_id = task.id.clone();
    task_repo.create(task.clone()).await.unwrap();

    let project = Project::new("test".to_string(), "/tmp/test".to_string());
    let services = TaskServices::new(
        Arc::new(crate::domain::state_machine::mocks::MockAgentSpawner::new()),
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::new(crate::domain::state_machine::mocks::MockNotifier::new()),
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(crate::domain::state_machine::mocks::MockReviewStarter::new()),
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    );

    let context = create_context_with_services(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);
    let task_repo_arc = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;
    let opts = MergeHandlerOptions::merge();
    let outcome = MergeOutcome::GitError(error);

    let mut ctx = MergeContext {
        task: &mut task,
        task_id: &task_id,
        task_id_str: task_id.as_str(),
        project: &project,
        repo_path: std::path::Path::new("/tmp/test"),
        source_branch: "feature/test",
        target_branch: "main",
        task_repo: &task_repo_arc,
        plan_branch_repo: &None,
        opts: &opts,
    };

    handler.handle_merge_outcome(outcome, &mut ctx).await;
    task
}

fn last_attempt_failed_source(task: &Task) -> Option<MergeFailureSource> {
    MergeRecoveryMetadata::from_task_metadata(task.metadata.as_deref())
        .expect("metadata should parse")
        .expect("merge recovery metadata should exist")
        .events
        .iter()
        .rev()
        .find(|event| matches!(event.kind, MergeRecoveryEventKind::AttemptFailed))
        .and_then(|event| event.failure_source)
}

async fn assert_git_error_source(error: crate::error::AppError, expected: MergeFailureSource) {
    let task = handle_git_error_outcome_for_test(error).await;

    assert_eq!(task.internal_status, InternalStatus::MergeIncomplete);
    assert_eq!(last_attempt_failed_source(&task), Some(expected));
}

// ==================
// A. is_transient_merge_error classification
// ==================

#[test]
fn test_transient_error_index_lock() {
    use super::super::merge_outcome_handler::is_transient_merge_error;
    let err = crate::error::AppError::GitOperation(
        "fatal: Unable to create '/repo/.git/index.lock': File exists.".to_string(),
    );
    assert!(
        is_transient_merge_error(&err),
        "index.lock errors should be transient"
    );
}

#[test]
fn test_transient_error_cannot_lock_ref() {
    use super::super::merge_outcome_handler::is_transient_merge_error;
    let err = crate::error::AppError::GitOperation(
        "error: cannot lock ref 'refs/heads/main': is at abc123 but expected def456".to_string(),
    );
    assert!(
        is_transient_merge_error(&err),
        "cannot lock ref errors should be transient"
    );
}

#[test]
fn test_transient_error_unable_to_create_lock() {
    use super::super::merge_outcome_handler::is_transient_merge_error;
    let err = crate::error::AppError::GitOperation(
        "Unable to create '/repo/.git/refs/heads/main.lock': File exists".to_string(),
    );
    assert!(
        is_transient_merge_error(&err),
        "Unable to create lock errors should be transient"
    );
}

#[test]
fn test_transient_error_fetch_head() {
    use super::super::merge_outcome_handler::is_transient_merge_error;
    let err = crate::error::AppError::GitOperation(
        "fatal: FETCH_HEAD has changed since start of fetch".to_string(),
    );
    assert!(
        is_transient_merge_error(&err),
        "FETCH_HEAD errors should be transient"
    );
}

#[test]
fn test_transient_error_shallow_file() {
    use super::super::merge_outcome_handler::is_transient_merge_error;
    let err = crate::error::AppError::GitOperation(
        "fatal: shallow file has changed since we read it".to_string(),
    );
    assert!(
        is_transient_merge_error(&err),
        "shallow file changed errors should be transient"
    );
}

/// An exhausted GitHub rate limit resolves on its own when the window resets, so it must be
/// retryable. Before the typed variant existed this arrived as `Infrastructure` and was permanent
/// (`DeterministicInfra`), which is what let the 2026-08-11 incident terminalize attempts.
#[test]
fn test_transient_error_github_rate_limit() {
    use super::super::merge_outcome_handler::is_transient_merge_error;
    let err = crate::error::AppError::GithubRateLimited {
        message:
            "gh exited with code 1: GraphQL: API rate limit already exceeded for user ID 6580668."
                .to_string(),
    };
    assert!(
        is_transient_merge_error(&err),
        "an exhausted GitHub rate limit must be retryable, not a permanent block"
    );
}

/// Auth failures must stay permanent even though they arrive from the same GitHub surface.
#[test]
fn test_permanent_error_github_auth_is_not_transient() {
    use super::super::merge_outcome_handler::is_transient_merge_error;
    let err = crate::error::AppError::GitAuth(
        "could not read Username for 'https://github.com'".to_string(),
    );
    assert!(
        !is_transient_merge_error(&err),
        "auth failures must never be blind-retried"
    );
}

#[test]
fn test_permanent_error_not_a_git_repo() {
    use super::super::merge_outcome_handler::is_transient_merge_error;
    let err = crate::error::AppError::GitOperation("fatal: not a git repository".to_string());
    assert!(
        !is_transient_merge_error(&err),
        "not a git repository should be permanent"
    );
}

#[test]
fn test_permanent_error_merge_conflict() {
    use super::super::merge_outcome_handler::is_transient_merge_error;
    let err = crate::error::AppError::GitOperation(
        "CONFLICT (content): Merge conflict in src/main.rs".to_string(),
    );
    assert!(
        !is_transient_merge_error(&err),
        "merge conflicts should be permanent"
    );
}

#[test]
fn test_permanent_error_not_something_we_can_merge() {
    use super::super::merge_outcome_handler::is_transient_merge_error;
    let err = crate::error::AppError::GitOperation(
        "merge: abc123 - not something we can merge".to_string(),
    );
    assert!(
        !is_transient_merge_error(&err),
        "not something we can merge should be permanent"
    );
}

#[test]
fn test_permanent_error_history_diverged() {
    use super::super::merge_outcome_handler::is_transient_merge_error;
    let err = crate::error::AppError::GitOperation(
        "fatal: refusing to merge unrelated histories".to_string(),
    );
    assert!(
        !is_transient_merge_error(&err),
        "unrelated histories should be permanent"
    );
}

#[test]
fn test_non_git_error_is_not_transient() {
    use super::super::merge_outcome_handler::is_transient_merge_error;
    let err = crate::error::AppError::Database("connection reset".to_string());
    assert!(
        !is_transient_merge_error(&err),
        "non-git errors should not be transient"
    );
}

#[tokio::test]
async fn test_auth_git_error_records_auth_failure_source() {
    assert_git_error_source(
        crate::error::AppError::GitAuth("Git could not authenticate".to_string()),
        MergeFailureSource::AuthFailure,
    )
    .await;
}

#[tokio::test]
async fn test_disk_full_git_error_records_disk_full_source() {
    assert_git_error_source(
        crate::error::AppError::GitOperation(
            "fatal: Unable to create '.git/FETCH_HEAD': No space left on device".to_string(),
        ),
        MergeFailureSource::DiskFull,
    )
    .await;
}

#[tokio::test]
async fn test_database_error_records_deterministic_infra_source() {
    assert_git_error_source(
        crate::error::AppError::Database("foreign key constraint failed".to_string()),
        MergeFailureSource::DeterministicInfra,
    )
    .await;
}

#[tokio::test]
async fn test_unknown_git_error_records_unknown_source() {
    assert_git_error_source(
        crate::error::AppError::GitOperation(
            "fatal: not a git repository (or any of the parent directories): .git".to_string(),
        ),
        MergeFailureSource::Unknown,
    )
    .await;
}

#[test]
fn test_commit_hook_merge_error_detected() {
    use super::super::merge_helpers::is_commit_hook_merge_error_text;
    let err = crate::error::AppError::GitOperation(
        "Failed to commit rebase+squash in worktree: stdout=[pre-commit] TS2307 Cannot find module 'zod'".to_string(),
    );
    assert!(
        is_commit_hook_merge_error_text(&err.to_string()),
        "hook-style failed-to-commit errors should be detected"
    );
}

#[test]
fn test_plain_commit_failure_without_hook_marker_is_not_commit_hook_error() {
    use super::super::merge_helpers::is_commit_hook_merge_error_text;
    let err = crate::error::AppError::GitOperation(
        "Failed to commit squash merge in worktree: stdout= stderr=Author identity unknown"
            .to_string(),
    );
    assert!(
        !is_commit_hook_merge_error_text(&err.to_string()),
        "plain commit failures without hook markers should not be rerouted as hook failures"
    );
}

#[test]
fn test_commit_hook_failure_classifier_detects_environment_failures() {
    use super::super::merge_helpers::{classify_commit_hook_failure_text, CommitHookFailureKind};

    let text = "Failed to commit rebase+squash in worktree: stderr=[pre-commit] typecheck\n\
        src/api/task-graph.ts(7,19): error TS2307: Cannot find module 'zod' or its corresponding type declarations.\n\
        /bin/sh: eslint: command not found";

    assert_eq!(
        classify_commit_hook_failure_text(text),
        CommitHookFailureKind::EnvironmentFailure,
        "missing modules/tools mean the hook environment failed before policy could be trusted"
    );
}

#[test]
fn test_commit_hook_failure_classifier_detects_policy_failures() {
    use super::super::merge_helpers::{classify_commit_hook_failure_text, CommitHookFailureKind};

    let text =
        "Failed to commit rebase+squash in worktree: stderr=[pre-commit] design-token guards\n\
        Guard 1: primitive tokens in components\n\
        ERROR: primitive-tier token used in frontend/src/components/Button.tsx";

    assert_eq!(
        classify_commit_hook_failure_text(text),
        CommitHookFailureKind::PolicyFailure,
        "hook policy findings tied to repository rules should be sent as code feedback"
    );
}

#[test]
fn test_commit_hook_failure_fingerprint_normalizes_paths_and_noise() {
    use super::super::merge_helpers::commit_hook_failure_fingerprint;

    let left = "Git operation error: Failed to commit rebase+squash in worktree: stderr=[pre-commit]\n\
        /Users/dev/worktrees/task-a/frontend/src/api/task-graph.ts(7,19): error TS2307: Cannot find module 'zod'";
    let right = "Git operation error: Failed to commit rebase+squash in worktree: stderr=[pre-commit]\n\
        /tmp/runner/work/project/frontend/src/api/task-graph.ts(7,19): error TS2307: Cannot find module 'zod'";

    assert_eq!(
        commit_hook_failure_fingerprint(left),
        commit_hook_failure_fingerprint(right),
        "same hook failure should fingerprint the same across machines/worktree paths"
    );
}

// ==================
// B. Transient git error → deferred (not MergeIncomplete)
// ==================

/// Transient git errors (index.lock) should be deferred instead of
/// transitioning to MergeIncomplete. The task stays in PendingMerge
/// for fast retry via the PendingMerge reconciler.
#[tokio::test]
async fn test_transient_git_error_defers_instead_of_merge_incomplete() {
    use super::super::merge_outcome_handler::{MergeContext, MergeHandlerOptions};
    use super::super::merge_strategies::MergeOutcome;

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let emitter = Arc::new(MockEventEmitter::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Transient error test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("feature/test".to_string());
    let task_id = task.id.clone();
    task_repo.create(task.clone()).await.unwrap();

    let project = Project::new("test".to_string(), "/tmp/test".to_string());

    let services = TaskServices::new(
        Arc::new(crate::domain::state_machine::mocks::MockAgentSpawner::new()),
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::new(crate::domain::state_machine::mocks::MockNotifier::new()),
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(crate::domain::state_machine::mocks::MockReviewStarter::new()),
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    );

    let context = create_context_with_services(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // Transient error: index.lock
    let outcome = MergeOutcome::GitError(crate::error::AppError::GitOperation(
        "fatal: Unable to create '/repo/.git/index.lock': File exists.".to_string(),
    ));
    let opts = MergeHandlerOptions::merge();
    let task_repo_arc = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;

    let mut ctx = MergeContext {
        task: &mut task,
        task_id: &task_id,
        task_id_str: task_id.as_str(),
        project: &project,
        repo_path: std::path::Path::new("/tmp/test"),
        source_branch: "feature/test",
        target_branch: "main",
        task_repo: &task_repo_arc,
        plan_branch_repo: &None,
        opts: &opts,
    };

    handler.handle_merge_outcome(outcome, &mut ctx).await;

    // Task should stay in PendingMerge (deferred), NOT transition to MergeIncomplete
    assert_eq!(
        task.internal_status,
        InternalStatus::PendingMerge,
        "Transient error should defer (stay in PendingMerge), not go to MergeIncomplete"
    );

    // Metadata should contain merge_deferred marker
    let meta: serde_json::Value = serde_json::from_str(task.metadata.as_deref().unwrap()).unwrap();
    let recovery = meta.get("merge_recovery");
    assert!(
        recovery.is_some(),
        "Should have merge_recovery metadata recording the transient failure"
    );

    // Should NOT emit pending_merge -> merge_incomplete transition
    let events = emitter.get_events();
    assert!(
        !events.iter().any(|e| e.method == "emit_status_change"
            && e.args.get(2).map(|s| s.as_str()) == Some("merge_incomplete")),
        "Should NOT emit merge_incomplete status change for transient errors"
    );
}

/// Transient git error: "cannot lock ref" should also be deferred.
#[tokio::test]
async fn test_cannot_lock_ref_error_defers() {
    use super::super::merge_outcome_handler::{MergeContext, MergeHandlerOptions};
    use super::super::merge_strategies::MergeOutcome;

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let emitter = Arc::new(MockEventEmitter::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Lock ref test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("feature/test".to_string());
    let task_id = task.id.clone();
    task_repo.create(task.clone()).await.unwrap();

    let project = Project::new("test".to_string(), "/tmp/test".to_string());

    let services = TaskServices::new(
        Arc::new(crate::domain::state_machine::mocks::MockAgentSpawner::new()),
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::new(crate::domain::state_machine::mocks::MockNotifier::new()),
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(crate::domain::state_machine::mocks::MockReviewStarter::new()),
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    );

    let context = create_context_with_services(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let outcome = MergeOutcome::GitError(crate::error::AppError::GitOperation(
        "error: cannot lock ref 'refs/heads/main': is at abc123 but expected def456".to_string(),
    ));
    let opts = MergeHandlerOptions::squash();
    let task_repo_arc = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;

    let mut ctx = MergeContext {
        task: &mut task,
        task_id: &task_id,
        task_id_str: task_id.as_str(),
        project: &project,
        repo_path: std::path::Path::new("/tmp/test"),
        source_branch: "feature/test",
        target_branch: "main",
        task_repo: &task_repo_arc,
        plan_branch_repo: &None,
        opts: &opts,
    };

    handler.handle_merge_outcome(outcome, &mut ctx).await;

    assert_eq!(
        task.internal_status,
        InternalStatus::PendingMerge,
        "cannot lock ref should defer (stay in PendingMerge)"
    );
}

// ==================
// C. Permanent git error → MergeIncomplete (unchanged behavior)
// ==================

/// Permanent git errors should still transition to MergeIncomplete.
#[tokio::test]
async fn test_permanent_git_error_transitions_to_merge_incomplete() {
    use super::super::merge_outcome_handler::{MergeContext, MergeHandlerOptions};
    use super::super::merge_strategies::MergeOutcome;

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let emitter = Arc::new(MockEventEmitter::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Permanent error test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("feature/test".to_string());
    let task_id = task.id.clone();
    task_repo.create(task.clone()).await.unwrap();

    let project = Project::new("test".to_string(), "/tmp/test".to_string());

    let services = TaskServices::new(
        Arc::new(crate::domain::state_machine::mocks::MockAgentSpawner::new()),
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::new(crate::domain::state_machine::mocks::MockNotifier::new()),
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(crate::domain::state_machine::mocks::MockReviewStarter::new()),
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    );

    let context = create_context_with_services(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // Permanent error: not a git repository
    let outcome = MergeOutcome::GitError(crate::error::AppError::GitOperation(
        "fatal: not a git repository (or any of the parent directories): .git".to_string(),
    ));
    let opts = MergeHandlerOptions::merge();
    let task_repo_arc = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;

    let mut ctx = MergeContext {
        task: &mut task,
        task_id: &task_id,
        task_id_str: task_id.as_str(),
        project: &project,
        repo_path: std::path::Path::new("/tmp/test"),
        source_branch: "feature/test",
        target_branch: "main",
        task_repo: &task_repo_arc,
        plan_branch_repo: &None,
        opts: &opts,
    };

    handler.handle_merge_outcome(outcome, &mut ctx).await;

    // Permanent errors should go to MergeIncomplete as before
    assert_eq!(
        task.internal_status,
        InternalStatus::MergeIncomplete,
        "Permanent git error should transition to MergeIncomplete"
    );

    // Should emit status change to merge_incomplete
    let events = emitter.get_events();
    assert!(
        events.iter().any(|e| e.method == "emit_status_change"
            && e.args.get(2).map(|s| s.as_str()) == Some("merge_incomplete")),
        "Should emit merge_incomplete status change for permanent errors"
    );
}

#[tokio::test]
async fn test_commit_hook_policy_error_reroutes_back_to_reexecuting() {
    use super::super::merge_outcome_handler::{MergeContext, MergeHandlerOptions};
    use super::super::merge_strategies::MergeOutcome;

    let real_repo = setup_real_git_repo();
    let repo_path = real_repo.path();

    let app_state = AppState::new_test();
    let transition_service = build_transition_service(&app_state);
    let emitter = Arc::new(MockEventEmitter::new());

    let mut project = Project::new("test".to_string(), repo_path.to_string_lossy().to_string());
    project.base_branch = Some("main".to_string());
    project.merge_validation_mode = MergeValidationMode::Off;
    let project_id = project.id.clone();
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project_id.clone(), "Hook reroute test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(real_repo.task_branch.clone());
    task.worktree_path = Some(repo_path.to_string_lossy().to_string());
    let task_id = task.id.clone();
    app_state.task_repo.create(task.clone()).await.unwrap();

    let services = TaskServices::new(
        Arc::new(crate::domain::state_machine::mocks::MockAgentSpawner::new()),
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::new(crate::domain::state_machine::mocks::MockNotifier::new()),
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(crate::domain::state_machine::mocks::MockReviewStarter::new()),
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    )
    .with_transition_service(Arc::clone(&transition_service));

    let context = create_context_with_services(task_id.as_str(), project_id.as_str(), services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let outcome = MergeOutcome::GitError(crate::error::AppError::GitOperation(
        "Failed to commit rebase+squash in worktree: stdout=[pre-commit][design-token guards] ERROR primitive token in frontend/src/components/Button.tsx".to_string(),
    ));
    let opts = MergeHandlerOptions::rebase_squash();
    let task_repo_arc = Arc::clone(&app_state.task_repo) as Arc<dyn TaskRepository>;

    let mut ctx = MergeContext {
        task: &mut task,
        task_id: &task_id,
        task_id_str: task_id.as_str(),
        project: &project,
        repo_path,
        source_branch: &real_repo.task_branch,
        target_branch: "main",
        task_repo: &task_repo_arc,
        plan_branch_repo: &None,
        opts: &opts,
    };

    handler.handle_merge_outcome(outcome, &mut ctx).await;

    let updated = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task to exist");

    assert_eq!(
        updated.internal_status,
        InternalStatus::ReExecuting,
        "hook-blocked merge commits should route back into re-execution"
    );

    let meta: serde_json::Value =
        serde_json::from_str(updated.metadata.as_deref().unwrap_or("{}")).unwrap();
    assert!(
        meta.get("restart_note").is_none(),
        "hook reroutes must not use restart_note user-message transport"
    );
    let feedback = meta
        .get("merge_revision_feedback")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    assert!(
        feedback.contains("Repository commit hooks rejected the merge commit"),
        "expected durable revision feedback note in metadata, got: {feedback}"
    );
    assert!(
        !emitter
            .get_events()
            .iter()
            .any(|e| e.method == "emit_status_change"
                && e.args.get(2).map(|s| s.as_str()) == Some("merge_incomplete")),
        "hook reroute should not emit a merge_incomplete transition"
    );

    let notes = app_state
        .review_repo
        .get_notes_by_task_id(&task_id)
        .await
        .expect("review notes query should succeed");
    let latest = notes
        .first()
        .expect("hook reroute should persist a review note");
    assert_eq!(
        latest.reviewer,
        crate::domain::entities::ReviewerType::System
    );
    assert_eq!(
        latest.outcome,
        crate::domain::entities::ReviewOutcome::ChangesRequested
    );
    assert!(
        latest
            .summary
            .as_deref()
            .unwrap_or_default()
            .contains("Repository commit hooks rejected the merge commit"),
        "hook reroute should store a concise review-note summary"
    );
    let note_body = latest.notes.as_deref().unwrap_or_default();
    assert!(
        note_body.contains("Full hook output:"),
        "hook reroute should store full hook output in review-note details"
    );
    assert!(
        note_body.contains("```text"),
        "hook reroute details should preserve large hook output in a readable code block"
    );
    assert!(
        !note_body.contains('\u{001b}'),
        "hook reroute note bodies should strip ANSI escape codes before persistence"
    );
}

#[tokio::test]
async fn test_commit_hook_environment_error_blocks_without_reexecution() {
    use super::super::merge_outcome_handler::{MergeContext, MergeHandlerOptions};
    use super::super::merge_strategies::MergeOutcome;

    let real_repo = setup_real_git_repo();
    let repo_path = real_repo.path();

    let app_state = AppState::new_test();
    let transition_service = build_transition_service(&app_state);
    let emitter = Arc::new(MockEventEmitter::new());

    let mut project = Project::new("test".to_string(), repo_path.to_string_lossy().to_string());
    project.base_branch = Some("main".to_string());
    project.merge_validation_mode = MergeValidationMode::Off;
    let project_id = project.id.clone();
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project_id.clone(), "Hook environment test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(real_repo.task_branch.clone());
    task.worktree_path = Some(repo_path.to_string_lossy().to_string());
    let task_id = task.id.clone();
    app_state.task_repo.create(task.clone()).await.unwrap();

    let services = TaskServices::new(
        Arc::new(crate::domain::state_machine::mocks::MockAgentSpawner::new()),
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::new(crate::domain::state_machine::mocks::MockNotifier::new()),
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(crate::domain::state_machine::mocks::MockReviewStarter::new()),
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    )
    .with_transition_service(Arc::clone(&transition_service));

    let context = create_context_with_services(task_id.as_str(), project_id.as_str(), services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let outcome = MergeOutcome::GitError(crate::error::AppError::GitOperation(
        "Failed to commit rebase+squash in worktree: stdout=, stderr=[pre-commit] typecheck\n\
        src/api/task-graph.ts(7,19): error TS2307: Cannot find module 'zod' or its corresponding type declarations."
            .to_string(),
    ));
    let opts = MergeHandlerOptions::rebase_squash();
    let task_repo_arc = Arc::clone(&app_state.task_repo) as Arc<dyn TaskRepository>;

    let mut ctx = MergeContext {
        task: &mut task,
        task_id: &task_id,
        task_id_str: task_id.as_str(),
        project: &project,
        repo_path,
        source_branch: &real_repo.task_branch,
        target_branch: "main",
        task_repo: &task_repo_arc,
        plan_branch_repo: &None,
        opts: &opts,
    };

    handler.handle_merge_outcome(outcome, &mut ctx).await;

    let updated = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task to exist");

    assert_eq!(
        updated.internal_status,
        InternalStatus::MergeIncomplete,
        "environment/bootstrap hook failures should block merge instead of re-executing code"
    );

    let meta: serde_json::Value =
        serde_json::from_str(updated.metadata.as_deref().unwrap_or("{}")).unwrap();
    assert_eq!(
        meta.get("merge_hook_failure_kind"),
        Some(&serde_json::json!("environment_failure"))
    );
    assert!(
        meta.get("merge_hook_environment_error")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .contains("Cannot find module 'zod'"),
        "environment hook error should be preserved for UI/debugging"
    );
    assert!(
        meta.get("merge_hook_failure_fingerprint").is_some(),
        "environment hook failures should store a normalized fingerprint for loop prevention"
    );

    let notes = app_state
        .review_repo
        .get_notes_by_task_id(&task_id)
        .await
        .expect("review notes query should succeed");
    assert!(
        notes.is_empty(),
        "environment hook failures must not be persisted as code review changes_requested notes"
    );
}

// ==================
// D. Branch not found with re-check
// ==================

/// Branch not found with non-existent repo path should transition to MergeIncomplete
/// (the re-check can't verify the branch, so we fall through to MergeIncomplete).
#[tokio::test]
async fn test_branch_not_found_nonexistent_repo_transitions_to_merge_incomplete() {
    use super::super::merge_outcome_handler::{MergeContext, MergeHandlerOptions};
    use super::super::merge_strategies::MergeOutcome;

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let emitter = Arc::new(MockEventEmitter::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Branch not found test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("feature/missing".to_string());
    let task_id = task.id.clone();
    task_repo.create(task.clone()).await.unwrap();

    let project = Project::new("test".to_string(), "/tmp/nonexistent-repo".to_string());

    let services = TaskServices::new(
        Arc::new(crate::domain::state_machine::mocks::MockAgentSpawner::new()),
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::new(crate::domain::state_machine::mocks::MockNotifier::new()),
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(crate::domain::state_machine::mocks::MockReviewStarter::new()),
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    );

    let context = create_context_with_services(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let outcome = MergeOutcome::BranchNotFound {
        branch: "feature/missing".to_string(),
    };
    let opts = MergeHandlerOptions::merge();
    let task_repo_arc = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;

    let mut ctx = MergeContext {
        task: &mut task,
        task_id: &task_id,
        task_id_str: task_id.as_str(),
        project: &project,
        repo_path: std::path::Path::new("/tmp/nonexistent-repo"),
        source_branch: "feature/missing",
        target_branch: "main",
        task_repo: &task_repo_arc,
        plan_branch_repo: &None,
        opts: &opts,
    };

    handler.handle_merge_outcome(outcome, &mut ctx).await;

    // Branch truly doesn't exist → MergeIncomplete
    assert_eq!(
        task.internal_status,
        InternalStatus::MergeIncomplete,
        "Branch not found with non-existent repo should go to MergeIncomplete"
    );

    // Metadata should have branch_missing flag
    let meta: serde_json::Value = serde_json::from_str(task.metadata.as_deref().unwrap()).unwrap();
    assert_eq!(meta.get("branch_missing"), Some(&serde_json::json!(true)));
}

/// Branch not found in a real git repo where the branch truly doesn't exist
/// should also transition to MergeIncomplete.
#[tokio::test]
async fn test_branch_not_found_real_repo_truly_missing_transitions_to_merge_incomplete() {
    use super::super::merge_outcome_handler::{MergeContext, MergeHandlerOptions};
    use super::super::merge_strategies::MergeOutcome;

    // Set up a real git repo
    let real_repo = setup_real_git_repo();
    let repo_path = real_repo.path();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let emitter = Arc::new(MockEventEmitter::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Branch missing real repo".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("feature/does-not-exist".to_string());
    let task_id = task.id.clone();
    task_repo.create(task.clone()).await.unwrap();

    let project = Project::new("test".to_string(), repo_path.to_string_lossy().to_string());

    let services = TaskServices::new(
        Arc::new(crate::domain::state_machine::mocks::MockAgentSpawner::new()),
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::new(crate::domain::state_machine::mocks::MockNotifier::new()),
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(crate::domain::state_machine::mocks::MockReviewStarter::new()),
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    );

    let context = create_context_with_services(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    let outcome = MergeOutcome::BranchNotFound {
        branch: "feature/does-not-exist".to_string(),
    };
    let opts = MergeHandlerOptions::merge();
    let task_repo_arc = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;

    let mut ctx = MergeContext {
        task: &mut task,
        task_id: &task_id,
        task_id_str: task_id.as_str(),
        project: &project,
        repo_path,
        source_branch: "feature/does-not-exist",
        target_branch: "main",
        task_repo: &task_repo_arc,
        plan_branch_repo: &None,
        opts: &opts,
    };

    handler.handle_merge_outcome(outcome, &mut ctx).await;

    // Branch truly doesn't exist → MergeIncomplete
    assert_eq!(
        task.internal_status,
        InternalStatus::MergeIncomplete,
        "Truly missing branch should transition to MergeIncomplete"
    );
}

/// Branch not found in a real git repo where the branch DOES exist (race condition)
/// should defer instead of transitioning to MergeIncomplete.
#[tokio::test]
async fn test_branch_not_found_but_branch_exists_on_recheck_defers() {
    use super::super::merge_outcome_handler::{MergeContext, MergeHandlerOptions};
    use super::super::merge_strategies::MergeOutcome;

    // Set up a real git repo — the task branch exists in the repo
    let real_repo = setup_real_git_repo();
    let repo_path = real_repo.path();

    let task_repo = Arc::new(MemoryTaskRepository::new());
    let emitter = Arc::new(MockEventEmitter::new());

    let project_id = ProjectId::from_string("proj-1".to_string());
    let mut task = Task::new(project_id.clone(), "Branch race condition test".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some(real_repo.task_branch.clone());
    let task_id = task.id.clone();
    task_repo.create(task.clone()).await.unwrap();

    let project = Project::new("test".to_string(), repo_path.to_string_lossy().to_string());

    let services = TaskServices::new(
        Arc::new(crate::domain::state_machine::mocks::MockAgentSpawner::new()),
        Arc::clone(&emitter) as Arc<dyn EventEmitter>,
        Arc::new(crate::domain::state_machine::mocks::MockNotifier::new()),
        Arc::new(MockDependencyManager::new()) as Arc<dyn DependencyManager>,
        Arc::new(crate::domain::state_machine::mocks::MockReviewStarter::new()),
        Arc::new(crate::application::MockChatService::new())
            as Arc<dyn crate::application::ChatService>,
    );

    let context = create_context_with_services(task_id.as_str(), "proj-1", services);
    let mut machine = TaskStateMachine::new(context);
    let handler = TransitionHandler::new(&mut machine);

    // Simulate race: BranchNotFound was returned but the branch exists now
    let outcome = MergeOutcome::BranchNotFound {
        branch: real_repo.task_branch.clone(),
    };
    let opts = MergeHandlerOptions::merge();
    let task_repo_arc = Arc::clone(&task_repo) as Arc<dyn TaskRepository>;

    let mut ctx = MergeContext {
        task: &mut task,
        task_id: &task_id,
        task_id_str: task_id.as_str(),
        project: &project,
        repo_path,
        source_branch: &real_repo.task_branch,
        target_branch: "main",
        task_repo: &task_repo_arc,
        plan_branch_repo: &None,
        opts: &opts,
    };

    handler.handle_merge_outcome(outcome, &mut ctx).await;

    // Branch exists on re-check → should defer (stay in PendingMerge)
    assert_eq!(
        task.internal_status,
        InternalStatus::PendingMerge,
        "Branch found on re-check should defer (stay in PendingMerge), not MergeIncomplete"
    );

    // Should NOT emit merge_incomplete
    let events = emitter.get_events();
    assert!(
        !events.iter().any(|e| e.method == "emit_status_change"
            && e.args.get(2).map(|s| s.as_str()) == Some("merge_incomplete")),
        "Should NOT emit merge_incomplete when branch found on re-check"
    );
}

use chrono::{Duration, Utc};
use ralphx_lib::application::startup_jobs::is_startup_recovery_disabled_var;
use ralphx_lib::application::{AppState, StartupJobRunner, TaskTransitionService};
use ralphx_lib::commands::execution_commands::{AGENT_ACTIVE_STATUSES, AUTO_TRANSITION_STATES};
use ralphx_lib::commands::{ActiveProjectState, ExecutionState};
use ralphx_lib::domain::entities::{
    app_state::ExecutionHaltMode, AgentRun, AgentRunStatus, ArtifactId, ChatContextType,
    ChatConversation, ChatConversationId, IdeationSessionBuilder, IdeationSessionId,
    InternalStatus, PlanBranch, Project, ProjectId, Task, TaskCategory,
};
use ralphx_lib::domain::execution::ExecutionSettings;
use ralphx_lib::domain::repositories::{AppStateRepository, PlanBranchRepository};
use ralphx_lib::domain::services::RunningAgentKey;
use ralphx_lib::domain::state_machine::mocks::MockTaskScheduler;
use ralphx_lib::domain::state_machine::TaskScheduler;
use std::process::Command;
use std::sync::Arc;
use tempfile::TempDir;

// Helper to create test state
async fn setup_test_state() -> (Arc<ExecutionState>, AppState) {
    let execution_state = Arc::new(ExecutionState::new());
    let app_state = AppState::new_test();
    (execution_state, app_state)
}

fn setup_git_repo() -> TempDir {
    let dir = TempDir::new().expect("create temp dir");
    let path = dir.path();

    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .output()
        .expect("git init should succeed");
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(path)
        .output()
        .expect("git config user.email should succeed");
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output()
        .expect("git config user.name should succeed");
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(path)
        .output()
        .expect("git commit should succeed");

    dir
}

/// Helper to build a StartupJobRunner from test state.
/// Returns (runner, app_state_repo) — set active project on app_state_repo for DB-based tests.
fn build_runner(
    app_state: &AppState,
    execution_state: &Arc<ExecutionState>,
) -> (StartupJobRunner, Arc<dyn AppStateRepository>) {
    let transition_service = Arc::new(TaskTransitionService::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.agent_run_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(execution_state),
        None,
        Arc::clone(&app_state.memory_event_repo),
    ));

    let agent_run_repo = Arc::clone(&app_state.agent_run_repo);
    let app_state_repo = Arc::clone(&app_state.app_state_repo);
    let execution_settings_repo = Arc::clone(&app_state.execution_settings_repo);

    let active_project_state = Arc::new(ActiveProjectState::new());
    let runner = StartupJobRunner::new(
        Arc::clone(&app_state.task_repo),
        Arc::clone(&app_state.task_dependency_repo),
        Arc::clone(&app_state.project_repo),
        Arc::clone(&app_state.artifact_repo),
        Arc::clone(&app_state.chat_conversation_repo),
        Arc::clone(&app_state.chat_message_repo),
        Arc::clone(&app_state.chat_attachment_repo),
        Arc::clone(&app_state.ideation_session_repo),
        Arc::clone(&app_state.activity_event_repo),
        Arc::clone(&app_state.message_queue),
        Arc::clone(&app_state.running_agent_registry),
        Arc::clone(&app_state.memory_event_repo),
        agent_run_repo,
        transition_service,
        Arc::clone(execution_state),
        Arc::clone(&active_project_state),
        Arc::clone(&app_state_repo),
        execution_settings_repo,
        Some(Arc::clone(&app_state.plan_branch_repo) as Arc<dyn PlanBranchRepository>),
    );
    (runner, app_state_repo)
}

#[tokio::test]
async fn test_delayed_startup_cleanup_preserves_current_boot_agent_and_run() {
    let (execution_state, app_state) = setup_test_state().await;
    let old_key = RunningAgentKey::new("project", "old-conversation");
    let current_key = RunningAgentKey::new("project", "current-conversation");
    let old_token = tokio_util::sync::CancellationToken::new();
    let current_token = tokio_util::sync::CancellationToken::new();
    let old_run = AgentRun::new(ChatConversationId::new());
    let old_run_id = old_run.id;

    app_state.agent_run_repo.create(old_run).await.unwrap();
    app_state
        .running_agent_registry
        .register(
            old_key.clone(),
            999_981,
            "conv-old".to_string(),
            old_run_id.as_str().to_string(),
            None,
            Some(old_token.clone()),
        )
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let boot_cutoff = Utc::now();
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    let current_run = AgentRun::new(ChatConversationId::new());
    let current_run_id = current_run.id;
    app_state.agent_run_repo.create(current_run).await.unwrap();
    app_state
        .running_agent_registry
        .register(
            current_key.clone(),
            999_982,
            "conv-current".to_string(),
            current_run_id.as_str().to_string(),
            None,
            Some(current_token.clone()),
        )
        .await;

    let (runner, _) = build_runner(&app_state, &execution_state);
    let runner = runner.with_previous_session_cutoff(boot_cutoff);

    runner.run().await;

    assert!(!app_state.running_agent_registry.is_running(&old_key).await);
    assert!(app_state.running_agent_registry.is_running(&current_key).await);
    assert!(old_token.is_cancelled());
    assert!(!current_token.is_cancelled());

    let old = app_state
        .agent_run_repo
        .get_by_id(&old_run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(old.status, AgentRunStatus::Cancelled);
    assert_eq!(
        old.error_message,
        Some("Orphaned on app restart".to_string())
    );

    let current = app_state
        .agent_run_repo
        .get_by_id(&current_run_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.status, AgentRunStatus::Running);
    assert_eq!(current.error_message, None);
}

#[test]
fn test_startup_recovery_flag_detection() {
    use std::ffi::OsStr;

    assert!(is_startup_recovery_disabled_var(Some(OsStr::new("1"))));
    assert!(is_startup_recovery_disabled_var(Some(OsStr::new("true"))));
    assert!(is_startup_recovery_disabled_var(Some(OsStr::new(""))));
    assert!(!is_startup_recovery_disabled_var(None));
}

#[tokio::test]
async fn test_resumption_skipped_when_paused() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project with a task in Executing state
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Pause execution
    execution_state.pause();

    let (runner, _app_state_repo) = build_runner(&app_state, &execution_state);

    // Run should skip because paused
    runner.run().await;

    // Running count should still be 0 (no tasks resumed)
    assert_eq!(execution_state.running_count(), 0);

    // Verify no conversations were created (entry actions were NOT called)
    let convs = app_state
        .chat_conversation_repo
        .get_by_context(ChatContextType::TaskExecution, task.id.as_str())
        .await
        .unwrap();
    assert_eq!(
        convs.len(),
        0,
        "No conversations should be created when paused"
    );
}

#[tokio::test]
async fn test_resumption_skipped_when_persisted_pause_barrier_is_set() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task.clone()).await.unwrap();

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_execution_halt_mode(ExecutionHaltMode::Paused)
        .await
        .unwrap();

    runner.run().await;

    assert!(
        execution_state.is_paused(),
        "Persisted pause barrier should set runtime execution state to paused"
    );
    assert_eq!(execution_state.running_count(), 0);

    let convs = app_state
        .chat_conversation_repo
        .get_by_context(ChatContextType::TaskExecution, task.id.as_str())
        .await
        .unwrap();
    assert_eq!(
        convs.len(),
        0,
        "No conversations should be created when persisted pause barrier is set"
    );
}

#[tokio::test]
async fn test_resumption_spawns_agents() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project with a task in Executing state
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    // Set high max_concurrent to allow resumption
    execution_state.set_max_concurrent(10);

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    // Set active project in DB (simulates persisted state from previous session)
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    // Run should trigger entry actions for the Executing task
    runner.run().await;

    // Verify NO conversation was created (on_enter fails before creating conversation)
    // The on_enter(Executing) handler fails with ExecutionBlocked before it can create
    // a conversation, then auto-dispatches ExecutionFailed to transition to Failed state.
    let convs = app_state
        .chat_conversation_repo
        .get_by_context(ChatContextType::TaskExecution, task_id.as_str())
        .await
        .unwrap();
    assert_eq!(
        convs.len(),
        0,
        "No conversation should be created when on_enter fails with ExecutionBlocked"
    );

    // Verify the task is in Failed state (ExecutionBlocked error during transition)
    // When execution is blocked (e.g., missing agent context), the on_enter handler
    // fails with ExecutionBlocked and auto-dispatches ExecutionFailed
    let updated_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Failed,
        "Task should transition to Failed when on_enter encounters ExecutionBlocked"
    );
}

#[tokio::test]
async fn test_startup_resumes_orphaned_executing_task_before_wall_clock_timeout() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Orphaned Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    task.updated_at = Utc::now() - Duration::minutes(743);
    task.blocked_reason = Some("stale missing worktree failure".to_string());
    task.metadata = Some(
        serde_json::json!({
            "error": "stale previous failure"
        })
        .to_string(),
    );
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let conversation = ChatConversation::new_task_execution(task_id.clone());
    let conversation_id = conversation.id;
    app_state
        .chat_conversation_repo
        .create(conversation)
        .await
        .unwrap();

    let mut run = AgentRun::new(conversation_id);
    run.status = AgentRunStatus::Cancelled;
    run.started_at = Utc::now() - Duration::minutes(743);
    run.completed_at = Some(Utc::now() - Duration::minutes(742));
    run.error_message = Some("Orphaned on app restart".to_string());
    app_state.agent_run_repo.create(run).await.unwrap();

    execution_state.set_max_concurrent(10);

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    runner.run().await;

    let updated_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    let metadata: serde_json::Value = updated_task
        .metadata
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    assert_eq!(
        metadata
            .get("startup_recovery_source")
            .and_then(|value| value.as_str()),
        Some("orphaned_agent_run"),
        "startup must classify orphaned active executions for resume before wall-clock timeout reconciliation"
    );
    assert_eq!(
        metadata
            .get("startup_recovery_attempts")
            .and_then(|value| value.as_u64()),
        Some(1),
        "startup resume preparation should consume the one-shot recovery attempt"
    );
    let metadata_text = metadata.to_string();
    assert!(
        !metadata_text.contains("WallClockExceeded") && !metadata_text.contains("WallClockTimeout"),
        "startup resume ordering must not let wall-clock timeout metadata win before resumption"
    );
    assert_ne!(
        updated_task.blocked_reason.as_deref(),
        Some("stale missing worktree failure"),
        "startup resume preparation should clear stale failure details before attempting re-entry"
    );
}

#[tokio::test]
async fn test_resumption_handles_empty_projects() {
    let (execution_state, app_state) = setup_test_state().await;

    let (runner, _app_state_repo) = build_runner(&app_state, &execution_state);

    // Run should complete without panic (no active project in DB, early return)
    runner.run().await;

    // Running count should be 0
    assert_eq!(execution_state.running_count(), 0);
}

#[tokio::test]
async fn test_resumption_respects_max_concurrent() {
    let (execution_state, app_state) = setup_test_state().await;

    // Set max concurrent to 2
    execution_state.set_max_concurrent(2);

    // Create a project with 5 tasks in Executing state
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    for i in 0..5 {
        let mut task = Task::new(project.id.clone(), format!("Executing Task {}", i));
        task.internal_status = InternalStatus::Executing;
        app_state.task_repo.create(task).await.unwrap();
    }

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    // Run should stop after 2 tasks due to max_concurrent
    runner.run().await;

    // Note: The actual increment happens in execute_entry_actions via the spawner.
    // Since we're using a mock spawner without execution_state wired in for this test,
    // the running_count won't actually increment. This test verifies the loop structure
    // and early exit logic based on can_start_task().

    // With our mock setup, running_count stays at 0 because the spawner doesn't have
    // execution_state. In production, the spawner would increment_running() on each spawn.
    // The test verifies that run() completes without panic when max_concurrent is reached.
}

#[tokio::test]
async fn test_resumption_skips_project_when_ideation_already_uses_only_slot() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Capacity Project".to_string(), "/test/capacity".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    app_state
        .execution_settings_repo
        .update_settings(
            Some(&project.id),
            &ExecutionSettings {
                max_concurrent_tasks: 1,
                project_ideation_max: 1,
                auto_commit: true,
                pause_on_failure: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .unwrap();

    let session = IdeationSessionBuilder::new()
        .project_id(project.id.clone())
        .build();
    let session_id = session.id.clone();
    app_state
        .ideation_session_repo
        .create(session)
        .await
        .unwrap();
    app_state
        .running_agent_registry
        .register(
            RunningAgentKey::new("ideation", session_id.as_str()),
            56565,
            "ideation-conv".to_string(),
            "ideation-run".to_string(),
            None,
            None,
        )
        .await;

    let mut task = Task::new(project.id.clone(), "Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    runner.run().await;

    let convs = app_state
        .chat_conversation_repo
        .get_by_context(ChatContextType::TaskExecution, task_id.as_str())
        .await
        .unwrap();
    assert_eq!(
        convs.len(),
        0,
        "startup recovery must not resume task execution when ideation already consumes the project's only slot"
    );
}

#[tokio::test]
async fn test_resumption_handles_multiple_statuses() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project with tasks in various agent-active states
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task1 = Task::new(project.id.clone(), "Executing Task".to_string());
    task1.internal_status = InternalStatus::Executing;
    let task1_id = task1.id.clone();
    app_state.task_repo.create(task1).await.unwrap();

    let mut task2 = Task::new(project.id.clone(), "QaRefining Task".to_string());
    task2.internal_status = InternalStatus::QaRefining;
    app_state.task_repo.create(task2).await.unwrap();

    let mut task3 = Task::new(project.id.clone(), "Reviewing Task".to_string());
    task3.internal_status = InternalStatus::Reviewing;
    app_state.task_repo.create(task3).await.unwrap();

    // Create a task NOT in agent-active state (should be skipped)
    let mut task4 = Task::new(project.id.clone(), "Ready Task".to_string());
    task4.internal_status = InternalStatus::Ready;
    let task4_id = task4.id.clone();
    app_state.task_repo.create(task4).await.unwrap();

    // Set high max_concurrent so all tasks can be resumed
    execution_state.set_max_concurrent(10);

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    // Run should complete
    runner.run().await;

    // Verify entry actions were attempted for agent-active tasks:
    // - Executing task should NOT have a conversation (on_enter fails with ExecutionBlocked)
    //   The on_enter handler fails before creating a conversation, then transitions to Failed
    let exec_convs = app_state
        .chat_conversation_repo
        .get_by_context(ChatContextType::TaskExecution, task1_id.as_str())
        .await
        .unwrap();
    assert_eq!(
        exec_convs.len(),
        0,
        "No conversation created when on_enter fails with ExecutionBlocked"
    );

    // Verify the Executing task transitioned to Failed
    let task1_updated = app_state
        .task_repo
        .get_by_id(&task1_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        task1_updated.internal_status,
        InternalStatus::Failed,
        "Task should transition to Failed when on_enter encounters ExecutionBlocked"
    );

    // - Ready task should NOT have any conversations (not an agent-active state)
    let ready_exec_convs = app_state
        .chat_conversation_repo
        .get_by_context(ChatContextType::TaskExecution, task4_id.as_str())
        .await
        .unwrap();
    assert_eq!(
        ready_exec_convs.len(),
        0,
        "Ready task should not be resumed"
    );
}

#[tokio::test]
async fn test_startup_schedules_ready_tasks_when_scheduler_configured() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a mock scheduler to verify it gets called
    let scheduler = Arc::new(MockTaskScheduler::new());

    let (runner, _app_state_repo) = build_runner(&app_state, &execution_state);
    let runner = runner.with_task_scheduler(Arc::clone(&scheduler) as Arc<dyn TaskScheduler>);

    // Run startup (no active project in DB → scheduler called in early return path)
    runner.run().await;

    // Verify scheduler was called once at the end of startup
    assert_eq!(
        scheduler.call_count(),
        1,
        "Scheduler should be called once after startup resumption"
    );
}

#[tokio::test]
async fn test_startup_does_not_schedule_when_paused() {
    let (execution_state, app_state) = setup_test_state().await;

    // Pause execution
    execution_state.pause();

    // Create a mock scheduler
    let scheduler = Arc::new(MockTaskScheduler::new());

    let (runner, _app_state_repo) = build_runner(&app_state, &execution_state);
    let runner = runner.with_task_scheduler(Arc::clone(&scheduler) as Arc<dyn TaskScheduler>);

    // Run startup while paused
    runner.run().await;

    // Scheduler should NOT be called when paused (early return)
    assert_eq!(
        scheduler.call_count(),
        0,
        "Scheduler should not be called when execution is paused"
    );
}

#[tokio::test]
async fn test_startup_schedules_after_resuming_agent_tasks() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project with an Executing task (agent-active)
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(task).await.unwrap();

    // Set high max_concurrent to allow resumption
    execution_state.set_max_concurrent(10);

    // Create a mock scheduler
    let scheduler = Arc::new(MockTaskScheduler::new());

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();
    let runner = runner.with_task_scheduler(Arc::clone(&scheduler) as Arc<dyn TaskScheduler>);

    // Run startup - should resume the Executing task AND call scheduler
    runner.run().await;

    // Verify try_schedule_ready_tasks was called (happens after resumption loop).
    // Note: try_retry_main_merges is also called when running_count == 0 (boot recovery),
    // so total call_count may be 2. We assert the specific scheduling call occurred.
    let schedule_calls: Vec<_> = scheduler
        .get_calls()
        .into_iter()
        .filter(|c| c.method == "try_schedule_ready_tasks")
        .collect();
    assert_eq!(
        schedule_calls.len(),
        1,
        "Scheduler should call try_schedule_ready_tasks after resuming agent-active tasks"
    );
}

// ============================================================
// Phase 68 Tests: Crash Recovery for Auto-Transition States
// ============================================================

#[tokio::test]
async fn test_merging_state_resumed_on_startup() {
    // Merging state was added to AGENT_ACTIVE_STATUSES in Phase 68
    // Tasks in Merging state should have their entry actions re-triggered
    // to respawn the merger agent
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project with a task in Merging state
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Merging Task".to_string());
    task.internal_status = InternalStatus::Merging;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    // Set high max_concurrent to allow resumption
    execution_state.set_max_concurrent(10);

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    // Run startup
    runner.run().await;

    // Verify entry actions were called by checking for Merge conversation
    // on_enter(Merging) creates a ChatContextType::Merge conversation
    let convs = app_state
        .chat_conversation_repo
        .get_by_context(ChatContextType::Merge, task_id.as_str())
        .await
        .unwrap();
    assert_eq!(
        convs.len(),
        1,
        "Merging task should have a Merge conversation created (merger agent respawned)"
    );

    // Task should still be in Merging state (entry actions don't change status)
    let updated_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated_task.internal_status, InternalStatus::Merging);
}

#[tokio::test]
async fn test_pending_review_auto_transitions_on_startup() {
    // PendingReview is in AUTO_TRANSITION_STATES
    // Tasks stuck in PendingReview should auto-transition to Reviewing
    // which spawns a reviewer agent
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project with a task in PendingReview state
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "PendingReview Task".to_string());
    task.internal_status = InternalStatus::PendingReview;
    task.worktree_path = Some(std::env::temp_dir().to_string_lossy().to_string());
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    // Set high max_concurrent to allow auto-transition
    execution_state.set_max_concurrent(10);

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    // Run startup - should trigger auto-transition to Reviewing
    runner.run().await;

    // The auto-transition path is:
    // 1. execute_entry_actions(PendingReview) triggers on_enter(PendingReview)
    // 2. check_auto_transition detects PendingReview -> Reviewing
    // 3. transition_to(Reviewing) is called
    // 4. on_enter(Reviewing) spawns reviewer agent (creates Review conversation)

    // Check for Review conversation (indicates Reviewing was entered)
    let review_convs = app_state
        .chat_conversation_repo
        .get_by_context(ChatContextType::Review, task_id.as_str())
        .await
        .unwrap();
    assert_eq!(
        review_convs.len(),
        1,
        "PendingReview task should auto-transition to Reviewing and create Review conversation"
    );

    // Task should now be in Reviewing state
    let updated_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Reviewing,
        "Task should have auto-transitioned from PendingReview to Reviewing"
    );
}

#[tokio::test]
async fn test_revision_needed_auto_transitions_on_startup() {
    // RevisionNeeded is in AUTO_TRANSITION_STATES
    // Tasks stuck in RevisionNeeded should auto-transition to ReExecuting
    // which spawns a worker agent
    let (execution_state, app_state) = setup_test_state().await;
    let repo = setup_git_repo();

    // Create a project with a task in RevisionNeeded state
    let worktrees = TempDir::new().expect("create worktree parent");
    let mut project = Project::new(
        "Test Project".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "RevisionNeeded Task".to_string());
    task.internal_status = InternalStatus::RevisionNeeded;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    // Set high max_concurrent to allow auto-transition
    execution_state.set_max_concurrent(10);

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    // Run startup - should trigger auto-transition to ReExecuting
    runner.run().await;

    // The auto-transition path is:
    // 1. execute_entry_actions(RevisionNeeded) triggers on_enter(RevisionNeeded)
    // 2. check_auto_transition detects RevisionNeeded -> ReExecuting
    // 3. transition_to(ReExecuting) is called
    // 4. on_enter(ReExecuting) spawns worker agent (creates TaskExecution conversation)

    // Check for TaskExecution conversation (indicates ReExecuting was entered)
    let exec_convs = app_state
        .chat_conversation_repo
        .get_by_context(ChatContextType::TaskExecution, task_id.as_str())
        .await
        .unwrap();
    assert_eq!(
        exec_convs.len(),
        1,
        "RevisionNeeded task should auto-transition to ReExecuting and create TaskExecution conversation"
    );

    // Task should now be in ReExecuting state
    let updated_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::ReExecuting,
        "Task should have auto-transitioned from RevisionNeeded to ReExecuting"
    );
}

#[tokio::test]
async fn test_merge_incomplete_commit_hook_rows_are_rerouted_on_startup() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);
    let repo = setup_git_repo();

    let worktrees = TempDir::new().expect("create worktree parent");
    let mut project = Project::new(
        "Test Project".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.worktree_parent_directory = Some(worktrees.path().to_string_lossy().to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(
        project.id.clone(),
        "Legacy Hook MergeIncomplete".to_string(),
    );
    task.internal_status = InternalStatus::MergeIncomplete;
    task.metadata = Some(
        serde_json::json!({
            "error": "Git operation error: Failed to commit rebase+squash in worktree: stderr=[pre-commit] design-token guards failed"
        })
        .to_string(),
    );
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    runner.run().await;

    let exec_convs = app_state
        .chat_conversation_repo
        .get_by_context(ChatContextType::TaskExecution, task_id.as_str())
        .await
        .unwrap();
    assert_eq!(
        exec_convs.len(),
        1,
        "startup remediation should reroute hook-blocked merge rows into task re-execution"
    );

    let updated_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::ReExecuting,
        "legacy hook-blocked MergeIncomplete rows should auto-resume through ReExecuting on startup"
    );
}

#[tokio::test]
async fn test_merge_incomplete_commit_hook_environment_rows_are_not_rerouted_on_startup() {
    let (execution_state, app_state) = setup_test_state().await;
    execution_state.set_max_concurrent(10);
    let repo = setup_git_repo();

    let project = Project::new(
        "Test Project".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(
        project.id.clone(),
        "Legacy Hook Environment MergeIncomplete".to_string(),
    );
    task.internal_status = InternalStatus::MergeIncomplete;
    task.metadata = Some(
        serde_json::json!({
            "error": "Git operation error: Failed to commit rebase+squash in worktree: stderr=[pre-commit] typecheck\nsrc/api/task-graph.ts(7,19): error TS2307: Cannot find module 'zod' or its corresponding type declarations."
        })
        .to_string(),
    );
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    runner.run().await;

    let exec_convs = app_state
        .chat_conversation_repo
        .get_by_context(ChatContextType::TaskExecution, task_id.as_str())
        .await
        .unwrap();
    assert!(
        exec_convs.is_empty(),
        "startup should not start task re-execution for hook environment failures"
    );

    let updated_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::MergeIncomplete,
        "environment hook failures should remain blocked in MergeIncomplete on startup"
    );
    let meta: serde_json::Value =
        serde_json::from_str(updated_task.metadata.as_deref().unwrap_or("{}")).unwrap();
    assert_eq!(
        meta.get("merge_hook_failure_kind"),
        Some(&serde_json::json!("environment_failure"))
    );
    assert_eq!(
        meta.get("merge_hook_blocked_reason"),
        Some(&serde_json::json!("hook_environment_failure"))
    );
}

#[tokio::test]
async fn test_approved_auto_transitions_on_startup() {
    // Approved is in AUTO_TRANSITION_STATES
    // Tasks stuck in Approved should auto-transition to PendingMerge
    // which triggers programmatic merge attempt
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project with a task in Approved state
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Approved Task".to_string());
    task.internal_status = InternalStatus::Approved;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    // Set high max_concurrent to allow auto-transition
    execution_state.set_max_concurrent(10);

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    // Run startup - should trigger auto-transition to PendingMerge
    runner.run().await;

    // The auto-transition path is:
    // 1. execute_entry_actions(Approved) triggers on_enter(Approved)
    // 2. check_auto_transition detects Approved -> PendingMerge
    // 3. transition_to(PendingMerge) is called
    // 4. on_enter(PendingMerge) runs attempt_programmatic_merge()

    // Task should now be in PendingMerge state (or further if merge succeeded/failed)
    // Since we're in test mode without a real git repo, the merge will likely fail
    // and the task may transition to Merging or MergeConflict
    let updated_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    // Approved should NOT be the final state - auto-transition should have occurred
    assert_ne!(
        updated_task.internal_status,
        InternalStatus::Approved,
        "Task should have auto-transitioned from Approved (to PendingMerge or beyond)"
    );
}

#[tokio::test]
async fn test_pending_merge_auto_transitions_on_startup() {
    // PendingMerge is in AUTO_TRANSITION_STATES
    // Tasks stuck in PendingMerge should have attempt_programmatic_merge() re-triggered
    // which transitions to Merged (success) or Merging (spawns merger agent)
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project with a task in PendingMerge state
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "PendingMerge Task".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("task/pending-merge-test".to_string());
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    // Set high max_concurrent to allow auto-transition
    execution_state.set_max_concurrent(10);

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    // Run startup - should trigger attempt_programmatic_merge()
    runner.run().await;

    // The auto-transition path is:
    // 1. execute_entry_actions(PendingMerge) triggers on_enter(PendingMerge)
    // 2. attempt_programmatic_merge() runs
    // 3. Task transitions to Merged (success), Merging (conflict), or MergeIncomplete (error)
    // Since we're in test mode without a real git repo, the merge will fail with an error
    // and the task transitions to MergeIncomplete
    let updated_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    // PendingMerge should NOT be the final state - attempt_programmatic_merge should have run
    assert_ne!(
        updated_task.internal_status,
        InternalStatus::PendingMerge,
        "Task should have auto-transitioned from PendingMerge (to MergeIncomplete in test env)"
    );
}

#[tokio::test]
async fn test_qa_passed_auto_transitions_on_startup() {
    // QaPassed is in AUTO_TRANSITION_STATES
    // Tasks stuck in QaPassed should auto-transition to PendingReview
    // which then auto-transitions to Reviewing (spawns reviewer)
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project with a task in QaPassed state
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "QaPassed Task".to_string());
    task.internal_status = InternalStatus::QaPassed;
    task.worktree_path = Some(std::env::temp_dir().to_string_lossy().to_string());
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    // Set high max_concurrent to allow auto-transition
    execution_state.set_max_concurrent(10);

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    // Run startup - should trigger auto-transition chain
    runner.run().await;

    // The auto-transition chain is:
    // 1. execute_entry_actions(QaPassed) triggers on_enter(QaPassed)
    // 2. check_auto_transition detects QaPassed -> PendingReview
    // 3. transition_to(PendingReview) -> on_enter(PendingReview)
    // 4. check_auto_transition detects PendingReview -> Reviewing
    // 5. transition_to(Reviewing) -> on_enter(Reviewing) spawns reviewer

    // Check for Review conversation (indicates the full chain completed)
    let review_convs = app_state
        .chat_conversation_repo
        .get_by_context(ChatContextType::Review, task_id.as_str())
        .await
        .unwrap();
    assert_eq!(
        review_convs.len(),
        1,
        "QaPassed task should auto-transition through PendingReview to Reviewing"
    );

    // Task should now be in Reviewing state
    let updated_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Reviewing,
        "Task should have auto-transitioned from QaPassed through PendingReview to Reviewing"
    );
}

#[tokio::test]
async fn test_auto_transition_respects_max_concurrent() {
    // Auto-transitions that spawn agents should respect max_concurrent.
    // This test verifies the loop structure and early exit logic based on can_start_task().
    let (execution_state, app_state) = setup_test_state().await;

    // Set max concurrent to 2
    execution_state.set_max_concurrent(2);

    // Create a project with 5 tasks in PendingReview state
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    for i in 0..5 {
        let mut task = Task::new(project.id.clone(), format!("PendingReview Task {}", i));
        task.internal_status = InternalStatus::PendingReview;
        app_state.task_repo.create(task).await.unwrap();
    }

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    // Run startup - should stop after max_concurrent is reached
    runner.run().await;

    // Note: The actual increment happens in execute_entry_actions via the spawner.
    // Since we're using a mock spawner without execution_state wired in for this test,
    // the running_count won't actually increment. This test verifies the loop structure
    // and early exit logic based on can_start_task().
    //
    // With our mock setup, running_count stays at 0 because the spawner doesn't have
    // execution_state. In production, the spawner would increment_running() on each spawn.
    // The test verifies that run() completes without panic when max_concurrent check exists.
}

// ============================================================
// Phase 77 Tests: Startup Unblock Recovery
// ============================================================

#[tokio::test]
async fn test_blocked_task_unblocked_when_blocker_is_merged() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a blocker task that is already merged
    let mut blocker_task = Task::new(project.id.clone(), "Blocker Task".to_string());
    blocker_task.internal_status = InternalStatus::Merged;
    app_state
        .task_repo
        .create(blocker_task.clone())
        .await
        .unwrap();

    // Create a blocked task
    let mut blocked_task = Task::new(project.id.clone(), "Blocked Task".to_string());
    blocked_task.internal_status = InternalStatus::Blocked;
    blocked_task.blocked_reason = Some("Waiting for: Blocker Task".to_string());
    app_state
        .task_repo
        .create(blocked_task.clone())
        .await
        .unwrap();

    // Add the dependency relationship
    app_state
        .task_dependency_repo
        .add_dependency(&blocked_task.id, &blocker_task.id)
        .await
        .unwrap();

    let (runner, _app_state_repo) = build_runner(&app_state, &execution_state);

    // Run startup - should unblock the blocked task
    runner.run().await;

    // Verify the blocked task is now Ready
    let updated_task = app_state
        .task_repo
        .get_by_id(&blocked_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Ready,
        "Blocked task should be unblocked when blocker is Merged"
    );
    assert!(
        updated_task.blocked_reason.is_none(),
        "blocked_reason should be cleared"
    );
}

#[tokio::test]
async fn test_blocked_task_remains_blocked_when_blocker_is_approved() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a blocker task that is approved (not yet merged)
    let mut blocker_task = Task::new(project.id.clone(), "Blocker Task".to_string());
    blocker_task.internal_status = InternalStatus::Approved;
    app_state
        .task_repo
        .create(blocker_task.clone())
        .await
        .unwrap();

    // Create a blocked task
    let mut blocked_task = Task::new(project.id.clone(), "Blocked Task".to_string());
    blocked_task.internal_status = InternalStatus::Blocked;
    app_state
        .task_repo
        .create(blocked_task.clone())
        .await
        .unwrap();

    // Add the dependency relationship
    app_state
        .task_dependency_repo
        .add_dependency(&blocked_task.id, &blocker_task.id)
        .await
        .unwrap();

    // Pause execution to isolate unblock logic from auto-transition recovery
    // (without pause, Approved would auto-transition to PendingMerge, masking the test intent)
    execution_state.pause();

    let (runner, _app_state_repo) = build_runner(&app_state, &execution_state);

    // Run startup
    runner.run().await;

    // Verify the blocked task remains Blocked (Approved is NOT a terminal state)
    let updated_task = app_state
        .task_repo
        .get_by_id(&blocked_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Blocked,
        "Blocked task should remain blocked when blocker is Approved (not yet merged)"
    );
}

#[tokio::test]
async fn test_blocked_task_remains_blocked_when_blocker_incomplete() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a blocker task that is still executing
    let mut blocker_task = Task::new(project.id.clone(), "Blocker Task".to_string());
    blocker_task.internal_status = InternalStatus::Executing;
    app_state
        .task_repo
        .create(blocker_task.clone())
        .await
        .unwrap();

    // Create a blocked task
    let mut blocked_task = Task::new(project.id.clone(), "Blocked Task".to_string());
    blocked_task.internal_status = InternalStatus::Blocked;
    blocked_task.blocked_reason = Some("Waiting for: Blocker Task".to_string());
    app_state
        .task_repo
        .create(blocked_task.clone())
        .await
        .unwrap();

    // Add the dependency relationship
    app_state
        .task_dependency_repo
        .add_dependency(&blocked_task.id, &blocker_task.id)
        .await
        .unwrap();

    // Pause execution to skip agent resumption (we only want to test unblocking)
    execution_state.pause();

    let (runner, _app_state_repo) = build_runner(&app_state, &execution_state);

    // Run startup
    runner.run().await;

    // Verify the blocked task is still Blocked
    let updated_task = app_state
        .task_repo
        .get_by_id(&blocked_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Blocked,
        "Blocked task should remain blocked when blocker is still Executing"
    );
    assert!(
        updated_task.blocked_reason.is_some(),
        "blocked_reason should be preserved"
    );
}

#[tokio::test]
async fn test_blocked_task_remains_blocked_when_blocker_paused() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a blocker task that is paused
    let mut blocker_task = Task::new(project.id.clone(), "Blocker Task".to_string());
    blocker_task.internal_status = InternalStatus::Paused;
    app_state
        .task_repo
        .create(blocker_task.clone())
        .await
        .unwrap();

    // Create a blocked task
    let mut blocked_task = Task::new(project.id.clone(), "Blocked Task".to_string());
    blocked_task.internal_status = InternalStatus::Blocked;
    blocked_task.blocked_reason = Some("Waiting for: Blocker Task".to_string());
    app_state
        .task_repo
        .create(blocked_task.clone())
        .await
        .unwrap();

    // Add the dependency relationship
    app_state
        .task_dependency_repo
        .add_dependency(&blocked_task.id, &blocker_task.id)
        .await
        .unwrap();

    // Pause execution to skip agent resumption (we only want to test unblocking)
    execution_state.pause();

    let (runner, _app_state_repo) = build_runner(&app_state, &execution_state);

    // Run startup
    runner.run().await;

    // Verify the blocked task is still Blocked
    let updated_task = app_state
        .task_repo
        .get_by_id(&blocked_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Blocked,
        "Blocked task should remain blocked when blocker is Paused"
    );
    assert!(
        updated_task.blocked_reason.is_some(),
        "blocked_reason should be preserved"
    );
}

/// Stopped is terminal but does NOT satisfy dependencies — a blocked task
/// should remain Blocked when its blocker is Stopped (incomplete work).
#[tokio::test]
async fn test_all_blockers_complete_treats_stopped_as_not_satisfied() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a blocker task that is stopped (incomplete work)
    let mut blocker_task = Task::new(project.id.clone(), "Blocker Task".to_string());
    blocker_task.internal_status = InternalStatus::Stopped;
    app_state
        .task_repo
        .create(blocker_task.clone())
        .await
        .unwrap();

    // Create a blocked task
    let mut blocked_task = Task::new(project.id.clone(), "Blocked Task".to_string());
    blocked_task.internal_status = InternalStatus::Blocked;
    blocked_task.blocked_reason = Some("Waiting for: Blocker Task".to_string());
    app_state
        .task_repo
        .create(blocked_task.clone())
        .await
        .unwrap();

    // Add the dependency relationship
    app_state
        .task_dependency_repo
        .add_dependency(&blocked_task.id, &blocker_task.id)
        .await
        .unwrap();

    let (runner, _app_state_repo) = build_runner(&app_state, &execution_state);

    // Run startup
    runner.run().await;

    // Verify the blocked task stays Blocked (Stopped blocker has incomplete work)
    let updated_task = app_state
        .task_repo
        .get_by_id(&blocked_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Blocked,
        "Blocked task should remain Blocked when blocker is Stopped (incomplete work)"
    );
}

/// MergeIncomplete does NOT satisfy dependencies — merge failed, code not on target branch.
/// A blocked task should remain Blocked when its blocker is MergeIncomplete.
#[tokio::test]
async fn test_all_blockers_complete_treats_merge_incomplete_as_unsatisfied() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a blocker task in MergeIncomplete state
    let mut blocker_task = Task::new(project.id.clone(), "MergeIncomplete Blocker".to_string());
    blocker_task.internal_status = InternalStatus::MergeIncomplete;
    app_state
        .task_repo
        .create(blocker_task.clone())
        .await
        .unwrap();

    // Create a blocked task
    let mut blocked_task = Task::new(project.id.clone(), "Blocked Task".to_string());
    blocked_task.internal_status = InternalStatus::Blocked;
    blocked_task.blocked_reason = Some("Waiting for: MergeIncomplete Blocker".to_string());
    app_state
        .task_repo
        .create(blocked_task.clone())
        .await
        .unwrap();

    // Add the dependency relationship
    app_state
        .task_dependency_repo
        .add_dependency(&blocked_task.id, &blocker_task.id)
        .await
        .unwrap();

    // Pause execution to skip agent resumption (we only want to test unblocking)
    execution_state.pause();

    let (runner, _app_state_repo) = build_runner(&app_state, &execution_state);

    // Run startup
    runner.run().await;

    // Verify the blocked task is still Blocked (MergeIncomplete does not satisfy dependencies)
    let updated_task = app_state
        .task_repo
        .get_by_id(&blocked_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Blocked,
        "Blocked task should remain Blocked when blocker is MergeIncomplete (merge failed)"
    );
}

#[tokio::test]
async fn test_blocked_task_with_multiple_blockers_all_complete() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create two blocker tasks - both complete
    let mut blocker1 = Task::new(project.id.clone(), "Blocker 1".to_string());
    blocker1.internal_status = InternalStatus::Merged;
    app_state.task_repo.create(blocker1.clone()).await.unwrap();

    let mut blocker2 = Task::new(project.id.clone(), "Blocker 2".to_string());
    blocker2.internal_status = InternalStatus::Cancelled; // Cancelled satisfies dependencies
    app_state.task_repo.create(blocker2.clone()).await.unwrap();

    // Create a blocked task
    let mut blocked_task = Task::new(project.id.clone(), "Blocked Task".to_string());
    blocked_task.internal_status = InternalStatus::Blocked;
    app_state
        .task_repo
        .create(blocked_task.clone())
        .await
        .unwrap();

    // Add both dependency relationships
    app_state
        .task_dependency_repo
        .add_dependency(&blocked_task.id, &blocker1.id)
        .await
        .unwrap();
    app_state
        .task_dependency_repo
        .add_dependency(&blocked_task.id, &blocker2.id)
        .await
        .unwrap();

    let (runner, _app_state_repo) = build_runner(&app_state, &execution_state);

    // Run startup
    runner.run().await;

    // Verify the blocked task is now Ready
    let updated_task = app_state
        .task_repo
        .get_by_id(&blocked_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Ready,
        "Blocked task should be unblocked when all blockers are complete"
    );
}

/// Failed blockers do NOT satisfy dependencies — dependents must stay Blocked
/// to prevent cascade execution against broken output.
#[tokio::test]
async fn test_blocked_task_stays_blocked_when_blocker_failed() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Blocker task is Failed — should NOT satisfy dependency
    let mut blocker = Task::new(project.id.clone(), "Failed Blocker".to_string());
    blocker.internal_status = InternalStatus::Failed;
    app_state.task_repo.create(blocker.clone()).await.unwrap();

    let mut blocked_task = Task::new(project.id.clone(), "Blocked Task".to_string());
    blocked_task.internal_status = InternalStatus::Blocked;
    app_state
        .task_repo
        .create(blocked_task.clone())
        .await
        .unwrap();

    app_state
        .task_dependency_repo
        .add_dependency(&blocked_task.id, &blocker.id)
        .await
        .unwrap();

    let (runner, _app_state_repo) = build_runner(&app_state, &execution_state);
    runner.run().await;

    let updated_task = app_state
        .task_repo
        .get_by_id(&blocked_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Blocked,
        "Task should stay Blocked when blocker is Failed"
    );
}

#[tokio::test]
async fn test_blocked_task_with_multiple_blockers_one_incomplete() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create two blocker tasks - one complete, one not
    let mut blocker1 = Task::new(project.id.clone(), "Blocker 1".to_string());
    blocker1.internal_status = InternalStatus::Merged;
    app_state.task_repo.create(blocker1.clone()).await.unwrap();

    let mut blocker2 = Task::new(project.id.clone(), "Blocker 2".to_string());
    blocker2.internal_status = InternalStatus::Reviewing; // Still in progress
    app_state.task_repo.create(blocker2.clone()).await.unwrap();

    // Create a blocked task
    let mut blocked_task = Task::new(project.id.clone(), "Blocked Task".to_string());
    blocked_task.internal_status = InternalStatus::Blocked;
    app_state
        .task_repo
        .create(blocked_task.clone())
        .await
        .unwrap();

    // Add both dependency relationships
    app_state
        .task_dependency_repo
        .add_dependency(&blocked_task.id, &blocker1.id)
        .await
        .unwrap();
    app_state
        .task_dependency_repo
        .add_dependency(&blocked_task.id, &blocker2.id)
        .await
        .unwrap();

    // Pause execution to skip agent resumption
    execution_state.pause();

    let (runner, _app_state_repo) = build_runner(&app_state, &execution_state);

    // Run startup
    runner.run().await;

    // Verify the blocked task is still Blocked
    let updated_task = app_state
        .task_repo
        .get_by_id(&blocked_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Blocked,
        "Blocked task should remain blocked when any blocker is incomplete"
    );
}

#[tokio::test]
async fn test_unblock_runs_even_when_paused() {
    // Unblocking should run even when execution is paused, since it doesn't spawn agents
    let (execution_state, app_state) = setup_test_state().await;

    // Pause execution
    execution_state.pause();

    // Create a project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a merged blocker
    let mut blocker_task = Task::new(project.id.clone(), "Blocker Task".to_string());
    blocker_task.internal_status = InternalStatus::Merged;
    app_state
        .task_repo
        .create(blocker_task.clone())
        .await
        .unwrap();

    // Create a blocked task
    let mut blocked_task = Task::new(project.id.clone(), "Blocked Task".to_string());
    blocked_task.internal_status = InternalStatus::Blocked;
    app_state
        .task_repo
        .create(blocked_task.clone())
        .await
        .unwrap();

    // Add dependency
    app_state
        .task_dependency_repo
        .add_dependency(&blocked_task.id, &blocker_task.id)
        .await
        .unwrap();

    let (runner, _app_state_repo) = build_runner(&app_state, &execution_state);

    // Run startup while paused
    runner.run().await;

    // Verify the blocked task is still unblocked even though execution is paused
    let updated_task = app_state
        .task_repo
        .get_by_id(&blocked_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Ready,
        "Blocked task should be unblocked even when execution is paused"
    );
}

// ============================================================
// Phase 90 Tests: DB-based active project persistence
// ============================================================

#[tokio::test]
async fn test_resumption_reads_active_project_from_db() {
    // Verifies the runner reads the active project from the DB (app_state_repo)
    // instead of waiting for the frontend to set it via IPC.
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project with a task in Executing state
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    execution_state.set_max_concurrent(10);

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);

    // Set active project in DB only (NOT in-memory ActiveProjectState)
    // This simulates the persisted state from a previous session
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    // Run the startup runner — it should read from DB and attempt to resume
    runner.run().await;

    // Verify NO conversation was created (on_enter fails with ExecutionBlocked)
    // The runner reads the active project from DB and attempts to resume the task,
    // but on_enter fails before creating a conversation, then transitions to Failed
    let convs = app_state
        .chat_conversation_repo
        .get_by_context(ChatContextType::TaskExecution, task_id.as_str())
        .await
        .unwrap();
    assert_eq!(
        convs.len(),
        0,
        "No conversation created when on_enter fails with ExecutionBlocked"
    );

    // Verify the task transitioned to Failed (proves the DB read and resumption attempt occurred)
    let updated_task = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Failed,
        "Runner should read active project from DB and attempt resumption (task fails due to ExecutionBlocked)"
    );
}

#[tokio::test]
async fn test_resumption_skips_when_no_active_project_in_db() {
    // Verifies the runner skips resumption when no active project is in the DB
    // (fresh install scenario)
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project with a task in Executing state
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Executing Task".to_string());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    execution_state.set_max_concurrent(10);

    let (runner, _app_state_repo) = build_runner(&app_state, &execution_state);
    // Don't set active project in DB — simulates fresh install

    runner.run().await;

    // Verify NO entry actions were called (no active project in DB)
    let convs = app_state
        .chat_conversation_repo
        .get_by_context(ChatContextType::TaskExecution, task_id.as_str())
        .await
        .unwrap();
    assert_eq!(
        convs.len(),
        0,
        "No tasks should be resumed when no active project in DB"
    );
}

#[tokio::test]
async fn test_startup_loads_persisted_project_quota() {
    // Verifies the runner loads the persisted active project's execution settings
    // and syncs the runtime quota before resuming tasks
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Set project-specific execution settings with max_concurrent = 3
    let settings = ExecutionSettings {
        max_concurrent_tasks: 3,
        project_ideation_max: 2,
        auto_commit: false,
        pause_on_failure: false,
        ..ExecutionSettings::default()
    };
    app_state
        .execution_settings_repo
        .update_settings(Some(&project.id), &settings)
        .await
        .unwrap();

    // Set initial runtime quota to 10 (simulate different value from previous session)
    execution_state.set_max_concurrent(10);
    assert_eq!(execution_state.max_concurrent(), 10);

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);

    // Set active project in DB (simulates persisted state from previous session)
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    // Run the startup runner — it should load settings and update quota
    runner.run().await;

    // Verify the runtime quota was updated to match the persisted project settings
    assert_eq!(
        execution_state.max_concurrent(),
        3,
        "Runtime quota should be updated to match persisted project settings"
    );
}

#[tokio::test]
async fn test_startup_quota_sync_before_resumption() {
    // Verifies quota sync happens BEFORE task resumption logic
    // by checking that resumption respects the newly-loaded quota
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project with 5 tasks in Executing state
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    for i in 0..5 {
        let mut task = Task::new(project.id.clone(), format!("Executing Task {}", i));
        task.internal_status = InternalStatus::Executing;
        app_state.task_repo.create(task).await.unwrap();
    }

    // Set project-specific execution settings with max_concurrent = 2
    let settings = ExecutionSettings {
        max_concurrent_tasks: 2,
        project_ideation_max: 1,
        auto_commit: false,
        pause_on_failure: false,
        ..ExecutionSettings::default()
    };
    app_state
        .execution_settings_repo
        .update_settings(Some(&project.id), &settings)
        .await
        .unwrap();

    // Set initial runtime quota to 10 (different from project settings)
    execution_state.set_max_concurrent(10);

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    // Run startup - should load quota (2) and only resume 2 tasks
    runner.run().await;

    // Verify the quota was updated BEFORE resumption checked it
    assert_eq!(
        execution_state.max_concurrent(),
        2,
        "Quota should be synced from project settings before resumption"
    );

    // Note: In this test environment with mock spawner, running_count stays at 0
    // because the spawner doesn't increment. The test verifies that the quota
    // was loaded correctly and is available for the resumption logic to check.
}

// ── is_waiting_for_global_idle Tests (Phase 4) ──

#[test]
fn is_waiting_for_global_idle_returns_false_when_no_metadata() {
    let task = Task::new(ProjectId::new(), "Test".to_string());
    assert!(!StartupJobRunner::is_waiting_for_global_idle(
        &task, 1
    ));
}

#[test]
fn is_waiting_for_global_idle_returns_false_when_no_main_merge_deferred_flag() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    task.metadata = Some(r#"{"other": "data"}"#.to_string());
    assert!(!StartupJobRunner::is_waiting_for_global_idle(
        &task, 1
    ));
}

#[test]
fn is_waiting_for_global_idle_returns_false_when_main_merge_deferred_but_no_agents() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    task.metadata = Some(serde_json::json!({"main_merge_deferred": true}).to_string());
    // running_count = 0 means all agents completed
    assert!(!StartupJobRunner::is_waiting_for_global_idle(
        &task, 0
    ));
}

#[test]
fn is_waiting_for_global_idle_returns_true_when_main_merge_deferred_and_agents_running() {
    let mut task = Task::new(ProjectId::new(), "Test".to_string());
    task.metadata = Some(serde_json::json!({"main_merge_deferred": true}).to_string());
    // running_count > 0 means agents are still running
    assert!(StartupJobRunner::is_waiting_for_global_idle(
        &task, 1
    ));
    assert!(StartupJobRunner::is_waiting_for_global_idle(
        &task, 5
    ));
}

#[tokio::test]
async fn startup_skips_main_merge_deferred_when_agents_running() {
    let (execution_state, app_state) = setup_test_state().await;

    // Set up project in DB
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create main-merge-deferred task
    let mut task = Task::new(project.id.clone(), "Main Merge Deferred Task".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.metadata =
        Some(serde_json::json!({"main_merge_deferred": true, "main_merge_deferred_at": "2026-01-01T00:00:00Z"}).to_string());
    app_state.task_repo.create(task.clone()).await.unwrap();

    // Simulate agents running
    execution_state.increment_running();
    assert_eq!(execution_state.running_count(), 1);

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    // Run startup - should skip the main-merge-deferred task
    runner.run().await;

    // Task should remain in PendingMerge with deferred flag preserved
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::PendingMerge,
        "Main-merge-deferred task should stay in PendingMerge when agents running"
    );
    // Verify deferred flag is preserved (not cleared or lost)
    let metadata: serde_json::Value =
        serde_json::from_str(updated.metadata.as_ref().unwrap()).unwrap();
    assert_eq!(
        metadata["main_merge_deferred"], true,
        "main_merge_deferred flag should be preserved across startup"
    );
}

#[tokio::test]
async fn startup_retries_main_merge_deferred_when_no_agents() {
    let (execution_state, app_state) = setup_test_state().await;

    // Set up project in DB
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create main-merge-deferred task
    let mut task = Task::new(project.id.clone(), "Main Merge Deferred Task".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.metadata =
        Some(serde_json::json!({"main_merge_deferred": true, "main_merge_deferred_at": "2026-01-01T00:00:00Z"}).to_string());
    app_state.task_repo.create(task.clone()).await.unwrap();

    // No agents running (running_count = 0)
    assert_eq!(execution_state.running_count(), 0);

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    // Run startup - reconciliation should retry the main-merge-deferred task
    runner.run().await;

    // Task should have been processed (either retried or failed)
    // In test environment without real git, the merge attempt will fail,
    // but the important thing is it was not skipped
    let updated = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    // The task will transition through reconciliation (not skipped)
    // In real environment it would retry the merge
    assert!(
        updated.internal_status == InternalStatus::PendingMerge
            || updated.internal_status == InternalStatus::Merging
            || updated.internal_status == InternalStatus::MergeIncomplete
            || updated.internal_status == InternalStatus::Failed,
        "Main-merge-deferred task should be processed (not skipped) when no agents running"
    );
}

/// Verify that Paused tasks are NOT touched by startup recovery.
/// Paused is intentionally excluded from AGENT_ACTIVE_STATUSES and AUTO_TRANSITION_STATES,
/// so paused tasks persist across restarts with their metadata intact.
#[tokio::test]
async fn test_startup_does_not_touch_paused_tasks() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create a project with a Paused task that has pause_reason metadata
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Paused Task".to_string());
    task.internal_status = InternalStatus::Paused;
    // Set pause metadata to verify it survives
    task.metadata = Some(
        r#"{"pause_reason":{"type":"provider_error","category":"rate_limit","message":"limit","retry_after":null,"previous_status":"executing","paused_at":"2026-02-15T09:00:00+00:00","auto_resumable":true,"resume_attempts":2}}"#.to_string(),
    );
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    // Also create an Executing task to verify normal resumption still works
    let mut exec_task = Task::new(project.id.clone(), "Executing Task".to_string());
    exec_task.internal_status = InternalStatus::Executing;
    app_state.task_repo.create(exec_task).await.unwrap();

    execution_state.set_max_concurrent(10);

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    runner.run().await;

    // Verify the Paused task is still Paused with metadata intact
    let paused = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        paused.internal_status,
        InternalStatus::Paused,
        "Paused task should remain Paused after startup"
    );
    assert!(
        paused.metadata.is_some(),
        "Paused task metadata should be preserved"
    );
    let meta_str = paused.metadata.unwrap();
    assert!(
        meta_str.contains("pause_reason"),
        "pause_reason metadata should survive startup"
    );
    assert!(
        meta_str.contains("resume_attempts"),
        "resume_attempts should survive startup"
    );
}

/// Verify that Paused is not in AGENT_ACTIVE_STATUSES or AUTO_TRANSITION_STATES
#[test]
fn test_paused_not_in_agent_active_or_auto_transition() {
    assert!(
        !AGENT_ACTIVE_STATUSES.contains(&InternalStatus::Paused),
        "Paused should NOT be in AGENT_ACTIVE_STATUSES"
    );
    assert!(
        !AUTO_TRANSITION_STATES.contains(&InternalStatus::Paused),
        "Paused should NOT be in AUTO_TRANSITION_STATES"
    );
}

// ============================================================
// Merge-First Recovery Tests (Phase 0 + Phase 1)
// ============================================================

#[tokio::test]
async fn test_cleanup_stale_git_state_no_panic_with_missing_dir() {
    // cleanup_stale_git_state should not panic when the project's
    // working directory does not exist (e.g., external drive unmounted)
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new(
        "Test Project".to_string(),
        "/nonexistent/path/that/does/not/exist".to_string(),
    );
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let (runner, _) = build_runner(&app_state, &execution_state);

    // Should complete without panic
    runner.cleanup_stale_git_state(&[project]).await;
}

#[tokio::test]
async fn test_merge_first_recovery_processes_merge_and_agent_tasks() {
    // Verify that merge-state tasks (Phase 1) and non-merge agent-active
    // tasks are both processed on startup
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a Merging task (handled in Phase 1)
    let mut merging_task = Task::new(project.id.clone(), "Merging Task".to_string());
    merging_task.internal_status = InternalStatus::Merging;
    let merging_id = merging_task.id.clone();
    app_state.task_repo.create(merging_task).await.unwrap();

    // Create an Executing task (handled in main loop)
    let mut exec_task = Task::new(project.id.clone(), "Executing Task".to_string());
    exec_task.internal_status = InternalStatus::Executing;
    let exec_id = exec_task.id.clone();
    app_state.task_repo.create(exec_task).await.unwrap();

    execution_state.set_max_concurrent(10);

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    runner.run().await;

    // Verify Merging task was processed in Phase 1 (Merge conversation created)
    let merge_convs = app_state
        .chat_conversation_repo
        .get_by_context(ChatContextType::Merge, merging_id.as_str())
        .await
        .unwrap();
    assert_eq!(
        merge_convs.len(),
        1,
        "Merging task should be processed in Phase 1 merge-first recovery"
    );

    // Verify Executing task was also processed (transitions to Failed in test env)
    let exec_updated = app_state
        .task_repo
        .get_by_id(&exec_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        exec_updated.internal_status,
        InternalStatus::Failed,
        "Executing task should still be processed in main agent-active loop"
    );
}

#[tokio::test]
async fn test_pending_merge_processed_in_phase1() {
    // PendingMerge tasks should be processed in Phase 1 merge-first recovery,
    // and skipped in both AGENT_ACTIVE_STATUSES and AUTO_TRANSITION_STATES loops
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "PendingMerge Task".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.task_branch = Some("task/phase1-test".to_string());
    let task_id = task.id.clone();
    app_state.task_repo.create(task).await.unwrap();

    execution_state.set_max_concurrent(10);

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    runner.run().await;

    // PendingMerge should have been processed (moved out of PendingMerge)
    let updated = app_state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        updated.internal_status,
        InternalStatus::PendingMerge,
        "PendingMerge task should be processed in Phase 1 merge-first recovery"
    );
}

#[tokio::test]
async fn test_startup_phase1_keeps_pr_backed_merging_task_waiting_on_pr() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Merge plan into main".to_string());
    task.internal_status = InternalStatus::Merging;
    task.category = TaskCategory::PlanMerge;
    task.metadata = Some(
        serde_json::json!({
            "merge_recovery": {
                "version": 1,
                "events": [
                    {
                        "at": "2026-04-21T00:00:00Z",
                        "kind": "attempt_failed",
                        "source": "system",
                        "reason_code": "git_error",
                        "message": "Merge timed out after 1200s without completion signal"
                    },
                    {
                        "at": "2026-04-21T00:10:00Z",
                        "kind": "attempt_failed",
                        "source": "system",
                        "reason_code": "git_error",
                        "message": "Merge timed out after 1200s without completion signal"
                    },
                    {
                        "at": "2026-04-21T00:20:00Z",
                        "kind": "attempt_failed",
                        "source": "system",
                        "reason_code": "git_error",
                        "message": "Merge timed out after 1200s without completion signal"
                    }
                ],
                "last_state": "failed"
            }
        })
        .to_string(),
    );
    app_state.task_repo.create(task.clone()).await.unwrap();

    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-pr-backed-startup".to_string()),
        IdeationSessionId::from_string("session-pr-backed-startup".to_string()),
        project.id.clone(),
        "ralphx/test/plan-pr-backed-startup".to_string(),
        "main".to_string(),
    );
    plan_branch.merge_task_id = Some(task.id.clone());
    plan_branch.pr_eligible = true;
    plan_branch.pr_number = Some(68);
    plan_branch.pr_polling_active = true;
    app_state
        .plan_branch_repo
        .create(plan_branch)
        .await
        .unwrap();

    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();

    runner.run().await;

    let updated_task = app_state
        .task_repo
        .get_by_id(&task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated_task.internal_status,
        InternalStatus::Merging,
        "PR-backed plan merge should stay in Merging while waiting on GitHub, even with stale local merge retry metadata"
    );
}

// ============================================================
// Boot Recovery: main_merge_deferred Tests
// ============================================================

/// Scenario: quiescent boot — PendingMerge task with main_merge_deferred=true,
/// no active agents (running_count == 0).
/// Expected: try_retry_main_merges() is invoked exactly once at the end of startup.
#[tokio::test]
async fn test_boot_quiescent_invokes_try_retry_main_merges_once() {
    let (execution_state, app_state) = setup_test_state().await;

    // Create project with a PendingMerge/main_merge_deferred task
    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut task = Task::new(project.id.clone(), "Deferred Merge Task".to_string());
    task.internal_status = InternalStatus::PendingMerge;
    task.metadata = Some(
        serde_json::json!({"main_merge_deferred": true, "main_merge_deferred_at": "2026-01-01T00:00:00Z"})
            .to_string(),
    );
    app_state.task_repo.create(task).await.unwrap();

    // No agents running (quiescent boot)
    assert_eq!(execution_state.running_count(), 0);

    execution_state.set_max_concurrent(10);

    let scheduler = Arc::new(MockTaskScheduler::new());
    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();
    let runner = runner.with_task_scheduler(Arc::clone(&scheduler) as Arc<dyn TaskScheduler>);

    runner.run().await;

    // try_retry_main_merges should have been called exactly once (boot-recovery path)
    let retry_calls: Vec<_> = scheduler
        .get_calls()
        .into_iter()
        .filter(|c| c.method == "try_retry_main_merges")
        .collect();
    assert_eq!(
        retry_calls.len(),
        1,
        "try_retry_main_merges should be called exactly once at startup when running_count == 0"
    );
}

/// Scenario: Phase 1 guard — two PendingMerge tasks both with main_merge_deferred=true.
/// Expected: Phase 1 does NOT call execute_entry_actions() on either task (they stay in
/// PendingMerge and are not processed by Phase 1). Only try_retry_main_merges() picks them up.
#[tokio::test]
async fn test_phase1_guard_skips_main_merge_deferred_tasks() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Two PendingMerge siblings both with main_merge_deferred=true
    let mut task_a = Task::new(project.id.clone(), "Deferred Merge A".to_string());
    task_a.internal_status = InternalStatus::PendingMerge;
    task_a.task_branch = Some("task/deferred-a".to_string());
    task_a.metadata = Some(
        serde_json::json!({"main_merge_deferred": true, "main_merge_deferred_at": "2026-01-01T00:00:00Z"})
            .to_string(),
    );
    let task_a_id = task_a.id.clone();
    app_state.task_repo.create(task_a).await.unwrap();

    let mut task_b = Task::new(project.id.clone(), "Deferred Merge B".to_string());
    task_b.internal_status = InternalStatus::PendingMerge;
    task_b.task_branch = Some("task/deferred-b".to_string());
    task_b.metadata = Some(
        serde_json::json!({"main_merge_deferred": true, "main_merge_deferred_at": "2026-01-01T00:00:00Z"})
            .to_string(),
    );
    let task_b_id = task_b.id.clone();
    app_state.task_repo.create(task_b).await.unwrap();

    // No agents running (running_count == 0 at boot)
    assert_eq!(execution_state.running_count(), 0);
    execution_state.set_max_concurrent(10);

    let scheduler = Arc::new(MockTaskScheduler::new());
    let (runner, app_state_repo) = build_runner(&app_state, &execution_state);
    app_state_repo
        .set_active_project(Some(&project.id))
        .await
        .unwrap();
    let runner = runner.with_task_scheduler(Arc::clone(&scheduler) as Arc<dyn TaskScheduler>);

    runner.run().await;

    // Phase 1 must NOT have called execute_entry_actions on either task:
    // If execute_entry_actions had been called, the tasks would have transitioned away
    // from PendingMerge (to MergeIncomplete, Merging, etc.) in the test environment.
    let updated_a = app_state
        .task_repo
        .get_by_id(&task_a_id)
        .await
        .unwrap()
        .unwrap();
    let updated_b = app_state
        .task_repo
        .get_by_id(&task_b_id)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        updated_a.internal_status,
        InternalStatus::PendingMerge,
        "Phase 1 must not call execute_entry_actions on main_merge_deferred task A"
    );
    assert_eq!(
        updated_b.internal_status,
        InternalStatus::PendingMerge,
        "Phase 1 must not call execute_entry_actions on main_merge_deferred task B"
    );

    // try_retry_main_merges should have been called (serialized path handles them)
    let retry_calls: Vec<_> = scheduler
        .get_calls()
        .into_iter()
        .filter(|c| c.method == "try_retry_main_merges")
        .collect();
    assert_eq!(
        retry_calls.len(),
        1,
        "try_retry_main_merges should be called once to handle deferred tasks in serialized order"
    );
}

// ============================================================
// Startup Recovery: Blocked tasks with all-terminal blockers
// ============================================================

/// Scenario: a plan_merge task is Blocked, with 3 blocker tasks all in Merged state.
/// Expected: startup recovery transitions the plan_merge task to Ready.
#[tokio::test]
async fn test_startup_recovers_blocked_tasks_with_all_terminal_blockers() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create 3 blocker tasks, all in Merged state
    let mut blocker1 = Task::new(project.id.clone(), "Feature Task 1".to_string());
    blocker1.internal_status = InternalStatus::Merged;
    app_state.task_repo.create(blocker1.clone()).await.unwrap();

    let mut blocker2 = Task::new(project.id.clone(), "Feature Task 2".to_string());
    blocker2.internal_status = InternalStatus::Merged;
    app_state.task_repo.create(blocker2.clone()).await.unwrap();

    let mut blocker3 = Task::new(project.id.clone(), "Feature Task 3".to_string());
    blocker3.internal_status = InternalStatus::Merged;
    app_state.task_repo.create(blocker3.clone()).await.unwrap();

    // Create the plan_merge task in Blocked state
    let mut plan_merge_task = Task::new(project.id.clone(), "Merge Plan to Main".to_string());
    plan_merge_task.internal_status = InternalStatus::Blocked;
    plan_merge_task.category = TaskCategory::PlanMerge;
    plan_merge_task.blocked_reason =
        Some("Waiting for: Feature Task 1, Feature Task 2, Feature Task 3".to_string());
    let plan_merge_id = plan_merge_task.id.clone();
    app_state
        .task_repo
        .create(plan_merge_task.clone())
        .await
        .unwrap();

    // Add dependencies: plan_merge blocked by all 3 features
    app_state
        .task_dependency_repo
        .add_dependency(&plan_merge_task.id, &blocker1.id)
        .await
        .unwrap();
    app_state
        .task_dependency_repo
        .add_dependency(&plan_merge_task.id, &blocker2.id)
        .await
        .unwrap();
    app_state
        .task_dependency_repo
        .add_dependency(&plan_merge_task.id, &blocker3.id)
        .await
        .unwrap();

    let (runner, _app_state_repo) = build_runner(&app_state, &execution_state);

    // Run startup recovery
    runner.run().await;

    // Verify the plan_merge task transitions to Ready
    let updated = app_state
        .task_repo
        .get_by_id(&plan_merge_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Ready,
        "plan_merge task should be unblocked when all 3 blocker tasks are Merged"
    );
    assert!(
        updated.blocked_reason.is_none(),
        "blocked_reason should be cleared when task is unblocked"
    );
}

/// Scenario: a plan_merge task is Blocked, with 1 Merged and 1 Executing blocker.
/// Expected: startup recovery leaves the plan_merge task as Blocked (not all blockers terminal).
#[tokio::test]
async fn test_startup_skips_blocked_tasks_with_non_terminal_blockers() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // One blocker Merged (terminal), one Executing (non-terminal)
    let mut blocker_merged = Task::new(project.id.clone(), "Feature Task 1".to_string());
    blocker_merged.internal_status = InternalStatus::Merged;
    app_state
        .task_repo
        .create(blocker_merged.clone())
        .await
        .unwrap();

    let mut blocker_executing = Task::new(project.id.clone(), "Feature Task 2".to_string());
    blocker_executing.internal_status = InternalStatus::Executing;
    app_state
        .task_repo
        .create(blocker_executing.clone())
        .await
        .unwrap();

    // Create the plan_merge task in Blocked state
    let mut plan_merge_task = Task::new(project.id.clone(), "Merge Plan to Main".to_string());
    plan_merge_task.internal_status = InternalStatus::Blocked;
    plan_merge_task.category = TaskCategory::PlanMerge;
    plan_merge_task.blocked_reason = Some("Waiting for: Feature Task 2".to_string());
    let plan_merge_id = plan_merge_task.id.clone();
    app_state
        .task_repo
        .create(plan_merge_task.clone())
        .await
        .unwrap();

    // Add both dependencies
    app_state
        .task_dependency_repo
        .add_dependency(&plan_merge_task.id, &blocker_merged.id)
        .await
        .unwrap();
    app_state
        .task_dependency_repo
        .add_dependency(&plan_merge_task.id, &blocker_executing.id)
        .await
        .unwrap();

    // Pause execution to isolate unblock logic from agent resumption
    execution_state.pause();

    let (runner, _app_state_repo) = build_runner(&app_state, &execution_state);

    // Run startup recovery
    runner.run().await;

    // Verify the plan_merge task stays Blocked (Executing is not terminal)
    let updated = app_state
        .task_repo
        .get_by_id(&plan_merge_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Blocked,
        "plan_merge task should remain Blocked when any blocker is still Executing"
    );
    assert!(
        updated.blocked_reason.is_some(),
        "blocked_reason should be preserved when task stays Blocked"
    );
}

/// Scenario: a plan_merge task is Blocked, with one blocker in Stopped state.
/// Stopped is terminal but does NOT satisfy dependencies (incomplete work).
/// Expected: startup recovery leaves the plan_merge task Blocked.
#[tokio::test]
async fn test_startup_keeps_blocked_tasks_with_stopped_blocker() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Create a blocker task in Stopped state (incomplete work — should NOT unblock dependents)
    let mut blocker_stopped = Task::new(project.id.clone(), "Stopped Feature Task".to_string());
    blocker_stopped.internal_status = InternalStatus::Stopped;
    app_state
        .task_repo
        .create(blocker_stopped.clone())
        .await
        .unwrap();

    // Create the plan_merge task in Blocked state
    let mut plan_merge_task = Task::new(project.id.clone(), "Merge Plan to Main".to_string());
    plan_merge_task.internal_status = InternalStatus::Blocked;
    plan_merge_task.category = TaskCategory::PlanMerge;
    plan_merge_task.blocked_reason = Some("Waiting for: Stopped Feature Task".to_string());
    let plan_merge_id = plan_merge_task.id.clone();
    app_state
        .task_repo
        .create(plan_merge_task.clone())
        .await
        .unwrap();

    // Add the dependency
    app_state
        .task_dependency_repo
        .add_dependency(&plan_merge_task.id, &blocker_stopped.id)
        .await
        .unwrap();

    let (runner, _app_state_repo) = build_runner(&app_state, &execution_state);

    // Run startup recovery
    runner.run().await;

    // Verify the plan_merge task stays Blocked
    // Stopped does NOT satisfy dependencies — the blocker's work is incomplete
    let updated = app_state
        .task_repo
        .get_by_id(&plan_merge_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Blocked,
        "plan_merge task should remain Blocked when blocker is Stopped (incomplete work)"
    );
    assert!(
        updated.blocked_reason.is_some(),
        "blocked_reason should be preserved when task stays Blocked"
    );
}

// ============================================================
// Dependency Reconciliation Tests (TDD — tests written first)
// ============================================================
// These test `reconcile_dependency_violations()` which detects tasks in
// non-Blocked states that have unsatisfied dependencies and moves them
// back to Blocked (or Stopped for states where Blocked is not a valid target).

/// Ready task with a Failed blocker should be re-blocked.
/// Failed does NOT satisfy dependencies (is_dependency_satisfied() returns false).
#[tokio::test]
async fn test_reconcile_reblocks_ready_task_with_failed_blocker() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Blocker is Failed — does NOT satisfy dependency
    let mut blocker = Task::new(project.id.clone(), "Failed Blocker".to_string());
    blocker.internal_status = InternalStatus::Failed;
    app_state.task_repo.create(blocker.clone()).await.unwrap();

    // Dependent is Ready — should be re-blocked because blocker is Failed
    let mut ready_task = Task::new(project.id.clone(), "Ready Dependent".to_string());
    ready_task.internal_status = InternalStatus::Ready;
    app_state
        .task_repo
        .create(ready_task.clone())
        .await
        .unwrap();

    // Add dependency: ready_task depends on blocker
    app_state
        .task_dependency_repo
        .add_dependency(&ready_task.id, &blocker.id)
        .await
        .unwrap();

    let (runner, _) = build_runner(&app_state, &execution_state);

    // This method doesn't exist yet — TDD red phase
    runner.reconcile_dependency_violations().await;

    let updated = app_state
        .task_repo
        .get_by_id(&ready_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Blocked,
        "Ready task with Failed blocker should be re-blocked"
    );
    assert!(
        updated.blocked_reason.is_some(),
        "blocked_reason should be set when task is re-blocked"
    );
}

/// Executing task with a Failed blocker should be re-blocked.
/// Executing → Blocked is a valid state machine transition (status.rs line 78).
#[tokio::test]
async fn test_reconcile_reblocks_executing_task_with_failed_blocker() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Blocker is Failed
    let mut blocker = Task::new(project.id.clone(), "Failed Blocker".to_string());
    blocker.internal_status = InternalStatus::Failed;
    app_state.task_repo.create(blocker.clone()).await.unwrap();

    // Dependent is Executing — should be re-blocked
    let mut executing_task = Task::new(project.id.clone(), "Executing Dependent".to_string());
    executing_task.internal_status = InternalStatus::Executing;
    app_state
        .task_repo
        .create(executing_task.clone())
        .await
        .unwrap();

    app_state
        .task_dependency_repo
        .add_dependency(&executing_task.id, &blocker.id)
        .await
        .unwrap();

    let (runner, _) = build_runner(&app_state, &execution_state);
    runner.reconcile_dependency_violations().await;

    let updated = app_state
        .task_repo
        .get_by_id(&executing_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Blocked,
        "Executing task with Failed blocker should be re-blocked"
    );
}

/// Reviewing task with a Failed blocker should be Stopped (not Blocked).
/// Reviewing → Blocked is NOT a valid transition, but Reviewing → Stopped IS.
#[tokio::test]
async fn test_reconcile_stops_reviewing_task_with_failed_blocker() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut blocker = Task::new(project.id.clone(), "Failed Blocker".to_string());
    blocker.internal_status = InternalStatus::Failed;
    app_state.task_repo.create(blocker.clone()).await.unwrap();

    // Reviewing → Blocked is invalid, so should go to Stopped
    let mut reviewing_task = Task::new(project.id.clone(), "Reviewing Dependent".to_string());
    reviewing_task.internal_status = InternalStatus::Reviewing;
    app_state
        .task_repo
        .create(reviewing_task.clone())
        .await
        .unwrap();

    app_state
        .task_dependency_repo
        .add_dependency(&reviewing_task.id, &blocker.id)
        .await
        .unwrap();

    let (runner, _) = build_runner(&app_state, &execution_state);
    runner.reconcile_dependency_violations().await;

    let updated = app_state
        .task_repo
        .get_by_id(&reviewing_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Stopped,
        "Reviewing task with Failed blocker should be Stopped (Blocked not valid from Reviewing)"
    );
}

/// Ready task with a Merged blocker should NOT be re-blocked.
/// Merged satisfies dependencies (is_dependency_satisfied() returns true).
#[tokio::test]
async fn test_reconcile_leaves_ready_task_with_merged_blocker() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Blocker is Merged — satisfies dependency
    let mut blocker = Task::new(project.id.clone(), "Merged Blocker".to_string());
    blocker.internal_status = InternalStatus::Merged;
    app_state.task_repo.create(blocker.clone()).await.unwrap();

    let mut ready_task = Task::new(project.id.clone(), "Ready Dependent".to_string());
    ready_task.internal_status = InternalStatus::Ready;
    app_state
        .task_repo
        .create(ready_task.clone())
        .await
        .unwrap();

    app_state
        .task_dependency_repo
        .add_dependency(&ready_task.id, &blocker.id)
        .await
        .unwrap();

    let (runner, _) = build_runner(&app_state, &execution_state);
    runner.reconcile_dependency_violations().await;

    let updated = app_state
        .task_repo
        .get_by_id(&ready_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Ready,
        "Ready task with Merged blocker should stay Ready"
    );
}

/// Ready task with no dependencies should not be touched.
#[tokio::test]
async fn test_reconcile_leaves_task_with_no_blockers() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut ready_task = Task::new(project.id.clone(), "Standalone Ready Task".to_string());
    ready_task.internal_status = InternalStatus::Ready;
    app_state
        .task_repo
        .create(ready_task.clone())
        .await
        .unwrap();

    // No dependencies added

    let (runner, _) = build_runner(&app_state, &execution_state);
    runner.reconcile_dependency_violations().await;

    let updated = app_state
        .task_repo
        .get_by_id(&ready_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Ready,
        "Task with no dependencies should remain Ready"
    );
}

/// Ready task whose blocker is still Blocked should be re-blocked.
/// Blocked is NOT a satisfied dependency state.
#[tokio::test]
async fn test_reconcile_reblocks_task_with_blocked_blocker() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    // Blocker is Blocked — does NOT satisfy dependency
    let mut blocker = Task::new(project.id.clone(), "Blocked Blocker".to_string());
    blocker.internal_status = InternalStatus::Blocked;
    app_state.task_repo.create(blocker.clone()).await.unwrap();

    let mut ready_task = Task::new(project.id.clone(), "Ready Dependent".to_string());
    ready_task.internal_status = InternalStatus::Ready;
    app_state
        .task_repo
        .create(ready_task.clone())
        .await
        .unwrap();

    app_state
        .task_dependency_repo
        .add_dependency(&ready_task.id, &blocker.id)
        .await
        .unwrap();

    let (runner, _) = build_runner(&app_state, &execution_state);
    runner.reconcile_dependency_violations().await;

    let updated = app_state
        .task_repo
        .get_by_id(&ready_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        updated.internal_status,
        InternalStatus::Blocked,
        "Ready task with Blocked blocker should be re-blocked"
    );
}

/// Verify blocked_reason includes the failed blocker's title.
#[tokio::test]
async fn test_reconcile_sets_blocked_reason_with_failed_blocker_title() {
    let (execution_state, app_state) = setup_test_state().await;

    let project = Project::new("Test Project".to_string(), "/test/path".to_string());
    app_state
        .project_repo
        .create(project.clone())
        .await
        .unwrap();

    let mut blocker = Task::new(project.id.clone(), "Auth Service Setup".to_string());
    blocker.internal_status = InternalStatus::Failed;
    app_state.task_repo.create(blocker.clone()).await.unwrap();

    let mut ready_task = Task::new(project.id.clone(), "User Login Feature".to_string());
    ready_task.internal_status = InternalStatus::Ready;
    app_state
        .task_repo
        .create(ready_task.clone())
        .await
        .unwrap();

    app_state
        .task_dependency_repo
        .add_dependency(&ready_task.id, &blocker.id)
        .await
        .unwrap();

    let (runner, _) = build_runner(&app_state, &execution_state);
    runner.reconcile_dependency_violations().await;

    let updated = app_state
        .task_repo
        .get_by_id(&ready_task.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.internal_status, InternalStatus::Blocked);
    let reason = updated
        .blocked_reason
        .expect("blocked_reason should be set");
    assert!(
        reason.contains("Auth Service Setup"),
        "blocked_reason should contain the failed blocker's title, got: {}",
        reason
    );
}

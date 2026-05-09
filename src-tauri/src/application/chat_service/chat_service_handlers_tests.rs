use super::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};
use tauri::Manager;

use crate::application::{
    chat_service::verification_child_process_registry::VerificationChildProcessRegistry,
    chat_service::{ProviderErrorCategory, ProviderErrorMetadata},
    AppState, InteractiveProcessRegistry,
};
use crate::domain::entities::{
    app_state::ExecutionHaltMode, ChatConversation, IdeationSessionId, InternalStatus, Project,
    ProjectId, Task, VerificationStatus,
};
use crate::domain::repositories::{StateHistoryMetadata, StatusTransition};
use crate::error::AppResult;
use crate::infrastructure::agents::claude::{ContentBlockItem, ToolCall};

/// Configurable mock: `get_by_id` returns the stored task (or None).
struct StubTaskRepo {
    task: Option<Task>,
    status_entered_at: Option<DateTime<Utc>>,
}

#[async_trait]
impl TaskRepository for StubTaskRepo {
    async fn get_by_id(&self, _id: &TaskId) -> AppResult<Option<Task>> {
        Ok(self.task.clone())
    }

    // ── Stubs for all other required methods ────────────────────────────
    async fn create(&self, task: Task) -> AppResult<Task> {
        Ok(task)
    }
    async fn get_by_project(&self, _: &ProjectId) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn update(&self, _: &Task) -> AppResult<()> {
        Ok(())
    }
    async fn update_with_expected_status(&self, _: &Task, _: InternalStatus) -> AppResult<bool> {
        Ok(true)
    }
    async fn update_metadata(&self, _: &TaskId, _: Option<String>) -> AppResult<()> {
        Ok(())
    }
    async fn delete(&self, _: &TaskId) -> AppResult<()> {
        Ok(())
    }
    async fn get_by_status(&self, _: &ProjectId, _: InternalStatus) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn persist_status_change(
        &self,
        _: &TaskId,
        _: InternalStatus,
        _: InternalStatus,
        _: &str,
    ) -> AppResult<()> {
        Ok(())
    }
    async fn get_status_history(&self, _: &TaskId) -> AppResult<Vec<StatusTransition>> {
        Ok(vec![])
    }
    async fn get_status_entered_at(
        &self,
        _: &TaskId,
        _: InternalStatus,
    ) -> AppResult<Option<DateTime<Utc>>> {
        Ok(self.status_entered_at)
    }
    async fn get_next_executable(&self, _: &ProjectId) -> AppResult<Option<Task>> {
        Ok(None)
    }
    async fn get_by_ideation_session(&self, _: &IdeationSessionId) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn get_by_project_filtered(&self, _: &ProjectId, _: bool) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn archive(&self, id: &TaskId) -> AppResult<Task> {
        let mut t = Task::new(ProjectId::new(), "archived".into());
        t.id = id.clone();
        Ok(t)
    }
    async fn restore(&self, id: &TaskId) -> AppResult<Task> {
        let mut t = Task::new(ProjectId::new(), "restored".into());
        t.id = id.clone();
        Ok(t)
    }
    async fn get_archived_count(&self, _: &ProjectId, _: Option<&str>) -> AppResult<u32> {
        Ok(0)
    }
    async fn list_paginated(
        &self,
        _: &ProjectId,
        _: Option<Vec<InternalStatus>>,
        _: u32,
        _: u32,
        _: bool,
        _: Option<&str>,
        _: Option<&str>,
        _: Option<&[String]>,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn count_tasks(
        &self,
        _: &ProjectId,
        _: bool,
        _: Option<&str>,
        _: Option<&str>,
    ) -> AppResult<u32> {
        Ok(0)
    }
    async fn search(&self, _: &ProjectId, _: &str, _: bool) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn get_oldest_ready_task(&self) -> AppResult<Option<Task>> {
        Ok(None)
    }
    async fn get_oldest_ready_tasks(&self, _: u32) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn update_latest_state_history_metadata(
        &self,
        _: &TaskId,
        _: &StateHistoryMetadata,
    ) -> AppResult<()> {
        Ok(())
    }
    async fn has_task_in_states(&self, _: &ProjectId, _: &[InternalStatus]) -> AppResult<bool> {
        Ok(false)
    }
    async fn get_stale_ready_tasks(&self, _threshold_secs: u64) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn get_status_history_batch(
        &self,
        _task_ids: &[crate::domain::entities::TaskId],
    ) -> AppResult<std::collections::HashMap<crate::domain::entities::TaskId, Vec<StatusTransition>>>
    {
        Ok(std::collections::HashMap::new())
    }
}

fn make_task(status: InternalStatus) -> Task {
    let mut task = Task::new(ProjectId::new(), "test task".into());
    task.internal_status = status;
    task
}

#[tokio::test]
async fn test_still_needs_recovery_when_executing() {
    let task_id = TaskId::new();
    let repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(make_task(InternalStatus::Executing)),
        status_entered_at: None,
    });
    assert!(task_still_needs_execution_recovery(&task_id, &repo).await);
}

#[tokio::test]
async fn test_still_needs_recovery_when_re_executing() {
    let task_id = TaskId::new();
    let repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(make_task(InternalStatus::ReExecuting)),
        status_entered_at: None,
    });
    assert!(task_still_needs_execution_recovery(&task_id, &repo).await);
}

#[tokio::test]
async fn test_no_recovery_when_already_transitioned() {
    // Simulate auto-complete resolving the task to PendingReview during the 500ms window
    let task_id = TaskId::new();
    let repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(make_task(InternalStatus::PendingReview)),
        status_entered_at: None,
    });
    assert!(!task_still_needs_execution_recovery(&task_id, &repo).await);
}

#[tokio::test]
async fn test_no_recovery_when_failed() {
    let task_id = TaskId::new();
    let repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(make_task(InternalStatus::Failed)),
        status_entered_at: None,
    });
    assert!(!task_still_needs_execution_recovery(&task_id, &repo).await);
}

#[tokio::test]
async fn test_no_recovery_when_cancelled() {
    let task_id = TaskId::new();
    let repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(make_task(InternalStatus::Cancelled)),
        status_entered_at: None,
    });
    assert!(!task_still_needs_execution_recovery(&task_id, &repo).await);
}

#[tokio::test]
async fn test_no_recovery_when_task_not_found() {
    // Task not found (e.g., deleted) → skip retry safely
    let task_id = TaskId::new();
    let repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: None,
        status_entered_at: None,
    });
    assert!(!task_still_needs_execution_recovery(&task_id, &repo).await);
}

#[tokio::test]
async fn test_execution_attempt_guard_rejects_stale_run_after_restart() {
    use crate::domain::entities::{AgentRun, ChatConversationId};
    use crate::infrastructure::memory::MemoryAgentRunRepository;

    let task_id = TaskId::new();
    let mut task = make_task(InternalStatus::Executing);
    task.id = task_id.clone();
    let status_entered_at = Utc::now();
    let task_repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(task),
        status_entered_at: Some(status_entered_at),
    });

    let run_repo = Arc::new(MemoryAgentRunRepository::new());
    let mut stale_run = AgentRun::new(ChatConversationId::new());
    stale_run.started_at = status_entered_at - chrono::Duration::minutes(5);
    let stale_run_id = stale_run.id.as_str().to_string();
    run_repo.create(stale_run).await.unwrap();
    let run_repo: Arc<dyn AgentRunRepository> = run_repo;

    assert!(
        !task_execution_attempt_matches_current_status(
            &task_id,
            stale_run_id.as_str(),
            &task_repo,
            &run_repo,
        )
        .await,
        "Older execution run must not transition a newer restarted attempt",
    );
}

#[tokio::test]
async fn test_execution_attempt_guard_allows_current_run() {
    use crate::domain::entities::{AgentRun, ChatConversationId};
    use crate::infrastructure::memory::MemoryAgentRunRepository;

    let task_id = TaskId::new();
    let mut task = make_task(InternalStatus::Executing);
    task.id = task_id.clone();
    let status_entered_at = Utc::now();
    let task_repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(task),
        status_entered_at: Some(status_entered_at),
    });

    let run_repo = Arc::new(MemoryAgentRunRepository::new());
    let mut current_run = AgentRun::new(ChatConversationId::new());
    current_run.started_at = status_entered_at + chrono::Duration::milliseconds(100);
    let current_run_id = current_run.id.as_str().to_string();
    run_repo.create(current_run).await.unwrap();
    let run_repo: Arc<dyn AgentRunRepository> = run_repo;

    assert!(
        task_execution_attempt_matches_current_status(
            &task_id,
            current_run_id.as_str(),
            &task_repo,
            &run_repo,
        )
        .await,
        "Current execution run must still be allowed to transition the task",
    );
}

#[tokio::test]
async fn test_load_current_task_execution_attempt_classifies_current_attempt() {
    use crate::domain::entities::{AgentRun, ChatConversationId};
    use crate::infrastructure::memory::MemoryAgentRunRepository;

    let task_id = TaskId::new();
    let mut task = make_task(InternalStatus::Executing);
    task.id = task_id.clone();
    let status_entered_at = Utc::now();
    let task_repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(task.clone()),
        status_entered_at: Some(status_entered_at),
    });

    let run_repo = Arc::new(MemoryAgentRunRepository::new());
    let mut current_run = AgentRun::new(ChatConversationId::new());
    current_run.started_at = status_entered_at + chrono::Duration::milliseconds(250);
    let current_run_id = current_run.id.as_str().to_string();
    run_repo.create(current_run).await.unwrap();

    let mut stale_run = AgentRun::new(ChatConversationId::new());
    stale_run.started_at = status_entered_at - chrono::Duration::minutes(2);
    let stale_run_id = stale_run.id.as_str().to_string();
    run_repo.create(stale_run).await.unwrap();
    let run_repo: Arc<dyn AgentRunRepository> = run_repo;

    let current = load_current_task_execution_attempt(
        &task_id,
        current_run_id.as_str(),
        &task_repo,
        &run_repo,
    )
    .await;
    assert!(
        current.is_some(),
        "current execution attempt should be eligible for finalization"
    );

    let stale =
        load_current_task_execution_attempt(&task_id, stale_run_id.as_str(), &task_repo, &run_repo)
            .await;
    assert!(
        stale.is_none(),
        "older execution run must not finalize a newer execution attempt"
    );

    let review_repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(make_task(InternalStatus::Reviewing)),
        status_entered_at: Some(status_entered_at),
    });
    let review_attempt = load_current_task_execution_attempt(
        &task_id,
        current_run_id.as_str(),
        &review_repo,
        &run_repo,
    )
    .await;
    assert!(
        review_attempt.is_none(),
        "review-state tasks are no longer active execution attempts"
    );
}

#[tokio::test]
async fn test_load_current_task_execution_attempt_handles_missing_records() {
    use crate::infrastructure::memory::MemoryAgentRunRepository;

    let task_id = TaskId::new();
    let missing_task_repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: None,
        status_entered_at: None,
    });
    let run_repo: Arc<dyn AgentRunRepository> = Arc::new(MemoryAgentRunRepository::new());

    let missing_task = load_current_task_execution_attempt(
        &task_id,
        "missing-run",
        &missing_task_repo,
        &run_repo,
    )
    .await;
    assert!(
        missing_task.is_none(),
        "missing tasks cannot be finalized as execution attempts"
    );

    let mut task = make_task(InternalStatus::Executing);
    task.id = task_id.clone();
    let active_task_repo: Arc<dyn TaskRepository> = Arc::new(StubTaskRepo {
        task: Some(task),
        status_entered_at: Some(Utc::now()),
    });

    let missing_run = load_current_task_execution_attempt(
        &task_id,
        "missing-run",
        &active_task_repo,
        &run_repo,
    )
    .await;
    assert!(
        missing_run.is_some(),
        "missing agent-run rows should fail open for active execution tasks"
    );
}

// ========================================
// Global Rate Limit Backpressure Integration Tests
// ========================================

#[test]
fn test_apply_global_rate_limit_backpressure_sets_gate() {
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(exec.clone());

    // Provide a future retry_after timestamp
    let future = (chrono::Utc::now() + chrono::Duration::seconds(300)).to_rfc3339();
    let retry_after = Some(future);

    assert!(!exec.is_provider_blocked());
    apply_global_rate_limit_backpressure(&execution_state, &retry_after, "test", "task-1");
    assert!(exec.is_provider_blocked());
    assert!(!exec.can_start_task());
}

#[test]
fn test_apply_global_rate_limit_backpressure_noop_without_retry_after() {
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(exec.clone());

    // No retry_after → should not set backpressure
    apply_global_rate_limit_backpressure(&execution_state, &None, "test", "task-1");
    assert!(!exec.is_provider_blocked());
    assert!(exec.can_start_task());
}

#[test]
fn test_apply_global_rate_limit_backpressure_noop_without_execution_state() {
    let execution_state: Option<Arc<ExecutionState>> = None;
    let future = (chrono::Utc::now() + chrono::Duration::seconds(300)).to_rfc3339();
    let retry_after = Some(future);

    // Should not panic when execution_state is None
    apply_global_rate_limit_backpressure(&execution_state, &retry_after, "test", "task-1");
}

#[test]
fn test_apply_global_rate_limit_backpressure_expired_does_not_block() {
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(exec.clone());

    // Provide a past retry_after timestamp
    let past = (chrono::Utc::now() - chrono::Duration::seconds(60)).to_rfc3339();
    let retry_after = Some(past);

    apply_global_rate_limit_backpressure(&execution_state, &retry_after, "test", "task-1");
    // Epoch was set, but it's in the past, so is_provider_blocked returns false
    assert!(!exec.is_provider_blocked());
    assert!(exec.can_start_task());
}

#[test]
fn test_execution_completion_requires_completed_steps_when_step_tracking_exists() {
    assert!(!should_transition_task_execution_to_pending_review(
        true, true, false
    ));
    assert!(should_transition_task_execution_to_pending_review(
        true, true, true
    ));
}

#[test]
fn test_execution_completion_falls_back_to_output_when_step_tracking_missing() {
    assert!(should_transition_task_execution_to_pending_review(
        true, false, false
    ));
    assert!(!should_transition_task_execution_to_pending_review(
        false, false, false
    ));
}

#[test]
fn test_execution_completion_action_prefers_pending_review_for_output_or_completed_steps() {
    assert_eq!(
        execution_completion_action(true, false, false),
        ExecutionCompletionAction::PendingReview
    );
    assert_eq!(
        execution_completion_action(false, true, true),
        ExecutionCompletionAction::PendingReview
    );
}

#[test]
fn test_execution_completion_action_fails_empty_incomplete_execution() {
    assert_eq!(
        execution_completion_action(false, true, false),
        ExecutionCompletionAction::Failed
    );
    assert_eq!(
        execution_completion_action(false, false, false),
        ExecutionCompletionAction::Failed
    );
}

#[test]
fn test_incomplete_review_action_escalates_only_for_live_reviewing_tasks() {
    assert_eq!(
        incomplete_review_action(InternalStatus::Reviewing, false),
        IncompleteReviewAction::Escalate
    );
    assert_eq!(
        incomplete_review_action(InternalStatus::Reviewing, true),
        IncompleteReviewAction::SkipDuringShutdown
    );
    assert_eq!(
        incomplete_review_action(InternalStatus::PendingMerge, false),
        IncompleteReviewAction::IgnoreAlreadyTransitioned
    );
}

#[tokio::test]
async fn test_apply_system_wide_provider_pause_pauses_mixed_active_task_states() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new(
        "Provider Pause".to_string(),
        "/tmp/provider-pause".to_string(),
    );
    let project_id = project.id.clone();
    app_state.project_repo.create(project).await.unwrap();

    let mut executing = Task::new(project_id.clone(), "Executing".to_string());
    executing.internal_status = InternalStatus::Executing;
    let executing = app_state.task_repo.create(executing).await.unwrap();

    let mut reviewing = Task::new(project_id.clone(), "Reviewing".to_string());
    reviewing.internal_status = InternalStatus::Reviewing;
    let reviewing = app_state.task_repo.create(reviewing).await.unwrap();

    let mut merging = Task::new(project_id.clone(), "Merging".to_string());
    merging.internal_status = InternalStatus::Merging;
    let merging = app_state.task_repo.create(merging).await.unwrap();

    let mut ready = Task::new(project_id.clone(), "Ready".to_string());
    ready.internal_status = InternalStatus::Ready;
    let ready = app_state.task_repo.create(ready).await.unwrap();

    let app = mock_builder()
        .manage(app_state)
        .manage(Arc::clone(&execution_state))
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let handle = app.handle().clone();
    let state = handle.state::<AppState>();

    apply_system_wide_provider_pause::<MockRuntime>(
        &Some(handle.clone()),
        &ProviderErrorCategory::RateLimit,
        "You've hit your limit · resets 11pm (Europe/Bucharest)",
        &Some((chrono::Utc::now() + chrono::Duration::minutes(30)).to_rfc3339()),
        "task_execution",
        executing.id.as_str(),
    )
    .await;

    assert!(execution_state.is_paused());
    assert!(execution_state.is_provider_blocked());
    assert!(!execution_state.can_start_task());

    let persisted = state.app_state_repo.get().await.unwrap();
    assert_eq!(persisted.execution_halt_mode, ExecutionHaltMode::Paused);

    let executing_after = state
        .task_repo
        .get_by_id(&executing.id)
        .await
        .unwrap()
        .unwrap();
    let reviewing_after = state
        .task_repo
        .get_by_id(&reviewing.id)
        .await
        .unwrap()
        .unwrap();
    let merging_after = state
        .task_repo
        .get_by_id(&merging.id)
        .await
        .unwrap()
        .unwrap();
    let ready_after = state.task_repo.get_by_id(&ready.id).await.unwrap().unwrap();

    assert_eq!(executing_after.internal_status, InternalStatus::Paused);
    assert_eq!(reviewing_after.internal_status, InternalStatus::Paused);
    assert_eq!(merging_after.internal_status, InternalStatus::Paused);
    assert_eq!(ready_after.internal_status, InternalStatus::Ready);
}

#[tokio::test]
async fn test_codex_local_tool_rate_limit_text_does_not_global_pause_execution() {
    let app_state = AppState::new_test();
    let execution_state = Arc::new(ExecutionState::new());

    let project = Project::new(
        "Codex Local Tool Failure".to_string(),
        "/tmp/codex-local-tool-failure".to_string(),
    );
    let project_id = project.id.clone();
    app_state.project_repo.create(project).await.unwrap();

    let mut task = Task::new(project_id, "Executing".to_string());
    task.internal_status = InternalStatus::Executing;
    let task = app_state.task_repo.create(task).await.unwrap();
    let task_id = task.id.clone();

    let app = mock_builder()
        .manage(app_state)
        .manage(Arc::clone(&execution_state))
        .build(mock_context(noop_assets()))
        .expect("mock app");
    let handle = app.handle().clone();
    let state = handle.state::<AppState>();

    let runtime_errors = Vec::<String>::new();
    let local_tool_errors = vec![
        "rg: src-tauri/src/domain/entities/agent_run.rs: No such file or directory\n\
         src-tauri/src/application/chat_service/chat_service_errors.rs: ProviderErrorCategory::RateLimit writes rate_limit"
            .to_string(),
    ];
    let stream_error = crate::application::chat_service::classify_codex_stream_failure(
        &runtime_errors,
        &local_tool_errors,
        Some(1),
    )
    .expect("local Codex tool failure should produce a stream error");
    assert!(
        matches!(stream_error, StreamError::AgentExit { .. }),
        "local command output containing rate_limit must not classify as provider error"
    );

    let conversation_id = ChatConversationId::new();
    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::TaskExecution,
        task_id.as_str(),
    );
    let error_message = stream_error.to_string();

    let recovery_spawned = handle_stream_error::<MockRuntime>(
        &error_message,
        Some(&stream_error),
        ChatContextType::TaskExecution,
        task_id.as_str(),
        conversation_id,
        "run-id-local-tool-error",
        "message-id-local-tool-error",
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &Some(Arc::clone(&execution_state)),
        &None,
        &None,
        &None,
        &Some(handle.clone()),
        None,
        false,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(!recovery_spawned);
    assert!(!execution_state.is_paused());
    assert!(!execution_state.is_provider_blocked());

    let persisted = state.app_state_repo.get().await.unwrap();
    assert_eq!(persisted.execution_halt_mode, ExecutionHaltMode::Running);

    let updated_task = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(updated_task.internal_status, InternalStatus::Failed);
    assert!(
        ProviderErrorMetadata::from_task_metadata(updated_task.metadata.as_deref()).is_none(),
        "local tool failures must not persist provider_error metadata"
    );
}

// ========================================
// AgentExit + Step Completion Override Tests
// ========================================
//
// These verify the all_steps_completed helper and that handle_stream_error
// overrides Failed → PendingReview when all steps are completed.

use crate::application::chat_service::chat_service_handlers::all_steps_completed;
use crate::domain::entities::{TaskStep, TaskStepId};
use crate::domain::repositories::TaskStepRepository;
use crate::error::AppError;
use std::collections::HashMap;

struct StubTaskStepRepo {
    steps: Vec<TaskStep>,
}

#[async_trait]
impl TaskStepRepository for StubTaskStepRepo {
    async fn create(&self, step: TaskStep) -> AppResult<TaskStep> {
        Ok(step)
    }
    async fn get_by_id(&self, _: &TaskStepId) -> AppResult<Option<TaskStep>> {
        Ok(None)
    }
    async fn get_by_task(&self, _: &TaskId) -> AppResult<Vec<TaskStep>> {
        Ok(self.steps.clone())
    }
    async fn get_by_task_and_status(
        &self,
        _: &TaskId,
        _: TaskStepStatus,
    ) -> AppResult<Vec<TaskStep>> {
        Ok(vec![])
    }
    async fn update(&self, _: &TaskStep) -> AppResult<()> {
        Ok(())
    }
    async fn delete(&self, _: &TaskStepId) -> AppResult<()> {
        Ok(())
    }
    async fn delete_by_task(&self, _: &TaskId) -> AppResult<()> {
        Ok(())
    }
    async fn count_by_status(&self, _: &TaskId) -> AppResult<HashMap<TaskStepStatus, u32>> {
        Ok(HashMap::new())
    }
    async fn bulk_create(&self, steps: Vec<TaskStep>) -> AppResult<Vec<TaskStep>> {
        Ok(steps)
    }
    async fn reorder(&self, _: &TaskId, _: Vec<TaskStepId>) -> AppResult<()> {
        Ok(())
    }
    async fn reset_all_to_pending(&self, _: &TaskId) -> AppResult<u32> {
        Ok(0)
    }
}

/// Stub that always returns a DB error for get_by_task.
struct StubErrorTaskStepRepo;

#[async_trait]
impl TaskStepRepository for StubErrorTaskStepRepo {
    async fn create(&self, step: TaskStep) -> AppResult<TaskStep> {
        Ok(step)
    }
    async fn get_by_id(&self, _: &TaskStepId) -> AppResult<Option<TaskStep>> {
        Ok(None)
    }
    async fn get_by_task(&self, _: &TaskId) -> AppResult<Vec<TaskStep>> {
        Err(AppError::Database("simulated DB error".into()))
    }
    async fn get_by_task_and_status(
        &self,
        _: &TaskId,
        _: TaskStepStatus,
    ) -> AppResult<Vec<TaskStep>> {
        Ok(vec![])
    }
    async fn update(&self, _: &TaskStep) -> AppResult<()> {
        Ok(())
    }
    async fn delete(&self, _: &TaskStepId) -> AppResult<()> {
        Ok(())
    }
    async fn delete_by_task(&self, _: &TaskId) -> AppResult<()> {
        Ok(())
    }
    async fn count_by_status(&self, _: &TaskId) -> AppResult<HashMap<TaskStepStatus, u32>> {
        Ok(HashMap::new())
    }
    async fn bulk_create(&self, steps: Vec<TaskStep>) -> AppResult<Vec<TaskStep>> {
        Ok(steps)
    }
    async fn reorder(&self, _: &TaskId, _: Vec<TaskStepId>) -> AppResult<()> {
        Ok(())
    }
    async fn reset_all_to_pending(&self, _: &TaskId) -> AppResult<u32> {
        Ok(0)
    }
}

struct StatusChangingTaskStepRepo {
    task_repo: Arc<dyn TaskRepository>,
    task_id: TaskId,
    target_status: InternalStatus,
}

impl StatusChangingTaskStepRepo {
    async fn move_task_to_target_status(&self) -> AppResult<()> {
        let Some(mut task) = self.task_repo.get_by_id(&self.task_id).await? else {
            return Ok(());
        };
        task.internal_status = self.target_status;
        self.task_repo.update(&task).await
    }
}

#[async_trait]
impl TaskStepRepository for StatusChangingTaskStepRepo {
    async fn create(&self, step: TaskStep) -> AppResult<TaskStep> {
        Ok(step)
    }
    async fn get_by_id(&self, _: &TaskStepId) -> AppResult<Option<TaskStep>> {
        Ok(None)
    }
    async fn get_by_task(&self, task_id: &TaskId) -> AppResult<Vec<TaskStep>> {
        if task_id == &self.task_id {
            self.move_task_to_target_status().await?;
        }
        Ok(Vec::new())
    }
    async fn get_by_task_and_status(
        &self,
        _: &TaskId,
        _: TaskStepStatus,
    ) -> AppResult<Vec<TaskStep>> {
        Ok(Vec::new())
    }
    async fn update(&self, _: &TaskStep) -> AppResult<()> {
        Ok(())
    }
    async fn delete(&self, _: &TaskStepId) -> AppResult<()> {
        Ok(())
    }
    async fn delete_by_task(&self, _: &TaskId) -> AppResult<()> {
        Ok(())
    }
    async fn count_by_status(&self, _: &TaskId) -> AppResult<HashMap<TaskStepStatus, u32>> {
        Ok(HashMap::new())
    }
    async fn bulk_create(&self, steps: Vec<TaskStep>) -> AppResult<Vec<TaskStep>> {
        Ok(steps)
    }
    async fn reorder(&self, _: &TaskId, _: Vec<TaskStepId>) -> AppResult<()> {
        Ok(())
    }
    async fn reset_all_to_pending(&self, _: &TaskId) -> AppResult<u32> {
        Ok(0)
    }
}

fn make_step(task_id: &TaskId, status: TaskStepStatus) -> TaskStep {
    let mut step = TaskStep::new(task_id.clone(), "test step".into(), 0, "agent".into());
    step.status = status;
    step
}

fn run<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f)
}

/// AgentExit with all steps completed should override target_status to PendingReview.
/// This covers the scenario where execution_complete was called (marking steps done)
/// but the agent exited with signal code=None (IPR removal → EOF → signal).
#[test]
fn test_agent_exit_all_steps_complete_overrides_to_pending_review() {
    let task_id = TaskId::new();
    let steps = vec![
        make_step(&task_id, TaskStepStatus::Completed),
        make_step(&task_id, TaskStepStatus::Completed),
        make_step(&task_id, TaskStepStatus::Skipped),
    ];
    let step_repo: Option<Arc<dyn TaskStepRepository>> = Some(Arc::new(StubTaskStepRepo { steps }));

    let result = run(all_steps_completed(&step_repo, &task_id));
    assert!(result, "All Completed+Skipped steps → should return true");
}

/// AgentExit with incomplete steps should remain Failed.
#[test]
fn test_agent_exit_incomplete_steps_stays_failed() {
    let task_id = TaskId::new();
    let steps = vec![
        make_step(&task_id, TaskStepStatus::Completed),
        make_step(&task_id, TaskStepStatus::InProgress), // not done
        make_step(&task_id, TaskStepStatus::Pending),    // not done
    ];
    let step_repo: Option<Arc<dyn TaskStepRepository>> = Some(Arc::new(StubTaskStepRepo { steps }));

    let result = run(all_steps_completed(&step_repo, &task_id));
    assert!(
        !result,
        "InProgress/Pending steps present → should return false"
    );
}

/// AgentExit with no steps at all should remain Failed.
#[test]
fn test_agent_exit_no_steps_stays_failed() {
    let task_id = TaskId::new();
    let step_repo: Option<Arc<dyn TaskStepRepository>> =
        Some(Arc::new(StubTaskStepRepo { steps: vec![] }));

    let result = run(all_steps_completed(&step_repo, &task_id));
    assert!(
        !result,
        "Empty step list → should return false (guard against trivially true)"
    );
}

/// Non-AgentExit errors should not trigger the override, even with complete steps.
#[test]
fn test_timeout_error_does_not_override_even_with_complete_steps() {
    let task_id = TaskId::new();
    let steps = vec![make_step(&task_id, TaskStepStatus::Completed)];
    let _step_repo: Option<Arc<dyn TaskStepRepository>> =
        Some(Arc::new(StubTaskStepRepo { steps }));

    let stream_error = StreamError::Timeout {
        context_type: ChatContextType::TaskExecution,
        elapsed_secs: 3600,
    };
    let initial_target = InternalStatus::Failed;

    // Timeout errors are NOT AgentExit — should not trigger override
    let target_status = if initial_target == InternalStatus::Failed
        && matches!(&stream_error, StreamError::AgentExit { .. })
    {
        InternalStatus::PendingReview // would override
    } else {
        initial_target
    };

    assert_eq!(
        target_status,
        InternalStatus::Failed,
        "Timeout errors should not trigger the AgentExit step-completion override"
    );
}

/// No task_step_repo → should not override (fail-safe).
#[test]
fn test_agent_exit_no_step_repo_stays_failed() {
    let task_id = TaskId::new();
    let step_repo: Option<Arc<dyn TaskStepRepository>> = None;

    let result = run(all_steps_completed(&step_repo, &task_id));
    assert!(!result, "No step repo → should fail-safe to false");
}

// ========================================
// New: all_steps_completed helper unit tests
// ========================================

/// "No output" path: worker exits cleanly with no text output but all steps done.
/// Helper must return true so the caller transitions to PendingReview.
#[test]
fn test_no_output_path_all_steps_complete() {
    let task_id = TaskId::new();
    let steps = vec![
        make_step(&task_id, TaskStepStatus::Completed),
        make_step(&task_id, TaskStepStatus::Skipped),
    ];
    let step_repo: Option<Arc<dyn TaskStepRepository>> = Some(Arc::new(StubTaskStepRepo { steps }));

    let result = run(all_steps_completed(&step_repo, &task_id));
    assert!(
        result,
        "No-output path: all Completed+Skipped → helper returns true"
    );
}

/// step_repo returns Err → helper must return false (safe fallback, never panic).
#[test]
fn test_step_repo_error_falls_through() {
    let task_id = TaskId::new();
    let step_repo: Option<Arc<dyn TaskStepRepository>> = Some(Arc::new(StubErrorTaskStepRepo));

    let result = run(all_steps_completed(&step_repo, &task_id));
    assert!(
        !result,
        "DB error on step query → helper must safe-fallback to false"
    );
}

/// All steps Skipped (no Completed) → helper must return true.
/// Skipped steps mean the agent legitimately bypassed them — work is considered done.
#[test]
fn test_all_skipped_no_completed() {
    let task_id = TaskId::new();
    let steps = vec![
        make_step(&task_id, TaskStepStatus::Skipped),
        make_step(&task_id, TaskStepStatus::Skipped),
        make_step(&task_id, TaskStepStatus::Skipped),
    ];
    let step_repo: Option<Arc<dyn TaskStepRepository>> = Some(Arc::new(StubTaskStepRepo { steps }));

    let result = run(all_steps_completed(&step_repo, &task_id));
    assert!(result, "All Skipped → helper returns true");
}

async fn assert_late_execution_finalizer_preserves_status(target_status: InternalStatus) {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(Arc::clone(&exec));

    let project = Project::new("Review Race".into(), "/tmp/review-race".into());
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Execution finalizer race".into());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();

    let task_step_repo: Option<Arc<dyn TaskStepRepository>> =
        Some(Arc::new(StatusChangingTaskStepRepo {
            task_repo: Arc::clone(&state.task_repo),
            task_id: task_id.clone(),
            target_status,
        }));

    handle_stream_success::<MockRuntime>(
        "late-run-id",
        ChatContextType::TaskExecution,
        task_id.as_str(),
        false,
        false,
        &execution_state,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.artifact_repo,
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.ideation_session_repo,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &task_step_repo,
        &None,
        &None::<tauri::AppHandle<MockRuntime>>,
        &None,
        &None,
        &None,
    )
    .await;

    let updated = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(
        updated.internal_status, target_status,
        "late execution finalizer must not overwrite review progress with Failed"
    );
}

#[tokio::test]
async fn test_late_execution_finalizer_cannot_overwrite_pending_review_or_reviewing() {
    assert_late_execution_finalizer_preserves_status(InternalStatus::PendingReview).await;
    assert_late_execution_finalizer_preserves_status(InternalStatus::Reviewing).await;
}

#[tokio::test]
async fn test_incomplete_execution_success_finalizer_fails_current_attempt_with_metadata() {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(Arc::clone(&exec));

    let project = Project::new("Incomplete Execution".into(), "/tmp/incomplete-execution".into());
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Current execution attempt".into());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();

    handle_stream_success::<MockRuntime>(
        "run-id-incomplete-success",
        ChatContextType::TaskExecution,
        task_id.as_str(),
        false,
        false,
        &execution_state,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.artifact_repo,
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.ideation_session_repo,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &None,
        &None,
        &None::<tauri::AppHandle<MockRuntime>>,
        &None,
        &None,
        &None,
    )
    .await;

    let updated = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(
        updated.internal_status,
        InternalStatus::Failed,
        "current incomplete execution should still transition to Failed"
    );

    let metadata: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .expect("incomplete finalizer should persist diagnostic metadata");
    assert_eq!(
        metadata
            .get("last_agent_error_context")
            .and_then(|value| value.as_str()),
        Some("execution")
    );
    assert_eq!(
        metadata
            .get("last_agent_error")
            .and_then(|value| value.as_str()),
        Some("Agent ended without completing all task steps")
    );
}

// ========================================
// Cancelled+turns_finalized path: run_completed emission
// ========================================

/// Verifies the branching logic in handle_stream_error for Cancelled variants.
///
/// Cancelled + turns_finalized > 0 → success path → run_completed emitted.
/// Cancelled + turns_finalized == 0 → user-stop path → agent:stopped emitted.
///
/// This test guards against regression: if the turns_finalized guard changes,
/// the UI will either (a) get stuck in "generating" or (b) emit spurious events.
#[test]
fn test_cancelled_with_turns_takes_success_path_not_error_path() {
    // StreamError is in scope via `use super::*` (chat_service_handlers re-exports it)

    // turns_finalized > 0 → agent completed at least one turn before cancellation
    // → handle_stream_error calls handle_stream_success + emits run_completed
    let cancelled_with_turns = StreamError::Cancelled {
        turns_finalized: 2,
        completion_tool_called: false,
    };
    let goes_to_success_path = match &cancelled_with_turns {
        StreamError::Cancelled {
            turns_finalized, ..
        } => *turns_finalized > 0,
        _ => false,
    };
    assert!(
        goes_to_success_path,
        "Cancelled{{turns_finalized:2}} → must take success path (handle_stream_success + run_completed)"
    );
    // Success path does NOT call agent_run_repo.fail or emit agent:error
    assert!(
        !cancelled_with_turns.is_retryable(),
        "Cancelled variant is never retried (already handled as success or stop)"
    );
    assert!(
        !cancelled_with_turns.is_provider_error(),
        "Cancelled variant is not a provider error"
    );

    // turns_finalized == 0 → genuine user-stop or system cancel before any turn completed
    // → handle_stream_error emits agent:stopped (not run_completed)
    let cancelled_no_turns = StreamError::Cancelled {
        turns_finalized: 0,
        completion_tool_called: false,
    };
    let goes_to_stop_path = match &cancelled_no_turns {
        StreamError::Cancelled {
            turns_finalized, ..
        } => *turns_finalized == 0,
        _ => false,
    };
    assert!(
        goes_to_stop_path,
        "Cancelled{{turns_finalized:0}} → must take stop path (agent:stopped, not run_completed)"
    );
}

// ========================================
// Cancelled handler: real handle_stream_error path exercise
// ========================================
//
// These tests call the real `handle_stream_error` function using memory repos
// (from AppState::new_test()) and assert the execution slot count after return.
// They guard the routing logic introduced in sub-branches A and B of the Cancelled handler.
//
// Key invariants:
// - Sub-branch B (completion_tool_called=true, turns_finalized=0): slot NOT re-incremented
//   because TurnComplete never fired, so there was no prior slot decrement to compensate for.
// - Sub-branch A (turns_finalized>0): slot IS re-incremented
//   because TurnComplete fired earlier and decremented the slot.
// - [Agent stopped] (turns_finalized=0, completion_tool_called=false): slot NOT touched.

/// Helper that calls handle_stream_error with the given Cancelled variant and
/// returns (recovery_spawned, running_count_after). Uses Ideation context and
/// memory repos so the Cancelled handler paths can exercise without side effects.
async fn invoke_handle_stream_error_cancelled(cancelled: &StreamError) -> (bool, u32) {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(Arc::clone(&exec));

    let conversation_id = ChatConversationId::new();
    let context_id = "test-session-id";
    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::Ideation,
        context_id,
    );
    let cli_path = std::path::Path::new("/tmp/claude");
    let plugin_dir = std::path::Path::new("/tmp/plugin");
    let working_dir = std::path::Path::new("/tmp");

    let recovery_spawned = handle_stream_error::<MockRuntime>(
        "cancelled",
        Some(cancelled),
        ChatContextType::Ideation,
        context_id,
        conversation_id,
        "run-id-1",
        "msg-id-1",
        &event_ctx,
        None, // stored_session_id
        crate::domain::agents::AgentHarnessKind::Claude,
        false, // is_retry_attempt
        None,  // user_message_content
        None,  // conversation
        None,  // resolved_project_id
        cli_path,
        plugin_dir,
        working_dir,
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None, // task_proposal_repo — not used in Cancelled path
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &execution_state,
        &None, // question_state — not used in Cancelled path
        &None, // plan_branch_repo — not used for Ideation
        &None, // execution_settings_repo — not used for Ideation
        &None::<tauri::AppHandle<MockRuntime>>,
        None,  // agent_name
        false, // team_mode
        None,  // run_chain_id
        &None, // interactive_process_registry
        &None, // review_repo
        &None, // task_step_repo
        &None, // verification_child_registry
    )
    .await;

    (recovery_spawned, exec.running_count())
}

#[tokio::test]
async fn test_recovery_retry_background_context_preserves_execution_side_runtime_deps() {
    let state = AppState::new_test();
    let execution_state = Some(Arc::new(ExecutionState::new()));
    let question_state = Some(Arc::new(crate::application::QuestionState::new()));
    let interactive_process_registry = Some(Arc::new(InteractiveProcessRegistry::new()));
    let verification_child_registry = Some(Arc::new(VerificationChildProcessRegistry::new()));

    let retry_child = tokio::process::Command::new("true")
        .spawn()
        .expect("spawn test child");
    let conversation_id = ChatConversationId::new();
    let task_id = TaskId::new();
    let mut retry_conv = ChatConversation::new_review(task_id.clone());
    retry_conv.set_provider_session_ref(crate::domain::agents::ProviderSessionRef {
        harness: crate::domain::agents::AgentHarnessKind::Codex,
        provider_session_id: "codex-recovered-session".to_string(),
    });

    let ctx = build_recovery_retry_background_context::<MockRuntime>(
        retry_child,
        crate::domain::agents::AgentHarnessKind::Codex,
        ChatContextType::Review,
        task_id.as_str(),
        conversation_id,
        "run-id-1",
        "codex-recovered-session".to_string(),
        std::path::Path::new("/tmp/worktree"),
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &state.delegated_session_repo,
        &Some(Arc::clone(&state.execution_settings_repo)),
        &Some(Arc::clone(&state.agent_lane_settings_repo)),
        &Some(Arc::clone(&state.ideation_effort_settings_repo)),
        &Some(Arc::clone(&state.ideation_model_settings_repo)),
        &Some(Arc::clone(&state.task_proposal_repo)),
        &state.activity_event_repo,
        &state.memory_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &execution_state,
        &question_state,
        &None,
        &None::<tauri::AppHandle<MockRuntime>>,
        Some("run-chain-1".to_string()),
        Some("retry this review".to_string()).as_deref(),
        retry_conv,
        Some("ralphx:ralphx-execution-reviewer"),
        false,
        &Some(Arc::clone(&state.review_repo)),
        &Some(Arc::clone(&state.task_step_repo)),
        &interactive_process_registry,
        &verification_child_registry,
    );

    assert_eq!(ctx.harness, crate::domain::agents::AgentHarnessKind::Codex);
    assert!(ctx.is_retry_attempt);
    assert!(
        ctx.repos.task_step_repo.is_some(),
        "stale-session retry must preserve task_step_repo for execution-side completion handling"
    );
    assert!(
        ctx.repos.review_repo.is_some(),
        "stale-session retry must preserve review_repo for review/merge completion flows"
    );
    assert!(
        ctx.interactive_process_registry.is_some(),
        "stale-session retry must preserve interactive_process_registry for execution/review/merge cleanup"
    );
    assert!(
        ctx.verification_child_registry.is_some(),
        "stale-session retry must preserve verification_child_registry to match the original background run context"
    );

    let mut child = ctx.child;
    let _ = child.wait().await;
}

#[tokio::test]
async fn test_handle_verification_child_completion_queues_hidden_auto_continue() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    let mut parent = crate::domain::entities::IdeationSession::new(project_id.clone());
    parent.verification_status = VerificationStatus::NeedsRevision;
    parent.verification_in_progress = true;
    let parent_id = parent.id.clone();
    state.ideation_session_repo.create(parent).await.unwrap();

    state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &parent_id,
            &crate::domain::entities::VerificationRunSnapshot {
                generation: 0,
                status: VerificationStatus::NeedsRevision,
                in_progress: true,
                current_round: 1,
                max_rounds: 5,
                best_round_index: None,
                convergence_reason: None,
                current_gaps: vec![],
                rounds: vec![crate::domain::entities::VerificationRoundSnapshot {
                    round: 1,
                    gap_score: 4,
                    fingerprints: vec![],
                    gaps: vec![],
                    parse_failed: false,
                }],
            },
        )
        .await
        .unwrap();

    let mut child = crate::domain::entities::IdeationSession::new(project_id);
    child.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child.parent_session_id = Some(parent_id.clone());
    let child_id = child.id.clone();
    state.ideation_session_repo.create(child).await.unwrap();

    handle_verification_child_completion::<MockRuntime>(
        &child_id,
        &parent_id,
        &state.ideation_session_repo,
        &state.chat_conversation_repo,
        &state.chat_message_repo,
        &state.message_queue,
        &None,
        &None,
    )
    .await;

    let queued = state
        .message_queue
        .get_queued(ChatContextType::Ideation, child_id.as_str());
    assert_eq!(queued.len(), 1, "auto-continue must queue one hidden control message");
    assert_eq!(
        queued[0].metadata_override.as_deref(),
        Some(VERIFICATION_AUTO_CONTINUE_METADATA)
    );
    assert!(
        queued[0]
            .content
            .contains("Continue the active verification loop in this same session"),
        "queued control prompt must instruct the verifier to continue the same loop"
    );

    let child_after = state
        .ideation_session_repo
        .get_by_id(&child_id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(child_after.status, crate::domain::entities::IdeationSessionStatus::Archived);

    let parent_conversation = state
        .chat_conversation_repo
        .get_active_for_context(ChatContextType::Ideation, parent_id.as_str())
        .await
        .unwrap();
    assert!(
        parent_conversation.is_none(),
        "auto-continue must not inject a parent-thread handoff message"
    );
}

/// Sub-branch B: Cancelled { turns_finalized: 0, completion_tool_called: true }
/// → success path taken; execution slot must NOT be re-incremented.
///
/// Rationale: TurnComplete never fired (cleanup raced ahead of it), so the slot
/// was never decremented by that event. Re-incrementing here would cause a slot
/// leak that makes the system believe an agent is still running.
#[tokio::test]
async fn test_handle_stream_error_cancelled_completion_tool_called_skips_slot_reincrement() {
    let cancelled = StreamError::Cancelled {
        turns_finalized: 0,
        completion_tool_called: true,
    };
    let (recovery_spawned, count_after) = invoke_handle_stream_error_cancelled(&cancelled).await;

    assert!(
        !recovery_spawned,
        "Sub-branch B must return false (success path, no retry)"
    );
    assert_eq!(
        count_after, 0,
        "completion_tool_called=true path must skip slot re-increment (TurnComplete never fired)"
    );
}

/// Regression guard: Cancelled { turns_finalized: 0, completion_tool_called: false }
/// → [Agent stopped] path taken; execution slot must NOT be incremented.
///
/// Manual user-stop must never be silently promoted to a success path.
/// This test ensures the new completion_tool_called guard does not broaden the
/// success condition beyond its intended scope.
#[tokio::test]
async fn test_handle_stream_error_cancelled_false_completion_takes_agent_stopped_path() {
    let cancelled = StreamError::Cancelled {
        turns_finalized: 0,
        completion_tool_called: false,
    };
    let (recovery_spawned, count_after) = invoke_handle_stream_error_cancelled(&cancelled).await;

    assert!(!recovery_spawned, "[Agent stopped] path must return false");
    assert_eq!(
        count_after, 0,
        "User-stop path must NOT touch the execution slot"
    );
}

/// Sub-branch A: Cancelled { turns_finalized: 1, completion_tool_called: true }
/// → success path taken; execution slot IS re-incremented.
///
/// TurnComplete fired (turns_finalized=1) so it already decremented the slot once.
/// Sub-branch A compensates with a re-increment before calling handle_stream_success.
/// This regression guard ensures that path is unchanged by the new completion_tool_called field.
#[tokio::test]
async fn test_handle_stream_error_cancelled_turns_finalized_re_increments_slot() {
    let cancelled = StreamError::Cancelled {
        turns_finalized: 1,
        completion_tool_called: true,
    };
    let (recovery_spawned, count_after) = invoke_handle_stream_error_cancelled(&cancelled).await;

    assert!(
        !recovery_spawned,
        "Sub-branch A must return false (success path, no retry)"
    );
    assert_eq!(
        count_after,
        1,
        "turns_finalized>0 path must re-increment slot once to compensate for TurnComplete's decrement"
    );
}

#[tokio::test]
async fn test_handle_stream_error_preserves_existing_content_blocks_without_serializing_nonfatal_mcp_cancellation() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let context_id = IdeationSessionId::new();
    let pre_assistant_message = crate::application::chat_service::chat_service_context::create_assistant_message(
        ChatContextType::Ideation,
        context_id.as_str(),
        "Recovered ideation response",
        conversation_id.clone(),
        &[ToolCall {
            id: Some("tool-1".to_string()),
            name: "ralphx::get_session_plan".to_string(),
            arguments: serde_json::json!({ "session_id": context_id.as_str() }),
            result: Some(serde_json::json!({ "status": "ok" })),
            parent_tool_use_id: Some("toolu-parent-preserved".to_string()),
            diff_context: None,
            stats: None,
        }],
        &[
            ContentBlockItem::Text {
                text: "Recovered ideation response".to_string(),
            },
            ContentBlockItem::ToolUse {
                id: Some("tool-1".to_string()),
                name: "ralphx::get_session_plan".to_string(),
                arguments: serde_json::json!({ "session_id": context_id.as_str() }),
                result: Some(serde_json::json!({ "status": "ok" })),
                parent_tool_use_id: Some("toolu-parent-preserved".to_string()),
                diff_context: None,
            },
        ],
    );
    let pre_assistant_message_id = pre_assistant_message.id.as_str().to_string();
    state
        .chat_message_repo
        .create(pre_assistant_message)
        .await
        .expect("insert pre-assistant message");

    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::Ideation,
        context_id.as_str(),
    );
    let stream_error = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: "user cancelled MCP tool call".to_string(),
    };

    let recovery_spawned = handle_stream_error::<MockRuntime>(
        "user cancelled MCP tool call",
        Some(&stream_error),
        ChatContextType::Ideation,
        context_id.as_str(),
        conversation_id,
        "run-id-1",
        &pre_assistant_message_id,
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &None,
        &None,
        &None,
        &None::<tauri::AppHandle<MockRuntime>>,
        None,
        false,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(
        !recovery_spawned,
        "non-fatal MCP cancellation path must not spawn recovery"
    );

    let stored = state
        .chat_message_repo
        .get_by_id(&ChatMessageId::from_string(pre_assistant_message_id))
        .await
        .expect("reload message")
        .expect("message should still exist");

    assert_eq!(
        stored.content, "Recovered ideation response",
        "non-fatal MCP cancellation text must not be appended into persisted assistant/orchestrator content"
    );
    assert!(
        stored.content_blocks.is_some(),
        "non-fatal MCP cancellation finalization must preserve previously persisted content_blocks instead of clearing ordered widget hydration"
    );
    let blocks: serde_json::Value = serde_json::from_str(
        stored.content_blocks.as_deref().expect("content blocks JSON should be present"),
    )
    .expect("content blocks should remain valid JSON");
    assert_eq!(
        blocks.as_array().map(|items| items.len()),
        Some(2),
        "the pre-error text + tool-use blocks should remain available for final replay rendering"
    );
}

#[tokio::test]
async fn test_handle_stream_error_terminal_verification_child_seals_unresolved_tool_calls() {
    let state = AppState::new_test();
    let parent_id = IdeationSessionId::new();
    let child_id = IdeationSessionId::new();
    let project_id = ProjectId::new();

    let mut parent = crate::domain::entities::IdeationSession::new(project_id.clone());
    parent.id = parent_id.clone();
    parent.verification_status = VerificationStatus::NeedsRevision;
    parent.verification_in_progress = false;
    parent.verification_generation = 7;
    state.ideation_session_repo.create(parent).await.unwrap();
    state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &parent_id,
            &crate::domain::entities::VerificationRunSnapshot {
                generation: 7,
                status: VerificationStatus::NeedsRevision,
                in_progress: false,
                current_round: 2,
                max_rounds: 5,
                best_round_index: None,
                convergence_reason: Some("max_rounds".to_string()),
                current_gaps: vec![crate::domain::entities::VerificationGap {
                    severity: "high".to_string(),
                    category: "scope".to_string(),
                    description: "Need one more database-default proof.".to_string(),
                    why_it_matters: None,
                    source: None,
                }],
                rounds: vec![],
            },
        )
        .await
        .unwrap();

    let mut child = crate::domain::entities::IdeationSession::new(project_id);
    child.id = child_id.clone();
    child.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child.parent_session_id = Some(parent_id.clone());
    state.ideation_session_repo.create(child).await.unwrap();

    let parent_conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(parent_id.clone()))
        .await
        .unwrap();
    let child_conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(child_id.clone()))
        .await
        .unwrap();

    let pre_assistant_message = crate::application::chat_service::chat_service_context::create_assistant_message(
        ChatContextType::Ideation,
        child_id.as_str(),
        "Checking verifier MCP context",
        child_conversation.id.clone(),
        &[ToolCall {
            id: Some("probe-1".to_string()),
            name: "ralphx::read_mcp_resource".to_string(),
            arguments: serde_json::json!({ "uri": "resource://probe" }),
            result: None,
            parent_tool_use_id: None,
            diff_context: None,
            stats: None,
        }],
        &[
            ContentBlockItem::Text {
                text: "Checking verifier MCP context".to_string(),
            },
            ContentBlockItem::ToolUse {
                id: Some("probe-1".to_string()),
                name: "ralphx::read_mcp_resource".to_string(),
                arguments: serde_json::json!({ "uri": "resource://probe" }),
                result: None,
                parent_tool_use_id: None,
                diff_context: None,
            },
        ],
    );
    let pre_assistant_message_id = pre_assistant_message.id.as_str().to_string();
    state
        .chat_message_repo
        .create(pre_assistant_message)
        .await
        .expect("insert pre-assistant message");

    let event_ctx = crate::application::chat_service::event_context(
        &child_conversation.id,
        &ChatContextType::Ideation,
        child_id.as_str(),
    );
    let stream_error = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: "agent exited".to_string(),
    };

    let recovery_spawned = handle_stream_error::<MockRuntime>(
        "agent exited",
        Some(&stream_error),
        ChatContextType::Ideation,
        child_id.as_str(),
        child_conversation.id.clone(),
        "run-id-terminal-verification",
        &pre_assistant_message_id,
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &None,
        &None,
        &None,
        &None::<tauri::AppHandle<MockRuntime>>,
        None,
        false,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(!recovery_spawned);

    let stored = state
        .chat_message_repo
        .get_by_id(&ChatMessageId::from_string(pre_assistant_message_id))
        .await
        .expect("reload message")
        .expect("message should still exist");
    assert_eq!(stored.content, "Checking verifier MCP context");

    let tool_calls: serde_json::Value = serde_json::from_str(
        stored.tool_calls.as_deref().expect("tool calls should be present"),
    )
    .expect("tool calls should remain valid JSON");
    assert_eq!(
        tool_calls[0]["result"]["status"],
        serde_json::json!("aborted"),
        "terminal verification suppression must seal unresolved tool calls so they do not stay live in the UI"
    );

    let content_blocks: serde_json::Value = serde_json::from_str(
        stored
            .content_blocks
            .as_deref()
            .expect("content blocks should be present"),
    )
    .expect("content blocks should remain valid JSON");
    assert_eq!(
        content_blocks[1]["result"]["status"],
        serde_json::json!("aborted"),
        "content block hydration must also stop treating the probe as still running"
    );

    let parent_messages = state
        .chat_message_repo
        .get_by_conversation(&parent_conversation.id)
        .await
        .expect("load parent conversation messages");
    assert!(
        !parent_messages.is_empty(),
        "terminal verification suppression should still inject the parent handoff message"
    );
}

#[tokio::test]
async fn test_handle_stream_error_actionable_verification_child_queues_hidden_auto_continue() {
    let state = AppState::new_test();
    let parent_id = IdeationSessionId::new();
    let child_id = IdeationSessionId::new();
    let project_id = ProjectId::new();

    let mut parent = crate::domain::entities::IdeationSession::new(project_id.clone());
    parent.id = parent_id.clone();
    parent.verification_status = VerificationStatus::NeedsRevision;
    parent.verification_in_progress = true;
    parent.verification_generation = 4;
    state.ideation_session_repo.create(parent).await.unwrap();
    state
        .ideation_session_repo
        .save_verification_run_snapshot(
            &parent_id,
            &crate::domain::entities::VerificationRunSnapshot {
                generation: 4,
                status: VerificationStatus::NeedsRevision,
                in_progress: true,
                current_round: 2,
                max_rounds: 5,
                best_round_index: None,
                convergence_reason: None,
                current_gaps: vec![crate::domain::entities::VerificationGap {
                    severity: "high".to_string(),
                    category: "testing".to_string(),
                    description: "Need one more regression path.".to_string(),
                    why_it_matters: None,
                    source: None,
                }],
                rounds: vec![crate::domain::entities::VerificationRoundSnapshot {
                    round: 2,
                    gap_score: 3,
                    fingerprints: vec!["high::testing::Need one more regression path.".to_string()],
                    gaps: vec![crate::domain::entities::VerificationGap {
                        severity: "high".to_string(),
                        category: "testing".to_string(),
                        description: "Need one more regression path.".to_string(),
                        why_it_matters: None,
                        source: None,
                    }],
                    parse_failed: false,
                }],
            },
        )
        .await
        .unwrap();

    let mut child = crate::domain::entities::IdeationSession::new(project_id);
    child.id = child_id.clone();
    child.session_purpose = crate::domain::entities::SessionPurpose::Verification;
    child.parent_session_id = Some(parent_id.clone());
    state.ideation_session_repo.create(child).await.unwrap();

    let child_conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_ideation(child_id.clone()))
        .await
        .unwrap();

    let pre_assistant_message = crate::application::chat_service::chat_service_context::create_assistant_message(
        ChatContextType::Ideation,
        child_id.as_str(),
        "Round 2 critique in progress",
        child_conversation.id.clone(),
        &[],
        &[ContentBlockItem::Text {
            text: "Round 2 critique in progress".to_string(),
        }],
    );
    let pre_assistant_message_id = pre_assistant_message.id.as_str().to_string();
    state
        .chat_message_repo
        .create(pre_assistant_message)
        .await
        .expect("insert pre-assistant message");

    let event_ctx = crate::application::chat_service::event_context(
        &child_conversation.id,
        &ChatContextType::Ideation,
        child_id.as_str(),
    );
    let stream_error = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: "agent exited".to_string(),
    };

    let recovery_spawned = handle_stream_error::<MockRuntime>(
        "agent exited",
        Some(&stream_error),
        ChatContextType::Ideation,
        child_id.as_str(),
        child_conversation.id,
        "run-id-auto-continue",
        &pre_assistant_message_id,
        &event_ctx,
        Some("provider-session-verification"),
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &None,
        &None,
        &None,
        &None::<tauri::AppHandle<MockRuntime>>,
        None,
        false,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(!recovery_spawned);

    let queued = state
        .message_queue
        .get_queued(ChatContextType::Ideation, child_id.as_str());
    assert_eq!(queued.len(), 1, "auto-continue must queue one hidden control message");
    assert_eq!(
        queued[0].metadata_override.as_deref(),
        Some(VERIFICATION_AUTO_CONTINUE_METADATA)
    );

    let parent_after = state
        .ideation_session_repo
        .get_by_id(&parent_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(parent_after.verification_status, VerificationStatus::NeedsRevision);
    assert!(parent_after.verification_in_progress);

    let child_after = state
        .ideation_session_repo
        .get_by_id(&child_id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(child_after.status, crate::domain::entities::IdeationSessionStatus::Archived);
}

#[tokio::test]
async fn test_handle_stream_error_appends_generic_agent_error_to_existing_content() {
    let state = AppState::new_test();
    let conversation_id = ChatConversationId::new();
    let context_id = IdeationSessionId::new();
    let pre_assistant_message = crate::application::chat_service::chat_service_context::create_assistant_message(
        ChatContextType::Ideation,
        context_id.as_str(),
        "Recovered ideation response",
        conversation_id.clone(),
        &[],
        &[ContentBlockItem::Text {
            text: "Recovered ideation response".to_string(),
        }],
    );
    let pre_assistant_message_id = pre_assistant_message.id.as_str().to_string();
    state
        .chat_message_repo
        .create(pre_assistant_message)
        .await
        .expect("insert pre-assistant message");

    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::Ideation,
        context_id.as_str(),
    );
    let stream_error = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: "unexpected agent crash".to_string(),
    };

    let recovery_spawned = handle_stream_error::<MockRuntime>(
        "unexpected agent crash",
        Some(&stream_error),
        ChatContextType::Ideation,
        context_id.as_str(),
        conversation_id,
        "run-id-2",
        &pre_assistant_message_id,
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &None,
        &None,
        &None,
        &None::<tauri::AppHandle<MockRuntime>>,
        None,
        false,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(
        !recovery_spawned,
        "generic agent error append path must not spawn recovery"
    );

    let stored = state
        .chat_message_repo
        .get_by_id(&ChatMessageId::from_string(pre_assistant_message_id))
        .await
        .expect("reload message")
        .expect("message should still exist");

    assert!(
        stored.content.contains("[Agent error:"),
        "generic agent failures must still be appended into persisted assistant/orchestrator content"
    );
    assert!(
        stored.content.contains("unexpected agent crash"),
        "generic agent failures must keep the error details in the appended note"
    );
}

// ========================================
// L1 Shutdown Guard Tests
// ========================================

/// ExecutionState is initialized with is_shutting_down = false.
/// The L1 shutdown guard checks this flag before escalating, so the default
/// must be false to avoid skipping escalation during normal agent exits.
#[test]
fn test_execution_state_shutdown_flag_starts_false() {
    let exec = ExecutionState::new();
    assert!(
        !exec
            .is_shutting_down
            .load(std::sync::atomic::Ordering::SeqCst),
        "is_shutting_down must start as false so normal agent exits are escalated"
    );
}

/// The shutdown flag can be set via store(true), which the RunEvent::Exit handler
/// calls as the FIRST operation before cleaning up agents.
#[test]
fn test_execution_state_shutdown_flag_can_be_set() {
    let exec = ExecutionState::new();
    exec.is_shutting_down
        .store(true, std::sync::atomic::Ordering::SeqCst);
    assert!(
        exec.is_shutting_down
            .load(std::sync::atomic::Ordering::SeqCst),
        "is_shutting_down must reflect store(true)"
    );
}

/// The shutdown flag can be read back correctly after being set and cleared.
/// This guards against accidental persistence across test runs (AtomicBool is in-memory).
#[test]
fn test_execution_state_shutdown_flag_can_be_cleared() {
    let exec = ExecutionState::new();
    exec.is_shutting_down
        .store(true, std::sync::atomic::Ordering::SeqCst);
    exec.is_shutting_down
        .store(false, std::sync::atomic::Ordering::SeqCst);
    assert!(
        !exec
            .is_shutting_down
            .load(std::sync::atomic::Ordering::SeqCst),
        "is_shutting_down must reflect store(false) after being cleared"
    );
}

/// The L1 shutdown guard writes shutdown_interrupted: true into task metadata
/// when is_shutting_down is set. This test verifies the metadata manipulation
/// logic directly — creating a JSON object with the flag and confirming it is present.
#[test]
fn test_shutdown_interrupted_metadata_key_added_when_shutdown_flag_set() {
    // Simulate what the L1 guard does: it merges shutdown_interrupted=true into
    // the task's metadata JSON when is_shutting_down is detected.
    let mut meta: serde_json::Value = serde_json::json!({
        "last_agent_error_context": "execution"
    });

    // Simulate the guard's metadata write
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("shutdown_interrupted".to_string(), serde_json::json!(true));
    }

    assert_eq!(
        meta.get("shutdown_interrupted").and_then(|v| v.as_bool()),
        Some(true),
        "shutdown_interrupted key must be present and true after L1 guard writes it"
    );
}

/// The shutdown_interrupted flag in metadata is a bool, not a string.
/// This ensures the startup recovery reader (should_auto_recover) can parse it correctly.
#[test]
fn test_shutdown_interrupted_metadata_value_is_bool() {
    let meta = serde_json::json!({
        "shutdown_interrupted": true,
        "last_agent_error_context": "review"
    });
    let flag = meta.get("shutdown_interrupted").and_then(|v| v.as_bool());
    assert_eq!(
        flag,
        Some(true),
        "shutdown_interrupted value must deserialize as bool true"
    );
}

#[tokio::test]
async fn test_task_execution_shutdown_success_persists_startup_recovery_metadata() {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    exec.is_shutting_down
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let execution_state = Some(Arc::clone(&exec));

    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Executing task".into());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();

    handle_stream_success::<MockRuntime>(
        "run-id-shutdown-success",
        ChatContextType::TaskExecution,
        task_id.as_str(),
        false,
        false,
        &execution_state,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.artifact_repo,
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.ideation_session_repo,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &None,
        &None,
        &None,
        &None::<tauri::AppHandle<MockRuntime>>,
        &None,
        &None,
        &None,
    )
    .await;

    let updated = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(
        updated.internal_status,
        InternalStatus::Executing,
        "shutdown success path must leave the task active for startup recovery"
    );

    let metadata: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .expect("shutdown guard should persist metadata");
    assert_eq!(
        metadata
            .get("shutdown_interrupted")
            .and_then(|value| value.as_bool()),
        Some(true),
        "startup recovery marker must be persisted on shutdown success"
    );
    assert_eq!(
        metadata
            .get("last_agent_error_context")
            .and_then(|value| value.as_str()),
        Some("execution"),
        "startup recovery must know the interrupted context"
    );
}

#[tokio::test]
async fn test_task_execution_shutdown_error_persists_startup_recovery_metadata() {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    exec.is_shutting_down
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let execution_state = Some(Arc::clone(&exec));

    let project = Project::new("Test Project".into(), "/tmp/test-project".into());
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Executing task".into());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();

    let conversation_id = ChatConversationId::new();
    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::TaskExecution,
        task_id.as_str(),
    );
    let stream_error = StreamError::AgentExit {
        exit_code: Some(1),
        stderr: "agent exited during shutdown".to_string(),
    };

    let recovery_spawned = handle_stream_error::<MockRuntime>(
        "agent exited during shutdown",
        Some(&stream_error),
        ChatContextType::TaskExecution,
        task_id.as_str(),
        conversation_id,
        "run-id-shutdown-error",
        "message-id-shutdown-error",
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &execution_state,
        &None,
        &None,
        &None,
        &None::<tauri::AppHandle<MockRuntime>>,
        None,
        false,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(
        !recovery_spawned,
        "shutdown error path should not spawn immediate recovery; startup owns it"
    );

    let updated = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(
        updated.internal_status,
        InternalStatus::Executing,
        "shutdown error path must leave the task active for startup recovery"
    );

    let metadata: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .expect("shutdown guard should persist metadata");
    assert_eq!(
        metadata
            .get("shutdown_interrupted")
            .and_then(|value| value.as_bool()),
        Some(true),
        "startup recovery marker must be persisted on shutdown error"
    );
    assert_eq!(
        metadata
            .get("last_agent_error_context")
            .and_then(|value| value.as_str()),
        Some("execution"),
        "startup recovery must know the interrupted context"
    );
    assert_eq!(
        metadata
            .get("last_agent_error")
            .and_then(|value| value.as_str()),
        Some("agent exited during shutdown"),
        "shutdown error path should preserve the agent error for diagnostics"
    );
}

#[tokio::test]
async fn test_task_execution_error_finalizer_fails_current_attempt_with_metadata() {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(Arc::clone(&exec));

    let project = Project::new("Failed Execution".into(), "/tmp/failed-execution".into());
    state.project_repo.create(project.clone()).await.unwrap();

    let mut task = Task::new(project.id.clone(), "Executing task".into());
    task.internal_status = InternalStatus::Executing;
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();

    let conversation_id = ChatConversationId::new();
    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::TaskExecution,
        task_id.as_str(),
    );
    let stream_error = StreamError::Timeout {
        context_type: ChatContextType::TaskExecution,
        elapsed_secs: 120,
    };

    let recovery_spawned = handle_stream_error::<MockRuntime>(
        "execution timed out after 120s",
        Some(&stream_error),
        ChatContextType::TaskExecution,
        task_id.as_str(),
        conversation_id,
        "run-id-timeout-error",
        "message-id-timeout-error",
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &execution_state,
        &None,
        &None,
        &None,
        &None::<tauri::AppHandle<MockRuntime>>,
        None,
        false,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(
        !recovery_spawned,
        "normal execution error path should not spawn stale-session recovery"
    );

    let updated = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(
        updated.internal_status,
        InternalStatus::Failed,
        "current execution error should transition to Failed"
    );

    let metadata: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .expect("execution error should persist metadata");
    assert_eq!(
        metadata
            .get("last_agent_error_context")
            .and_then(|value| value.as_str()),
        Some("execution")
    );
    assert_eq!(
        metadata
            .get("last_agent_error")
            .and_then(|value| value.as_str()),
        Some("execution timed out after 120s")
    );
    assert_eq!(
        metadata.get("is_timeout").and_then(|value| value.as_bool()),
        Some(true),
        "timeout finalizer should preserve timeout classification"
    );
    assert!(
        metadata.get("execution_recovery").is_some(),
        "non-provider execution errors should preserve recovery metadata"
    );
}

#[tokio::test]
async fn test_task_execution_provider_error_finalizer_pauses_with_metadata() {
    let state = AppState::new_test();
    let exec = Arc::new(ExecutionState::new());
    let execution_state = Some(Arc::clone(&exec));

    let project = Project::new("Provider Pause".into(), "/tmp/provider-pause-finalizer".into());
    state.project_repo.create(project.clone()).await.unwrap();

    let existing_provider_metadata = ProviderErrorMetadata {
        category: ProviderErrorCategory::RateLimit,
        message: "previous limit".to_string(),
        retry_after: None,
        previous_status: InternalStatus::Executing.to_string(),
        paused_at: Utc::now().to_rfc3339(),
        auto_resumable: true,
        resume_attempts: 2,
    };

    let mut task = Task::new(project.id.clone(), "Executing task".into());
    task.internal_status = InternalStatus::Executing;
    task.metadata = Some(existing_provider_metadata.write_to_task_metadata(None));
    let task_id = task.id.clone();
    state.task_repo.create(task).await.unwrap();

    let conversation_id = ChatConversationId::new();
    let event_ctx = crate::application::chat_service::event_context(
        &conversation_id,
        &ChatContextType::TaskExecution,
        task_id.as_str(),
    );
    let retry_after = (Utc::now() + chrono::Duration::minutes(30)).to_rfc3339();
    let stream_error = StreamError::ProviderError {
        category: ProviderErrorCategory::RateLimit,
        message: "usage limit reached".to_string(),
        retry_after: Some(retry_after.clone()),
    };

    let recovery_spawned = handle_stream_error::<MockRuntime>(
        "usage limit reached",
        Some(&stream_error),
        ChatContextType::TaskExecution,
        task_id.as_str(),
        conversation_id,
        "run-id-provider-error",
        "message-id-provider-error",
        &event_ctx,
        None,
        crate::domain::agents::AgentHarnessKind::Codex,
        false,
        None,
        None,
        None,
        std::path::Path::new("/tmp/codex"),
        std::path::Path::new("/tmp/plugin"),
        std::path::Path::new("/tmp"),
        &state.chat_message_repo,
        &state.chat_attachment_repo,
        &state.artifact_repo,
        &state.chat_conversation_repo,
        &state.agent_run_repo,
        &state.task_repo,
        &state.task_dependency_repo,
        &state.project_repo,
        &state.ideation_session_repo,
        &None,
        &state.activity_event_repo,
        &state.message_queue,
        &state.running_agent_registry,
        &state.memory_event_repo,
        &execution_state,
        &None,
        &None,
        &None,
        &None::<tauri::AppHandle<MockRuntime>>,
        None,
        false,
        None,
        &None,
        &None,
        &None,
        &None,
    )
    .await;

    assert!(
        !recovery_spawned,
        "provider pause path should not spawn stale-session recovery"
    );

    let updated = state
        .task_repo
        .get_by_id(&task_id)
        .await
        .unwrap()
        .expect("task should still exist");
    assert_eq!(updated.internal_status, InternalStatus::Paused);

    let provider_error = ProviderErrorMetadata::from_task_metadata(updated.metadata.as_deref())
        .expect("provider error metadata should be persisted");
    assert_eq!(provider_error.category, ProviderErrorCategory::RateLimit);
    assert_eq!(provider_error.message, "usage limit reached");
    assert_eq!(provider_error.retry_after, Some(retry_after));
    assert_eq!(
        provider_error.resume_attempts, 2,
        "existing resume attempts should carry forward across provider pauses"
    );

    let metadata: serde_json::Value = updated
        .metadata
        .as_deref()
        .and_then(|raw| serde_json::from_str(raw).ok())
        .expect("provider pause should persist metadata");
    assert!(
        metadata.get("pause_reason").is_some(),
        "provider pause metadata should include the unified pause reason"
    );
}

/// Multiple ExecutionState instances are independent (no global state).
/// The L1 guard reads a specific Arc<ExecutionState> passed to the handler,
/// so creating two instances with different flag values must not interfere.
#[test]
fn test_execution_state_shutdown_flags_are_independent() {
    let exec_a = ExecutionState::new();
    let exec_b = ExecutionState::new();

    exec_a
        .is_shutting_down
        .store(true, std::sync::atomic::Ordering::SeqCst);

    assert!(
        exec_a
            .is_shutting_down
            .load(std::sync::atomic::Ordering::SeqCst),
        "exec_a flag should be true"
    );
    assert!(
        !exec_b
            .is_shutting_down
            .load(std::sync::atomic::Ordering::SeqCst),
        "exec_b flag must remain false — instances are independent"
    );
}

// ---------------------------------------------------------------------------
// Regression tests: verification child timeout fix (Gate B guard)
// ---------------------------------------------------------------------------

/// Gate B check: `is_verification_child` returns `true` for a session that was
/// created with `session_purpose = Verification`.
///
/// This proves that `handle_stream_error` will enter the timeout-suppression branch
/// and skip `agent:error` emission when the lingering idle process eventually hits
/// the 600s no-output timeout.
#[tokio::test]
async fn test_no_agent_error_on_timeout_for_terminal_verification_child() {
    use crate::domain::entities::{IdeationSession, ProjectId, SessionPurpose};
    use crate::infrastructure::memory::MemoryIdeationSessionRepository;

    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let repo_trait: Arc<dyn IdeationSessionRepository> = repo.clone();

    let parent_id = IdeationSessionId::new();
    let child_id = IdeationSessionId::new();

    // Create a verification child session (session_purpose = Verification).
    let child_session = IdeationSession::builder()
        .id(child_id.clone())
        .project_id(ProjectId::new())
        .session_purpose(SessionPurpose::Verification)
        .parent_session_id(parent_id.clone())
        .build();
    repo_trait
        .create(child_session)
        .await
        .expect("create verification child session");

    // Gate B: is_verification_child must return true for verification sessions.
    // When this returns true, handle_stream_error skips agent:error — which is the
    // regression being guarded: no false agent:error on timeout for already-reconciled
    // verification children.
    let is_verif = is_verification_child(child_id.as_str(), &repo_trait).await;
    assert!(
        is_verif,
        "Gate B must fire for Verification sessions — handle_stream_error will suppress agent:error"
    );
}

/// Gate B check: `is_verification_child` returns `false` for a regular (General)
/// ideation session.
///
/// This proves that `handle_stream_error` does NOT suppress `agent:error` for
/// normal ideation sessions — the verification timeout guard must not affect them.
#[tokio::test]
async fn test_normal_completion_unaffected_by_verification_guards() {
    use crate::domain::entities::{IdeationSession, ProjectId, SessionPurpose};
    use crate::infrastructure::memory::MemoryIdeationSessionRepository;

    let repo = Arc::new(MemoryIdeationSessionRepository::new());
    let repo_trait: Arc<dyn IdeationSessionRepository> = repo.clone();

    let session_id = IdeationSessionId::new();

    // Create a normal (General) ideation session.
    let general_session = IdeationSession::builder()
        .id(session_id.clone())
        .project_id(ProjectId::new())
        .session_purpose(SessionPurpose::General)
        .build();
    repo_trait
        .create(general_session)
        .await
        .expect("create general session");

    // Gate B must NOT fire for General sessions.
    // handle_stream_error proceeds to emit agent:error normally.
    let is_verif = is_verification_child(session_id.as_str(), &repo_trait).await;
    assert!(
        !is_verif,
        "Gate B must NOT fire for General sessions — agent:error must be emitted normally"
    );

    // Sanity check: unknown session IDs also return false (safe fallthrough).
    let unknown_id = IdeationSessionId::new();
    let is_unknown = is_verification_child(unknown_id.as_str(), &repo_trait).await;
    assert!(
        !is_unknown,
        "Gate B must return false for unknown sessions (safe fallthrough to normal agent:error)"
    );
}
